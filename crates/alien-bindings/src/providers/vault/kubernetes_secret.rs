use crate::error::{ErrorData, Result};
use alien_error::{Context, ContextError, IntoAlienError};
use alien_k8s_clients::secrets::SecretsApi;
use async_trait::async_trait;
use k8s_openapi::api::core::v1::Secret;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Kubernetes Secret vault binding implementation.
///
/// Stores secrets as Kubernetes Secret resources with naming convention:
/// {vaultPrefix}-{secretName} (e.g., "acme-monitoring-secrets-API_KEY")
///
/// Each secret is a separate Kubernetes Secret resource to enable:
/// - Granular access control via RBAC
/// - Individual secret rotation without affecting others
/// - Simpler secret management (no parsing of bundled secrets)
#[derive(Debug)]
pub struct KubernetesSecretVault {
    client: Arc<dyn SecretsApi>,
    namespace: String,
    vault_prefix: String,
}

impl KubernetesSecretVault {
    /// Create a new Kubernetes Secret vault binding.
    ///
    /// # Arguments
    /// * `client` - Kubernetes Secrets API client
    /// * `namespace` - Kubernetes namespace where secrets are stored
    /// * `vault_prefix` - Prefix for secret names (e.g., "acme-monitoring-secrets")
    pub fn new(client: Arc<dyn SecretsApi>, namespace: String, vault_prefix: String) -> Self {
        Self {
            client,
            namespace,
            vault_prefix,
        }
    }

    /// Generate the Kubernetes Secret name for a given secret key.
    /// Format: {vault_prefix}-{secret_name}
    /// Example: "acme-monitoring-secrets-api-key"
    fn secret_resource_name(&self, secret_name: &str) -> String {
        let combined = format!("{}-{}", self.vault_prefix, secret_name);

        // Kubernetes names must be lowercase and follow DNS-1123 label requirements
        let clean = combined
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect::<String>()
            .to_lowercase()
            .replace('_', "-");

        // Truncate to 253 characters (Kubernetes Secret name limit)
        if clean.len() > 253 {
            clean[..253].to_string()
        } else {
            clean
        }
    }
}

#[async_trait]
impl crate::traits::Binding for KubernetesSecretVault {}

#[async_trait]
impl crate::traits::Vault for KubernetesSecretVault {
    /// Get a secret value by name
    async fn get_secret(&self, secret_name: &str) -> Result<String> {
        let secret_resource_name = self.secret_resource_name(secret_name);

        // Get the Kubernetes Secret
        let secret = self
            .client
            .get_secret(&self.namespace, &secret_resource_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to get secret '{}'", secret_name),
                resource_id: None,
            })?;

        // Extract the secret value from the "value" key in secret data
        let value = secret
            .data
            .as_ref()
            .and_then(|data| data.get("value"))
            .ok_or_else(|| {
                alien_error::AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Secret '{}' has no 'value' field", secret_name),
                    resource_id: None,
                })
            })?;

        // Decode base64 value (Kubernetes stores secret data as base64)
        let decoded = String::from_utf8(value.0.clone())
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to decode secret '{}' value", secret_name),
                resource_id: None,
            })?;

        Ok(decoded)
    }

    /// Set a secret value, creating it if it doesn't exist or updating it if it does
    async fn set_secret(&self, secret_name: &str, value: &str) -> Result<()> {
        let secret_resource_name = self.secret_resource_name(secret_name);

        // Build the Secret resource
        let mut data = BTreeMap::new();
        data.insert(
            "value".to_string(),
            k8s_openapi::ByteString(value.as_bytes().to_vec()),
        );

        let secret = Secret {
            metadata: ObjectMeta {
                name: Some(secret_resource_name.clone()),
                namespace: Some(self.namespace.clone()),
                labels: Some({
                    let mut labels = BTreeMap::new();
                    labels.insert("managed-by".to_string(), "alien".to_string());
                    labels.insert("vault-prefix".to_string(), self.vault_prefix.clone());
                    labels
                }),
                ..Default::default()
            },
            data: Some(data),
            ..Default::default()
        };

        // Try to create the secret first
        match self.client.create_secret(&self.namespace, &secret).await {
            Ok(_) => Ok(()),
            Err(e) => {
                // If secret already exists, update it instead
                if matches!(
                    e.error,
                    Some(alien_client_core::ErrorData::RemoteResourceConflict { .. })
                ) {
                    self.client
                        .update_secret(&self.namespace, &secret_resource_name, &secret)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!("Failed to update secret '{}'", secret_name),
                            resource_id: None,
                        })?;
                    Ok(())
                } else {
                    Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to create secret '{}'", secret_name),
                        resource_id: None,
                    }))
                }
            }
        }
    }

    /// Delete a secret
    async fn delete_secret(&self, secret_name: &str) -> Result<()> {
        let secret_resource_name = self.secret_resource_name(secret_name);

        self.client
            .delete_secret(&self.namespace, &secret_resource_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete secret '{}'", secret_name),
                resource_id: None,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_resource_name_generation() {
        // Test basic generation
        let vault_prefix = "acme-monitoring-secrets";
        let secret_name = "API_KEY";
        let combined = format!("{}-{}", vault_prefix, secret_name);
        let result = combined
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect::<String>()
            .to_lowercase()
            .replace('_', "-");

        assert_eq!(result, "acme-monitoring-secrets-api-key");

        // Test character filtering (underscores become hyphens)
        let secret_name2 = "MY_SECRET_KEY";
        let combined2 = format!("{}-{}", vault_prefix, secret_name2);
        let result2 = combined2
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect::<String>()
            .to_lowercase()
            .replace('_', "-");

        assert_eq!(result2, "acme-monitoring-secrets-my-secret-key");

        // Test length truncation
        let long_name = "A".repeat(300);
        let combined3 = format!("{}-{}", vault_prefix, long_name);
        let result3 = combined3
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect::<String>()
            .to_lowercase();
        let truncated = if result3.len() > 253 {
            result3[..253].to_string()
        } else {
            result3
        };
        assert!(truncated.len() <= 253);
    }
}
