use async_trait::async_trait;

use alien_core::{ClientConfig, ManagementConfig, Platform};
use alien_error::{AlienError, GenericError};

use super::deployment_store::DeploymentRecord;

/// Credentials resolved for a deployment plus the lifecycle phases they may
/// drive safely.
#[derive(Debug, Clone)]
pub struct ResolvedCredentials {
    /// Platform client configuration to pass into the deployment runner.
    pub client_config: ClientConfig,
    /// Whether these credentials may create the bootstrap layer-2 resources.
    pub has_provision_capability: bool,
}

/// Credentials for the setup-owned, stack-scoped Remote Bindings identity.
pub enum RemoteStorageCredentialSource {
    /// Provider config resolved from the Remote Bindings identity handoff.
    Direct(ClientConfig),
}

impl std::fmt::Debug for RemoteStorageCredentialSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Direct(config) => f
                .debug_struct("RemoteStorageCredentialSource::Direct")
                .field("platform", &config.platform())
                .field("credentials", &"[REDACTED]")
                .finish(),
        }
    }
}

/// Resolves cloud credentials for push-model deployments.
///
/// In push mode, alien-manager needs credentials to call cloud APIs in the remote
/// environment. The resolver reads base credentials and optionally impersonates
/// a service account in the target environment.
#[async_trait]
pub trait CredentialResolver: Send + Sync {
    /// Resolve credentials for a deployment's target environment.
    ///
    /// For single-account setups, returns the server's own credentials.
    /// For cross-account setups, impersonates the target role/service account.
    async fn resolve(&self, deployment: &DeploymentRecord) -> Result<ClientConfig, AlienError>;

    /// Resolve credentials with their lifecycle capabilities.
    ///
    /// The default resolver behavior is suitable for direct environment/local
    /// credentials: if a resolver can return a client config, those credentials
    /// are allowed to provision. Cross-account resolvers override this for
    /// post-bootstrap impersonation credentials, which can manage layer-3 work
    /// and updates but must not create the initial customer-owned layer-2 stack.
    async fn resolve_with_capability(
        &self,
        deployment: &DeploymentRecord,
    ) -> Result<ResolvedCredentials, AlienError> {
        let client_config = self.resolve(deployment).await?;
        Ok(ResolvedCredentials {
            client_config,
            has_provision_capability: true,
        })
    }

    /// Resolve authority for a purpose-specific remote Storage lease.
    ///
    /// There is deliberately no direct-credential fallback. Deployment,
    /// installer, and management credentials are not substitutes for the
    /// setup-owned Remote Bindings identity.
    async fn resolve_remote_storage_source(
        &self,
        _deployment: &DeploymentRecord,
        _resource_id: &str,
    ) -> Result<RemoteStorageCredentialSource, AlienError> {
        Err(AlienError::new(GenericError {
            message: "Remote Bindings require a credential resolver that selects the setup-owned Access identity".to_string(),
        }))
    }

    /// Resolve the management identity for a target platform.
    ///
    /// Returns the ManagementConfig describing which identity should be granted
    /// cross-account access in the customer's cloud. Derived from the management
    /// ServiceAccount binding for the given platform's target provider.
    ///
    /// Returns `Ok(None)` when no management binding is available (e.g. standalone mode).
    async fn resolve_management_config(
        &self,
        _platform: Platform,
    ) -> Result<Option<ManagementConfig>, AlienError> {
        Ok(None)
    }
}
