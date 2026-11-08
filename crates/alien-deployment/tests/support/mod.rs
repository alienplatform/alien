use alien_core::{ClientConfig, DeploymentConfig, DeploymentState};
use alien_deployment::DeploymentStepResult;
use alien_infra::PlatformServiceProvider;
use std::sync::Arc;

/// `alien_deployment::step` with the process's secrets digest key set, as an embedder does at startup.
pub async fn step(
    current: DeploymentState,
    config: DeploymentConfig,
    client_config: ClientConfig,
    service_provider: Option<Arc<dyn PlatformServiceProvider>>,
) -> alien_deployment::Result<DeploymentStepResult> {
    alien_deployment::set_ephemeral_secrets_digest_key();
    alien_deployment::step(current, config, client_config, service_provider).await
}
