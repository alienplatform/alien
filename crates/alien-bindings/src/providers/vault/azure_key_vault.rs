use crate::error::{ErrorData, Result};
use crate::traits::SecretPresence;
use alien_azure_clients::keyvault::{AzureKeyVaultSecretsClient, KeyVaultSecretsApi};
use alien_azure_clients::models::secrets::{SecretItem, SecretSetParameters};
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

/// Whether the secret's newest version can be used, from its versions.
///
/// An unversioned read returns the newest version, but Key Vault records
/// `created` in whole seconds, and version ids are random, so versions written
/// in the same second cannot be ordered. When the newest second holds several
/// versions, each must be usable, and the reported version names all of them
/// (sorted, joined with `+`). Any later write then changes it: a write in a
/// later second leaves one newest version, and a write in the same second adds
/// a version to the set.
fn presence_from_versions(versions: &[SecretItem], sanitized: &str, now: i64) -> SecretPresence {
    let dated = || {
        versions.iter().filter_map(|item| {
            let attributes = item.attributes.as_ref()?;
            Some((item, attributes, attributes.created.unwrap_or_default()))
        })
    };
    let Some(newest_created) = dated().map(|(_, _, created)| created).max() else {
        return SecretPresence::Missing;
    };
    let newest = dated()
        .filter(|(_, _, created)| *created == newest_created)
        .map(|(item, attributes, _)| (item, attributes));

    let mut version_ids = Vec::new();
    for (item, attributes) in newest {
        let reason = if attributes.enabled == Some(false) {
            Some("is disabled")
        } else if attributes.exp.is_some_and(|expires| expires <= now) {
            Some("has expired")
        } else if attributes.nbf.is_some_and(|not_before| not_before > now) {
            Some("is not active yet")
        } else {
            None
        };
        if let Some(reason) = reason {
            return SecretPresence::Invalid {
                reason: format!("the newest version of secret '{sanitized}' {reason}"),
            };
        }
        // The item id is `{vault}/secrets/{name}/{version}`; the last segment
        // is the version id, new for every write.
        if let Some(version_id) = item.id.as_deref().and_then(|id| id.rsplit('/').next()) {
            version_ids.push(version_id);
        }
    }
    version_ids.sort_unstable();
    SecretPresence::Present {
        version: (!version_ids.is_empty()).then(|| version_ids.join("+")),
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

        Ok(presence_from_versions(
            &versions,
            &sanitized,
            chrono::Utc::now().timestamp(),
        ))
    }

    /// Get a secret value by name
    async fn get_secret(&self, secret_name: &str) -> Result<String> {
        let sanitized = Self::sanitize_secret_name(secret_name);
        let response = self
            .client
            .get_secret(self.vault_base_url.clone(), sanitized, None)
            .await
            .map_err(|error| {
                super::secret_read_error(
                    error,
                    &self.vault_base_url,
                    secret_name,
                    format!(
                        "Failed to get secret '{}' from vault '{}'",
                        secret_name, self.vault_base_url
                    ),
                )
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

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;

    fn version(id: &str, created: i64, attributes: serde_json::Value) -> SecretItem {
        let mut attributes = attributes;
        attributes["created"] = created.into();
        serde_json::from_value(serde_json::json!({
            "id": format!("https://vault.vault.azure.net/secrets/api-key/{id}"),
            "attributes": attributes,
        }))
        .expect("a Key Vault secret item")
    }

    fn enabled(id: &str, created: i64) -> SecretItem {
        version(id, created, serde_json::json!({ "enabled": true }))
    }

    fn presence(versions: &[SecretItem]) -> SecretPresence {
        presence_from_versions(versions, "api-key", NOW)
    }

    fn present(version: &str) -> SecretPresence {
        SecretPresence::Present {
            version: Some(version.to_string()),
        }
    }

    #[test]
    fn reports_the_newest_version_whatever_the_listing_order() {
        let versions = [enabled("bbb", NOW - 10), enabled("aaa", NOW - 100)];
        assert_eq!(presence(&versions), present("bbb"));
        let reversed = [enabled("aaa", NOW - 100), enabled("bbb", NOW - 10)];
        assert_eq!(presence(&reversed), present("bbb"));
    }

    #[test]
    fn a_second_write_in_the_same_second_changes_the_version() {
        // `created` has whole-second precision, so both writes carry the same
        // timestamp and nothing orders them.
        let first = presence(&[enabled("old", NOW - 100), enabled("v2", NOW - 10)]);
        assert_eq!(first, present("v2"));

        let after_rotation = [
            enabled("v2", NOW - 10),
            enabled("old", NOW - 100),
            enabled("v1", NOW - 10),
        ];
        assert_eq!(presence(&after_rotation), present("v1+v2"));
        let reordered = [
            enabled("v1", NOW - 10),
            enabled("old", NOW - 100),
            enabled("v2", NOW - 10),
        ];
        assert_eq!(presence(&reordered), present("v1+v2"));

        // A later write leaves a single newest version again.
        let mut after_next_write = after_rotation.to_vec();
        after_next_write.push(enabled("v3", NOW - 5));
        assert_eq!(presence(&after_next_write), present("v3"));
    }

    #[test]
    fn every_version_that_may_be_current_must_be_usable() {
        let versions = [
            enabled("v1", NOW - 10),
            version("v2", NOW - 10, serde_json::json!({ "enabled": false })),
        ];
        assert_eq!(
            presence(&versions),
            SecretPresence::Invalid {
                reason: "the newest version of secret 'api-key' is disabled".to_string(),
            }
        );
    }

    #[test]
    fn an_unusable_newest_version_is_invalid_even_with_a_usable_older_one() {
        for (attributes, reason) in [
            (serde_json::json!({ "enabled": false }), "is disabled"),
            (
                serde_json::json!({ "enabled": true, "exp": NOW }),
                "has expired",
            ),
            (
                serde_json::json!({ "enabled": true, "nbf": NOW + 60 }),
                "is not active yet",
            ),
        ] {
            let versions = [
                enabled("old", NOW - 100),
                version("new", NOW - 10, attributes),
            ];
            assert_eq!(
                presence(&versions),
                SecretPresence::Invalid {
                    reason: format!("the newest version of secret 'api-key' {reason}"),
                }
            );
        }
    }

    #[test]
    fn a_secret_without_versions_is_missing() {
        assert_eq!(presence(&[]), SecretPresence::Missing);
    }
}
