//! Direct setup with automatic management grants every resource type's management permission
//! sets, so the management role's policy count depends on the whole permission registry. These
//! tests run the controller against an IAM fake that enforces AWS's default quota of 20 managed
//! policies attached to a role.

use std::{
    collections::{BTreeMap, HashSet},
    sync::{Arc, Mutex},
};

use alien_aws_clients::iam::{
    AttachedPolicies, AttachedPolicy, CreatePolicyResponse, CreatePolicyResult,
    CreatePolicyVersionResponse, CreatePolicyVersionResult, CreateRoleResponse, CreateRoleResult,
    GetPolicyResponse, GetPolicyResult, ListAttachedRolePoliciesResponse,
    ListAttachedRolePoliciesResult, ListPolicyVersionsResponse, ListPolicyVersionsResult,
    ListRolePoliciesResponse, ListRolePoliciesResult, MockIamApi, Policy, PolicyNames,
    PolicyVersion, Role,
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
    /// Grants (see [`grants`]) the attached policies must hold after every change.
    required: Vec<(String, String)>,
    /// Grants that join `required` once `-0` has been written.
    required_after_first_write: Vec<(String, String)>,
    /// How many times `required` was checked.
    checks: usize,
}

impl Account {
    /// Panics if a required grant is in no attached policy.
    fn check_required(&mut self) {
        if self.required.is_empty() {
            return;
        }
        let statements: Vec<serde_json::Value> = self
            .attached
            .iter()
            .flat_map(|arn| {
                let document: serde_json::Value =
                    serde_json::from_str(&self.policies[arn]).unwrap();
                document["Statement"].as_array().unwrap().clone()
            })
            .collect();
        let held: HashSet<(String, String)> = grants(&statements).into_iter().collect();
        let missing: Vec<&(String, String)> = self
            .required
            .iter()
            .filter(|grant| !held.contains(*grant))
            .collect();
        assert!(
            missing.is_empty(),
            "after change {}, the role is missing {missing:#?}",
            self.checks
        );
        self.checks += 1;
    }
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
            if arn.ends_with("-0") {
                let later = std::mem::take(&mut account.required_after_first_write);
                account.required.extend(later);
            }
            account.check_required();
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
        account.check_required();
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
        account.check_required();
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
    iam.expect_list_role_policies().returning(|_| {
        Ok(ListRolePoliciesResponse {
            list_role_policies_result: ListRolePoliciesResult {
                policy_names: Some(PolicyNames { member: Vec::new() }),
                is_truncated: Some(false),
                marker: None,
            },
        })
    });
    iam
}

/// Seeds numbered policies: `attached` are created and attached, `detached` only created.
fn seed(account: &Arc<Mutex<Account>>, attached: &[usize], detached: &[usize]) {
    let mut account = account.lock().unwrap();
    for &index in attached.iter().chain(detached) {
        account
            .policies
            .insert(arn(&numbered(index)), "{}".to_string());
    }
    for &index in attached {
        account.attached.push(arn(&numbered(index)));
    }
    account.most_attached = account.attached.len();
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
        .chunk_management_policy_documents("management", statements)
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

/// Earlier releases detached and deleted stale policies one at a time, so an interrupted cleanup
/// can leave a gap with a detached policy above it.
#[tokio::test]
async fn update_deletes_stale_policies_past_a_gap() {
    let account = Arc::new(Mutex::new(Account::default()));
    let executor = automatic_setup(account.clone()).await;
    let mut controller = AwsRemoteStackManagementController::default();
    controller.role_name = Some(ROLE.to_string());
    let documents = executor
        .with_context(|ctx| controller.planned_management_policy_documents(ctx, ROLE))
        .expect("the automatic grant fits");
    let packed = documents.len();
    // `packed` is missing, `packed + 1` is detached, `packed + 2` is still attached, and
    // `packed + 4` is detached past a second gap.
    let attached: Vec<usize> = (0..packed).chain([packed + 2]).collect();
    seed(&account, &attached, &[packed + 1, packed + 4]);

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
    let expected: Vec<String> = (0..packed).map(|i| arn(&numbered(i))).collect();
    assert_eq!(account.attached, expected);
    let mut sorted = expected;
    sorted.sort();
    assert_eq!(
        account.policies.keys().cloned().collect::<Vec<_>>(),
        sorted,
        "every stale policy is deleted, past gaps and whether attached or not"
    );
}

/// An interrupted setup can leave policies it created but never attached, and an interrupted
/// update can leave policies it detached but never deleted. The role doesn't list either.
#[tokio::test]
async fn teardown_deletes_policies_the_role_no_longer_lists() {
    let account = Arc::new(Mutex::new(Account::default()));
    seed(&account, &[0, 1, 2], &[3, 5, 21]);
    let executor = automatic_setup(account.clone()).await;
    let mut controller = AwsRemoteStackManagementController::default();
    controller.role_name = Some(ROLE.to_string());

    let action = executor
        .with_context(|ctx| block_on(controller.delete_start(ctx)))
        .expect("policy cleanup succeeds");

    assert!(matches!(
        action,
        AwsRemoteStackManagementHandlerAction::Continue {
            state: AwsRemoteStackManagementState::DeletingRole,
            ..
        }
    ));
    let account = account.lock().unwrap();
    assert!(account.attached.is_empty(), "{:?}", account.attached);
    assert!(account.policies.is_empty(), "{:?}", account.policies.keys());
}

/// The statements of the automatic grant, before packing.
async fn automatic_statements() -> Vec<AwsIamStatement> {
    let executor = automatic_setup(Arc::new(Mutex::new(Account::default()))).await;
    executor.with_context(|ctx| {
        AwsRemoteStackManagementController::default()
            .management_policy_statements(
                ctx,
                ROLE,
                &arn("test-prefix-deployment-management-boundary"),
            )
            .expect("the grant generates")
    })
}

/// The layout earlier releases wrote: statements in generation order, each policy filled
/// until the next statement no longer fits, as pretty-printed JSON.
fn earlier_release_layout(statements: Vec<AwsIamStatement>) -> Vec<String> {
    let document = |statements: Vec<AwsIamStatement>| {
        serde_json::to_string_pretty(&AwsIamPolicy {
            version: "2012-10-17".to_string(),
            statement: statements,
        })
        .unwrap()
    };
    let mut documents = Vec::new();
    let mut current: Vec<AwsIamStatement> = Vec::new();
    for statement in statements {
        let mut candidate = current.clone();
        candidate.push(statement.clone());
        if document(candidate.clone()).len() <= MAX_MANAGED_POLICY_BYTES {
            current = candidate;
        } else {
            documents.push(document(std::mem::take(&mut current)));
            current = vec![statement];
        }
    }
    documents.push(document(current));
    documents
}

/// Writing the new documents moves statements between policies, and a statement is missing
/// from the role between its old policy's rewrite and its new one's. Starting from the layout
/// earlier releases left on a role that hit the quota, no Deny statement (the role's
/// protection of itself and of the deployer's secrets) that the role holds is ever dropped,
/// and from the first write on the role holds them all.
#[tokio::test]
async fn an_update_from_the_earlier_layout_never_drops_a_deny() {
    let statements = automatic_statements().await;
    let deny_statements: Vec<serde_json::Value> = statements
        .iter()
        .filter(|statement| statement.effect == "Deny")
        .map(|statement| serde_json::to_value(statement).unwrap())
        .collect();
    assert!(!deny_statements.is_empty(), "the grant has Deny statements");
    let old = earlier_release_layout(statements);
    assert_eq!(old.len(), 22, "earlier releases needed 22 policies");

    let account = Arc::new(Mutex::new(Account::default()));
    {
        let mut account = account.lock().unwrap();
        for (index, document) in old.iter().enumerate() {
            let arn = arn(&numbered(index));
            account.policies.insert(arn.clone(), document.clone());
            if index < AWS_ATTACHED_POLICIES_PER_ROLE {
                account.attached.push(arn);
            }
        }
        account.most_attached = AWS_ATTACHED_POLICIES_PER_ROLE;
        // Earlier releases filled policies in generation order, so a role that hit the quota
        // never got the Deny statements that landed in -20 and -21.
        let attached_statements: Vec<serde_json::Value> = account
            .attached
            .iter()
            .flat_map(|arn| {
                let document: serde_json::Value =
                    serde_json::from_str(&account.policies[arn]).unwrap();
                document["Statement"].as_array().unwrap().clone()
            })
            .collect();
        let held: HashSet<(String, String)> = grants(&attached_statements).into_iter().collect();
        let (attached_denies, missing_denies): (Vec<_>, Vec<_>) = grants(&deny_statements)
            .into_iter()
            .partition(|grant| held.contains(grant));
        println!(
            "earlier layout: {} Deny grants attached, {} only in -20/-21",
            attached_denies.len(),
            missing_denies.len()
        );
        assert!(
            !missing_denies.is_empty(),
            "the earlier layout leaves some Deny statements unattached"
        );
        account.required = attached_denies;
        account.required_after_first_write = missing_denies;
    }
    let executor = automatic_setup(account.clone()).await;
    let mut controller = AwsRemoteStackManagementController::default();
    controller.role_name = Some(ROLE.to_string());
    let documents = executor
        .with_context(|ctx| controller.planned_management_policy_documents(ctx, ROLE))
        .expect("the automatic grant fits");
    let first: serde_json::Value = serde_json::from_str(&documents[0]).unwrap();
    assert!(
        first["Statement"]
            .as_array()
            .unwrap()
            .iter()
            .all(|statement| statement["Effect"] == "Deny"),
        "-0 holds only the Deny statements"
    );

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
    assert!(
        account.required_after_first_write.is_empty(),
        "-0 was written"
    );
    assert_eq!(account.required.len(), grants(&deny_statements).len());
    // Checked after each of the `documents.len()` writes and each detach of the other old
    // policies; the desired ones were already attached.
    assert_eq!(account.checks, AWS_ATTACHED_POLICIES_PER_ROLE);
    let expected: Vec<String> = (0..documents.len()).map(|i| arn(&numbered(i))).collect();
    assert_eq!(account.attached, expected);
}

/// Where a statement lands doesn't depend on the order the permission sets were generated in.
#[tokio::test]
async fn packing_ignores_generation_order() {
    let statements = automatic_statements().await;
    let mut reversed = statements.clone();
    reversed.reverse();
    let controller = AwsRemoteStackManagementController::default();
    assert_eq!(
        controller
            .chunk_management_policy_documents("management", statements)
            .unwrap(),
        controller
            .chunk_management_policy_documents("management", reversed)
            .unwrap()
    );
}
