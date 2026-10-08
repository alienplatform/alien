use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;
use std::time::Duration;
use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::core::ResourcePermissionsHelper;
use crate::error::{ErrorData, Result};
use alien_aws_clients::ssm::{
    DescribeParametersRequest, DescribeParametersResponse, ParameterMetadata, ParameterStringFilter,
};
use alien_core::{
    AwsParameterStoreVaultHeartbeatData, HeartbeatBackend, ObservedHealth, Platform,
    ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus, Vault, VaultHeartbeatData, VaultHeartbeatStatus, VaultOutputs,
};
use chrono::{DateTime, Utc};

/// AWS Vault controller.
///
/// AWS SSM Parameter Store implicitly exists in every AWS account and region.
/// This controller simply sets up the vault reference without creating any infrastructure.
/// The vault represents a namespace prefix for SecureString parameters in SSM.
#[controller]
pub struct AwsVaultController {
    /// Revision of the vault grants successfully applied by setup.
    #[serde(default)]
    pub(crate) permissions_revision: Option<String>,
    /// AWS account ID for generating the Secrets Manager reference
    pub(crate) account_id: Option<String>,
    /// The AWS region for this vault
    pub(crate) region: Option<String>,
    /// The vault prefix (resource id)
    pub(crate) vault_prefix: Option<String>,
}

#[controller]
impl AwsVaultController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<Vault>()?;

        info!(
            vault_id = %config.id,
            region = %aws_cfg.region,
            "Setting up AWS SSM Parameter Store vault reference"
        );

        let account_id = aws_cfg.account_id.to_string();

        let vault_prefix = format!("{}-{}", ctx.resource_prefix, config.id);

        ResourcePermissionsHelper::apply_aws_resource_scoped_permissions(
            ctx,
            &config.id,
            &vault_prefix,
            "vault",
        )
        .await?;
        ResourcePermissionsHelper::remove_stale_aws_resource_scoped_permissions(
            ctx, &config.id, "vault",
        )
        .await?;
        if ResourcePermissionsHelper::resource_is_setup_owned(ctx, &config.id)? {
            self.permissions_revision = Some(super::permissions_revision(ctx)?);
        }

        // Store the vault prefix using resource_prefix-config.id pattern
        self.vault_prefix = Some(vault_prefix);

        info!(
            vault_id = %config.id,
            account_id = %account_id,
            region = %aws_cfg.region,
            vault_prefix = %self.vault_prefix.as_deref().unwrap_or("unknown"),
            "AWS SSM Parameter Store vault is ready (implicitly exists)"
        );

        self.account_id = Some(account_id);
        self.region = Some(aws_cfg.region.clone());

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Vault>()?;

        let vault_prefix = self.vault_prefix.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Vault prefix missing during permission update".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // The namespace needs no update, but a new consumer or changed grant
        // still needs its setup-owned policy, and a dropped grant needs its
        // policy removed. The helpers enforce setup authority; IAM policy
        // upserts and deletes that tolerate NotFound make the whole permission
        // phase safe to resume. The revision is recorded only after both.
        ResourcePermissionsHelper::apply_aws_resource_scoped_permissions(
            ctx,
            &config.id,
            vault_prefix,
            "vault",
        )
        .await?;
        ResourcePermissionsHelper::remove_stale_aws_resource_scoped_permissions(
            ctx, &config.id, "vault",
        )
        .await?;

        if ResourcePermissionsHelper::resource_is_setup_owned(ctx, &config.id)? {
            self.permissions_revision = Some(super::permissions_revision(ctx)?);
        }
        info!(vault_id = %config.id, "AWS vault permissions reconciled");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────
    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Vault>()?;

        info!(
            vault_id = %config.id,
            "Deleting AWS SSM Parameter Store vault reference (no infrastructure to delete)"
        );

        // Clear stored values
        self.account_id = None;
        self.region = None;
        self.vault_prefix = None;

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ──────────────────────────────
    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<Vault>()?;

        // Heartbeat check: verify stored account/region haven't drifted
        if let (Some(stored_account_id), Some(stored_region)) = (&self.account_id, &self.region) {
            // Check for configuration drift
            if stored_account_id != &aws_cfg.account_id.to_string() {
                return Err(AlienError::new(ErrorData::ResourceDrift {
                    resource_id: config.id.clone(),
                    message: format!(
                        "AWS account ID changed from {} to {}",
                        stored_account_id, aws_cfg.account_id
                    ),
                }));
            }

            if stored_region != &aws_cfg.region {
                return Err(AlienError::new(ErrorData::ResourceDrift {
                    resource_id: config.id.clone(),
                    message: format!(
                        "AWS region changed from {} to {}",
                        stored_region, aws_cfg.region
                    ),
                }));
            }

            debug!(account_id=%stored_account_id, region=%stored_region, "AWS SSM Parameter Store vault heartbeat check passed");
        }

        if let Some(vault_prefix) = &self.vault_prefix {
            let client = ctx.service_provider.get_aws_ssm_client(aws_cfg).await?;
            let response = client
                .describe_parameters(DescribeParametersRequest {
                    parameter_filters: Some(vec![ParameterStringFilter {
                        key: "Name".to_string(),
                        option: Some("BeginsWith".to_string()),
                        values: Some(vec![vault_prefix.clone()]),
                    }]),
                    max_results: Some(50),
                    next_token: None,
                })
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to describe SSM Parameter Store metadata for prefix '{}'",
                        vault_prefix
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            emit_aws_parameter_store_vault_heartbeat(
                ctx,
                &config.id,
                &aws_cfg.account_id.to_string(),
                &aws_cfg.region,
                vault_prefix,
                response,
            );
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    // ─────────────── TERMINAL STATES ──────────────────────────
    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn needs_update(&self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        if ctx.initial_setup_authority != alien_core::InitialSetupAuthority::DirectSetup
            || !ResourcePermissionsHelper::resource_is_setup_owned(ctx, ctx.desired_config.id())?
        {
            return Ok(false);
        }
        Ok(
            self.permissions_revision.as_deref()
                != Some(super::permissions_revision(ctx)?.as_str()),
        )
    }

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        if let (Some(account_id), Some(region)) = (&self.account_id, &self.region) {
            let vault_id = format!("{}:{}", account_id, region);
            Some(ResourceOutputs::new(VaultOutputs { vault_id }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::VaultBinding;

        if let Some(vault_prefix) = &self.vault_prefix {
            let binding = VaultBinding::parameter_store(vault_prefix.clone());

            Ok(Some(
                serde_json::to_value(binding).into_alien_error().context(
                    ErrorData::ResourceStateSerializationFailed {
                        resource_id: "binding".to_string(),
                        message: "Failed to serialize binding parameters".to_string(),
                    },
                )?,
            ))
        } else {
            Ok(None)
        }
    }
}

fn emit_aws_parameter_store_vault_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    account_id: &str,
    region: &str,
    prefix: &str,
    response: DescribeParametersResponse,
) {
    let parameters = response.parameters.unwrap_or_default();
    let has_more_parameters = response.next_token.is_some();
    let sampled_parameter_count = parameters.len() as u32;
    let latest_modified_at = latest_modified_at(&parameters);

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: Vault::RESOURCE_TYPE,
        controller_platform: Platform::Aws,
        backend: HeartbeatBackend::Aws,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Vault(VaultHeartbeatData::AwsParameterStore(
            AwsParameterStoreVaultHeartbeatData {
                status: VaultHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: Some(format!(
                        "SSM Parameter Store metadata sample for prefix '{}' is reachable",
                        prefix
                    )),
                    stale: false,
                    partial: has_more_parameters,
                    collection_issues: vec![],
                },
                account_id: account_id.to_string(),
                region: region.to_string(),
                prefix: prefix.to_string(),
                parameter_metadata_sampled: true,
                sampled_parameter_count: Some(sampled_parameter_count),
                sampled_secure_string_count: Some(count_parameter_type(
                    &parameters,
                    "SecureString",
                )),
                sampled_string_count: Some(count_parameter_type(&parameters, "String")),
                sampled_string_list_count: Some(count_parameter_type(&parameters, "StringList")),
                sampled_advanced_tier_count: Some(count_parameter_tier(&parameters, "Advanced")),
                sampled_kms_key_metadata_present_count: Some(
                    parameters
                        .iter()
                        .filter(|parameter| parameter.key_id.is_some())
                        .count() as u32,
                ),
                latest_modified_at,
                has_more_parameters: Some(has_more_parameters),
            },
        )),
        raw: vec![],
    });
}

fn count_parameter_type(parameters: &[ParameterMetadata], parameter_type: &str) -> u32 {
    parameters
        .iter()
        .filter(|parameter| parameter.parameter_type.as_deref() == Some(parameter_type))
        .count() as u32
}

fn count_parameter_tier(parameters: &[ParameterMetadata], tier: &str) -> u32 {
    parameters
        .iter()
        .filter(|parameter| parameter.tier.as_deref() == Some(tier))
        .count() as u32
}

fn latest_modified_at(parameters: &[ParameterMetadata]) -> Option<DateTime<Utc>> {
    parameters
        .iter()
        .filter_map(|parameter| {
            parameter
                .last_modified_date
                .and_then(aws_epoch_seconds_to_utc)
        })
        .max()
}

fn aws_epoch_seconds_to_utc(seconds: f64) -> Option<DateTime<Utc>> {
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }

    let secs = seconds.trunc() as i64;
    let nanos = (seconds.fract() * 1_000_000_000.0).round() as u32;
    DateTime::<Utc>::from_timestamp(secs, nanos.min(999_999_999))
}

#[cfg(test)]
mod permission_update_tests {
    use super::*;
    use crate::core::{
        MockPlatformServiceProvider, ResourceController, StackExecutor, StackResourceStateExt,
    };
    use crate::service_account::AwsServiceAccountController;
    use alien_aws_clients::{
        iam::{ListRolePoliciesResponse, ListRolePoliciesResult, MockIamApi, PolicyNames},
        AwsClientConfigExt as _,
    };
    use alien_client_core::ErrorData as CloudError;
    use alien_core::permissions::PermissionProfile;
    use alien_core::{
        AwsClientConfig, ClientConfig, DeploymentConfig, EnvironmentVariablesSnapshot,
        ExternalBindings, InitialSetupAuthority, Resource, ResourceLifecycle, ResourceRef,
        ServiceAccount, Stack, StackResourceState, StackSettings, StackState,
    };
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    const CONSUMER_ROLE: &str = "test-consumer-sa";
    const FORMER_ROLE: &str = "test-former-sa";
    const READ_POLICY: &str = "alien-secrets-vault-data-read";

    /// Which IAM response the fake loses or refuses.
    #[derive(Clone, Copy, PartialEq)]
    enum Fault {
        None,
        /// The first PutRolePolicy commits, then the response is lost.
        LoseFirstPut,
        /// The first DeleteRolePolicy commits, then the response is lost.
        LoseFirstDelete,
        /// Setup credentials cannot delete inline policies.
        DenyDelete,
    }

    /// In-memory IAM inline policies (role -> name -> document) plus a call log.
    #[derive(Default)]
    struct Iam {
        roles: BTreeMap<String, BTreeMap<String, String>>,
        calls: Vec<String>,
    }

    impl Iam {
        fn policies(&self, role: &str) -> Vec<String> {
            self.roles
                .get(role)
                .map(|policies| policies.keys().cloned().collect())
                .unwrap_or_default()
        }

        fn count(&self, prefix: &str) -> usize {
            self.calls.iter().filter(|c| c.starts_with(prefix)).count()
        }
    }

    fn assert_read_policy(document: &str) {
        let policy: serde_json::Value = serde_json::from_str(document).unwrap();
        assert!(policy["Statement"]
            .as_array()
            .unwrap()
            .iter()
            .any(|statement| {
                statement["Action"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|action| action == "ssm:GetParameter")
                    && statement["Resource"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|resource| {
                            resource
                                .as_str()
                                .unwrap()
                                .ends_with(":parameter/test-secrets-*")
                        })
            }));
    }

    fn fake_iam(iam: Arc<Mutex<Iam>>, fault: Fault) -> MockIamApi {
        let mut mock = MockIamApi::new();
        let put = iam.clone();
        mock.expect_put_role_policy()
            .returning(move |role, name, document| {
                assert_eq!(role, CONSUMER_ROLE);
                assert_eq!(name, READ_POLICY);
                assert_read_policy(document);
                let mut iam = put.lock().unwrap();
                iam.calls.push(format!("put {role} {name}"));
                iam.roles
                    .entry(role.to_string())
                    .or_default()
                    .insert(name.to_string(), document.to_string());
                if fault == Fault::LoseFirstPut && iam.count("put ") == 1 {
                    return Err(AlienError::new(CloudError::HttpRequestFailed {
                        message: "Connection closed after the policy write".to_string(),
                    }));
                }
                Ok(())
            });
        let list = iam.clone();
        mock.expect_list_role_policies().returning(move |role| {
            let mut iam = list.lock().unwrap();
            iam.calls.push(format!("list {role}"));
            Ok(ListRolePoliciesResponse {
                list_role_policies_result: ListRolePoliciesResult {
                    policy_names: Some(PolicyNames {
                        member: iam.policies(role),
                    }),
                    is_truncated: Some(false),
                    marker: None,
                },
            })
        });
        mock.expect_delete_role_policy()
            .returning(move |role, name| {
                let mut iam = iam.lock().unwrap();
                iam.calls.push(format!("delete {role} {name}"));
                if fault == Fault::DenyDelete {
                    return Err(AlienError::new(CloudError::RemoteAccessDenied {
                        resource_type: "IAM role policy".to_string(),
                        resource_name: name.to_string(),
                    }));
                }
                let removed = iam
                    .roles
                    .get_mut(role)
                    .and_then(|policies| policies.remove(name));
                if removed.is_none() {
                    return Err(AlienError::new(CloudError::RemoteResourceNotFound {
                        resource_type: "IAM role policy".to_string(),
                        resource_name: name.to_string(),
                    }));
                }
                if fault == Fault::LoseFirstDelete && iam.count("delete ") == 1 {
                    return Err(AlienError::new(CloudError::HttpRequestFailed {
                        message: "Connection closed after the policy delete".to_string(),
                    }));
                }
                Ok(())
            });
        mock
    }

    /// Trust generated for a service account with no workload principals.
    const DEFAULT_TRUST_POLICY: &str = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"lambda.amazonaws.com"},"Action":"sts:AssumeRole"}]}"#;

    fn ready_service_account(state: &mut StackState, account: ServiceAccount, role: &str) {
        let mut controller = AwsServiceAccountController::mock_ready(role);
        // Model a role whose trust is already applied. A missing trust
        // checkpoint would instead schedule legacy trust repair.
        controller.assume_role_policy = Some(DEFAULT_TRUST_POLICY.to_string());
        let id = account.id.clone();
        let mut account_state = StackResourceState::new_pending(
            ServiceAccount::RESOURCE_TYPE.to_string(),
            Resource::new(account),
            Some(ResourceLifecycle::Frozen),
            vec![],
        );
        account_state.status = ResourceStatus::Running;
        account_state.outputs = controller.get_outputs();
        account_state
            .set_internal_controller(Some(Box::new(controller)))
            .unwrap();
        state.resources.insert(id, account_state);
    }

    // `consumer` reads the vault. `former` is a profile with no vault grant,
    // e.g. one whose grant a release removed. Roles start with `existing`.
    fn fixture(
        lifecycle: ResourceLifecycle,
        authority: InitialSetupAuthority,
        fault: Fault,
        existing: &[(&str, &str)],
    ) -> (StackExecutor, StackState, Arc<Mutex<Iam>>) {
        let iam = Arc::new(Mutex::new(Iam::default()));
        for (role, name) in existing {
            iam.lock()
                .unwrap()
                .roles
                .entry(role.to_string())
                .or_default()
                .insert(name.to_string(), "{}".to_string());
        }
        let mock = Arc::new(fake_iam(iam.clone(), fault));
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(mock.clone()));
        let vault = Vault::new("secrets".to_string()).build();
        let consumer = ServiceAccount::new("consumer-sa".to_string()).build();
        let former = ServiceAccount::new("former-sa".to_string()).build();
        let stack = Stack::new("test".to_string())
            .add_with_dependencies(
                vault.clone(),
                lifecycle,
                vec![ResourceRef::new(
                    ServiceAccount::RESOURCE_TYPE,
                    "consumer-sa",
                )],
            )
            .add(consumer.clone(), ResourceLifecycle::Frozen)
            .add(former.clone(), ResourceLifecycle::Frozen)
            .permission(
                "consumer",
                PermissionProfile::new().resource("secrets", ["vault/data-read"]),
            )
            .permission("former", PermissionProfile::new())
            .build();
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(ExternalBindings::default())
            .allow_frozen_changes(true)
            .build();
        let executor =
            StackExecutor::builder(&stack, ClientConfig::Aws(Box::new(AwsClientConfig::mock())))
                .deployment_config(&config)
                .service_provider(Arc::new(provider))
                .initial_setup_authority(authority)
                .step_running_resources(false)
                .build()
                .unwrap();
        let mut state = StackState::with_resource_prefix(Platform::Aws, "test".to_string());
        let controller = AwsVaultController {
            state: AwsVaultState::Ready,
            account_id: Some("123456789012".to_string()),
            region: Some("us-east-1".to_string()),
            vault_prefix: Some("test-secrets".to_string()),
            ..Default::default()
        };
        let mut vault_state = StackResourceState::new_pending(
            Vault::RESOURCE_TYPE.to_string(),
            Resource::new(vault),
            Some(lifecycle),
            vec![],
        );
        vault_state.status = ResourceStatus::Running;
        vault_state.outputs = controller.get_outputs();
        vault_state
            .set_internal_controller(Some(Box::new(controller)))
            .unwrap();
        state.resources.insert("secrets".to_string(), vault_state);
        // The newly created role is ready, but the existing vault has not yet
        // recorded its new dependency or installed its read policy.
        ready_service_account(&mut state, consumer, CONSUMER_ROLE);
        ready_service_account(&mut state, former, FORMER_ROLE);
        (executor, state, iam)
    }

    fn set_revision(state: &mut StackState, revision: &str) {
        let resource = state.resources.get_mut("secrets").unwrap();
        let mut controller = resource
            .get_internal_controller_typed::<AwsVaultController>()
            .unwrap();
        controller.permissions_revision = Some(revision.to_string());
        resource
            .set_internal_controller(Some(Box::new(controller)))
            .unwrap();
    }

    fn revision(state: &StackState) -> Option<String> {
        state.resources["secrets"]
            .get_internal_controller_typed::<AwsVaultController>()
            .unwrap()
            .permissions_revision
    }

    fn reload(state: &StackState) -> StackState {
        serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap()
    }

    /// Exercises the existing-vault update through the real executor and IAM
    /// client. The cloud runner owns the isolated role and parameter fixtures
    /// and verifies GetParameter using freshly assumed consumer credentials.
    #[tokio::test]
    #[ignore = "requires isolated AWS role fixtures from the live test runner"]
    async fn live_permission_only_update_reconciles_existing_vault() {
        let prefix = std::env::var("ALIEN_TEST_VAULT_PREFIX").unwrap();
        assert!(prefix.starts_with("e2e-"), "use a task-owned test prefix");
        let account_id = std::env::var("AWS_TARGET_ACCOUNT_ID").unwrap();
        let region = std::env::var("AWS_TARGET_REGION").unwrap();
        let role_name = format!("{prefix}-consumer-sa");
        let role_arn = format!("arn:aws:iam::{account_id}:role/{role_name}");
        let vault = Vault::new("secrets".to_string()).build();
        let account = ServiceAccount::new("consumer-sa".to_string()).build();
        let dependencies = vec![ResourceRef::new(
            ServiceAccount::RESOURCE_TYPE,
            "consumer-sa",
        )];
        let stack = Stack::new("permission-update".to_string())
            .add_with_dependencies(
                vault.clone(),
                ResourceLifecycle::Frozen,
                dependencies.clone(),
            )
            .add(account.clone(), ResourceLifecycle::Frozen)
            .permission(
                "consumer",
                PermissionProfile::new().resource("secrets", ["vault/data-read"]),
            )
            .build();
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(ExternalBindings::default())
            .allow_frozen_changes(true)
            .build();
        let aws = AwsClientConfig {
            account_id: account_id.clone(),
            region: region.clone(),
            credentials: alien_core::AwsCredentials::AccessKeys {
                access_key_id: std::env::var("AWS_TARGET_ACCESS_KEY_ID").unwrap(),
                secret_access_key: std::env::var("AWS_TARGET_SECRET_ACCESS_KEY").unwrap(),
                session_token: std::env::var("AWS_TARGET_SESSION_TOKEN").ok(),
            },
            service_overrides: None,
        };
        let executor = StackExecutor::builder(&stack, ClientConfig::Aws(Box::new(aws)))
            .deployment_config(&config)
            .initial_setup_authority(InitialSetupAuthority::DirectSetup)
            .step_running_resources(false)
            .build()
            .unwrap();
        let mut state = StackState::with_resource_prefix(Platform::Aws, prefix.clone());
        let controller = AwsVaultController {
            state: AwsVaultState::Ready,
            account_id: Some(account_id),
            region: Some(region),
            vault_prefix: Some(format!("{prefix}-secrets")),
            ..Default::default()
        };
        let mut resource = StackResourceState::new_pending(
            Vault::RESOURCE_TYPE.to_string(),
            Resource::new(vault),
            Some(ResourceLifecycle::Frozen),
            dependencies,
        );
        resource.status = ResourceStatus::Running;
        resource.outputs = controller.get_outputs();
        resource
            .set_internal_controller(Some(Box::new(controller)))
            .unwrap();
        state.resources.insert("secrets".to_string(), resource);
        let controller: AwsServiceAccountController = serde_json::from_value(serde_json::json!({
            "state": "ready", "roleName": role_name, "roleArn": role_arn,
            "stackPermissionsApplied": true, "internalStayCount": null,
            // The runner owns this role's trust; record it as applied so the
            // update exercises only the vault grant.
            "assumeRolePolicy": DEFAULT_TRUST_POLICY,
        }))
        .unwrap();
        let mut resource = StackResourceState::new_pending(
            ServiceAccount::RESOURCE_TYPE.to_string(),
            Resource::new(account),
            Some(ResourceLifecycle::Frozen),
            vec![],
        );
        resource.status = ResourceStatus::Running;
        resource.outputs = controller.get_outputs();
        resource
            .set_internal_controller(Some(Box::new(controller)))
            .unwrap();
        state.resources.insert("consumer-sa".to_string(), resource);
        assert!(
            executor
                .plan(&state)
                .unwrap()
                .updates
                .contains_key("secrets")
        );
        let state = executor.step(state).await.unwrap().next_state;
        assert_eq!(state.resources["secrets"].status, ResourceStatus::Running);
        assert!(
            !executor
                .plan(&state)
                .unwrap()
                .updates
                .contains_key("secrets")
        );
    }

    #[tokio::test]
    async fn setup_update_grants_existing_vault_access_to_consumer() {
        let (executor, state, iam) = fixture(
            ResourceLifecycle::Frozen,
            InitialSetupAuthority::DirectSetup,
            Fault::None,
            &[],
        );
        assert!(executor
            .plan(&state)
            .unwrap()
            .updates
            .contains_key("secrets"));
        let state = executor.step(state).await.unwrap().next_state;
        assert_eq!(state.resources["secrets"].status, ResourceStatus::Running);
        let iam = iam.lock().unwrap();
        assert_eq!(iam.count("put "), 1);
        assert_eq!(iam.policies(CONSUMER_ROLE), vec![READ_POLICY]);
        assert_eq!(iam.count("delete "), 0);
        // Repeating setup does not schedule another update after convergence.
        assert!(!executor
            .plan(&state)
            .unwrap()
            .updates
            .contains_key("secrets"));
    }

    #[tokio::test]
    async fn permission_only_update_reconciles_and_then_converges() {
        let (executor, mut state, iam) = fixture(
            ResourceLifecycle::Frozen,
            InitialSetupAuthority::DirectSetup,
            Fault::None,
            &[],
        );
        state.resources.get_mut("secrets").unwrap().dependencies = vec![ResourceRef::new(
            ServiceAccount::RESOURCE_TYPE,
            "consumer-sa",
        )];
        set_revision(&mut state, "previous-grants");
        assert!(executor
            .plan(&state)
            .unwrap()
            .updates
            .contains_key("secrets"));
        let state = executor.step(state).await.unwrap().next_state;
        assert_eq!(state.resources["secrets"].status, ResourceStatus::Running);
        assert_eq!(iam.lock().unwrap().count("put "), 1);
        assert!(!executor
            .plan(&state)
            .unwrap()
            .updates
            .contains_key("secrets"));
    }

    #[tokio::test]
    async fn removed_grants_delete_only_this_vaults_stale_policies() {
        let (executor, mut state, iam) = fixture(
            ResourceLifecycle::Frozen,
            InitialSetupAuthority::DirectSetup,
            Fault::None,
            &[
                // `former` lost its read grant.
                (FORMER_ROLE, READ_POLICY),
                // `consumer` lost write but keeps read.
                (CONSUMER_ROLE, READ_POLICY),
                (CONSUMER_ROLE, "alien-secrets-vault-data-write"),
                // Not owned by this vault: another vault, an unregistered set
                // name, and a policy that only shares the prefix.
                (FORMER_ROLE, "alien-other-vault-data-read"),
                (FORMER_ROLE, "alien-secrets-custom"),
                (FORMER_ROLE, "alien-secrets-vault-data-read-extra"),
            ],
        );
        set_revision(&mut state, "previous-grants");
        assert!(executor
            .plan(&state)
            .unwrap()
            .updates
            .contains_key("secrets"));
        let state = executor.step(state).await.unwrap().next_state;
        assert_eq!(state.resources["secrets"].status, ResourceStatus::Running);
        {
            let iam = iam.lock().unwrap();
            assert_eq!(
                iam.policies(FORMER_ROLE),
                vec![
                    "alien-other-vault-data-read",
                    "alien-secrets-custom",
                    "alien-secrets-vault-data-read-extra",
                ]
            );
            assert_eq!(iam.policies(CONSUMER_ROLE), vec![READ_POLICY]);
            assert_eq!(iam.count("delete "), 2);
        }
        assert_ne!(revision(&state).as_deref(), Some("previous-grants"));
        // The recorded revision is the converged one: nothing is rescheduled.
        assert!(!executor
            .plan(&state)
            .unwrap()
            .updates
            .contains_key("secrets"));
    }

    #[tokio::test]
    async fn denied_delete_keeps_the_previous_revision_so_setup_retries() {
        let (executor, mut state, iam) = fixture(
            ResourceLifecycle::Frozen,
            InitialSetupAuthority::DirectSetup,
            Fault::DenyDelete,
            &[(FORMER_ROLE, READ_POLICY)],
        );
        set_revision(&mut state, "previous-grants");
        let state = executor.step(state).await.unwrap().next_state;
        assert_ne!(state.resources["secrets"].status, ResourceStatus::Running);
        let error = state.resources["secrets"]
            .error
            .as_ref()
            .unwrap()
            .to_string();
        assert!(error.contains("iam:DeleteRolePolicy"), "{error}");
        assert_eq!(revision(&state).as_deref(), Some("previous-grants"));
        assert_eq!(iam.lock().unwrap().policies(FORMER_ROLE), vec![READ_POLICY]);
    }

    #[tokio::test]
    async fn lost_delete_response_resumes_and_records_the_revision_once_removed() {
        let (executor, mut state, iam) = fixture(
            ResourceLifecycle::Frozen,
            InitialSetupAuthority::DirectSetup,
            Fault::LoseFirstDelete,
            &[(FORMER_ROLE, READ_POLICY)],
        );
        set_revision(&mut state, "previous-grants");
        let state = executor.step(state).await.unwrap().next_state;
        assert_ne!(state.resources["secrets"].status, ResourceStatus::Running);
        assert!(state.resources["secrets"].error.is_some());
        assert_eq!(revision(&state).as_deref(), Some("previous-grants"));
        // Reload the durable checkpoint and drive the executor's actual retry.
        let state = executor.step(reload(&state)).await.unwrap().next_state;
        assert_eq!(state.resources["secrets"].status, ResourceStatus::Running);
        let iam = iam.lock().unwrap();
        assert!(iam.policies(FORMER_ROLE).is_empty());
        assert_eq!(
            iam.count("delete "),
            1,
            "retry sees the committed delete and does not repeat it"
        );
        assert_ne!(revision(&state).as_deref(), Some("previous-grants"));
        assert!(!executor
            .plan(&state)
            .unwrap()
            .updates
            .contains_key("secrets"));
    }

    #[tokio::test]
    async fn previous_checkpoint_without_revision_reconciles_once() {
        let (executor, mut state, iam) = fixture(
            ResourceLifecycle::Frozen,
            InitialSetupAuthority::DirectSetup,
            Fault::None,
            &[],
        );
        assert!(state
            .resources
            .get_mut("secrets")
            .unwrap()
            .internal_state
            .as_mut()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("permissionsRevision")
            .is_some());
        let state = executor.step(reload(&state)).await.unwrap().next_state;
        assert_eq!(state.resources["secrets"].status, ResourceStatus::Running);
        assert_eq!(iam.lock().unwrap().count("put "), 1);
        assert!(!executor
            .plan(&state)
            .unwrap()
            .updates
            .contains_key("secrets"));
    }

    #[tokio::test]
    async fn imported_vault_update_refuses_permission_writes() {
        let (executor, state, iam) = fixture(
            ResourceLifecycle::Frozen,
            InitialSetupAuthority::ImportedHandoff,
            Fault::None,
            &[(FORMER_ROLE, READ_POLICY)],
        );
        let state = executor.step(state).await.unwrap().next_state;
        assert_ne!(state.resources["secrets"].status, ResourceStatus::Running);
        assert!(state.resources["secrets"]
            .error
            .as_ref()
            .unwrap()
            .to_string()
            .contains("rerun setup"));
        assert!(iam.lock().unwrap().calls.is_empty());
    }

    #[tokio::test]
    async fn live_vault_update_leaves_setup_owned_iam_untouched() {
        let (executor, state, iam) = fixture(
            ResourceLifecycle::Live,
            InitialSetupAuthority::ImportedHandoff,
            Fault::None,
            &[(FORMER_ROLE, READ_POLICY)],
        );
        let state = executor.step(state).await.unwrap().next_state;
        assert_eq!(state.resources["secrets"].status, ResourceStatus::Running);
        assert!(iam.lock().unwrap().calls.is_empty());
    }

    #[tokio::test]
    async fn lost_response_resumes_the_saved_update_and_upserts_the_same_policy() {
        let (executor, state, iam) = fixture(
            ResourceLifecycle::Frozen,
            InitialSetupAuthority::DirectSetup,
            Fault::LoseFirstPut,
            &[],
        );
        let state = executor.step(state).await.unwrap().next_state;
        assert_ne!(state.resources["secrets"].status, ResourceStatus::Running);
        assert!(state.resources["secrets"].error.is_some());
        // Reload the durable checkpoint and drive the executor's actual retry.
        let state = executor.step(reload(&state)).await.unwrap().next_state;
        assert_eq!(state.resources["secrets"].status, ResourceStatus::Running);
        let iam = iam.lock().unwrap();
        assert_eq!(iam.count("put "), 2);
        assert_eq!(iam.policies(CONSUMER_ROLE), vec![READ_POLICY]);
    }
}
