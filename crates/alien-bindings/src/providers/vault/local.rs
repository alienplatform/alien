use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;

/// Local vault binding implementation for development and testing.
///
/// Secrets are stored in a JSON file in the vault directory without encryption.
/// Encryption will be added in a future iteration.
#[derive(Debug)]
pub struct LocalVault {
    vault_name: String,
    vault_dir: PathBuf,
}

impl LocalVault {
    /// Create a new local vault binding.
    ///
    /// # Arguments
    /// * `vault_name` - Name of the vault
    /// * `vault_dir` - Directory where secrets are stored
    pub fn new(vault_name: String, vault_dir: PathBuf) -> Self {
        Self {
            vault_name,
            vault_dir,
        }
    }

    /// Get the path to the secrets file.
    fn secrets_file_path(&self) -> PathBuf {
        self.vault_dir.join("secrets.json")
    }

    /// Load secrets from disk.
    async fn load_secrets(&self) -> Result<HashMap<String, String>> {
        let secrets_file = self.secrets_file_path();

        if !secrets_file.exists() {
            return Ok(HashMap::new());
        }

        let content = tokio::fs::read_to_string(&secrets_file)
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to read vault secrets file: {}",
                    secrets_file.display()
                ),
                resource_id: None,
            })?;

        serde_json::from_str(&content)
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse vault secrets file: {}",
                    secrets_file.display()
                ),
                resource_id: None,
            })
    }

    /// Save secrets to disk.
    async fn save_secrets(&self, secrets: &HashMap<String, String>) -> Result<()> {
        let secrets_file = self.secrets_file_path();

        // Ensure directory exists
        if let Some(parent) = secrets_file.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create vault directory: {}", parent.display()),
                    resource_id: None,
                })?;
        }

        let json = serde_json::to_string_pretty(secrets)
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Failed to serialize vault secrets".to_string(),
                resource_id: None,
            })?;

        let path = secrets_file.clone();
        let data = json.into_bytes();
        tokio::task::spawn_blocking(move || {
            alien_core::file_utils::write_secret_file(&path, &data)
        })
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Failed to spawn blocking write task".to_string(),
            resource_id: None,
        })?
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to write vault secrets file: {}",
                secrets_file.display()
            ),
            resource_id: None,
        })
    }
}

#[async_trait]
impl crate::traits::Binding for LocalVault {}

#[async_trait]
impl crate::traits::Vault for LocalVault {
    /// Get a secret value by name
    async fn get_secret(&self, secret_name: &str) -> Result<String> {
        let secrets = self.load_secrets().await?;

        secrets.get(secret_name).cloned().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Secret '{}' not found in vault '{}'",
                    secret_name, self.vault_name
                ),
                resource_id: None,
            })
        })
    }

    /// Set a secret value
    async fn set_secret(&self, secret_name: &str, value: &str) -> Result<()> {
        let mut secrets = self.load_secrets().await?;
        secrets.insert(secret_name.to_string(), value.to_string());
        self.save_secrets(&secrets).await
    }

    /// Delete a secret
    async fn delete_secret(&self, secret_name: &str) -> Result<()> {
        let mut secrets = self.load_secrets().await?;

        secrets.remove(secret_name).ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Secret '{}' not found in vault '{}'",
                    secret_name, self.vault_name
                ),
                resource_id: None,
            })
        })?;

        self.save_secrets(&secrets).await
    }
}
