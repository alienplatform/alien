//! Composite credential resolver for dev mode.
//!
//! Dispatches based on the deployment's platform:
//! - `Platform::Local` → `ClientConfig::Local` with the server's state directory
//! - Anything else → `ClientConfig::from_std_env(platform)` (reads AWS_*, GCP_*, AZURE_* from env)
//!
//! This allows `alien dev --platform aws` to resolve cloud credentials from the
//! developer's environment while keeping local deployments working as before.

use crate::traits::credential_resolver::CredentialResolver;
use crate::traits::deployment_store::DeploymentRecord;
use alien_core::{ClientConfig, Platform};
use alien_error::AlienError;
use alien_infra::ClientConfigExt;
use async_trait::async_trait;
use std::path::PathBuf;

pub struct CompositeCredentialResolver {
    state_dir: PathBuf,
}

impl CompositeCredentialResolver {
    pub fn new(state_dir: PathBuf) -> Self {
        Self { state_dir }
    }
}

#[async_trait]
impl CredentialResolver for CompositeCredentialResolver {
    async fn resolve(&self, deployment: &DeploymentRecord) -> Result<ClientConfig, AlienError> {
        match deployment.platform {
            Platform::Local => Ok(ClientConfig::Local {
                state_directory: self.state_dir.to_string_lossy().to_string(),
                artifact_registry_config: None,
            }),
            _ => ClientConfig::from_std_env(deployment.platform.clone())
                .await
                .map_err(|e| e.into_generic()),
        }
    }
}
