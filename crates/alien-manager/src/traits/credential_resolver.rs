use async_trait::async_trait;

use alien_core::ClientConfig;
use alien_error::AlienError;

use super::deployment_store::DeploymentRecord;

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
}
