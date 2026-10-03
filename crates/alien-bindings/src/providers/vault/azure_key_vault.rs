use crate::error::{ErrorData, Result};
use crate::traits::SecretPresence;
use alien_azure_clients::keyvault::{AzureKeyVaultSecretsClient, KeyVaultSecretsApi};
use alien_azure_clients::models::secrets::SecretSetParameters;
use alien_error::{Context, ContextError};
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
        alien_core::vault_naming::key_vault_secret_name(name)
    }
}

#[async_trait]
impl crate::traits::Binding for AzureKeyVault {}

#[async_trait]
impl crate::traits::Vault for AzureKeyVault {
    /// Lists the secret's versions, which carry attributes but no values. An
    /// unversioned read returns the newest version, so that one must be
    /// enabled and inside its activation window.
    async fn secret_presence(&self, secret_name: &str) -> Result<SecretPresence> {
        let sanitized = Self::sanitize_secret_name(secret_name);

        let versions = match self
            .client
            .list_secret_versions(self.vault_base_url.clone(), sanitized.clone())
            .await
        {
            Ok(versions) => versions,
            Err(error)
                if matches!(
                    error.error,
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                return Ok(SecretPresence::Missing);
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to list versions of secret '{}' in vault '{}'",
                        sanitized, self.vault_base_url
                    ),
                    resource_id: None,
                }))
            }
        };

        let newest = versions
            .value
            .iter()
            .filter_map(|item| item.attributes.as_ref())
            .max_by_key(|attributes| attributes.created.unwrap_or_default());
        let Some(attributes) = newest else {
            return Ok(SecretPresence::Missing);
        };
        let now = chrono::Utc::now().timestamp();
        let reason = if attributes.enabled == Some(false) {
            Some("is disabled")
        } else if attributes.exp.is_some_and(|expires| expires <= now) {
            Some("has expired")
        } else if attributes.nbf.is_some_and(|not_before| not_before > now) {
            Some("is not active yet")
        } else {
            None
        };
        Ok(match reason {
            Some(reason) => SecretPresence::Invalid {
                reason: format!("the newest version of secret '{sanitized}' {reason}"),
            },
            None => SecretPresence::Present,
        })
    }

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
        match self
            .client
            .delete_secret(self.vault_base_url.clone(), sanitized)
            .await
        {
            Ok(_) => Ok(()),
            Err(error)
                if matches!(
                    error.error,
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(error.context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to delete secret '{}' from vault '{}'",
                    secret_name, self.vault_base_url
                ),
                resource_id: None,
            })),
        }
    }

    async fn list_secrets(&self) -> Result<Vec<String>> {
        // Azure Key Vault list is GET /secrets. The alien-azure-clients
        // KeyVaultSecretsApi wrapper does not expose it, so this fails
        // explicitly rather than guessing.
        Err(alien_error::AlienError::new(
            ErrorData::OperationNotSupported {
                operation: "vault.list_secrets".to_string(),
                reason: "Azure Key Vault list is not exposed by the Key Vault secrets client"
                    .to_string(),
            },
        ))
    }
}
