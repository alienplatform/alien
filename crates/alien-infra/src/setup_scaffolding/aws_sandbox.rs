//! An AWS sandbox's image-build role. `sandbox/provision` grants `iam:PassRole` and never
//! `iam:CreateRole`: with it, the management identity could mint a Lambda-trusted role with any
//! policy and pass it to a build running a customer-authored Dockerfile.

use std::collections::BTreeMap;

use alien_aws_clients::iam::{CreateRoleRequest, CreateRoleTag, IamApi, Role};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::import::data::AwsSandboxImportData;
use alien_core::sandbox_build_role::{
    sandbox_build_role_arn, sandbox_build_role_name, SandboxBuildRole, SANDBOX_BUILD_POLICY_NAME,
};
use alien_core::{
    setup_resource_tags, AwsSandboxEgressScaffolding, ClientConfig, Platform, ResourceLifecycle,
    Sandbox, SandboxCode, SetupScaffolding, Stack, StackState,
};
use alien_error::{AlienError, Context, IntoAlienError};
use tracing::info;

use super::aws_sandbox_egress;
use super::{ScaffoldingProgress, ScaffoldingSeed, SetupScaffoldingContext};
use crate::sandbox::aws_partition;
use crate::{ErrorData, Result};

/// An existing role is verified before it is adopted: `iam:PassRole` is scoped by name alone, so
/// a same-named role someone else made would be handed to a build running customer code.
///
/// A deny sandbox's egress objects come between verifying the role and applying its policy, so
/// each call still makes at most one mutating call.
pub(super) async fn reconcile(
    ctx: &SetupScaffoldingContext<'_>,
    stack: &Stack,
    stack_state: &StackState,
    sandbox: &Sandbox,
    lifecycle: ResourceLifecycle,
    records: &mut BTreeMap<String, SetupScaffolding>,
) -> Result<ScaffoldingProgress> {
    let egress_network = aws_sandbox_egress::egress_network(stack, sandbox)?;
    let aws = aws_config(ctx.client_config)?;
    let partition = aws_partition(&aws.region);
    let role_name = sandbox_build_role_name(ctx.resource_prefix, &sandbox.id);
    let SandboxCode::Image { image } = &sandbox.code else {
        return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            message: "an AWS sandbox is built from a prebuilt s3:// bundle, not from source"
                .to_string(),
            resource_id: Some(sandbox.id.clone()),
        }));
    };
    let build_role = SandboxBuildRole::builder()
        .sandbox_id(&sandbox.id)
        .partition(partition)
        .account_id(&aws.account_id)
        .region(&aws.region)
        .bundle_uri(image)
        .runtime_built(lifecycle == ResourceLifecycle::Live)
        .build();
    let policy = serde_json::to_string(&build_role.policy().context(
        ErrorData::ResourceConfigInvalid {
            message: "the sandbox's build role policy cannot be resolved".to_string(),
            resource_id: Some(sandbox.id.clone()),
        },
    )?)
    .into_alien_error()
    .context(serialize_failed(&sandbox.id))?;
    let trust = serde_json::to_value(build_role.trust_policy())
        .into_alien_error()
        .context(serialize_failed(&sandbox.id))?;

    let iam = ctx.service_provider.get_aws_iam_client(aws).await?;

    let role = match iam.get_role(&role_name).await {
        Ok(response) => response.get_role_result.role,
        Err(error) if is_not_found(&error) => {
            let created = iam
                .create_role(
                    CreateRoleRequest::builder()
                        .role_name(role_name.clone())
                        .assume_role_policy_document(trust.to_string())
                        .tags(setup_tags(ctx.resource_prefix, &sandbox.id))
                        .build(),
                )
                .await;
            return match created {
                Ok(_) => {
                    info!(sandbox_id = %sandbox.id, role = %role_name, "Created sandbox build role");
                    record(records, &sandbox.id, role_name);
                    Ok(ScaffoldingProgress::InProgress)
                }
                // IAM reads lag its writes, so this may be our own earlier create; the next call
                // reads the role and verifies it like any other existing one.
                Err(error) if is_conflict(&error) => Ok(ScaffoldingProgress::InProgress),
                Err(error) => Err(error).context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create sandbox build role '{role_name}'"),
                    resource_id: Some(sandbox.id.clone()),
                }),
            };
        }
        Err(error) => {
            return Err(error).context(ErrorData::CloudPlatformError {
                message: format!("Failed to read sandbox build role '{role_name}'"),
                resource_id: Some(sandbox.id.clone()),
            });
        }
    };

    let expected_arn =
        sandbox_build_role_arn(partition, &aws.account_id, ctx.resource_prefix, &sandbox.id);
    let mismatches = adoption_mismatches(
        iam.as_ref(),
        &role,
        &expected_arn,
        &trust,
        SANDBOX_BUILD_POLICY_NAME,
    )
    .await?;
    if !mismatches.is_empty() {
        return Err(AlienError::new(ErrorData::SetupScaffoldingNotAdoptable {
            resource_id: sandbox.id.clone(),
            object: format!("IAM role '{role_name}'"),
            reason: format!(
                "{}. Delete or rename it, then run setup again.",
                mismatches.join("; ")
            ),
        }));
    }

    record(records, &sandbox.id, role_name.clone());

    if let Some(network_id) = egress_network {
        let SetupScaffolding::AwsSandbox { egress, .. } = records
            .get_mut(&sandbox.id)
            .unwrap_or_else(|| unreachable!("recorded above"));
        let progress =
            aws_sandbox_egress::reconcile(ctx, aws, &sandbox.id, network_id, stack_state, egress)
                .await?;
        if progress == ScaffoldingProgress::InProgress {
            return Ok(progress);
        }
    }

    iam.put_role_policy(&role_name, SANDBOX_BUILD_POLICY_NAME, &policy)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to apply policy '{SANDBOX_BUILD_POLICY_NAME}' to sandbox build role \
                 '{role_name}'"
            ),
            resource_id: Some(sandbox.id.clone()),
        })?;
    Ok(ScaffoldingProgress::Done)
}

/// What the template setups register for this sandbox, from the build role this step verified
/// and the connector it recorded.
pub(super) fn seed(
    ctx: &SetupScaffoldingContext<'_>,
    sandbox: &Sandbox,
    records: &BTreeMap<String, SetupScaffolding>,
) -> Result<ScaffoldingSeed> {
    let aws = aws_config(ctx.client_config)?;
    let connector_arn = match records.get(&sandbox.id) {
        Some(SetupScaffolding::AwsSandbox {
            egress: Some(egress),
            ..
        }) => egress.connector_arn.as_deref(),
        _ => None,
    };
    let import_data = AwsSandboxImportData::runtime_built(
        sandbox,
        sandbox_build_role_arn(
            aws_partition(&aws.region),
            &aws.account_id,
            ctx.resource_prefix,
            &sandbox.id,
        ),
        &aws.region,
        connector_arn,
    )
    .context(ErrorData::ResourceConfigInvalid {
        message: "the sandbox's registration cannot be resolved".to_string(),
        resource_id: Some(sandbox.id.clone()),
    })?;
    Ok(ScaffoldingSeed {
        resource_id: sandbox.id.clone(),
        platform: Platform::Aws,
        region: aws.region.clone(),
        import_data: serde_json::to_value(import_data)
            .into_alien_error()
            .context(serialize_failed(&sandbox.id))?,
    })
}

/// Egress first: its connector, group and operator role, then the build role.
pub(super) async fn teardown(
    ctx: &SetupScaffoldingContext<'_>,
    resource_id: &str,
    role_name: &str,
    egress: &mut Option<AwsSandboxEgressScaffolding>,
) -> Result<ScaffoldingProgress> {
    let aws = aws_config(ctx.client_config)?;
    if let Some(objects) = egress {
        if aws_sandbox_egress::teardown(ctx, aws, resource_id, objects).await?
            == ScaffoldingProgress::InProgress
        {
            return Ok(ScaffoldingProgress::InProgress);
        }
        *egress = None;
    }
    let iam = ctx.service_provider.get_aws_iam_client(aws).await?;

    match iam
        .delete_role_policy(role_name, SANDBOX_BUILD_POLICY_NAME)
        .await
    {
        Ok(()) => {}
        Err(error) if is_not_found(&error) => {}
        Err(error) => {
            return Err(error).context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to delete policy '{SANDBOX_BUILD_POLICY_NAME}' from sandbox build \
                     role '{role_name}'"
                ),
                resource_id: Some(resource_id.to_string()),
            });
        }
    }
    match iam.delete_role(role_name).await {
        Ok(()) => {}
        Err(error) if is_not_found(&error) => {}
        Err(error) => {
            return Err(error).context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete sandbox build role '{role_name}'"),
                resource_id: Some(resource_id.to_string()),
            });
        }
    }
    info!(sandbox_id = %resource_id, role = %role_name, "Deleted sandbox build role");
    Ok(ScaffoldingProgress::Done)
}

/// No inline policy is accepted as well as `policy_name` alone: that is a role this step created
/// before an interruption. Trust is compared as raw JSON because a typed read drops keys it does
/// not model, such as an extra `AWS` principal, and would call the two equal.
pub(super) async fn adoption_mismatches(
    iam: &dyn IamApi,
    role: &Role,
    expected_arn: &str,
    expected_trust: &serde_json::Value,
    policy_name: &str,
) -> Result<Vec<String>> {
    let mut mismatches = Vec::new();

    if role.arn != expected_arn {
        mismatches.push(format!("its ARN is '{}', not '{expected_arn}'", role.arn));
    }

    let trust = role
        .assume_role_policy_document
        .as_deref()
        .and_then(|encoded| urlencoding::decode(encoded).ok())
        .and_then(|decoded| serde_json::from_str::<serde_json::Value>(&decoded).ok());
    if trust.as_ref() != Some(expected_trust) {
        mismatches.push(format!(
            "its trust policy is not exactly {expected_trust}, which admits only Lambda on \
             behalf of this account"
        ));
    }

    let inline = iam
        .list_role_policies(&role.role_name)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to list inline policies of role '{}'",
                role.role_name
            ),
            resource_id: None,
        })?
        .list_role_policies_result;
    let inline_names = inline
        .policy_names
        .map(|names| names.member)
        .unwrap_or_default();
    let foreign_inline: Vec<&str> = inline_names
        .iter()
        .map(String::as_str)
        .filter(|name| *name != policy_name)
        .collect();
    if !foreign_inline.is_empty() || inline.is_truncated == Some(true) {
        mismatches.push(format!(
            "it carries inline policies other than '{policy_name}': {}",
            foreign_inline.join(", ")
        ));
    }

    let attached = iam
        .list_attached_role_policies(&role.role_name)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to list managed policies of role '{}'",
                role.role_name
            ),
            resource_id: None,
        })?
        .list_attached_role_policies_result;
    let attached_arns: Vec<String> = attached
        .attached_policies
        .map(|policies| policies.member)
        .unwrap_or_default()
        .into_iter()
        .map(|policy| policy.policy_arn)
        .collect();
    if !attached_arns.is_empty() || attached.is_truncated == Some(true) {
        mismatches.push(format!(
            "it has managed policies attached: {}",
            attached_arns.join(", ")
        ));
    }

    Ok(mismatches)
}

/// Keeps whatever egress objects the record already holds.
fn record(records: &mut BTreeMap<String, SetupScaffolding>, sandbox_id: &str, role_name: String) {
    match records.get_mut(sandbox_id) {
        Some(SetupScaffolding::AwsSandbox {
            build_role_name, ..
        }) => *build_role_name = role_name,
        None => {
            records.insert(
                sandbox_id.to_string(),
                SetupScaffolding::AwsSandbox {
                    build_role_name: role_name,
                    egress: None,
                },
            );
        }
    }
}

pub(super) fn setup_tags(resource_prefix: &str, sandbox_id: &str) -> Vec<CreateRoleTag> {
    setup_resource_tags(resource_prefix, sandbox_id, Sandbox::RESOURCE_TYPE.as_ref())
        .into_iter()
        .map(|(key, value)| CreateRoleTag { key, value })
        .collect()
}

fn aws_config(client_config: &ClientConfig) -> Result<&alien_aws_clients::AwsClientConfig> {
    match client_config {
        ClientConfig::Aws(config) => Ok(config.as_ref()),
        other => Err(AlienError::new(ErrorData::ClientConfigMismatch {
            required_platform: Platform::Aws,
            found_platform: other.platform(),
        })),
    }
}

fn serialize_failed(sandbox_id: &str) -> ErrorData {
    ErrorData::ResourceConfigInvalid {
        message: "the sandbox's build role documents cannot be serialized".to_string(),
        resource_id: Some(sandbox_id.to_string()),
    }
}

pub(super) fn is_not_found(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        error.error,
        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
    )
}

pub(super) fn is_conflict(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        error.error,
        Some(CloudClientErrorData::RemoteResourceConflict { .. })
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller_test::SingleControllerExecutor;
    use crate::core::MockPlatformServiceProvider;
    use crate::sandbox::AwsSandboxController;
    use alien_aws_clients::iam::{
        AttachedPolicies, AttachedPolicy, CreateRoleResponse, CreateRoleResult, GetRoleResponse,
        GetRoleResult, ListAttachedRolePoliciesResponse, ListAttachedRolePoliciesResult,
        ListRolePoliciesResponse, ListRolePoliciesResult, MockIamApi, PolicyNames,
    };
    use alien_aws_clients::lambda_microvms::{
        CreateMicrovmImageRequest, CreateMicrovmImageResponse, MockLambdaMicrovmsApi,
    };
    use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt as _};
    use alien_core::{SandboxEgress, SandboxLifecyclePolicy};
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};

    const PREFIX: &str = "test";
    const ROLE_NAME: &str = "test-agents-build";
    const ROLE_ARN: &str = "arn:aws:iam::123456789012:role/test-agents-build";
    const BUNDLE_URI: &str = "s3://acme-artifacts/sandbox-bundle/f00dcafe/bundle.zip";

    fn sandbox() -> Sandbox {
        Sandbox::new("agents".to_string())
            .code(SandboxCode::Image {
                image: BUNDLE_URI.to_string(),
            })
            .egress(SandboxEgress::Allow)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build()
    }

    fn build_role() -> SandboxBuildRole<'static> {
        SandboxBuildRole::builder()
            .sandbox_id("agents")
            .partition("aws")
            .account_id("123456789012")
            .region("us-east-1")
            .bundle_uri(BUNDLE_URI)
            .runtime_built(true)
            .build()
    }

    fn expected_trust() -> Value {
        serde_json::to_value(build_role().trust_policy()).unwrap()
    }

    fn expected_policy() -> Value {
        serde_json::to_value(build_role().policy().unwrap()).unwrap()
    }

    fn client_config() -> ClientConfig {
        ClientConfig::Aws(Box::new(AwsClientConfig::mock()))
    }

    fn not_found() -> AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
            resource_type: "IAM Resource".to_string(),
            resource_name: ROLE_NAME.to_string(),
        })
    }

    fn role(arn: &str, trust: &Value) -> Role {
        Role {
            path: "/".to_string(),
            role_name: ROLE_NAME.to_string(),
            role_id: "AROAEXAMPLE".to_string(),
            arn: arn.to_string(),
            create_date: "2026-09-23T00:00:00Z".to_string(),
            // IAM returns the document URL-encoded.
            assume_role_policy_document: Some(urlencoding::encode(&trust.to_string()).into()),
            description: None,
            max_session_duration: None,
            permissions_boundary: None,
            tags: None,
            role_last_used: None,
        }
    }

    fn inline(names: &[&str]) -> ListRolePoliciesResponse {
        ListRolePoliciesResponse {
            list_role_policies_result: ListRolePoliciesResult {
                policy_names: Some(PolicyNames {
                    member: names.iter().map(|n| n.to_string()).collect(),
                }),
                is_truncated: Some(false),
                marker: None,
            },
        }
    }

    fn attached(arns: &[&str]) -> ListAttachedRolePoliciesResponse {
        ListAttachedRolePoliciesResponse {
            list_attached_role_policies_result: ListAttachedRolePoliciesResult {
                attached_policies: Some(AttachedPolicies {
                    member: arns
                        .iter()
                        .map(|arn| AttachedPolicy {
                            policy_name: "p".to_string(),
                            policy_arn: arn.to_string(),
                        })
                        .collect(),
                }),
                is_truncated: Some(false),
                marker: None,
            },
        }
    }

    /// An IAM client holding one existing role; `put_role_policy` is expected exactly `puts` times.
    fn existing(
        arn: &str,
        trust: Value,
        inline_names: &[&str],
        attached_arns: &[&str],
        puts: usize,
    ) -> MockIamApi {
        let mut iam = MockIamApi::new();
        let arn = arn.to_string();
        iam.expect_get_role().returning(move |_| {
            Ok(GetRoleResponse {
                get_role_result: GetRoleResult {
                    role: role(&arn, &trust),
                },
            })
        });
        let names: Vec<String> = inline_names.iter().map(|n| n.to_string()).collect();
        iam.expect_list_role_policies().returning(move |_| {
            Ok(inline(
                &names.iter().map(String::as_str).collect::<Vec<_>>(),
            ))
        });
        let arns: Vec<String> = attached_arns.iter().map(|a| a.to_string()).collect();
        iam.expect_list_attached_role_policies()
            .returning(move |_| {
                Ok(attached(
                    &arns.iter().map(String::as_str).collect::<Vec<_>>(),
                ))
            });
        iam.expect_put_role_policy()
            .withf(|role_name, policy_name, document| {
                role_name == ROLE_NAME
                    && policy_name == SANDBOX_BUILD_POLICY_NAME
                    && serde_json::from_str::<Value>(document).unwrap() == expected_policy()
            })
            .times(puts)
            .returning(|_, _, _| Ok(()));
        iam.expect_create_role().times(0);
        iam
    }

    fn provider(iam: MockIamApi) -> MockPlatformServiceProvider {
        let iam = Arc::new(iam);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(iam.clone()));
        provider
    }

    async fn step(
        provider: &MockPlatformServiceProvider,
        records: &mut BTreeMap<String, SetupScaffolding>,
    ) -> Result<ScaffoldingProgress> {
        let client_config = client_config();
        let ctx = SetupScaffoldingContext {
            client_config: &client_config,
            service_provider: provider,
            resource_prefix: PREFIX,
        };
        let stack = Stack::new("acme".to_string())
            .add(sandbox(), ResourceLifecycle::Live)
            .build();
        let mut stack_state = StackState::new(Platform::Aws);
        stack_state.resource_prefix = PREFIX.to_string();
        reconcile(
            &ctx,
            &stack,
            &stack_state,
            &sandbox(),
            ResourceLifecycle::Live,
            records,
        )
        .await
    }

    async fn tear_down(
        iam: MockIamApi,
        records: &mut BTreeMap<String, SetupScaffolding>,
    ) -> Result<ScaffoldingProgress> {
        let provider = provider(iam);
        let client_config = client_config();
        let ctx = SetupScaffoldingContext {
            client_config: &client_config,
            service_provider: &provider,
            resource_prefix: PREFIX,
        };
        super::super::teardown(&ctx, records).await
    }

    fn recorded() -> BTreeMap<String, SetupScaffolding> {
        BTreeMap::from([(
            "agents".to_string(),
            SetupScaffolding::AwsSandbox {
                build_role_name: ROLE_NAME.to_string(),
                egress: None,
            },
        )])
    }

    #[tokio::test]
    async fn a_missing_role_is_created_then_given_its_policy() {
        let mut absent = MockIamApi::new();
        absent
            .expect_get_role()
            .times(1)
            .returning(|_| Err(not_found()));
        absent
            .expect_create_role()
            .withf(|request| {
                let tags: BTreeMap<&str, &str> = request
                    .tags
                    .iter()
                    .flatten()
                    .map(|tag| (tag.key.as_str(), tag.value.as_str()))
                    .collect();
                request.role_name == ROLE_NAME
                    && serde_json::from_str::<Value>(&request.assume_role_policy_document).unwrap()
                        == expected_trust()
                    && request.path.is_none()
                    && tags
                        == BTreeMap::from([
                            ("deployment", PREFIX),
                            ("managed-by", "setup"),
                            ("resource", "agents"),
                            ("resource-type", "sandbox"),
                        ])
            })
            .times(1)
            .returning(|request| {
                Ok(CreateRoleResponse {
                    create_role_result: CreateRoleResult {
                        role: Role {
                            role_name: request.role_name,
                            ..role(ROLE_ARN, &expected_trust())
                        },
                    },
                })
            });
        absent.expect_put_role_policy().times(0);

        let mut records = BTreeMap::new();
        let progress = step(&provider(absent), &mut records).await.unwrap();
        assert_eq!(progress, ScaffoldingProgress::InProgress);
        assert_eq!(
            records,
            recorded(),
            "a created role is recorded before its policy lands"
        );

        // The next call finds the role it made, with no inline policy yet, and finishes it.
        let created = existing(ROLE_ARN, expected_trust(), &[], &[], 1);
        let progress = step(&provider(created), &mut records).await.unwrap();
        assert_eq!(progress, ScaffoldingProgress::Done);
        assert_eq!(records, recorded());
    }

    #[tokio::test]
    async fn a_matching_role_is_adopted_and_its_policy_reapplied_on_every_run() {
        let iam = existing(
            ROLE_ARN,
            expected_trust(),
            &[SANDBOX_BUILD_POLICY_NAME],
            &[],
            2,
        );
        let provider = provider(iam);
        let mut records = BTreeMap::new();
        for _ in 0..2 {
            let progress = step(&provider, &mut records).await.unwrap();
            assert_eq!(progress, ScaffoldingProgress::Done);
            assert_eq!(records, recorded());
        }
    }

    #[tokio::test]
    async fn a_create_that_races_its_own_read_waits_for_the_next_call() {
        let mut iam = MockIamApi::new();
        iam.expect_get_role().returning(|_| Err(not_found()));
        iam.expect_create_role().times(1).returning(|_| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceConflict {
                    message: "EntityAlreadyExists".to_string(),
                    resource_type: "IAM Resource".to_string(),
                    resource_name: ROLE_NAME.to_string(),
                },
            ))
        });
        iam.expect_put_role_policy().times(0);
        let mut records = BTreeMap::new();
        let progress = step(&provider(iam), &mut records).await.unwrap();
        assert_eq!(progress, ScaffoldingProgress::InProgress);
        assert!(
            records.is_empty(),
            "a role not yet verified is not recorded"
        );
    }

    async fn assert_refused(iam: MockIamApi, expected_reason: &str) {
        let mut records = BTreeMap::new();
        let error = step(&provider(iam), &mut records)
            .await
            .expect_err("a role that differs from setup's must not be adopted");
        assert_eq!(error.code, "SETUP_SCAFFOLDING_NOT_ADOPTABLE");
        assert!(
            error.message.contains(ROLE_NAME) && error.message.contains(expected_reason),
            "{}",
            error.message
        );
        assert!(records.is_empty(), "a refused role must never be recorded");
    }

    #[tokio::test]
    async fn a_role_with_a_different_trust_policy_is_refused() {
        let mut trust = expected_trust();
        trust["Statement"][0]["Principal"]["Service"] = json!("ec2.amazonaws.com");
        assert_refused(
            existing(ROLE_ARN, trust, &[SANDBOX_BUILD_POLICY_NAME], &[], 0),
            "trust policy",
        )
        .await;
    }

    /// A typed comparison drops the principal key it does not model and calls these equal.
    #[tokio::test]
    async fn a_role_trusting_an_extra_principal_is_refused() {
        let mut trust = expected_trust();
        trust["Statement"][0]["Principal"]["AWS"] = json!("arn:aws:iam::999999999999:root");
        assert_refused(
            existing(ROLE_ARN, trust, &[SANDBOX_BUILD_POLICY_NAME], &[], 0),
            "trust policy",
        )
        .await;
    }

    #[tokio::test]
    async fn a_role_with_an_extra_inline_policy_is_refused() {
        assert_refused(
            existing(
                ROLE_ARN,
                expected_trust(),
                &[SANDBOX_BUILD_POLICY_NAME, "admin"],
                &[],
                0,
            ),
            "inline policies other than 'sandbox-image-build': admin",
        )
        .await;
    }

    /// The check is by name, not by count. Counting would accept a role whose one inline policy is
    /// someone else's, then add the build policy beside it and pass the result to the build.
    #[tokio::test]
    async fn a_role_whose_only_inline_policy_is_foreign_is_refused() {
        assert_refused(
            existing(ROLE_ARN, expected_trust(), &["admin"], &[], 0),
            "inline policies other than 'sandbox-image-build': admin",
        )
        .await;
    }

    #[tokio::test]
    async fn a_role_with_an_attached_managed_policy_is_refused() {
        assert_refused(
            existing(
                ROLE_ARN,
                expected_trust(),
                &[SANDBOX_BUILD_POLICY_NAME],
                &["arn:aws:iam::aws:policy/AdministratorAccess"],
                0,
            ),
            "managed policies attached: arn:aws:iam::aws:policy/AdministratorAccess",
        )
        .await;
    }

    /// `sandbox/provision` scopes its pass to the root path, and the controller derives that ARN.
    #[tokio::test]
    async fn a_role_under_another_path_is_refused() {
        assert_refused(
            existing(
                "arn:aws:iam::123456789012:role/other/test-agents-build",
                expected_trust(),
                &[SANDBOX_BUILD_POLICY_NAME],
                &[],
                0,
            ),
            "its ARN is",
        )
        .await;
    }

    #[tokio::test]
    async fn teardown_deletes_the_policy_then_the_role() {
        let mut sequence = mockall::Sequence::new();
        let mut iam = MockIamApi::new();
        iam.expect_delete_role_policy()
            .withf(|role, policy| role == ROLE_NAME && policy == SANDBOX_BUILD_POLICY_NAME)
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(()));
        iam.expect_delete_role()
            .withf(|role| role == ROLE_NAME)
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(()));
        let mut records = recorded();
        assert_eq!(
            tear_down(iam, &mut records).await.unwrap(),
            ScaffoldingProgress::Done
        );
        assert!(records.is_empty(), "a deleted role leaves the record");
    }

    #[tokio::test]
    async fn teardown_of_a_role_already_gone_succeeds() {
        let mut iam = MockIamApi::new();
        iam.expect_delete_role_policy()
            .times(1)
            .returning(|_, _| Err(not_found()));
        iam.expect_delete_role()
            .times(1)
            .returning(|_| Err(not_found()));
        let mut records = recorded();
        assert_eq!(
            tear_down(iam, &mut records).await.unwrap(),
            ScaffoldingProgress::Done
        );
        assert!(records.is_empty());
    }

    #[tokio::test]
    async fn teardown_keeps_the_record_when_the_role_cannot_be_deleted() {
        let mut iam = MockIamApi::new();
        iam.expect_delete_role_policy().returning(|_, _| Ok(()));
        iam.expect_delete_role().returning(|_| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceConflict {
                    message: "DeleteConflict".to_string(),
                    resource_type: "IAM Resource".to_string(),
                    resource_name: ROLE_NAME.to_string(),
                },
            ))
        });
        let mut records = recorded();
        tear_down(iam, &mut records)
            .await
            .expect_err("a role that will not delete fails teardown");
        assert_eq!(records, recorded(), "the next attempt still knows the role");
    }

    /// The controller passes the role by ARN and this step creates it by name; if the two were
    /// derived separately they could drift and the build would be handed a role that is not there.
    #[tokio::test]
    async fn the_controller_passes_the_role_this_step_creates() {
        let passed = Arc::new(Mutex::new(None::<String>));
        let mut microvms = MockLambdaMicrovmsApi::new();
        microvms.expect_get_microvm_image().returning(|_| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Microvm".to_string(),
                    resource_name: "probe".to_string(),
                },
            ))
        });
        let capture = passed.clone();
        microvms.expect_create_microvm_image().returning(
            move |request: CreateMicrovmImageRequest| {
                *capture.lock().unwrap() = Some(request.build_role_arn.clone());
                Ok(CreateMicrovmImageResponse {
                    image_arn: Some(
                        "arn:aws:lambda:us-east-1:123456789012:microvm-image:test-agents"
                            .to_string(),
                    ),
                    name: Some("test-agents".to_string()),
                    state: Some("CREATING".to_string()),
                    image_version: Some("1.0".to_string()),
                })
            },
        );
        let microvms = Arc::new(microvms);
        let mut controller_provider = MockPlatformServiceProvider::new();
        controller_provider
            .expect_get_aws_microvms_client()
            .returning(move |_| Ok(microvms.clone()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(sandbox())
            .controller(AwsSandboxController::default())
            .platform(Platform::Aws)
            .service_provider(Arc::new(controller_provider))
            .build()
            .await
            .unwrap();
        executor.step().await.unwrap();
        let passed_arn = passed
            .lock()
            .unwrap()
            .clone()
            .expect("the image build was requested");

        let created = Arc::new(Mutex::new(None::<String>));
        let capture = created.clone();
        let mut iam = MockIamApi::new();
        iam.expect_get_role().returning(|_| Err(not_found()));
        iam.expect_create_role().returning(move |request| {
            *capture.lock().unwrap() = Some(request.role_name.clone());
            Ok(CreateRoleResponse {
                create_role_result: CreateRoleResult {
                    role: role(ROLE_ARN, &expected_trust()),
                },
            })
        });
        step(&provider(iam), &mut BTreeMap::new()).await.unwrap();
        let created_name = created
            .lock()
            .unwrap()
            .clone()
            .expect("the role was created");

        assert_eq!(
            passed_arn,
            format!("arn:aws:iam::123456789012:role/{created_name}")
        );
    }
}
