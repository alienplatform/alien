use crate::error::{ErrorData, Result};
use alien_azure_clients::keyvault::{AzureKeyVaultSecretsClient, KeyVaultSecretsApi};
use alien_azure_clients::models::secrets::SecretSetParameters;
use alien_error::Context;
use async_trait::async_trait;
use std::sync::Arc;

/// Azure Key Vault binding implementation
#[derive(Debug)]
pub struct AzureKeyVault {
    client: Arc<AzureKeyVaultSecretsClient>,
    vault_base_url: String,
}

impl AzureKeyVault {
    /// Create a new Azure Key Vault binding
    pub fn new(client: Arc<AzureKeyVaultSecretsClient>, vault_base_url: String) -> Self {
        Self {
            client,
            vault_base_url,
        }
    }
}

#[async_trait]
impl crate::traits::Binding for AzureKeyVault {}

#[async_trait]
impl crate::traits::Vault for AzureKeyVault {
    /// Get a secret value by name
    async fn get_secret(&self, secret_name: &str) -> Result<String> {
        let response = self
            .client
            .get_secret(self.vault_base_url.clone(), secret_name.to_string(), None)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to get secret '{}' from vault '{}'",
                    secret_name, self.vault_base_url
                ),
                resource_id: None,
            })?;

        response.value.ok_or_else(|| {
            alien_error::AlienError::new(ErrorData::CloudPlatformError {
                message: format!("Secret '{}' has no value", secret_name),
                resource_id: None,
            })
        })
    }

    /// Set a secret value
    async fn set_secret(&self, secret_name: &str, value: &str) -> Result<()> {
        let parameters = SecretSetParameters {
            value: value.to_string(),
            content_type: None,
            attributes: None,
            tags: std::collections::HashMap::new(),
        };

        self.client
            .set_secret(
                self.vault_base_url.clone(),
                secret_name.to_string(),
                parameters,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to set secret '{}' in vault '{}'",
                    secret_name, self.vault_base_url
                ),
                resource_id: None,
            })?;

        Ok(())
    }

    /// Delete a secret
    async fn delete_secret(&self, secret_name: &str) -> Result<()> {
        self.client
            .delete_secret(self.vault_base_url.clone(), secret_name.to_string())
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to delete secret '{}' from vault '{}'",
                    secret_name, self.vault_base_url
                ),
                resource_id: None,
            })?;

        Ok(())
    }
}
