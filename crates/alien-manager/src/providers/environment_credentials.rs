use crate::traits::credential_resolver::CredentialResolver;
use crate::traits::deployment_store::DeploymentRecord;
use alien_core::ClientConfig;
use alien_error::AlienError;
use alien_infra::ClientConfigExt;
use async_trait::async_trait;

pub struct EnvironmentCredentialResolver;

impl EnvironmentCredentialResolver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CredentialResolver for EnvironmentCredentialResolver {
    async fn resolve(&self, deployment: &DeploymentRecord) -> Result<ClientConfig, AlienError> {
        // Use ClientConfigExt::from_std_env() which reads platform-specific env vars:
        // AWS: AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_REGION, etc.
        // GCP: GOOGLE_APPLICATION_CREDENTIALS, etc.
        // Azure: AZURE_SUBSCRIPTION_ID, AZURE_TENANT_ID, etc.
        ClientConfig::from_std_env(deployment.platform.clone())
            .await
            .map_err(|e| e.into_generic())
    }
}
