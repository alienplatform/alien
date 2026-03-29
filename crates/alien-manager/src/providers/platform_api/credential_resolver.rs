use std::collections::HashMap;
use std::sync::Arc;

use crate::traits::{CredentialResolver, DeploymentRecord};
use alien_bindings::traits::ImpersonationRequest;
use alien_bindings::BindingsProviderApi;
use alien_core::{ClientConfig, DeploymentStatus, Platform};
use alien_error::{AlienError, Context, GenericError, IntoAlienError};
use async_trait::async_trait;

use super::error::ErrorData;

/// Resolves cloud credentials for push-model deployments via service account impersonation.
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
                artifact_registry_config: None,
            });
        }

        let status = parse_status(&deployment.status);

        if matches!(
            status,
            DeploymentStatus::Pending | DeploymentStatus::InitialSetup
        ) {
            let provider = self.provider_for_target(platform);
            return impersonate_management_service_account(&**provider, platform)
                .await
                .context(GenericError {
                    message: format!(
                        "Failed to impersonate management service account for {:?}",
                        platform
                    ),
                });
        }

        if let Some(ref stack_state) = deployment.stack_state {
            let provider = self.provider_for_target(platform);
            let base_config = impersonate_management_service_account(&**provider, platform)
                .await
                .context(GenericError {
                    message: format!(
                        "Failed to impersonate management service account for {:?}",
                        platform
                    ),
                })?;

            let resolver = alien_infra::RemoteAccessResolver::new(std::env::vars().collect());
            return resolver
                .resolve(base_config, stack_state)
                .await
                .context(GenericError {
                    message: "Failed to resolve remote access from stack state".to_string(),
                });
        }

        let provider = self.provider_for_target(platform);
        impersonate_management_service_account(&**provider, platform)
            .await
            .context(GenericError {
                message: format!(
                    "Failed to impersonate management service account (fallback) for {:?}",
                    platform
                ),
            })
    }
}

/// Impersonate the management service account to get base credentials.
pub async fn impersonate_management_service_account(
    bindings_provider: &dyn BindingsProviderApi,
    platform: Platform,
) -> super::error::Result<ClientConfig> {
    let service_account = bindings_provider
        .load_service_account("management")
        .await
        .into_alien_error()
        .context(ErrorData::BindingNotFound {
            binding_name: "management".to_string(),
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
        .context(ErrorData::ServiceAccountOperationFailed {
            operation: "impersonate management ServiceAccount".to_string(),
        })?;

    if client_config.platform() != platform {
        return Err(AlienError::new(ErrorData::PlatformMismatch {
            expected: format!("{:?}", platform),
            actual: format!("{:?}", client_config.platform()),
            message: "Management service account impersonation returned unexpected platform"
                .to_string(),
        }));
    }

    Ok(client_config)
}

fn parse_status(status: &str) -> DeploymentStatus {
    match status {
        "pending" => DeploymentStatus::Pending,
        "initial-setup" => DeploymentStatus::InitialSetup,
        "provisioning" => DeploymentStatus::Provisioning,
        "running" => DeploymentStatus::Running,
        "updating" => DeploymentStatus::Updating,
        "deleting" => DeploymentStatus::Deleting,
        _ => DeploymentStatus::Pending,
    }
}
