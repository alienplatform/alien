use crate::traits::credential_resolver::CredentialResolver;
use crate::traits::deployment_store::DeploymentRecord;
use alien_core::ClientConfig;
use alien_error::AlienError;
use async_trait::async_trait;
use std::path::PathBuf;

pub struct LocalCredentialResolver {
    state_dir: PathBuf,
}

impl LocalCredentialResolver {
    pub fn new(state_dir: PathBuf) -> Self {
        Self { state_dir }
    }
}

#[async_trait]
impl CredentialResolver for LocalCredentialResolver {
    async fn resolve(&self, _deployment: &DeploymentRecord) -> Result<ClientConfig, AlienError> {
        Ok(ClientConfig::Local {
            state_directory: self.state_dir.to_string_lossy().to_string(),
            artifact_registry_config: None,
        })
    }
}
