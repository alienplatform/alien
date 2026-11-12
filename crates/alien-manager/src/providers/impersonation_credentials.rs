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
    CredentialResolver, DeploymentRecord, ExtensionIdentity, ManagementExternalIdSource,
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
    /// The manager's environment, which carries the trust parts of the GCP and Azure management
    /// identities.
    environment: HashMap<String, String>,
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
            environment: std::env::vars().collect(),
        }
    }

    /// Derive management identities from `environment` instead of the process environment.
    pub fn with_environment(mut self, environment: HashMap<String, String>) -> Self {
        self.environment = environment;
        self
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
    async fn aws_external_id(
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
                let external_id = self.aws_external_id(deployment).await?;
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
    async fn management_external_id(
        &self,
        deployment: &DeploymentRecord,
    ) -> Result<Option<ManagementExternalId>, AlienError> {
        self.aws_external_id(deployment).await
    }

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
            let external_id = self.aws_external_id(deployment).await?;
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

        // The platform has an [impersonation] section, so its management binding must load.
        let service_account =
            provider
                .load_service_account("management")
                .await
                .context(GenericError {
                    message: format!("Failed to load the {platform} management binding"),
                })?;

        let info = service_account.get_info().await.context(GenericError {
            message: format!(
                "Failed to get management service account info for {}",
                platform
            ),
        })?;

        Ok(Some(management_config_from_info(info, &self.environment)?))
    }

    async fn resolve_extension_credentials(
        &self,
        deployment: &DeploymentRecord,
        identity: &ExtensionIdentity,
    ) -> Result<alien_infra::step_protocol::protocol::Credentials, AlienError> {
        use super::extension_credentials::{check_identity, mint, MintSource};

        let platform = deployment.platform;
        if !self.management_binding_platforms.contains(&platform) {
            return Err(AlienError::new(GenericError {
                message: format!(
                    "Extension '{}' runs as its own identity, which trusts the manager's management identity, and this manager has none for {platform}",
                    identity.extension
                ),
            }));
        }
        // Before the managing role is assumed for it, or anything else is asked of the cloud.
        check_identity(deployment, identity)?;
        let provider = self.provider_for_target(platform);
        let external_id = self.aws_external_id(deployment).await?;
        let source = match platform {
            Platform::Aws => MintSource::Aws {
                base: impersonate_management_sa_trusted_by(
                    &**provider,
                    platform,
                    &deployment.id,
                    deployment.management_config.as_ref(),
                )
                .await?,
                external_id: external_id.as_ref(),
            },
            Platform::Gcp => MintSource::Gcp {
                source: super::gcp_federation::assertion_source(&**provider, &self.environment)
                    .await?,
            },
            Platform::Azure => MintSource::Azure {
                environment: &self.environment,
                management: self
                    .resolve_from_env(deployment, self.environment.clone())
                    .await?,
            },
            other => {
                return Err(AlienError::new(GenericError {
                    message: format!(
                        "Extension '{}' has no identity on {other}",
                        identity.extension
                    ),
                }))
            }
        };
        mint(deployment, identity, source).await
    }
}

/// Binding of the management identity a manager used before its current one. An operator that
/// moves the management role keeps the previous role bound under this name: stacks set up
/// earlier trust only that role, and stay reachable through it.
pub const PREVIOUS_MANAGEMENT_BINDING: &str = "previous-management";

/// Impersonate the management identity a deployment's stack trusts: the AWS role its management
/// config names, which is either the manager's current identity or the
/// [`PREVIOUS_MANAGEMENT_BINDING`]. Manager stacks bind the current identity as `management`, or
/// as `management-sa` when there is no `management` binding. A deployment with no recorded role,
/// or on another platform, uses the `management` binding. A recorded role no binding holds is
/// refused before any AssumeRole, so it surfaces as a configuration error rather than an access
/// denial.
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
    let (current_binding, current_role) =
        match management_role_arn(bindings_provider, "management", platform).await? {
            Some(role) => ("management", Some(role)),
            None => (
                "management-sa",
                management_role_arn(bindings_provider, "management-sa", platform).await?,
            ),
        };
    if current_role.as_ref() == Some(trusted_role) {
        return impersonate_service_account(bindings_provider, current_binding, platform).await;
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
    environment: &HashMap<String, String>,
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
                    environment,
                )
                .map_err(|error| error.into_generic())?,
                service_account_email: gcp_info.email,
            }))
        }
        // Azure's management identity is the OIDC token the manager presents, so its trust
        // comes from the environment, not from the binding.
        ServiceAccountInfo::Azure(_azure_info) => azure_management_config_from_env(environment),
    }
}

/// The Azure management identity customer setups trust: the managing tenant and the issuer and
/// subject of the OIDC token the manager presents through a federated identity credential.
pub(crate) fn azure_management_config_from_env(
    env: &HashMap<String, String>,
) -> Result<ManagementConfig, AlienError> {
    let missing: Vec<&str> = [
        "AZURE_TENANT_ID",
        "AZURE_MANAGEMENT_OIDC_ISSUER",
        "AZURE_MANAGEMENT_OIDC_SUBJECT",
    ]
    .into_iter()
    .filter(|name| env.get(*name).is_none_or(|value| value.trim().is_empty()))
    .collect();
    if !missing.is_empty() {
        return Err(AlienError::new(GenericError {
            message: format!(
                "[impersonation.azure] needs {} in the manager's environment: Azure setups trust the managing tenant and the issuer and subject of the manager's OIDC token",
                missing.join(", ")
            ),
        }));
    }

    Ok(ManagementConfig::Azure(alien_core::AzureManagementConfig {
        managing_tenant_id: env["AZURE_TENANT_ID"].clone(),
        oidc_issuer: env["AZURE_MANAGEMENT_OIDC_ISSUER"].clone(),
        oidc_subject: env["AZURE_MANAGEMENT_OIDC_SUBJECT"].clone(),
    }))
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

    /// Setups trust exactly the tenant, issuer and subject the manager runs with; a missing or
    /// blank one is named in the error instead of producing a partial identity.
    #[test]
    fn azure_management_identity_comes_from_complete_environment() {
        let env = HashMap::from([
            ("AZURE_TENANT_ID".to_string(), "managing-tenant".to_string()),
            (
                "AZURE_MANAGEMENT_OIDC_ISSUER".to_string(),
                "https://issuer.example.com".to_string(),
            ),
            (
                "AZURE_MANAGEMENT_OIDC_SUBJECT".to_string(),
                "system:serviceaccount:alien:manager".to_string(),
            ),
        ]);
        let ManagementConfig::Azure(config) =
            azure_management_config_from_env(&env).expect("complete environment")
        else {
            panic!("expected an Azure management config");
        };
        assert_eq!(config.managing_tenant_id, "managing-tenant");
        assert_eq!(config.oidc_issuer, "https://issuer.example.com");
        assert_eq!(config.oidc_subject, "system:serviceaccount:alien:manager");

        let mut incomplete = env.clone();
        incomplete.remove("AZURE_TENANT_ID");
        incomplete.insert("AZURE_MANAGEMENT_OIDC_SUBJECT".to_string(), " ".to_string());
        let error = azure_management_config_from_env(&incomplete).expect_err("incomplete");
        assert!(
            error
                .message
                .starts_with("[impersonation.azure] needs AZURE_TENANT_ID, AZURE_MANAGEMENT_OIDC_SUBJECT in the manager's environment:"),
            "{}",
            error.message
        );
        assert!(!error.message.contains("ISSUER"), "{}", error.message);
    }

    /// A platform with an [impersonation] section has a management identity; failing to load
    /// it is an error, never "no management identity".
    #[tokio::test]
    async fn a_configured_management_binding_that_fails_to_load_is_an_error() {
        let provider: Arc<dyn BindingsProviderApi> = Arc::new(
            BindingsProvider::new(ClientConfig::Test, HashMap::new())
                .expect("empty test provider should be valid"),
        );
        let resolver = ImpersonationCredentialResolver::new(
            provider.clone(),
            HashMap::from([(Platform::Azure, provider)]),
            HashSet::from([Platform::Azure]),
        );

        let error = resolver
            .resolve_management_config(Platform::Azure)
            .await
            .expect_err("a missing management binding must not resolve to None");
        assert_eq!(error.message, "Failed to load the azure management binding");
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
        aws_resolver_holding(
            sts_url,
            &[
                ("management", AWS_MANAGEMENT_ROLE),
                (PREVIOUS_MANAGEMENT_BINDING, AWS_PREVIOUS_MANAGEMENT_ROLE),
            ],
        )
    }

    /// A resolver whose manager binds each `(binding, role ARN)` pair.
    fn aws_resolver_holding(
        sts_url: String,
        roles: &[(&str, &str)],
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
        let bindings = roles
            .iter()
            .map(|(binding, arn)| {
                let role_name = arn.rsplit('/').next().expect("role ARN has a name");
                let value = serde_json::to_value(ServiceAccountBinding::aws_iam(role_name, *arn))
                    .expect("management binding should serialize");
                (binding.to_string(), value)
            })
            .collect::<HashMap<_, _>>();
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

    /// A manager stack that binds its current role as `management-sa`, with no `management`
    /// binding, reaches the deployments that trust that role.
    #[tokio::test]
    async fn a_current_role_bound_as_management_sa_reaches_deployments_trusting_it() {
        let server = httpmock::MockServer::start_async().await;
        let management = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST).body_contains(format!(
                    "RoleArn={}",
                    urlencoding::encode(AWS_MANAGEMENT_ROLE)
                ));
                then.status(200)
                    .header("content-type", "text/xml")
                    .body(aws_assume_role_xml(AWS_MANAGEMENT_ROLE, "ASIAMANAGER"));
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

        aws_resolver_holding(server.base_url(), &[("management-sa", AWS_MANAGEMENT_ROLE)])
            .resolve_from_env(
                &aws_deployment_trusting(Some(AWS_MANAGEMENT_ROLE)),
                HashMap::new(),
            )
            .await
            .expect("the role the stack trusts is the manager's current identity");

        management.assert_async().await;
        customer.assert_async().await;
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

    use crate::traits::ExtensionIdentity;
    use alien_infra::step_protocol::protocol::Credentials;

    const AWS_EXTENSION_ROLE: &str = "arn:aws:iam::210987654321:role/test-prefix-acme-ext";

    fn acme(identity: &str) -> ExtensionIdentity {
        ExtensionIdentity {
            extension: "acme".to_string(),
            id: "acme-ext".to_string(),
            identity: identity.to_string(),
        }
    }

    /// The service account setup creates for acme in [`gcp_handoff_deployment`]'s project.
    fn acme_gcp_account() -> String {
        format!(
            "{}@target-project.iam.gserviceaccount.com",
            alien_core::extension_identity_gcp_account_id("test-prefix", "acme-ext")
        )
    }

    /// The extension's role is assumed from the managing role, which its trust names, for STS's
    /// 900-second minimum and with the deployment's ExternalId. The customer's management role
    /// takes no part.
    #[tokio::test]
    async fn an_extension_role_is_assumed_from_the_managing_role_with_the_external_id() {
        let server = httpmock::MockServer::start_async().await;
        let managing = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST).body_contains(format!(
                    "RoleArn={}",
                    urlencoding::encode(AWS_MANAGEMENT_ROLE)
                ));
                then.status(200)
                    .header("content-type", "text/xml")
                    .body(aws_assume_role_xml(AWS_MANAGEMENT_ROLE, "ASIAMANAGER"));
            })
            .await;
        let extension = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .body_contains(format!(
                        "RoleArn={}",
                        urlencoding::encode(AWS_EXTENSION_ROLE)
                    ))
                    .body_contains("DurationSeconds=900")
                    .body_contains(format!("ExternalId={AWS_EXTERNAL_ID}"));
                then.status(200)
                    .header("content-type", "text/xml")
                    .body(aws_assume_role_xml(AWS_EXTENSION_ROLE, "ASIAEXTENSION"));
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

        let credentials = aws_resolver(
            server.base_url(),
            FixedExternalId(Some(ManagementExternalId::new(AWS_EXTERNAL_ID).unwrap())),
        )
        .resolve_extension_credentials(&aws_running_deployment(), &acme(AWS_EXTENSION_ROLE))
        .await
        .unwrap();

        managing.assert_async().await;
        extension.assert_async().await;
        customer.assert_hits_async(0).await;
        assert_eq!(
            credentials,
            Credentials::Aws {
                account_id: "210987654321".to_string(),
                region: "us-east-2".to_string(),
                access_key_id: "ASIAEXTENSION".to_string(),
                secret_access_key: "secret".to_string(),
                session_token: Some("token".to_string()),
                expires_at: Some("2026-05-27T11:00:00Z".to_string()),
            }
        );
    }

    /// A role ARN in the deployment's state names a role in the deployment's own account, or
    /// nothing is assumed, not even the managing role.
    #[tokio::test]
    async fn an_extension_role_in_another_account_is_never_assumed() {
        const ELSEWHERE: &str = "arn:aws:iam::999999999999:role/test-prefix-acme-ext";
        let server = httpmock::MockServer::start_async().await;
        let managing = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST).body_contains(format!(
                    "RoleArn={}",
                    urlencoding::encode(AWS_MANAGEMENT_ROLE)
                ));
                then.status(200)
                    .header("content-type", "text/xml")
                    .body(aws_assume_role_xml(AWS_MANAGEMENT_ROLE, "ASIAMANAGER"));
            })
            .await;
        let elsewhere = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .body_contains(format!("RoleArn={}", urlencoding::encode(ELSEWHERE)));
                then.status(200)
                    .header("content-type", "text/xml")
                    .body(aws_assume_role_xml(ELSEWHERE, "ASIAELSEWHERE"));
            })
            .await;

        let error = aws_resolver(server.base_url(), FixedExternalId(None))
            .resolve_extension_credentials(&aws_running_deployment(), &acme(ELSEWHERE))
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("not in the deployment's account"),
            "{error}"
        );
        managing.assert_hits_async(0).await;
        elsewhere.assert_hits_async(0).await;
    }

    /// The extension's service account is impersonated with the management pool's federated
    /// token, which its workload identity binding trusts, for at most an hour.
    #[tokio::test]
    async fn an_extension_service_account_is_impersonated_through_the_management_pool() {
        let extension_sa = acme_gcp_account();
        let extension_sa = extension_sa.as_str();
        let server = httpmock::MockServer::start_async().await;
        let assertion = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST).path_contains(
                    "management%40managing-project.iam.gserviceaccount.com:generateIdToken",
                );
                then.status(200)
                    .json_body(serde_json::json!({ "token": "manager-assertion" }));
            })
            .await;
        let exchange = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/v1/token")
                    .body_contains("subject_token=manager-assertion");
                then.status(200).json_body(serde_json::json!({
                    "access_token": "federated-token",
                    "expires_in": 3600,
                    "token_type": "Bearer",
                    "issued_token_type": "urn:ietf:params:oauth:token-type:access_token"
                }));
            })
            .await;
        let expires = (Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        let impersonation = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path_contains(format!(
                        "{}:generateAccessToken",
                        urlencoding::encode(extension_sa)
                    ))
                    .header("authorization", "Bearer federated-token")
                    .body_contains("3600s");
                then.status(200).json_body(serde_json::json!({
                    "accessToken": "extension-token",
                    "expireTime": expires
                }));
            })
            .await;
        let base_config = ClientConfig::Gcp(Box::new(GcpClientConfig {
            project_id: "managing-project".to_string(),
            region: "us-central1".to_string(),
            credentials: GcpCredentials::AccessToken {
                token: "source-token".to_string(),
            },
            service_overrides: Some(GcpServiceOverrides {
                endpoints: HashMap::from([
                    ("iamcredentials".to_string(), server.url("/v1/")),
                    ("sts".to_string(), server.url("/v1/token")),
                ]),
            }),
            project_number: Some("123456789".to_string()),
        }));
        let provider: Arc<dyn BindingsProviderApi> = Arc::new(
            BindingsProvider::new(
                base_config,
                HashMap::from([(
                    "management".to_string(),
                    serde_json::to_value(ServiceAccountBinding::gcp_service_account(
                        "management@managing-project.iam.gserviceaccount.com",
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
        )
        .with_environment(HashMap::new());

        let credentials = resolver
            .resolve_extension_credentials(&gcp_handoff_deployment(), &acme(extension_sa))
            .await
            .unwrap_or_else(|error| panic!("{error:?}"));

        assertion.assert_async().await;
        exchange.assert_async().await;
        impersonation.assert_async().await;
        let Credentials::Gcp {
            project_id,
            access_token,
            expires_at,
            ..
        } = credentials
        else {
            panic!("{credentials:?}");
        };
        assert_eq!(project_id, "target-project");
        assert_eq!(access_token, "extension-token");
        assert_eq!(
            chrono::DateTime::parse_from_rfc3339(&expires_at).unwrap(),
            chrono::DateTime::parse_from_rfc3339(&expires).unwrap()
        );
    }

    /// A manager without the management identity for a deployment's cloud mints nothing, so
    /// the step fails instead of running with other credentials.
    #[tokio::test]
    async fn a_cloud_without_a_management_identity_mints_nothing() {
        let error = aws_resolver("http://127.0.0.1:9".to_string(), FixedExternalId(None))
            .resolve_extension_credentials(&gcp_handoff_deployment(), &acme("sa@x"))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("has none for gcp"), "{error}");
    }

    /// A deployment's AWS, GCP and Azure resolvers whose every cloud call goes to `server`.
    fn resolvers(server: &httpmock::MockServer) -> [ImpersonationCredentialResolver; 3] {
        let gcp_config = ClientConfig::Gcp(Box::new(GcpClientConfig {
            project_id: "managing-project".to_string(),
            region: "us-central1".to_string(),
            credentials: GcpCredentials::AccessToken {
                token: "source-token".to_string(),
            },
            service_overrides: Some(GcpServiceOverrides {
                endpoints: HashMap::from([
                    ("iamcredentials".to_string(), server.url("/v1/")),
                    ("sts".to_string(), server.url("/v1/token")),
                ]),
            }),
            project_number: Some("123456789".to_string()),
        }));
        let gcp: Arc<dyn BindingsProviderApi> = Arc::new(
            BindingsProvider::new(
                gcp_config,
                HashMap::from([(
                    "management".to_string(),
                    serde_json::to_value(ServiceAccountBinding::gcp_service_account(
                        "management@managing-project.iam.gserviceaccount.com",
                        "123456789012345678901",
                    ))
                    .unwrap(),
                )]),
            )
            .unwrap(),
        );
        let azure: Arc<dyn BindingsProviderApi> =
            Arc::new(BindingsProvider::new(ClientConfig::Test, HashMap::new()).unwrap());
        [
            aws_resolver(server.base_url(), FixedExternalId(None)),
            ImpersonationCredentialResolver::new(
                gcp.clone(),
                HashMap::from([(Platform::Gcp, gcp)]),
                HashSet::from([Platform::Gcp]),
            )
            .with_environment(HashMap::new()),
            ImpersonationCredentialResolver::new(
                azure.clone(),
                HashMap::from([(Platform::Azure, azure)]),
                HashSet::from([Platform::Azure]),
            )
            .with_environment(HashMap::from([
                (
                    "AZURE_FEDERATED_TOKEN_FILE".to_string(),
                    "/nonexistent/token".to_string(),
                ),
                ("AZURE_AUTHORITY_HOST".to_string(), server.base_url()),
            ])),
        ]
    }

    /// An Azure deployment in tenant `customer-tenant` whose management access names `tenant`.
    fn azure_deployment(tenant: &str) -> DeploymentRecord {
        let mut deployment = gcp_handoff_deployment();
        deployment.platform = Platform::Azure;
        deployment.environment_info = Some(EnvironmentInfo::Azure(AzureEnvironmentInfo {
            tenant_id: "customer-tenant".to_string(),
            subscription_id: "customer-subscription".to_string(),
            location: "eastus".to_string(),
        }));
        deployment
            .stack_state
            .as_mut()
            .unwrap()
            .resources
            .get_mut("management")
            .unwrap()
            .outputs = Some(ResourceOutputs::new(RemoteStackManagementOutputs {
            management_resource_id: "management-identity".to_string(),
            access_configuration: serde_json::json!({
                "uamiClientId": "management-client-id",
                "tenantId": tenant
            })
            .to_string(),
            legacy_remote_bindings_access: None,
        }));
        deployment
    }

    /// Refuses `identity` in `deployment` through `resolver`, with nothing asked of any cloud.
    async fn refused_without_a_call(
        server: &httpmock::MockServer,
        resolver: &ImpersonationCredentialResolver,
        deployment: &DeploymentRecord,
        identity: &str,
    ) -> String {
        let any = server
            .mock_async(|when, then| {
                when.any_request();
                then.status(500);
            })
            .await;
        let error = resolver
            .resolve_extension_credentials(deployment, &acme(identity))
            .await
            .expect_err(identity)
            .to_string();
        any.assert_hits_async(0).await;
        any.delete_async().await;
        error
    }

    /// An identity whose outputs name the deployment's management role, service account or
    /// managed identity is refused on every cloud before anything is assumed, so management
    /// credentials never reach a controller.
    #[tokio::test]
    async fn the_management_identity_is_never_minted_for_an_extension() {
        let server = httpmock::MockServer::start_async().await;
        let [aws, gcp, azure] = resolvers(&server);
        for (resolver, deployment, identity) in [
            (&aws, aws_running_deployment(), AWS_TARGET_ROLE),
            (
                &gcp,
                gcp_handoff_deployment(),
                "deployment@target-project.iam.gserviceaccount.com",
            ),
            (
                &azure,
                azure_deployment("customer-tenant"),
                "management-client-id",
            ),
        ] {
            let error = refused_without_a_call(&server, resolver, &deployment, identity).await;
            assert!(
                error.contains("is the deployment's management identity"),
                "{identity}: {error}"
            );
        }
    }

    /// A role or service account other than the one setup names after the deployment's prefix
    /// is never assumed, even in the deployment's own account or project.
    #[tokio::test]
    async fn an_identity_setup_did_not_name_is_never_minted() {
        let server = httpmock::MockServer::start_async().await;
        let [aws, gcp, _] = resolvers(&server);
        let error = refused_without_a_call(
            &server,
            &aws,
            &aws_running_deployment(),
            "arn:aws:iam::210987654321:role/admin",
        )
        .await;
        assert!(
            error.contains("not the role setup creates for it"),
            "{error}"
        );
        let error = refused_without_a_call(
            &server,
            &gcp,
            &gcp_handoff_deployment(),
            "owner@target-project.iam.gserviceaccount.com",
        )
        .await;
        assert!(
            error.contains("not the one setup creates for it"),
            "{error}"
        );
    }

    /// A service account in another project, or a deployment whose project number isn't the
    /// management access's, is never impersonated.
    #[tokio::test]
    async fn an_extension_service_account_in_another_project_is_never_impersonated() {
        let server = httpmock::MockServer::start_async().await;
        let [_, gcp, _] = resolvers(&server);
        let elsewhere = acme_gcp_account().replace("@target-project.", "@other-project.");
        let error =
            refused_without_a_call(&server, &gcp, &gcp_handoff_deployment(), &elsewhere).await;
        assert!(error.contains("not in the deployment's project"), "{error}");

        let mut renumbered = gcp_handoff_deployment();
        if let Some(EnvironmentInfo::Gcp(environment)) = &mut renumbered.environment_info {
            environment.project_number = "111111111".to_string();
        }
        let error = refused_without_a_call(&server, &gcp, &renumbered, &acme_gcp_account()).await;
        assert!(error.contains("not in the deployment's project"), "{error}");
    }

    /// A management access in another tenant than the deployment's sends no token request, and
    /// the refusal names both tenants.
    #[tokio::test]
    async fn an_azure_identity_in_another_tenant_is_never_minted() {
        let server = httpmock::MockServer::start_async().await;
        let [_, _, azure] = resolvers(&server);
        let error = refused_without_a_call(
            &server,
            &azure,
            &azure_deployment("other-tenant"),
            "extension-client-id",
        )
        .await;
        assert!(
            error.contains("other-tenant") && error.contains("customer-tenant"),
            "{error}"
        );
    }

    const AZURE_TENANT: &str = "6c1b1a8e-1f2b-4a5e-9d3c-0a1b2c3d4e5f";
    const ACME_CLIENT_ID: &str = "0a7d6c8e-3b1f-4c2a-8e9d-111111111111";
    const OTHER_MANAGEMENT_CLIENT_ID: &str = "0a7d6c8e-3b1f-4c2a-8e9d-222222222222";
    const BILLING_CLIENT_ID: &str = "0a7d6c8e-3b1f-4c2a-8e9d-333333333333";

    /// An Azure deployment with prefix `test-prefix`, resource group `rg-a` and its management
    /// identity, in tenant [`AZURE_TENANT`].
    fn azure_pinned_deployment() -> DeploymentRecord {
        let mut deployment = azure_deployment(AZURE_TENANT);
        deployment.environment_info = Some(EnvironmentInfo::Azure(AzureEnvironmentInfo {
            tenant_id: AZURE_TENANT.to_string(),
            subscription_id: "customer-subscription".to_string(),
            location: "eastus".to_string(),
        }));
        let group =
            alien_core::AzureResourceGroup::new("default-resource-group".to_string()).build();
        deployment.stack_state.as_mut().unwrap().resources.insert(
            "default-resource-group".to_string(),
            StackResourceState::builder()
                .resource_type(alien_core::AzureResourceGroup::RESOURCE_TYPE.to_string())
                .status(ResourceStatus::Running)
                .config(Resource::new(group))
                .outputs(ResourceOutputs::new(
                    alien_core::AzureResourceGroupOutputs {
                        name: "rg-a".to_string(),
                        resource_id: "/subscriptions/customer-subscription/resourceGroups/rg-a"
                            .to_string(),
                        location: "eastus".to_string(),
                    },
                ))
                .lifecycle(ResourceLifecycle::Frozen)
                .dependencies(vec![])
                .build(),
        );
        deployment
    }

    /// Mints `identity` for [`azure_pinned_deployment`], where ARM says acme's managed identity
    /// `test-prefix-acme-ext` has client id [`ACME_CLIENT_ID`]. Returns the result and how many
    /// token requests reached the authority host.
    async fn mint_pinned_azure(identity: &str) -> (Result<Credentials, AlienError>, usize) {
        mint_pinned_azure_in(identity, AZURE_TENANT, 200).await
    }

    /// [`mint_pinned_azure`], where ARM puts the identity in `arm_tenant`.
    async fn mint_pinned_azure_in(
        identity: &str,
        arm_tenant: &str,
        arm_status: u16,
    ) -> (Result<Credentials, AlienError>, usize) {
        mint_pinned_azure_after(identity, arm_tenant, arm_status, std::time::Duration::ZERO).await
    }

    /// [`mint_pinned_azure_in`], where ARM answers after `arm_delay`.
    async fn mint_pinned_azure_after(
        identity: &str,
        arm_tenant: &str,
        arm_status: u16,
        arm_delay: std::time::Duration,
    ) -> (Result<Credentials, AlienError>, usize) {
        use base64::Engine as _;
        let server = httpmock::MockServer::start_async().await;
        let arm = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/subscriptions/customer-subscription/resourceGroups/rg-a/providers/Microsoft.ManagedIdentity/userAssignedIdentities/test-prefix-acme-ext")
                    .header("authorization", "Bearer management-token");
                let then = then.delay(arm_delay);
                if arm_status != 200 {
                    then.status(arm_status)
                        .json_body(serde_json::json!({ "error": { "code": "Failed", "message": "synthetic" } }));
                    return;
                }
                then.status(200).json_body(serde_json::json!({
                    "id": "/subscriptions/customer-subscription/resourceGroups/rg-a/providers/Microsoft.ManagedIdentity/userAssignedIdentities/test-prefix-acme-ext",
                    "name": "test-prefix-acme-ext",
                    "location": "eastus",
                    "properties": {
                        "clientId": ACME_CLIENT_ID,
                        "principalId": "9f9f9f9f-0000-4000-8000-000000000001",
                        "tenantId": arm_tenant
                    }
                }));
            })
            .await;
        let exp = (Utc::now() + chrono::Duration::minutes(75)).timestamp();
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::json!({ "exp": exp }).to_string());
        let token = format!("header.{payload}.signature");
        let exchange = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path(format!("/{AZURE_TENANT}/oauth2/v2.0/token"))
                    .body_contains("client_assertion=manager-federated-token");
                then.status(200).json_body(serde_json::json!({
                    "access_token": token,
                    "expires_in": 4500,
                    "token_type": "Bearer"
                }));
            })
            .await;
        let directory = tempfile::tempdir().unwrap();
        let token_file = directory.path().join("token");
        std::fs::write(&token_file, "manager-federated-token").unwrap();
        let environment = HashMap::from([
            (
                "AZURE_FEDERATED_TOKEN_FILE".to_string(),
                token_file.to_string_lossy().to_string(),
            ),
            ("AZURE_AUTHORITY_HOST".to_string(), server.base_url()),
        ]);
        let management = ClientConfig::Azure(Box::new(AzureClientConfig {
            subscription_id: "customer-subscription".to_string(),
            tenant_id: AZURE_TENANT.to_string(),
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::AccessToken {
                token: "management-token".to_string(),
            },
            service_overrides: Some(alien_core::AzureServiceOverrides {
                endpoints: HashMap::from([("management".to_string(), server.base_url())]),
            }),
        }));
        let minted = super::super::extension_credentials::mint(
            &azure_pinned_deployment(),
            &acme(identity),
            super::super::extension_credentials::MintSource::Azure {
                environment: &environment,
                management,
            },
        )
        .await;
        assert!(arm.hits_async().await >= 1, "ARM is asked");
        (minted, exchange.hits_async().await)
    }

    /// The managed identity ARM names `test-prefix-acme-ext`, read as the deployment's
    /// management identity, is the one minted for, by exchanging the manager's federated token.
    #[tokio::test]
    async fn the_managed_identity_arm_names_for_the_extension_is_minted() {
        let (minted, exchanges) = mint_pinned_azure(ACME_CLIENT_ID).await;
        let Credentials::Azure {
            subscription_id,
            tenant_id,
            expires_at,
            ..
        } = minted.unwrap_or_else(|error| panic!("{error:?}"))
        else {
            panic!("Azure credentials")
        };
        assert_eq!(exchanges, 1);
        assert_eq!(subscription_id, "customer-subscription");
        assert_eq!(tenant_id, AZURE_TENANT);
        assert!(chrono::DateTime::parse_from_rfc3339(&expires_at).is_ok());
    }

    /// Another deployment's management identity, which trusts the same federated token in the
    /// tenant, is refused: ARM says acme's identity has another client id, and no token is
    /// requested.
    #[tokio::test]
    async fn another_deployments_management_identity_is_never_minted_on_azure() {
        let (minted, exchanges) = mint_pinned_azure(OTHER_MANAGEMENT_CLIENT_ID).await;
        let error = minted.expect_err("refused").to_string();
        assert!(
            error.contains("is not the client id of its managed identity"),
            "{error}"
        );
        assert_eq!(exchanges, 0);
    }

    /// Another extension's identity in the same deployment is refused the same way.
    #[tokio::test]
    async fn another_extensions_identity_is_never_minted_on_azure() {
        let (minted, exchanges) = mint_pinned_azure(BILLING_CLIENT_ID).await;
        let error = minted.expect_err("refused").to_string();
        assert!(
            error.contains("is not the client id of its managed identity"),
            "{error}"
        );
        assert_eq!(exchanges, 0);
    }

    /// An identity ARM places in another tenant than the deployment's is refused, naming both.
    #[tokio::test]
    async fn an_identity_arm_places_in_another_tenant_is_never_minted() {
        let (minted, exchanges) =
            mint_pinned_azure_in(ACME_CLIENT_ID, "7d2c2b9f-2a3c-4b6f-8e4d-1b2c3d4e5f60", 200).await;
        let error = minted.expect_err("refused").to_string();
        assert!(
            error.contains("7d2c2b9f-2a3c-4b6f-8e4d-1b2c3d4e5f60") && error.contains(AZURE_TENANT),
            "{error}"
        );
        assert_eq!(exchanges, 0);
    }

    /// ARM throttling or an ARM outage fails the step for a retry; ARM refusing the read, or
    /// having no such identity, fails it for good. No token is requested either way.
    #[tokio::test]
    async fn an_arm_throttle_or_outage_is_retried_and_a_refusal_is_not() {
        for (status, retryable) in [(429, true), (503, true), (403, false), (404, false)] {
            let (minted, exchanges) =
                mint_pinned_azure_in(ACME_CLIENT_ID, AZURE_TENANT, status).await;
            let error = minted.expect_err("the read failed");
            assert_eq!(error.retryable, retryable, "ARM {status}: {error}");
            assert_eq!(exchanges, 0, "ARM {status}");
        }
    }

    /// An ARM read that stalls fails the step for a retry once the read's limit passes, retries
    /// included, before the step's own deadline.
    #[tokio::test]
    async fn a_stalled_arm_read_times_out() {
        let started = std::time::Instant::now();
        let (minted, exchanges) = mint_pinned_azure_after(
            ACME_CLIENT_ID,
            AZURE_TENANT,
            200,
            std::time::Duration::from_secs(60),
        )
        .await;
        let error = minted.expect_err("the read timed out");
        assert!(error.retryable, "a stalled read is retried: {error}");
        assert_eq!(exchanges, 0);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(55),
            "{:?}",
            started.elapsed()
        );
    }
}
