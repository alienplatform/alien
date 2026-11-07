//! Direct setup with automatic management grants every resource type's management permission
//! sets, so the management role's policy count depends on the whole permission registry. These
//! tests run the controller against an IAM fake that enforces AWS's default quota of 20 managed
//! policies attached to a role.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use alien_aws_clients::iam::{
    AttachedPolicies, AttachedPolicy, CreatePolicyResponse, CreatePolicyResult,
    CreatePolicyVersionResponse, CreatePolicyVersionResult, CreateRoleResponse, CreateRoleResult,
    GetPolicyResponse, GetPolicyResult, ListAttachedRolePoliciesResponse,
    ListAttachedRolePoliciesResult, ListPolicyVersionsResponse, ListPolicyVersionsResult,
    MockIamApi, Policy, PolicyVersion, Role,
};
use alien_client_core::ErrorData as ClientError;
use alien_core::{SetupManagement, StackSettings};
use futures::executor::block_on;

use super::*;
use crate::core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider};

const ACCOUNT_ID: &str = "123456789012";
const ROLE: &str = "test-prefix-management";
/// AWS's default quota of managed policies attached to one role.
const AWS_ATTACHED_POLICIES_PER_ROLE: usize = 20;

/// The IAM state of one account, as far as the management role's policies go.
#[derive(Default)]
struct Account {
    /// Customer managed policies by ARN, with their current document.
    policies: BTreeMap<String, String>,
    /// Policies attached to the management role, in attach order.
    attached: Vec<String>,
    /// The most policies the role ever had attached.
    most_attached: usize,
}

fn arn(name: &str) -> String {
    format!("arn:aws:iam::{ACCOUNT_ID}:policy/{name}")
}

fn numbered(index: usize) -> String {
    format!("test-prefix-deployment-management-{index}")
}

fn not_found(arn: &str) -> AlienError<ClientError> {
    AlienError::new(ClientError::RemoteResourceNotFound {
        resource_type: "policy".to_string(),
        resource_name: arn.to_string(),
    })
}

fn policy(arn: &str) -> Policy {
    Policy {
        policy_name: None,
        policy_id: None,
        arn: arn.to_string(),
        path: None,
        description: None,
        default_version_id: Some("v1".to_string()),
        attachment_count: None,
        is_attachable: None,
        create_date: None,
        update_date: None,
    }
}

fn version(id: &str) -> PolicyVersion {
    PolicyVersion {
        document: None,
        version_id: id.to_string(),
        is_default_version: true,
        create_date: Some(id.to_string()),
    }
}

/// An IAM client backed by `account`. Attaching past the quota fails like AWS does.
fn fake_iam(account: Arc<Mutex<Account>>) -> MockIamApi {
    let mut iam = MockIamApi::new();
    iam.expect_create_role().returning(|request| {
        Ok(CreateRoleResponse {
            create_role_result: CreateRoleResult {
                role: Role {
                    path: "/".to_string(),
                    role_name: request.role_name.clone(),
                    role_id: "AROAEXAMPLE".to_string(),
                    arn: format!("arn:aws:iam::{ACCOUNT_ID}:role/{}", request.role_name),
                    create_date: "2026-11-07T00:00:00Z".to_string(),
                    assume_role_policy_document: None,
                    description: None,
                    max_session_duration: None,
                    permissions_boundary: None,
                    tags: None,
                    role_last_used: None,
                },
            },
        })
    });
    let state = account.clone();
    iam.expect_create_policy()
        .returning(move |name, document, _, _| {
            let arn = arn(name);
            let mut account = state.lock().unwrap();
            if account.policies.contains_key(&arn) {
                return Err(AlienError::new(ClientError::RemoteResourceConflict {
                    resource_type: "policy".to_string(),
                    resource_name: arn,
                    message: "EntityAlreadyExists".to_string(),
                }));
            }
            account.policies.insert(arn.clone(), document.to_string());
            Ok(CreatePolicyResponse {
                create_policy_result: CreatePolicyResult {
                    policy: policy(&arn),
                },
            })
        });
    let state = account.clone();
    iam.expect_get_policy().returning(move |arn| {
        if state.lock().unwrap().policies.contains_key(arn) {
            Ok(GetPolicyResponse {
                get_policy_result: GetPolicyResult {
                    policy: policy(arn),
                },
            })
        } else {
            Err(not_found(arn))
        }
    });
    let state = account.clone();
    iam.expect_list_policy_versions().returning(move |arn| {
        if !state.lock().unwrap().policies.contains_key(arn) {
            return Err(not_found(arn));
        }
        Ok(ListPolicyVersionsResponse {
            list_policy_versions_result: ListPolicyVersionsResult {
                versions: Some(alien_aws_clients::iam::PolicyVersions {
                    member: vec![version("v1")],
                }),
                is_truncated: Some(false),
                marker: None,
            },
        })
    });
    let state = account.clone();
    iam.expect_create_policy_version()
        .returning(move |arn, document, _| {
            let mut account = state.lock().unwrap();
            let Some(current) = account.policies.get_mut(arn) else {
                return Err(not_found(arn));
            };
            *current = document.to_string();
            Ok(CreatePolicyVersionResponse {
                create_policy_version_result: CreatePolicyVersionResult {
                    policy_version: version("v2"),
                },
            })
        });
    let state = account.clone();
    iam.expect_attach_role_policy().returning(move |role, arn| {
        assert_eq!(role, ROLE);
        let mut account = state.lock().unwrap();
        assert!(
            account.policies.contains_key(arn),
            "{arn} exists before it is attached"
        );
        if account.attached.iter().any(|attached| attached == arn) {
            return Ok(());
        }
        if account.attached.len() == AWS_ATTACHED_POLICIES_PER_ROLE {
            return Err(AlienError::new(ClientError::QuotaExceeded {
                message: "Cannot exceed quota for PoliciesPerRole: 20".to_string(),
            }));
        }
        account.attached.push(arn.to_string());
        account.most_attached = account.most_attached.max(account.attached.len());
        Ok(())
    });
    let state = account.clone();
    iam.expect_detach_role_policy().returning(move |role, arn| {
        assert_eq!(role, ROLE);
        let mut account = state.lock().unwrap();
        let before = account.attached.len();
        account.attached.retain(|attached| attached != arn);
        if account.attached.len() == before {
            return Err(not_found(arn));
        }
        Ok(())
    });
    let state = account.clone();
    iam.expect_list_attached_role_policies()
        .returning(move |_| {
            let account = state.lock().unwrap();
            Ok(ListAttachedRolePoliciesResponse {
                list_attached_role_policies_result: ListAttachedRolePoliciesResult {
                    attached_policies: Some(AttachedPolicies {
                        member: account
                            .attached
                            .iter()
                            .map(|arn| AttachedPolicy {
                                policy_name: arn.rsplit('/').next().unwrap().to_string(),
                                policy_arn: arn.clone(),
                            })
                            .collect(),
                    }),
                    is_truncated: Some(false),
                    marker: None,
                },
            })
        });
    let state = account.clone();
    iam.expect_delete_policy().returning(move |arn| {
        let mut account = state.lock().unwrap();
        assert!(
            !account.attached.iter().any(|attached| attached == arn),
            "AWS refuses to delete an attached policy: {arn}"
        );
        account
            .policies
            .remove(arn)
            .map(|_| ())
            .ok_or_else(|| not_found(arn))
    });
    iam.expect_delete_policy_version().returning(|_, _| Ok(()));
    iam.expect_delete_role_policy()
        .returning(|_, name| Err(not_found(name)));
    iam
}

/// The smallest stack direct setup manages automatically: one function. The automatic grant
/// covers every resource type the registry knows, so the stack's own size barely matters.
async fn automatic_setup(account: Arc<Mutex<Account>>) -> SingleControllerExecutor {
    let iam = Arc::new(fake_iam(account));
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_aws_iam_client()
        .returning(move |_| Ok(iam.clone()));
    SingleControllerExecutor::builder()
        .resource(RemoteStackManagement::new("management".to_string()).build())
        .resource_lifecycle(ResourceLifecycle::Frozen)
        .controller(AwsRemoteStackManagementController::default())
        .platform(Platform::Aws)
        .resource_prefix("test-prefix")
        .stack_settings(StackSettings {
            setup_management: SetupManagement::Automatic,
            ..StackSettings::default()
        })
        .with_stack_resource(
            crate::core::controller_test::test_function_1(),
            ResourceLifecycle::Live,
        )
        .service_provider(Arc::new(provider))
        .build()
        .await
        .expect("executor builds")
}

/// `value` with every object's keys sorted. IAM ignores key order, and so does the merge: it
/// compares conditions as maps.
fn canonical(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => {
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort();
            serde_json::Value::Object(
                keys.into_iter()
                    .map(|key| (key.clone(), canonical(&object[key])))
                    .collect(),
            )
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(canonical).collect())
        }
        value => value.clone(),
    }
}

/// Every action each generated statement grants, keyed by everything else in the statement.
fn grants(statements: &[serde_json::Value]) -> Vec<(String, String)> {
    let mut grants = Vec::new();
    for statement in statements {
        let mut scope = statement.clone();
        let scope = scope.as_object_mut().unwrap();
        scope.remove("Sid");
        let actions = scope.remove("Action").unwrap();
        let scope = canonical(&serde_json::Value::Object(scope.clone())).to_string();
        for action in actions.as_array().unwrap() {
            grants.push((scope.clone(), action.as_str().unwrap().to_string()));
        }
    }
    grants.sort();
    grants.dedup();
    grants
}

#[tokio::test]
async fn automatic_direct_setup_fits_the_role_policy_quota() {
    let account = Arc::new(Mutex::new(Account::default()));
    let mut executor = automatic_setup(account.clone()).await;

    executor
        .run_until_terminal()
        .await
        .expect("setup attaches every management policy");
    assert_eq!(executor.status(), ResourceStatus::Running);

    let account = account.lock().unwrap();
    let attached: Vec<&String> = account
        .attached
        .iter()
        .filter(|arn| !arn.ends_with("-boundary"))
        .collect();
    assert!(
        attached.len() <= MAX_ATTACHED_MANAGEMENT_POLICIES,
        "{} management policies attached",
        attached.len()
    );
    assert_eq!(account.most_attached, attached.len());
    for (index, arn) in attached.iter().enumerate() {
        assert_eq!(**arn, self::arn(&numbered(index)));
        let document = &account.policies[*arn];
        assert!(
            document.len() <= MAX_MANAGED_POLICY_BYTES,
            "{arn}: {}",
            document.len()
        );
        assert!(!document.contains('\n'), "{arn} is sent as compact JSON");
    }
}

#[tokio::test]
async fn packing_keeps_every_grant_of_the_automatic_profile() {
    let account = Arc::new(Mutex::new(Account::default()));
    let executor = automatic_setup(account).await;

    let (packed, unpacked) = executor.with_context(|ctx| {
        let controller = AwsRemoteStackManagementController::default();
        let boundary = arn("test-prefix-deployment-management-boundary");
        let unpacked: Vec<serde_json::Value> = controller
            .management_policy_statements(ctx, ROLE, &boundary)
            .expect("the grant generates")
            .into_iter()
            .map(|statement| serde_json::to_value(statement).unwrap())
            .collect();
        let packed = controller
            .planned_management_policy_documents(ctx, ROLE)
            .expect("the automatic grant fits");
        (packed, unpacked)
    });

    assert!(packed.len() <= MAX_ATTACHED_MANAGEMENT_POLICIES);
    // A broad grant: many statements, with automatic setup's whole registry behind it.
    assert!(unpacked.len() > 100, "{} statements", unpacked.len());
    let statements: Vec<serde_json::Value> = packed
        .iter()
        .flat_map(|document| {
            serde_json::from_str::<serde_json::Value>(document).unwrap()["Statement"]
                .as_array()
                .unwrap()
                .clone()
        })
        .collect();
    let sids: std::collections::HashSet<&str> = statements
        .iter()
        .map(|statement| statement["Sid"].as_str().unwrap())
        .collect();
    assert_eq!(
        sids.len(),
        statements.len(),
        "Sids stay unique across policies"
    );
    let packed_grants = grants(&statements);
    let unpacked_grants = grants(&unpacked);
    let lost: Vec<_> = unpacked_grants
        .iter()
        .filter(|grant| !packed_grants.contains(grant))
        .collect();
    let added: Vec<_> = packed_grants
        .iter()
        .filter(|grant| !unpacked_grants.contains(grant))
        .collect();
    assert!(lost.is_empty(), "packing lost grants: {lost:#?}");
    assert!(added.is_empty(), "packing added grants: {added:#?}");
}

/// A deployment whose setup failed on the quota: policies 0-19 attached, 20 and 21 created but
/// never attached. Its next update converges to the packed set and removes the rest, without
/// ever going over the quota.
#[tokio::test]
async fn update_converges_a_role_that_hit_the_quota() {
    let account = Arc::new(Mutex::new(Account::default()));
    {
        let mut account = account.lock().unwrap();
        for index in 0..22 {
            let arn = arn(&numbered(index));
            account.policies.insert(arn.clone(), "{}".to_string());
            if index < AWS_ATTACHED_POLICIES_PER_ROLE {
                account.attached.push(arn);
            }
        }
        account.most_attached = AWS_ATTACHED_POLICIES_PER_ROLE;
    }
    let executor = automatic_setup(account.clone()).await;
    let mut controller = AwsRemoteStackManagementController::default();
    controller.role_name = Some(ROLE.to_string());

    let documents = executor
        .with_context(|ctx| controller.planned_management_policy_documents(ctx, ROLE))
        .expect("the automatic grant fits");
    executor
        .with_context(|ctx| {
            block_on(controller.apply_management_policy_documents(
                ctx,
                &fake_iam(account.clone()),
                ROLE,
                &documents,
            ))
        })
        .expect("the update applies");

    let account = account.lock().unwrap();
    let expected: Vec<String> = (0..documents.len()).map(|i| arn(&numbered(i))).collect();
    assert_eq!(account.attached, expected);
    assert_eq!(
        account.policies.keys().cloned().collect::<Vec<_>>(),
        {
            let mut sorted = expected.clone();
            sorted.sort();
            sorted
        },
        "policies the role no longer needs are deleted, attached or not"
    );
    for (index, document) in documents.iter().enumerate() {
        assert_eq!(account.policies[&arn(&numbered(index))], *document);
    }
    assert_eq!(account.most_attached, AWS_ATTACHED_POLICIES_PER_ROLE);
}

#[test]
fn a_grant_needing_more_than_the_limit_is_refused_and_not_retried() {
    let controller = AwsRemoteStackManagementController::default();
    // Distinct resources, so nothing merges; each statement fills most of a policy.
    let statements: Vec<AwsIamStatement> = (0..=MAX_ATTACHED_MANAGEMENT_POLICIES)
        .map(|index| AwsIamStatement {
            sid: format!("Grant{index}"),
            effect: "Allow".to_string(),
            action: vec!["s3:GetObject".to_string()],
            resource: vec![format!("arn:aws:s3:::bucket-{index}/{}", "x".repeat(4_000))],
            not_resource: Vec::new(),
            condition: None,
        })
        .collect();

    let error = controller
        .chunk_management_policy_documents(statements)
        .expect_err("19 policies don't fit");
    assert!(
        matches!(
            error.error,
            Some(ErrorData::ManagementPoliciesExceedRoleLimit {
                policy_count: 19,
                limit: 18,
                ..
            })
        ),
        "{error:?}"
    );
    assert!(!error.retryable);
}
