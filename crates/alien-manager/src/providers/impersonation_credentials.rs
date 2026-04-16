//! Credential resolver that impersonates a management service account
//! to obtain target-environment credentials.
//!
//! Used when `[impersonation]` is configured in alien-manager.toml.
//! The management SA is loaded from the per-platform target bindings provider.

use std::collections::HashMap;
use std::sync::Arc;

use alien_bindings::traits::ImpersonationRequest;
use alien_bindings::{BindingsProviderApi, ServiceAccountInfo};
use alien_core::{ClientConfig, DeploymentStatus, ManagementConfig, Platform};
use alien_error::{AlienError, Context, GenericError, IntoAlienError};
use async_trait::async_trait;

use crate::traits::{CredentialResolver, DeploymentRecord};

/// Resolves cloud credentials for push-model deployments via service account impersonation.
///
/// For each deployment, loads the management service account from the target platform's
/// bindings provider, impersonates it to get short-lived credentials, then resolves
/// the remote stack management identity from the deployment's stack state.
pub struct ImpersonationCredentialResolver {
    bindings_provider: Arc<dyn BindingsProviderApi>,
    target_providers: HashMap<Platform, Arc<dyn BindingsProviderApi>>,
}

impl ImpersonationCredentialResolver {
    pub fn new(
        bindings_provider: Arc<dyn BindingsProviderApi>,
        target_providers: HashMap<Platform, Arc<dyn BindingsProviderApi>>,
    ) -> Self {
        Self {
            bindings_provider,
            target_providers,
        }
    }

    fn provider_for_target(&self, platform: Platform) -> &Arc<dyn BindingsProviderApi> {
        self.target_providers
            .get(&platform)
            .unwrap_or(&self.bindings_provider)
    }
}

#[async_trait]
impl CredentialResolver for ImpersonationCredentialResolver {
    async fn resolve(&self, deployment: &DeploymentRecord) -> Result<ClientConfig, AlienError> {
        let platform = deployment.platform;

        if platform == Platform::Test {
            return Ok(ClientConfig::Test);
        }

        if platform == Platform::Local {
            return Ok(ClientConfig::Local {
                state_directory: "/tmp/alien-local".to_string(),
            });
        }

        let status = parse_status(&deployment.status);

        // During Pending and InitialSetup, use the management SA directly
        // (no remote stack management identity exists yet).
        if matches!(
            status,
            DeploymentStatus::Pending | DeploymentStatus::InitialSetup
        ) {
            let provider = self.provider_for_target(platform);
            return impersonate_management_sa(&**provider, platform).await;
        }

        // After initial setup, resolve via the remote stack management identity
        // in the stack state (two-hop: management SA → target identity).
        if let Some(ref stack_state) = deployment.stack_state {
            let provider = self.provider_for_target(platform);
            let base_config = impersonate_management_sa(&**provider, platform).await?;

            let resolver = alien_infra::RemoteAccessResolver::new(std::env::vars().collect());
            let resolved = resolver
                .resolve(
                    base_config,
                    stack_state,
                    deployment.environment_info.as_ref(),
                )
                .await
                .context(GenericError {
                    message: "Failed to resolve remote access from stack state".to_string(),
                })?;

            return Ok(resolved);
        }

        // Fallback: use management SA directly.
        let provider = self.provider_for_target(platform);
        impersonate_management_sa(&**provider, platform).await
    }

    async fn resolve_management_config(
        &self,
        platform: Platform,
    ) -> Result<Option<ManagementConfig>, AlienError> {
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
        ServiceAccountInfo::Azure(azure_info) => {
            let tenant_id = std::env::var("AZURE_TENANT_ID").map_err(|_| {
                AlienError::new(GenericError {
                    message: format!(
                        "AZURE_TENANT_ID required for Azure management config on {}",
                        platform
                    ),
                })
            })?;

            let oidc_issuer = std::env::var("AZURE_MANAGEMENT_OIDC_ISSUER")
                .ok()
                .filter(|value| !value.is_empty());
            let oidc_subject = std::env::var("AZURE_MANAGEMENT_OIDC_SUBJECT")
                .ok()
                .filter(|value| !value.is_empty());
            let management_principal_id = if oidc_issuer.is_none() {
                Some(azure_info.principal_id)
            } else {
                None
            };

            Ok(ManagementConfig::Azure(alien_core::AzureManagementConfig {
                managing_tenant_id: tenant_id,
                oidc_issuer,
                oidc_subject,
                management_principal_id,
            }))
        }
    }
}

fn parse_status(status: &str) -> DeploymentStatus {
    serde_json::from_value(serde_json::Value::String(status.to_string()))
        .unwrap_or(DeploymentStatus::Pending)
}
