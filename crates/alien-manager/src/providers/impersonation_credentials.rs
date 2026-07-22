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
    ClientConfig, DeploymentStatus, EnvironmentInfo, ManagementConfig, Platform,
    RemoteStackManagement, RemoteStackManagementOutputs,
};
use alien_error::{AlienError, Context, GenericError, IntoAlienError};
use alien_permissions::{
    generators::GcpRuntimePermissionsGenerator, get_permission_set, PermissionContext,
};
use async_trait::async_trait;

use crate::error::ErrorData;
use crate::traits::{
    CredentialResolver, DeploymentRecord, GcpCredentialAccessBoundarySource,
    RemoteStorageCredentialSource, ResolvedCredentials,
};

const GCP_REMOTE_STORAGE_PERMISSIONS: [&str; 4] = [
    "storage.objects.create",
    "storage.objects.delete",
    "storage.objects.get",
    "storage.objects.list",
];

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
            let base_config = impersonate_management_sa(&**provider, platform).await?;
            return apply_target_environment(base_config, deployment.environment_info.as_ref());
        }

        // After initial setup, resolve via the remote stack management identity
        // in the stack state (two-hop: management SA → target identity).
        if let Some(ref stack_state) = deployment.stack_state {
            let provider = self.provider_for_target(platform);
            let base_config = impersonate_management_sa(&**provider, platform).await?;

            let resolver = alien_infra::RemoteAccessResolver::new(env);
            let resolved = resolver
                .resolve(
                    base_config,
                    stack_state,
                    deployment.environment_info.as_ref(),
                )
                .await
                .context(ErrorData::RemoteCredentialHandoffFailed {
                    deployment_id: deployment.id.clone(),
                    platform,
                })
                .map_err(AlienError::into_generic)?;

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
    ) -> Result<RemoteStorageCredentialSource, AlienError> {
        if deployment.platform == Platform::Gcp
            && self.management_binding_platforms.contains(&Platform::Gcp)
            && !uses_direct_impersonation_credentials(deployment)
        {
            let stack_state = deployment.stack_state.as_ref().ok_or_else(|| {
                AlienError::new(GenericError {
                    message: format!(
                        "Remote stack state is required to attenuate GCP Storage credentials for deployment {}",
                        deployment.id
                    ),
                })
            })?;
            let provider = self.provider_for_target(Platform::Gcp);
            let base_config = impersonate_management_sa(&**provider, Platform::Gcp).await?;
            let resolved = alien_infra::RemoteAccessResolver::default()
                .resolve_gcp_for_access_boundary(
                    base_config,
                    stack_state,
                    deployment.environment_info.as_ref(),
                )
                .await
                .context(ErrorData::RemoteCredentialHandoffFailed {
                    deployment_id: deployment.id.clone(),
                    platform: Platform::Gcp,
                })
                .map_err(AlienError::into_generic)?;
            let ClientConfig::Gcp(source) = resolved else {
                return Err(AlienError::new(GenericError {
                    message: "GCP remote Storage credential handoff returned a non-GCP config"
                        .to_string(),
                }));
            };
            return Ok(RemoteStorageCredentialSource::GcpCredentialAccessBoundary(
                GcpCredentialAccessBoundarySource {
                    source,
                    available_role: gcp_remote_storage_access_boundary_role(deployment)?,
                },
            ));
        }

        if deployment.platform != Platform::Aws
            || !self.management_binding_platforms.contains(&Platform::Aws)
            || uses_direct_impersonation_credentials(deployment)
        {
            return Ok(RemoteStorageCredentialSource::Direct(
                self.resolve(deployment).await?,
            ));
        }

        let stack_state = deployment.stack_state.as_ref().ok_or_else(|| {
            AlienError::new(GenericError {
                message: format!(
                    "Remote stack state is required to attenuate AWS Storage credentials for deployment {}",
                    deployment.id
                ),
            })
        })?;
        let outputs = stack_state
            .resources
            .values()
            .find(|resource| {
                resource.resource_type == RemoteStackManagement::RESOURCE_TYPE.as_ref()
            })
            .and_then(|resource| resource.outputs.as_ref())
            .and_then(|outputs| outputs.downcast_ref::<RemoteStackManagementOutputs>())
            .ok_or_else(|| {
                AlienError::new(GenericError {
                    message: format!(
                        "Remote stack management outputs are required to attenuate AWS Storage credentials for deployment {}",
                        deployment.id
                    ),
                })
            })?;
        let provider = self.provider_for_target(Platform::Aws);
        let base = impersonate_management_sa(&**provider, Platform::Aws).await?;
        let ClientConfig::Aws(source) = base else {
            return Err(AlienError::new(GenericError {
                message: "AWS management service-account impersonation returned a non-AWS config"
                    .to_string(),
            }));
        };
        let (target_account_id, target_region) = if let Some(EnvironmentInfo::Aws(environment)) =
            &deployment.environment_info
        {
            (environment.account_id.clone(), environment.region.clone())
        } else {
            let account_id = outputs
                    .access_configuration
                    .split(':')
                    .nth(4)
                    .filter(|account| account.len() == 12 && account.bytes().all(|b| b.is_ascii_digit()))
                    .ok_or_else(|| {
                        AlienError::new(GenericError {
                            message: "AWS target account cannot be proven from deployment environment or remote role ARN".to_string(),
                        })
                    })?
                    .to_string();
            (account_id, source.region.clone())
        };

        Ok(RemoteStorageCredentialSource::AwsAssumeRole {
            source,
            role_arn: outputs.access_configuration.clone(),
            role_session_name: format!("alien-remote-storage-{}", uuid::Uuid::new_v4().simple()),
            target_account_id,
            target_region,
        })
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

        let info = service_account
            .get_info()
            .await
            .into_alien_error()
            .context(GenericError {
                message: format!(
                    "Failed to get management service account info for {}",
                    platform
                ),
            })?;

        Ok(Some(management_config_from_info(info, platform)?))
    }
}

fn gcp_remote_storage_access_boundary_role(
    deployment: &DeploymentRecord,
) -> Result<String, AlienError> {
    let stack_state = deployment.stack_state.as_ref().ok_or_else(|| {
        AlienError::new(GenericError {
            message: format!(
                "Remote stack state is required to attenuate GCP Storage credentials for deployment {}",
                deployment.id
            ),
        })
    })?;
    let EnvironmentInfo::Gcp(environment) = deployment.environment_info.as_ref().ok_or_else(|| {
        AlienError::new(GenericError {
            message: format!(
                "GCP environment identity is required to attenuate Storage credentials for deployment {}",
                deployment.id
            ),
        })
    })? else {
        return Err(AlienError::new(GenericError {
            message: format!(
                "GCP deployment {} has non-GCP environment identity",
                deployment.id
            ),
        }));
    };
    let permission_set = get_permission_set("storage/remote-data-write").ok_or_else(|| {
        AlienError::new(GenericError {
            message: "GCP remote Storage permission set is unavailable".to_string(),
        })
    })?;
    let context = PermissionContext::new()
        .with_stack_prefix(stack_state.resource_prefix.clone())
        .with_project_name(environment.project_id.clone());
    let mut roles = GcpRuntimePermissionsGenerator::new()
        .generate_custom_roles(permission_set, &context)
        .map_err(AlienError::into_generic)?;
    if roles.len() != 1 || roles[0].included_permissions != GCP_REMOTE_STORAGE_PERMISSIONS {
        return Err(AlienError::new(GenericError {
            message: format!(
                "GCP remote Storage permission set must generate one exact least-privilege custom role; generated {} role(s)",
                roles.len(),
            ),
        }));
    }
    Ok(roles.remove(0).name)
}

/// Impersonate the management service account to get base credentials.
pub async fn impersonate_management_sa(
    bindings_provider: &dyn BindingsProviderApi,
    platform: Platform,
) -> Result<ClientConfig, AlienError> {
    let service_account = bindings_provider
        .load_service_account("management")
        .await
        .into_alien_error()
        .context(GenericError {
            message: format!(
                "Management service account binding not found for {}",
                platform
            ),
        })?;

    let impersonation_request = ImpersonationRequest {
        session_name: Some(format!(
            "alien-managed-srv-{}",
            uuid::Uuid::new_v4().simple()
        )),
        duration_seconds: Some(3600),
        scopes: None,
    };

    let client_config = service_account
        .impersonate(impersonation_request)
        .await
        .into_alien_error()
        .context(GenericError {
            message: format!(
                "Failed to impersonate management service account for {}",
                platform
            ),
        })?;

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

fn uses_control_plane_credentials(platform: Platform) -> bool {
    matches!(platform, Platform::Machines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credential_materialization::{
        materialize_remote_storage_lease, RemoteStorageCredentialScope,
    };
    use alien_bindings::BindingsProvider;
    use alien_core::{
        bindings::ServiceAccountBinding, AwsClientConfig, AwsCredentials, AwsEnvironmentInfo,
        AzureClientConfig, AzureCredentials, AzureEnvironmentInfo, GcpClientConfig, GcpCredentials,
        GcpEnvironmentInfo, GcpServiceOverrides, RemoteStackManagement,
        RemoteStackManagementOutputs, Resource, ResourceLifecycle, ResourceOutputs, ResourceStatus,
        StackResourceState, StackState,
    };
    use chrono::Utc;
    use httpmock::{Method::POST, MockServer};
    use serde_json::json;

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
                    access_configuration: "deployment@target-project.iam.gserviceaccount.com"
                        .to_string(),
                }))
                .lifecycle(ResourceLifecycle::Frozen)
                .dependencies(vec![])
                .build(),
        );

        DeploymentRecord {
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
            retry_requested: false,
            locked_by: None,
            locked_at: None,
            created_at: Utc::now(),
            updated_at: None,
            error: None,
        }
    }

    #[test]
    fn gcp_remote_storage_uses_exact_generated_custom_role() {
        let role = gcp_remote_storage_access_boundary_role(&gcp_handoff_deployment())
            .expect("managed GCP deployment should have an exact access-boundary role");

        assert_eq!(
            role,
            "projects/target-project/roles/role_test_prefix_storage_remote_data_write"
        );
    }

    #[test]
    fn gcp_remote_storage_role_fails_closed_without_target_environment_identity() {
        let mut deployment = gcp_handoff_deployment();
        deployment.environment_info = None;

        assert!(gcp_remote_storage_access_boundary_role(&deployment).is_err());
    }

    #[tokio::test]
    async fn gcp_remote_storage_mints_each_impersonated_identity_once() {
        let server = MockServer::start_async().await;
        let management_mint = server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("authorization", "Bearer source-token");
                then.status(200).json_body(json!({
                    "accessToken": "management-token",
                    "expireTime": "2099-01-01T00:00:00Z"
                }));
            })
            .await;
        let target_mint = server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("authorization", "Bearer management-token");
                then.status(200).json_body(json!({
                    "accessToken": "target-token",
                    "expireTime": "2099-01-01T00:00:00Z"
                }));
            })
            .await;
        let downscope_exchange = server
            .mock_async(|when, then| {
                when.method(POST).path("/downscope");
                then.status(200).json_body(json!({
                    "access_token": "bucket-confined-token",
                    "expires_in": 900,
                    "issued_token_type": "urn:ietf:params:oauth:token-type:access_token",
                    "token_type": "Bearer"
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
                    ("iamcredentials".to_string(), server.url("/v1")),
                    ("sts".to_string(), server.url("/downscope")),
                ]),
            }),
            project_number: Some("123456789".to_string()),
        }));
        let bindings = HashMap::from([(
            "management".to_string(),
            serde_json::to_value(ServiceAccountBinding::gcp_service_account(
                "management@managing-project.iam.gserviceaccount.com",
                "management-unique-id",
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

        let source = resolver
            .resolve_remote_storage_source(&gcp_handoff_deployment())
            .await
            .expect("GCP remote Storage source should resolve lazily");
        let lease = materialize_remote_storage_lease(
            source,
            RemoteStorageCredentialScope::GcpGcs {
                bucket_name: "one-bucket".to_string(),
            },
        )
        .await
        .expect("GCP remote Storage lease should be downscoped");

        let gcp = lease
            .client_config
            .gcp_config()
            .expect("materialized lease should remain GCP");
        assert_eq!(
            gcp.credentials,
            GcpCredentials::AccessToken {
                token: "bucket-confined-token".to_string()
            }
        );
        assert_eq!(management_mint.hits_async().await, 1);
        assert_eq!(target_mint.hits_async().await, 1);
        assert_eq!(downscope_exchange.hits_async().await, 1);
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
                "management-unique-id",
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
}
