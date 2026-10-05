use crate::error::{ErrorData, Result};
use crate::traits::SecretPresence;
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
        alien_core::vault_naming::kubernetes_secret_name(&self.vault_prefix, secret_name)
    }
}

#[async_trait]
impl crate::traits::Binding for KubernetesSecretVault {}

#[async_trait]
impl crate::traits::Vault for KubernetesSecretVault {
    /// Reads the Secret's object metadata only. The metadata cannot show
    /// whether the `value` key is set; a pod that references a missing key
    /// fails to start with the key named, so that case still surfaces.
    async fn secret_presence(&self, secret_name: &str) -> Result<SecretPresence> {
        let secret_resource_name = self.secret_resource_name(secret_name);

        match self
            .client
            .get_secret_metadata(&self.namespace, &secret_resource_name)
            .await
        {
            Ok(_) => Ok(SecretPresence::Present),
            Err(error)
                if matches!(
                    error.error,
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                Ok(SecretPresence::Missing)
            }
            Err(error) => Err(error.context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to read Secret '{}' in namespace '{}'",
                    secret_resource_name, self.namespace
                ),
                resource_id: None,
            })),
        }
    }

    /// Get a secret value by name
    async fn get_secret(&self, secret_name: &str) -> Result<String> {
        let secret_resource_name = self.secret_resource_name(secret_name);

        // Get the Kubernetes Secret
        let secret = self
            .client
            .get_secret(&self.namespace, &secret_resource_name)
            .await
            .map_err(|error| {
                super::secret_read_error(
                    error,
                    &self.vault_prefix,
                    secret_name,
                    format!("Failed to get secret '{}'", secret_name),
                )
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
                    labels.insert("managed-by".to_string(), "operator".to_string());
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

        match self
            .client
            .delete_secret(&self.namespace, &secret_resource_name)
            .await
        {
            Ok(()) => Ok(()),
            Err(error)
                if matches!(
                    error.error,
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(error.context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete secret '{secret_name}'"),
                resource_id: None,
            })),
        }
    }

    /// List secret names by selecting the Secret resources this vault manages
    /// (via the `managed-by`/`vault-prefix` labels set in [`Self::set_secret`])
    /// and stripping the `{vault_prefix}-` resource-name prefix back off.
    async fn list_secrets(&self) -> Result<Vec<String>> {
        // Match the labels set in set_secret exactly: vault-prefix is stored
        // with the vault's raw (un-sanitized) prefix.
        let label_selector = format!("managed-by=operator,vault-prefix={}", self.vault_prefix);

        let list = self
            .client
            .list_secrets(&self.namespace, Some(label_selector), None)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to list secrets for vault prefix '{}'",
                    self.vault_prefix
                ),
                resource_id: None,
            })?;

        // Resource names are the sanitized `{vault_prefix}-{secret}`; strip the
        // sanitized prefix so results round-trip through get_secret.
        let name_prefix = self.secret_resource_name("");

        let names = list
            .items
            .into_iter()
            .filter_map(|secret| secret.metadata.name)
            .filter_map(|name| name.strip_prefix(&name_prefix).map(str::to_string))
            .collect();

        Ok(names)
    }
}
