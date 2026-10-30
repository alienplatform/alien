//! Credential resolver that impersonates a management service account
//! to obtain target-environment credentials.
//!
//! Used when `[impersonation]` is configured in alien-manager.toml.
//! The management SA is loaded from the per-platform target bindings provider.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use alien_bindings::traits::ImpersonationRequest;
use alien_bindings::{BindingsProviderApi, ServiceAccountInfo};
use alien_core::{
    ClientConfig, DeploymentStatus, EnvironmentInfo, ManagementConfig, ManagementExternalId,
    Platform,
};
use alien_error::{AlienError, Context, GenericError};
use async_trait::async_trait;

use super::credential_errors::classify_credential_error;
use crate::error::ErrorData;
use crate::traits::{
    CredentialResolver, DeploymentRecord, ManagementExternalIdSource,
    RemoteStorageCredentialSource, ResolvedCredentials,
};

/// Resolves cloud credentials for push-model deployments via service account impersonation.
///
/// For each deployment, loads the management service account from the target platform's
/// bindings provider, impersonates it to get short-lived credentials, then resolves
/// the remote stack management identity from the deployment's stack state.
pub struct ImpersonationCredentialResolver {
    bindings_provider: Arc<dyn BindingsProviderApi>,
    target_providers: HashMap<Platform, Arc<dyn BindingsProviderApi>>,
    management_binding_platforms: HashSet<Platform>,
    environment_resolver: super::environment_credentials::EnvironmentCredentialResolver,
    management_external_ids: Option<Arc<dyn ManagementExternalIdSource>>,
}

impl ImpersonationCredentialResolver {
    pub fn new(
        bindings_provider: Arc<dyn BindingsProviderApi>,
        target_providers: HashMap<Platform, Arc<dyn BindingsProviderApi>>,
        management_binding_platforms: HashSet<Platform>,
    ) -> Self {
        Self {
            bindings_provider,
            target_providers,
            management_binding_platforms,
            environment_resolver:
                super::environment_credentials::EnvironmentCredentialResolver::new(),
            management_external_ids: None,
        }
    }

    /// Send each deployment's ExternalId when assuming its AWS management and
    /// remote-bindings roles.
    pub fn with_management_external_ids(
        mut self,
        source: Arc<dyn ManagementExternalIdSource>,
    ) -> Self {
        self.management_external_ids = Some(source);
        self
    }

    /// The ExternalId for an AWS deployment. Other platforms have no
    /// equivalent, so they never trigger a lookup.
    async fn management_external_id(
        &self,
        deployment: &DeploymentRecord,
    ) -> Result<Option<ManagementExternalId>, AlienError> {
        match (&self.management_external_ids, deployment.platform) {
            (Some(source), Platform::Aws) => source.management_external_id(deployment).await,
            _ => Ok(None),
        }
    }

    fn provider_for_target(&self, platform: Platform) -> &Arc<dyn BindingsProviderApi> {
        self.target_providers
            .get(&platform)
            .unwrap_or(&self.bindings_provider)
    }

    async fn resolve_from_env(
        &self,
        deployment: &DeploymentRecord,
        env: HashMap<String, String>,
    ) -> Result<ClientConfig, AlienError> {
        let platform = deployment.platform;

        if platform == Platform::Test {
            return Ok(ClientConfig::Test);
        }

        if platform == Platform::Local {
            return Ok(ClientConfig::Local {
                state_directory: "/tmp/alien-local".to_string(),
            });
        }

        if platform == Platform::Machines {
            return Ok(ClientConfig::Machines);
        }

        if uses_control_plane_credentials(platform) {
            return Ok(ClientConfig::Test);
        }

        if !self.management_binding_platforms.contains(&platform) {
            return self
                .environment_resolver
                .resolve_from_env(deployment, env)
                .await;
        }

        // InitialSetup remains setup-owned until the remote stack management
        // identity is imported. Poll-only setup methods hand the deployment to
        // the runtime manager at that point, so continuing with the managing
        // identity would send target resource operations to the wrong account.
        if uses_direct_impersonation_credentials(&deployment) {
            let provider = self.provider_for_target(platform);
            let base_config = impersonate_management_sa_trusted_by(
                &**provider,
                platform,
                &deployment.id,
                deployment.management_config.as_ref(),
            )
            .await?;
            return apply_target_environment(base_config, deployment.environment_info.as_ref());
        }

        // After initial setup, resolve via the remote stack management identity
        // in the stack state (two-hop: management SA → target identity).
        if let Some(ref stack_state) = deployment.stack_state {
            let provider = self.provider_for_target(platform);
            let resolver = alien_infra::RemoteAccessResolver::new(env.clone());
            let resolved = if platform == Platform::Gcp {
                let source = super::gcp_federation::assertion_source(&**provider, &env).await?;
                resolver
                    .resolve_gcp_federation(
                        source,
                        &deployment.gcp_federation_scope(),
                        stack_state,
                        deployment.environment_info.as_ref(),
                    )
                    .await
            } else {
                let base_config = impersonate_management_sa_trusted_by(
                    &**provider,
                    platform,
                    &deployment.id,
                    deployment.management_config.as_ref(),
                )
                .await?;
                let external_id = self.management_external_id(deployment).await?;
                resolver
                    .resolve(
                        base_config,
                        stack_state,
                        deployment.environment_info.as_ref(),
                        external_id.as_ref(),
                    )
                    .await
            }
            .context(ErrorData::RemoteCredentialHandoffFailed {
                deployment_id: deployment.id.clone(),
                platform,
            })
            .map_err(|error| classify_credential_error(error.into_generic()))?;

            return Ok(resolved);
        }

        Err(AlienError::new(GenericError {
            message: format!(
                "Remote stack state is required to resolve credentials for deployment {} in status {}",
                deployment.id, deployment.status
            ),
        }))
    }
}

#[async_trait]
impl CredentialResolver for ImpersonationCredentialResolver {
    async fn resolve(&self, deployment: &DeploymentRecord) -> Result<ClientConfig, AlienError> {
        self.resolve_from_env(deployment, std::env::vars().collect())
            .await
    }

    async fn resolve_with_capability(
        &self,
        deployment: &DeploymentRecord,
    ) -> Result<ResolvedCredentials, AlienError> {
        if uses_control_plane_credentials(deployment.platform) {
            return Ok(ResolvedCredentials {
                client_config: self.resolve(deployment).await?,
                has_provision_capability: true,
            });
        }

        if !self
            .management_binding_platforms
            .contains(&deployment.platform)
        {
            return self
                .environment_resolver
                .resolve_with_capability(deployment)
                .await;
        }

        let client_config = self.resolve(deployment).await?;
        let status = parse_status(&deployment.status);
        let has_provision_capability = matches!(
            deployment.platform,
            Platform::Local | Platform::Test | Platform::Kubernetes | Platform::Machines
        ) || !matches!(
            status,
            DeploymentStatus::Pending | DeploymentStatus::PreflightsFailed
        );

        Ok(ResolvedCredentials {
            client_config,
            has_provision_capability,
        })
    }

    async fn resolve_remote_storage_source(
        &self,
        deployment: &DeploymentRecord,
        resource_id: &str,
    ) -> Result<RemoteStorageCredentialSource, AlienError> {
        if !self
            .management_binding_platforms
            .contains(&deployment.platform)
        {
            return Err(AlienError::new(GenericError {
                message: "Remote Bindings require setup-owned identity outputs; rerun setup"
                    .to_string(),
            }));
        }

        let stack_state = deployment.stack_state.as_ref().ok_or_else(|| {
            AlienError::new(GenericError {
                message: format!(
                    "Remote stack state is required to resolve Remote Bindings credentials for deployment {}",
                    deployment.id
                ),
            })
        })?;
        let first_class_outputs = stack_state
            .resources
            .values()
            .find(|resource| {
                resource.resource_type == alien_core::RemoteBindings::RESOURCE_TYPE.as_ref()
            })
            .and_then(|state| state.outputs.as_ref())
            .and_then(|outputs| outputs.downcast_ref::<alien_core::RemoteBindingsOutputs>());
        let legacy_outputs = stack_state
            .resources
            .values()
            .find(|resource| {
                resource.resource_type == alien_core::RemoteStackManagement::RESOURCE_TYPE.as_ref()
            })
            .and_then(|state| state.outputs.as_ref())
            .and_then(|outputs| outputs.downcast_ref::<alien_core::RemoteStackManagementOutputs>())
            .and_then(|outputs| outputs.legacy_remote_bindings_access.as_ref());
        if deployment.platform == Platform::Gcp && first_class_outputs.is_none() {
            return Err(
                AlienError::new(alien_core::ErrorData::GcpFederationInvalid {
                    field: "remoteBindings".to_string(),
                    reason: "requires its separate installed federation identity; regenerate setup"
                        .to_string(),
                })
                .into_generic(),
            );
        }
        let outputs = first_class_outputs.or(legacy_outputs).ok_or_else(|| {
            AlienError::new(GenericError {
                message: format!(
                    "Remote Bindings identity state is required for deployment {}; rerun setup",
                    deployment.id
                ),
            })
        })?;

        let provider = self.provider_for_target(deployment.platform);
        let env: HashMap<String, String> = std::env::vars().collect();
        let resolver = alien_infra::RemoteAccessResolver::new(env.clone());
        let resolved = if deployment.platform == Platform::Gcp {
            let source = super::gcp_federation::assertion_source(&**provider, &env).await?;
            resolver
                .resolve_gcp_remote_bindings(
                    source,
                    &deployment.gcp_federation_scope(),
                    outputs,
                    deployment.environment_info.as_ref(),
                )
                .await
        } else {
            let base = impersonate_management_sa_trusted_by(
                &**provider,
                deployment.platform,
                &deployment.id,
                deployment.management_config.as_ref(),
            )
            .await?;
            let external_id = self.management_external_id(deployment).await?;
            resolver
                .resolve_remote_bindings(
                    base,
                    outputs,
                    deployment.environment_info.as_ref(),
                    Some(remote_bindings_session_name(&deployment.id, resource_id)),
                    external_id.as_ref(),
                )
                .await
        }
        .context(ErrorData::RemoteCredentialHandoffFailed {
            deployment_id: deployment.id.clone(),
            platform: deployment.platform,
        })
        .map_err(|error| classify_credential_error(error.into_generic()))?;
        Ok(RemoteStorageCredentialSource::Direct(resolved))
    }

    async fn resolve_management_config(
        &self,
        platform: Platform,
    ) -> Result<Option<ManagementConfig>, AlienError> {
        if uses_control_plane_credentials(platform) {
            return Ok(None);
        }

        if !self.management_binding_platforms.contains(&platform) {
            return self
                .environment_resolver
                .resolve_management_config(platform)
                .await;
        }

        let provider = self.provider_for_target(platform);

        let service_account = match provider.load_service_account("management").await {
            Ok(sa) => sa,
            Err(_) => return Ok(None),
        };

        let info = service_account.get_info().await.context(GenericError {
            message: format!(
                "Failed to get management service account info for {}",
                platform
            ),
        })?;

        Ok(Some(management_config_from_info(info, platform)?))
    }
}

/// Binding of the management identity a manager used before its current one. An operator that
/// moves the management role keeps the previous role bound under this name: stacks set up
/// earlier trust only that role, and stay reachable through it.
pub const PREVIOUS_MANAGEMENT_BINDING: &str = "previous-management";

/// Impersonate the management identity a deployment's stack trusts: the AWS role its management
/// config names, which is either the current `management` binding or the
/// [`PREVIOUS_MANAGEMENT_BINDING`]. A deployment with no recorded role, or on another platform,
/// uses the current binding. A recorded role neither binding holds is refused before any
/// AssumeRole, so it surfaces as a configuration error rather than an access denial.
pub async fn impersonate_management_sa_trusted_by(
    bindings_provider: &dyn BindingsProviderApi,
    platform: Platform,
    deployment_id: &str,
    management_config: Option<&ManagementConfig>,
) -> Result<ClientConfig, AlienError> {
    let trusted_role = match (management_config, platform) {
        (Some(ManagementConfig::Aws(config)), Platform::Aws) => &config.managing_role_arn,
        _ => return impersonate_management_sa(bindings_provider, platform).await,
    };
    if management_role_arn(bindings_provider, "management", platform)
        .await?
        .as_ref()
        == Some(trusted_role)
    {
        return impersonate_service_account(bindings_provider, "management", platform).await;
    }
    if management_role_arn(bindings_provider, PREVIOUS_MANAGEMENT_BINDING, platform)
        .await?
        .as_ref()
        == Some(trusted_role)
    {
        return impersonate_service_account(
            bindings_provider,
            PREVIOUS_MANAGEMENT_BINDING,
            platform,
        )
        .await;
    }
    Err(AlienError::new(ErrorData::ManagingIdentityNotHeld {
        deployment_id: deployment_id.to_string(),
        managing_identity: trusted_role.clone(),
    })
    .into_generic())
}

/// The AWS role a management binding names, or `None` when the binding is absent.
async fn management_role_arn(
    bindings_provider: &dyn BindingsProviderApi,
    binding: &str,
    platform: Platform,
) -> Result<Option<String>, AlienError> {
    let service_account = match bindings_provider.load_service_account(binding).await {
        Ok(service_account) => service_account,
        Err(error) if error.code == "BINDING_NOT_CONFIGURED" => return Ok(None),
        Err(error) => {
            return Err(error)
                .context(ErrorData::CredentialMaterializationFailed {
                    platform,
                    purpose: format!("load the {binding} service account"),
                })
                .map_err(|error| classify_credential_error(error.into_generic()));
        }
    };
    let info = service_account
        .get_info()
        .await
        .context(ErrorData::CredentialMaterializationFailed {
            platform,
            purpose: format!("read the {binding} service account"),
        })
        .map_err(|error| classify_credential_error(error.into_generic()))?;
    Ok(match info {
        ServiceAccountInfo::Aws(info) => Some(info.role_arn),
        _ => None,
    })
}

/// Impersonate the management service account to get base credentials.
pub async fn impersonate_management_sa(
    bindings_provider: &dyn BindingsProviderApi,
    platform: Platform,
) -> Result<ClientConfig, AlienError> {
    impersonate_service_account(bindings_provider, "management", platform).await
}

async fn impersonate_service_account(
    bindings_provider: &dyn BindingsProviderApi,
    binding: &str,
    platform: Platform,
) -> Result<ClientConfig, AlienError> {
    let service_account = bindings_provider
        .load_service_account(binding)
        .await
        .context(ErrorData::CredentialMaterializationFailed {
            platform,
            purpose: format!("load the {binding} service account"),
        })
        .map_err(|error| classify_credential_error(error.into_generic()))?;

    let impersonation_request = ImpersonationRequest {
        session_name: Some(format!(
            "deployment-management-{}",
            uuid::Uuid::new_v4().simple()
        )),
        duration_seconds: Some(3600),
        scopes: None,
    };

    let client_config = service_account
        .impersonate(impersonation_request)
        .await
        .context(ErrorData::CredentialMaterializationFailed {
            platform,
            purpose: format!("impersonate the {binding} service account"),
        })
        .map_err(|error| classify_credential_error(error.into_generic()))?;

    if client_config.platform() != platform {
        return Err(AlienError::new(GenericError {
            message: format!(
                "Management SA impersonation returned wrong platform: expected {}, got {}",
                platform,
                client_config.platform()
            ),
        }));
    }

    Ok(client_config)
}

/// Derive ManagementConfig from ServiceAccountInfo.
fn management_config_from_info(
    info: ServiceAccountInfo,
    platform: Platform,
) -> Result<ManagementConfig, AlienError> {
    match info {
        ServiceAccountInfo::Aws(aws_info) => {
            Ok(ManagementConfig::Aws(alien_core::AwsManagementConfig {
                managing_role_arn: aws_info.role_arn,
            }))
        }
        ServiceAccountInfo::Gcp(gcp_info) => {
            Ok(ManagementConfig::Gcp(alien_core::GcpManagementConfig {
                federation_trust: super::gcp_federation::federation_trust(
                    Some(&gcp_info.unique_id),
                    &std::env::vars().collect(),
                )
                .map_err(|error| error.into_generic())?,
                service_account_email: gcp_info.email,
            }))
        }
        ServiceAccountInfo::Azure(_azure_info) => {
            let tenant_id = std::env::var("AZURE_TENANT_ID").map_err(|_| {
                AlienError::new(GenericError {
                    message: format!(
                        "AZURE_TENANT_ID required for Azure management config on {}",
                        platform
                    ),
                })
            })?;

            let oidc_issuer = std::env::var("AZURE_MANAGEMENT_OIDC_ISSUER").map_err(|_| {
                AlienError::new(GenericError {
                    message: format!(
                        "AZURE_MANAGEMENT_OIDC_ISSUER required for Azure management config on {}",
                        platform
                    ),
                })
            })?;
            let oidc_subject = std::env::var("AZURE_MANAGEMENT_OIDC_SUBJECT").map_err(|_| {
                AlienError::new(GenericError {
                    message: format!(
                        "AZURE_MANAGEMENT_OIDC_SUBJECT required for Azure management config on {}",
                        platform
                    ),
                })
            })?;
            if oidc_issuer.is_empty() || oidc_subject.is_empty() {
                return Err(AlienError::new(GenericError {
                    message: format!(
                        "Azure management OIDC issuer and subject must be non-empty on {}",
                        platform
                    ),
                }));
            }

            Ok(ManagementConfig::Azure(alien_core::AzureManagementConfig {
                managing_tenant_id: tenant_id,
                oidc_issuer,
                oidc_subject,
            }))
        }
    }
}

pub(crate) fn apply_target_environment(
    client_config: ClientConfig,
    environment_info: Option<&EnvironmentInfo>,
) -> Result<ClientConfig, AlienError> {
    let Some(environment_info) = environment_info else {
        return Ok(client_config);
    };

    match (client_config, environment_info) {
        (ClientConfig::Aws(mut config), EnvironmentInfo::Aws(info)) => {
            config.account_id = info.account_id.clone();
            config.region = info.region.clone();
            Ok(ClientConfig::Aws(config))
        }
        (ClientConfig::Gcp(mut config), EnvironmentInfo::Gcp(info)) => {
            config.project_id = info.project_id.clone();
            config.region = info.region.clone();
            if !info.project_number.is_empty() {
                config.project_number = Some(info.project_number.clone());
            }
            Ok(ClientConfig::Gcp(config))
        }
        (ClientConfig::Azure(mut config), EnvironmentInfo::Azure(info)) => {
            config.subscription_id = info.subscription_id.clone();
            config.region = Some(info.location.clone());
            Ok(ClientConfig::Azure(config))
        }
        (client_config, environment_info) if client_config.platform() == environment_info.platform() => {
            Ok(client_config)
        }
        (client_config, environment_info) => Err(AlienError::new(GenericError {
            message: format!(
                "Deployment environment platform mismatch: credentials are for {}, environment info is for {}",
                client_config.platform(),
                environment_info.platform()
            ),
        })),
    }
}

pub(crate) fn parse_status(status: &str) -> DeploymentStatus {
    serde_json::from_value(serde_json::Value::String(status.to_string()))
        .unwrap_or(DeploymentStatus::Pending)
}

pub(crate) fn uses_direct_management_credentials(status: DeploymentStatus) -> bool {
    matches!(
        status,
        DeploymentStatus::Pending
            | DeploymentStatus::PreflightsFailed
            | DeploymentStatus::InitialSetup
            | DeploymentStatus::InitialSetupFailed
    )
}

fn uses_direct_impersonation_credentials(deployment: &DeploymentRecord) -> bool {
    let status = parse_status(&deployment.status);

    match status {
        DeploymentStatus::Pending | DeploymentStatus::PreflightsFailed => true,
        DeploymentStatus::InitialSetup | DeploymentStatus::InitialSetupFailed => {
            deployment.stack_state.as_ref().is_none_or(|stack_state| {
                !stack_state.resources.values().any(|resource_state| {
                    resource_state.resource_type == "remote-stack-management"
                        && resource_state.outputs.is_some()
                })
            })
        }
        _ => false,
    }
}

fn remote_bindings_session_name(deployment_id: &str, resource_id: &str) -> String {
    format!("deployment-{deployment_id}-{resource_id}")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric()
                || matches!(character, '+' | '=' | ',' | '.' | '@' | '-')
            {
                character
            } else {
                '-'
            }
        })
        .take(64)
        .collect()
}

fn uses_control_plane_credentials(platform: Platform) -> bool {
    matches!(platform, Platform::Machines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_bindings::BindingsProvider;
    use alien_core::{
        bindings::ServiceAccountBinding, AwsClientConfig, AwsCredentials, AwsEnvironmentInfo,
        AzureClientConfig, AzureCredentials, AzureEnvironmentInfo, GcpClientConfig, GcpCredentials,
        GcpEnvironmentInfo, GcpServiceOverrides, RemoteStackManagement,
        RemoteStackManagementOutputs, Resource, ResourceLifecycle, ResourceOutputs, ResourceStatus,
        StackResourceState, StackState,
    };
    use chrono::Utc;

    #[test]
    fn azure_target_environment_overrides_subscription_and_region_but_keeps_managing_tenant() {
        let config = ClientConfig::Azure(Box::new(AzureClientConfig {
            subscription_id: "manager-subscription".to_string(),
            tenant_id: "managing-tenant".to_string(),
            region: Some("westus".to_string()),
            credentials: AzureCredentials::ServicePrincipal {
                client_id: "client".to_string(),
                client_secret: "secret".to_string(),
            },
            service_overrides: None,
        }));

        let environment = EnvironmentInfo::Azure(AzureEnvironmentInfo {
            tenant_id: "target-tenant".to_string(),
            subscription_id: "target-subscription".to_string(),
            location: "eastus".to_string(),
        });

        let resolved = apply_target_environment(config, Some(&environment)).unwrap();
        let azure = resolved.azure_config().unwrap();
        assert_eq!(azure.subscription_id, "target-subscription");
        assert_eq!(azure.tenant_id, "managing-tenant");
        assert_eq!(azure.region.as_deref(), Some("eastus"));
    }

    #[test]
    fn aws_target_environment_overrides_account_and_region() {
        let config = ClientConfig::Aws(Box::new(AwsClientConfig {
            account_id: "manager-account".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "key".to_string(),
                secret_access_key: "secret".to_string(),
                session_token: None,
            },
            service_overrides: None,
        }));

        let environment = EnvironmentInfo::Aws(AwsEnvironmentInfo {
            account_id: "target-account".to_string(),
            region: "us-east-2".to_string(),
        });

        let resolved = apply_target_environment(config, Some(&environment)).unwrap();
        let aws = resolved.aws_config().unwrap();
        assert_eq!(aws.account_id, "target-account");
        assert_eq!(aws.region, "us-east-2");
    }

    #[test]
    fn gcp_target_environment_overrides_project_region_and_project_number() {
        let config = ClientConfig::Gcp(Box::new(GcpClientConfig {
            project_id: "manager-project".to_string(),
            region: "us-central1".to_string(),
            credentials: GcpCredentials::AccessToken {
                token: "token".to_string(),
            },
            service_overrides: None,
            project_number: None,
        }));

        let environment = EnvironmentInfo::Gcp(GcpEnvironmentInfo {
            project_number: "123456789".to_string(),
            project_id: "target-project".to_string(),
            region: "us-east1".to_string(),
        });

        let resolved = apply_target_environment(config, Some(&environment)).unwrap();
        let gcp = resolved.gcp_config().unwrap();
        assert_eq!(gcp.project_id, "target-project");
        assert_eq!(gcp.region, "us-east1");
        assert_eq!(gcp.project_number.as_deref(), Some("123456789"));
    }

    fn gcp_handoff_deployment() -> DeploymentRecord {
        let scope = alien_core::GcpFederationScope {
            deployment_id: "deployment".into(),
        };
        let access = alien_core::GcpFederationAccess {
            project_number: "987654321".into(),
            pool_id: scope
                .pool_id(&RemoteStackManagement::RESOURCE_TYPE)
                .unwrap(),
            provider_id: "manager".into(),
            service_account_email: "deployment@target-project.iam.gserviceaccount.com".into(),
            assertion_audience: scope
                .assertion_audience(&RemoteStackManagement::RESOURCE_TYPE)
                .unwrap(),
            trust: alien_core::GcpFederationTrust {
                issuer: "https://accounts.google.com".into(),
                subject: "123456789012345678901".into(),
            },
        };
        let remote_management = RemoteStackManagement::new("management".to_string()).build();
        let mut stack_state =
            StackState::with_resource_prefix(Platform::Gcp, "test-prefix".to_string());
        stack_state.resources.insert(
            remote_management.id.clone(),
            StackResourceState::builder()
                .resource_type(RemoteStackManagement::RESOURCE_TYPE.to_string())
                .status(ResourceStatus::Running)
                .config(Resource::new(remote_management))
                .outputs(ResourceOutputs::new(RemoteStackManagementOutputs {
                    management_resource_id: "deployment@target-project.iam.gserviceaccount.com"
                        .to_string(),
                    access_configuration: serde_json::to_string(&access).unwrap(),
                    legacy_remote_bindings_access: None,
                }))
                .lifecycle(ResourceLifecycle::Frozen)
                .dependencies(vec![])
                .build(),
        );

        DeploymentRecord {
            supplied_stacks: None,
            id: "deployment".to_string(),
            workspace_id: "default".to_string(),
            project_id: "default".to_string(),
            name: "deployment".to_string(),
            deployment_group_id: "group".to_string(),
            platform: Platform::Gcp,
            deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            base_platform: None,
            status: "provisioning".to_string(),
            stack_settings: None,
            stack_state: Some(stack_state),
            environment_info: Some(EnvironmentInfo::Gcp(GcpEnvironmentInfo {
                project_id: "target-project".to_string(),
                project_number: "987654321".to_string(),
                region: "us-east1".to_string(),
            })),
            runtime_metadata: None,
            current_release_id: None,
            desired_release_id: None,
            configuration_revision: None,
            import_source: None,
            setup_method: None,
            setup_metadata: None,
            setup_target: None,
            setup_fingerprint: None,
            setup_fingerprint_version: None,
            user_environment_variables: None,
            management_config: None,
            deployment_config: None,
            deployment_token: None,
            input_values: Default::default(),
            retry_requested: false,
            locked_by: None,
            locked_at: None,
            created_at: Utc::now(),
            updated_at: None,
            error: None,
        }
    }

    #[tokio::test]
    async fn gcp_materialization_failure_is_classified_for_bounded_handoff_retry() {
        let base_config = ClientConfig::Gcp(Box::new(GcpClientConfig {
            project_id: "managing-project".to_string(),
            region: "us-central1".to_string(),
            credentials: GcpCredentials::AccessToken {
                token: "source-token".to_string(),
            },
            service_overrides: Some(GcpServiceOverrides {
                endpoints: HashMap::from([(
                    "iamcredentials".to_string(),
                    "http://127.0.0.1:9".to_string(),
                )]),
            }),
            project_number: Some("123456789".to_string()),
        }));
        let bindings = HashMap::from([(
            "management".to_string(),
            serde_json::to_value(ServiceAccountBinding::gcp_service_account(
                "management@managing-project.iam.gserviceaccount.com",
                "123456789012345678901",
            ))
            .expect("management binding should serialize"),
        )]);
        let provider: Arc<dyn BindingsProviderApi> = Arc::new(
            BindingsProvider::new(base_config, bindings)
                .expect("GCP management bindings provider should be valid"),
        );
        let resolver = ImpersonationCredentialResolver::new(
            provider.clone(),
            HashMap::from([(Platform::Gcp, provider)]),
            HashSet::from([Platform::Gcp]),
        );

        let error = resolver
            .resolve_from_env(&gcp_handoff_deployment(), HashMap::new())
            .await
            .expect_err("failed target token materialization must fail credential handoff");

        assert_eq!(
            error.code, "REMOTE_CREDENTIAL_HANDOFF_FAILED",
            "the real resolver path must expose the code consumed by the bounded provisioning retry"
        );
    }

    #[tokio::test]
    async fn gcp_credential_handoff_preserves_token_error_retryability() {
        let server = httpmock::MockServer::start_async().await;
        let directory = tempfile::tempdir().unwrap();
        let token_path = directory.path().join("subject-token");
        std::fs::write(&token_path, "synthetic-token").unwrap();
        let base_config = ClientConfig::Gcp(Box::new(GcpClientConfig {
            project_id: "managing-project".to_string(),
            region: "us-central1".to_string(),
            credentials: GcpCredentials::ExternalAccount {
                audience: "synthetic-audience".to_string(),
                subject_token_type: "urn:ietf:params:oauth:token-type:jwt".to_string(),
                token_url: server.url("/token"),
                credential_source_file: token_path.to_string_lossy().to_string(),
                service_account_impersonation_url: None,
            },
            service_overrides: Some(GcpServiceOverrides {
                endpoints: HashMap::from([("iamcredentials".to_string(), server.base_url())]),
            }),
            project_number: Some("123456789".to_string()),
        }));
        let provider: Arc<dyn BindingsProviderApi> = Arc::new(
            BindingsProvider::new(
                base_config,
                HashMap::from([(
                    "management".to_string(),
                    serde_json::to_value(ServiceAccountBinding::gcp_service_account(
                        "management@example.invalid",
                        "123456789012345678901",
                    ))
                    .unwrap(),
                )]),
            )
            .unwrap(),
        );
        let resolver = ImpersonationCredentialResolver::new(
            provider.clone(),
            HashMap::from([(Platform::Gcp, provider)]),
            HashSet::from([Platform::Gcp]),
        );
        for (status, retryable) in [(403, false), (503, true)] {
            let response = server
                .mock_async(|when, then| {
                    when.method(httpmock::Method::POST).path("/token");
                    then.status(status).body("synthetic rejection");
                })
                .await;
            let error = resolver
                .resolve_from_env(&gcp_handoff_deployment(), HashMap::new())
                .await
                .unwrap_err();
            response.assert_hits_async(1).await;
            assert_eq!(error.code, "REMOTE_CREDENTIAL_HANDOFF_FAILED");
            assert_eq!(error.retryable, retryable, "HTTP {status}: {error}");
            assert!(error.to_string().contains(&status.to_string()));
            response.delete_async().await;
        }
    }

    #[tokio::test]
    async fn gcp_assertion_denials_and_missing_setup_preserve_error_classification() {
        let server = httpmock::MockServer::start_async().await;
        let config = ClientConfig::Gcp(Box::new(GcpClientConfig {
            project_id: "managing-project".to_string(),
            region: "us-central1".to_string(),
            credentials: GcpCredentials::AccessToken {
                token: "synthetic-source".to_string(),
            },
            service_overrides: Some(GcpServiceOverrides {
                endpoints: HashMap::from([("iamcredentials".to_string(), server.base_url())]),
            }),
            project_number: Some("123456789".to_string()),
        }));
        let provider: Arc<dyn BindingsProviderApi> = Arc::new(
            BindingsProvider::new(
                config,
                HashMap::from([(
                    "management".to_string(),
                    serde_json::to_value(ServiceAccountBinding::gcp_service_account(
                        "management@example.invalid",
                        "123456789012345678901",
                    ))
                    .unwrap(),
                )]),
            )
            .unwrap(),
        );
        let resolver = ImpersonationCredentialResolver::new(
            provider.clone(),
            HashMap::from([(Platform::Gcp, provider)]),
            HashSet::from([Platform::Gcp]),
        );
        for (status, retryable) in [(403, false), (429, true), (503, true)] {
            let response = server.mock_async(|when, then| {
                when.method(httpmock::Method::POST).path("/projects/-/serviceAccounts/management%40example.invalid:generateIdToken");
                then.status(status).json_body(serde_json::json!({"error": {"code": status, "message": "synthetic target rejection"}}));
            }).await;
            let error = resolver
                .resolve_from_env(&gcp_handoff_deployment(), HashMap::new())
                .await
                .unwrap_err();
            assert_eq!(
                error.retryable, retryable,
                "GCP target HTTP {status}: {error}"
            );
            assert!(response.hits_async().await > 0);
            response.delete_async().await;
        }
        let mut missing = gcp_handoff_deployment();
        missing.stack_state.as_mut().unwrap().resources.clear();
        let error = resolver
            .resolve_from_env(&missing, HashMap::new())
            .await
            .unwrap_err();
        assert!(!error.retryable, "{error}");
        assert!(error.to_string().contains("missing outputs"));
    }

    #[test]
    fn initial_setup_switches_impersonated_credentials_when_remote_management_is_ready() {
        let mut deployment = gcp_handoff_deployment();

        for status in ["pending", "preflights-failed"] {
            deployment.status = status.to_string();
            assert!(uses_direct_impersonation_credentials(&deployment));
        }

        deployment.stack_state = None;
        for status in ["initial-setup", "initial-setup-failed"] {
            deployment.status = status.to_string();
            assert!(uses_direct_impersonation_credentials(&deployment));
        }

        deployment.stack_state = gcp_handoff_deployment().stack_state;
        for status in [
            "initial-setup",
            "initial-setup-failed",
            "provisioning",
            "running",
        ] {
            deployment.status = status.to_string();
            assert!(!uses_direct_impersonation_credentials(&deployment));
        }
    }

    #[tokio::test]
    async fn unconfigured_azure_delegates_to_environment_credentials() {
        let provider: Arc<dyn BindingsProviderApi> = Arc::new(
            BindingsProvider::new(ClientConfig::Test, HashMap::new())
                .expect("empty test provider should be valid"),
        );
        let resolver = ImpersonationCredentialResolver::new(
            provider.clone(),
            HashMap::from([(Platform::Aws, provider)]),
            HashSet::from([Platform::Aws]),
        );

        let resolved = resolver
            .resolve_from_env(
                &super::super::environment_credentials::tests::azure_deployment("initial-setup"),
                super::super::environment_credentials::tests::azure_env(),
            )
            .await
            .expect("unconfigured Azure should use its environment credentials");
        let azure = resolved
            .azure_config()
            .expect("Azure config should resolve");

        assert_eq!(azure.subscription_id, "target-subscription");
        assert!(matches!(
            &azure.credentials,
            AzureCredentials::WorkloadIdentity { client_id, .. }
                if client_id == "management-client"
        ));
    }

    #[test]
    fn machines_uses_control_plane_credentials() {
        assert!(uses_control_plane_credentials(Platform::Machines));

        assert!(!uses_control_plane_credentials(Platform::Aws));
        assert!(!uses_control_plane_credentials(Platform::Gcp));
        assert!(!uses_control_plane_credentials(Platform::Azure));
        assert!(!uses_control_plane_credentials(Platform::Kubernetes));
        assert!(!uses_control_plane_credentials(Platform::Local));
        assert!(!uses_control_plane_credentials(Platform::Test));
    }

    #[tokio::test]
    async fn machines_resolves_without_environment_or_management_bindings() {
        let provider: Arc<dyn BindingsProviderApi> = Arc::new(
            BindingsProvider::new(ClientConfig::Test, HashMap::new())
                .expect("empty test provider should be valid"),
        );
        let resolver =
            ImpersonationCredentialResolver::new(provider, HashMap::new(), HashSet::new());
        let mut deployment = gcp_handoff_deployment();
        deployment.platform = Platform::Machines;
        deployment.stack_state = None;
        deployment.environment_info = None;

        let resolved = resolver
            .resolve_with_capability(&deployment)
            .await
            .expect("Machines should use control-plane credentials");

        assert!(matches!(resolved.client_config, ClientConfig::Machines));
        assert!(resolved.has_provision_capability);
    }

    const AWS_MANAGEMENT_ROLE: &str = "arn:aws:iam::123456789012:role/manager-management";
    const AWS_TARGET_ROLE: &str = "arn:aws:iam::210987654321:role/customer-management";
    const AWS_EXTERNAL_ID: &str = "9c1f7e2a4b6d8f0a1c3e5b7d9f1a3c5e";

    struct FixedExternalId(Option<ManagementExternalId>);

    #[async_trait]
    impl ManagementExternalIdSource for FixedExternalId {
        async fn management_external_id(
            &self,
            _deployment: &DeploymentRecord,
        ) -> Result<Option<ManagementExternalId>, AlienError> {
            Ok(self.0.clone())
        }
    }

    fn aws_assume_role_xml(role: &str, access_key: &str) -> String {
        format!(
            r#"<AssumeRoleResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <AssumeRoleResult>
    <AssumedRoleUser><Arn>{role}/session</Arn><AssumedRoleId>AROA:session</AssumedRoleId></AssumedRoleUser>
    <Credentials>
      <AccessKeyId>{access_key}</AccessKeyId>
      <SecretAccessKey>secret</SecretAccessKey>
      <SessionToken>token</SessionToken>
      <Expiration>2026-05-27T11:00:00Z</Expiration>
    </Credentials>
  </AssumeRoleResult>
  <ResponseMetadata><RequestId>request</RequestId></ResponseMetadata>
</AssumeRoleResponse>"#
        )
    }

    fn aws_running_deployment() -> DeploymentRecord {
        let mut deployment = gcp_handoff_deployment();
        let remote_management = RemoteStackManagement::new("management".to_string()).build();
        let mut stack_state =
            StackState::with_resource_prefix(Platform::Aws, "test-prefix".to_string());
        stack_state.resources.insert(
            remote_management.id.clone(),
            StackResourceState::builder()
                .resource_type(RemoteStackManagement::RESOURCE_TYPE.to_string())
                .status(ResourceStatus::Running)
                .config(Resource::new(remote_management))
                .outputs(ResourceOutputs::new(RemoteStackManagementOutputs {
                    management_resource_id: AWS_TARGET_ROLE.to_string(),
                    access_configuration: AWS_TARGET_ROLE.to_string(),
                    legacy_remote_bindings_access: None,
                }))
                .lifecycle(ResourceLifecycle::Frozen)
                .dependencies(vec![])
                .build(),
        );
        deployment.platform = Platform::Aws;
        deployment.status = "running".to_string();
        deployment.stack_state = Some(stack_state);
        deployment.environment_info = Some(EnvironmentInfo::Aws(AwsEnvironmentInfo {
            account_id: "210987654321".to_string(),
            region: "us-east-2".to_string(),
        }));
        deployment
    }

    fn aws_resolver(
        sts_url: String,
        external_ids: FixedExternalId,
    ) -> ImpersonationCredentialResolver {
        let base_config = ClientConfig::Aws(Box::new(AwsClientConfig {
            account_id: "123456789012".to_string(),
            region: "us-east-2".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "AKIAMANAGER".to_string(),
                secret_access_key: "secret".to_string(),
                session_token: None,
            },
            service_overrides: Some(alien_core::AwsServiceOverrides {
                endpoints: HashMap::from([("sts".to_string(), sts_url)]),
            }),
        }));
        let bindings = HashMap::from([(
            "management".to_string(),
            serde_json::to_value(ServiceAccountBinding::aws_iam(
                "manager-management",
                AWS_MANAGEMENT_ROLE,
            ))
            .expect("management binding should serialize"),
        )]);
        let provider: Arc<dyn BindingsProviderApi> = Arc::new(
            BindingsProvider::new(base_config, bindings)
                .expect("AWS management bindings provider should be valid"),
        );
        ImpersonationCredentialResolver::new(
            provider.clone(),
            HashMap::from([(Platform::Aws, provider)]),
            HashSet::from([Platform::Aws]),
        )
        .with_management_external_ids(Arc::new(external_ids))
    }

    const AWS_PREVIOUS_MANAGEMENT_ROLE: &str =
        "arn:aws:iam::123456789012:role/manager-management-previous";

    /// A resolver holding the current management role and the previous one.
    fn aws_resolver_with_previous_role(sts_url: String) -> ImpersonationCredentialResolver {
        let base_config = ClientConfig::Aws(Box::new(AwsClientConfig {
            account_id: "123456789012".to_string(),
            region: "us-east-2".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "AKIAMANAGER".to_string(),
                secret_access_key: "secret".to_string(),
                session_token: None,
            },
            service_overrides: Some(alien_core::AwsServiceOverrides {
                endpoints: HashMap::from([("sts".to_string(), sts_url)]),
            }),
        }));
        let binding = |name: &str, arn: &str| {
            serde_json::to_value(ServiceAccountBinding::aws_iam(name, arn))
                .expect("management binding should serialize")
        };
        let bindings = HashMap::from([
            (
                "management".to_string(),
                binding("manager-management", AWS_MANAGEMENT_ROLE),
            ),
            (
                PREVIOUS_MANAGEMENT_BINDING.to_string(),
                binding("manager-management-previous", AWS_PREVIOUS_MANAGEMENT_ROLE),
            ),
        ]);
        let provider: Arc<dyn BindingsProviderApi> = Arc::new(
            BindingsProvider::new(base_config, bindings)
                .expect("AWS management bindings provider should be valid"),
        );
        ImpersonationCredentialResolver::new(
            provider.clone(),
            HashMap::from([(Platform::Aws, provider)]),
            HashSet::from([Platform::Aws]),
        )
    }

    fn aws_deployment_trusting(role: Option<&str>) -> DeploymentRecord {
        let mut deployment = aws_running_deployment();
        deployment.management_config = role.map(|role| {
            ManagementConfig::Aws(alien_core::AwsManagementConfig {
                managing_role_arn: role.to_string(),
            })
        });
        deployment
    }

    /// The first hop assumes the managing role the deployment's stack trusts, so a stack set up
    /// before the manager's role moved is still reached through the previous role.
    #[tokio::test]
    async fn a_deployment_is_reached_through_the_managing_role_its_stack_trusts() {
        for (trusted, expected) in [
            (
                Some(AWS_PREVIOUS_MANAGEMENT_ROLE),
                AWS_PREVIOUS_MANAGEMENT_ROLE,
            ),
            (Some(AWS_MANAGEMENT_ROLE), AWS_MANAGEMENT_ROLE),
            (None, AWS_MANAGEMENT_ROLE),
        ] {
            let server = httpmock::MockServer::start_async().await;
            let management = server
                .mock_async(|when, then| {
                    when.method(httpmock::Method::POST)
                        .body_contains(format!("RoleArn={}", urlencoding::encode(expected)));
                    then.status(200)
                        .header("content-type", "text/xml")
                        .body(aws_assume_role_xml(expected, "ASIAMANAGER"));
                })
                .await;
            let customer = server
                .mock_async(|when, then| {
                    when.method(httpmock::Method::POST)
                        .body_contains(format!("RoleArn={}", urlencoding::encode(AWS_TARGET_ROLE)));
                    then.status(200)
                        .header("content-type", "text/xml")
                        .body(aws_assume_role_xml(AWS_TARGET_ROLE, "ASIACUSTOMER"));
                })
                .await;

            aws_resolver_with_previous_role(server.base_url())
                .resolve_from_env(&aws_deployment_trusting(trusted), HashMap::new())
                .await
                .unwrap_or_else(|error| panic!("{trusted:?}: {error:?}"));

            management.assert_async().await;
            customer.assert_async().await;
        }
    }

    /// A previous binding that is present but unusable reports why, rather than reading as a
    /// role the manager does not hold.
    #[tokio::test]
    async fn a_broken_previous_binding_reports_its_own_error() {
        let base_config = ClientConfig::Aws(Box::new(AwsClientConfig {
            account_id: "123456789012".to_string(),
            region: "us-east-2".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "AKIAMANAGER".to_string(),
                secret_access_key: "secret".to_string(),
                session_token: None,
            },
            service_overrides: None,
        }));
        let bindings = HashMap::from([
            (
                "management".to_string(),
                serde_json::to_value(ServiceAccountBinding::aws_iam(
                    "manager-management",
                    AWS_MANAGEMENT_ROLE,
                ))
                .unwrap(),
            ),
            (
                PREVIOUS_MANAGEMENT_BINDING.to_string(),
                serde_json::json!({ "service": "awsiam" }),
            ),
        ]);
        let provider: Arc<dyn BindingsProviderApi> =
            Arc::new(BindingsProvider::new(base_config, bindings).unwrap());

        let error = impersonate_management_sa_trusted_by(
            &*provider,
            Platform::Aws,
            "deployment",
            Some(&ManagementConfig::Aws(alien_core::AwsManagementConfig {
                managing_role_arn: AWS_PREVIOUS_MANAGEMENT_ROLE.to_string(),
            })),
        )
        .await
        .expect_err("the previous binding cannot be parsed");

        assert_eq!(error.code, "CREDENTIAL_MATERIALIZATION_FAILED", "{error:?}");
    }

    /// A stack trusting a role the manager holds no binding for is refused before any AssumeRole.
    #[tokio::test]
    async fn a_managing_role_the_manager_does_not_hold_is_refused() {
        let server = httpmock::MockServer::start_async().await;
        let any = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST);
                then.status(500);
            })
            .await;

        let error = aws_resolver_with_previous_role(server.base_url())
            .resolve_from_env(
                &aws_deployment_trusting(Some("arn:aws:iam::123456789012:role/someone-else")),
                HashMap::new(),
            )
            .await
            .expect_err("no binding holds that role");

        assert_eq!(error.code, "MANAGING_IDENTITY_NOT_HELD");
        assert!(!error.retryable);
        any.assert_hits_async(0).await;
    }

    /// The manager's own management role is assumed without an ExternalId;
    /// the customer's role, the second hop, is assumed with the deployment's.
    #[tokio::test]
    async fn aws_handoff_sends_the_deployment_external_id_to_the_customer_role_only() {
        let server = httpmock::MockServer::start_async().await;
        let management = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .body_contains(format!(
                        "RoleArn={}",
                        urlencoding::encode(AWS_MANAGEMENT_ROLE)
                    ))
                    .matches(|request| {
                        !String::from_utf8_lossy(request.body.as_deref().unwrap_or_default())
                            .contains("ExternalId=")
                    });
                then.status(200)
                    .header("content-type", "text/xml")
                    .body(aws_assume_role_xml(AWS_MANAGEMENT_ROLE, "ASIAMANAGER"));
            })
            .await;
        let customer = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .body_contains(format!("RoleArn={}", urlencoding::encode(AWS_TARGET_ROLE)))
                    .body_contains(format!("ExternalId={AWS_EXTERNAL_ID}"));
                then.status(200)
                    .header("content-type", "text/xml")
                    .body(aws_assume_role_xml(AWS_TARGET_ROLE, "ASIACUSTOMER"));
            })
            .await;
        let resolver = aws_resolver(
            server.base_url(),
            FixedExternalId(Some(ManagementExternalId::new(AWS_EXTERNAL_ID).unwrap())),
        );

        let resolved = resolver
            .resolve_from_env(&aws_running_deployment(), HashMap::new())
            .await
            .expect("the protected customer role accepts the deployment's ExternalId");

        management.assert_async().await;
        customer.assert_async().await;
        let aws = resolved.aws_config().expect("resolved access stays AWS");
        assert_eq!(aws.account_id, "210987654321");
        assert!(matches!(
            &aws.credentials,
            AwsCredentials::SessionCredentials { access_key_id, .. } if access_key_id == "ASIACUSTOMER"
        ));
    }
}
