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

    /// Azure Key Vault secret names only allow alphanumerics and hyphens.
    /// Convert underscores to hyphens for compatibility.
    fn sanitize_secret_name(name: &str) -> String {
        name.replace('_', "-")
    }
}

#[async_trait]
impl crate::traits::Binding for AzureKeyVault {}

#[async_trait]
impl crate::traits::Vault for AzureKeyVault {
    /// Get a secret value by name
    async fn get_secret(&self, secret_name: &str) -> Result<String> {
        let sanitized = Self::sanitize_secret_name(secret_name);
        let response = self
            .client
            .get_secret(self.vault_base_url.clone(), sanitized, None)
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
        let sanitized = Self::sanitize_secret_name(secret_name);
        let parameters = SecretSetParameters {
            value: value.to_string(),
            content_type: None,
            attributes: None,
            tags: std::collections::HashMap::new(),
        };

        self.client
            .set_secret(self.vault_base_url.clone(), sanitized, parameters)
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
        let sanitized = Self::sanitize_secret_name(secret_name);
        self.client
            .delete_secret(self.vault_base_url.clone(), sanitized)
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
