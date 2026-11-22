use crate::error::{ErrorData, Result};
use crate::traits::SecretPresence;
use alien_error::{AlienError, Context, ContextError};
use alien_gcp_clients::secret_manager::{
    AddSecretVersionRequest, AutomaticReplication, Replication, ReplicationPolicy, Secret,
    SecretManagerApi, SecretPayload, SecretVersionState,
};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as base64_standard, Engine as _};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, warn};

/// GCP Secret Manager vault binding implementation
#[derive(Debug)]
pub struct GcpSecretManagerVault {
    client: Arc<dyn SecretManagerApi>,
    vault_prefix: String,
    project_id: String,
    vault_id: String,
}

impl GcpSecretManagerVault {
    /// Create a new GCP Secret Manager vault binding
    ///
    /// `vault_id` is the id of the vault resource; it marks the secrets this binding creates.
    pub fn new(
        client: Arc<dyn SecretManagerApi>,
        vault_prefix: String,
        project_id: String,
        vault_id: String,
    ) -> Self {
        Self {
            client,
            vault_prefix,
            project_id,
            vault_id,
        }
    }

    /// Labels that mark a secret as created by this vault.
    fn marker_labels(&self) -> HashMap<String, String> {
        use alien_core::vault_naming::{
            vault_marker_value, VAULT_ID_MARKER_KEY, VAULT_PREFIX_MARKER_KEY,
        };
        HashMap::from([
            (
                VAULT_PREFIX_MARKER_KEY.to_string(),
                vault_marker_value(&self.vault_prefix),
            ),
            (
                VAULT_ID_MARKER_KEY.to_string(),
                vault_marker_value(&self.vault_id),
            ),
        ])
    }

    /// Generate the full secret name with vault prefix
    fn full_secret_name(&self, secret_name: &str) -> String {
        alien_core::vault_naming::secret_manager_secret_id(&self.vault_prefix, secret_name)
    }

    /// Generate the secret resource name for GCP API
    fn secret_resource_name(&self, secret_name: &str) -> String {
        format!(
            "projects/{}/secrets/{}",
            self.project_id,
            self.full_secret_name(secret_name)
        )
    }
}

#[async_trait]
impl crate::traits::Binding for GcpSecretManagerVault {}

#[async_trait]
impl crate::traits::Vault for GcpSecretManagerVault {
    /// Reads the latest version's metadata (`versions.get`, not `access`), so
    /// the payload never leaves Secret Manager. The workload reads `latest`,
    /// so that version must be enabled.
    async fn secret_presence(&self, secret_name: &str) -> Result<SecretPresence> {
        let full_name = self.full_secret_name(secret_name);

        let version = match self
            .client
            .get_secret_version(format!("{full_name}/versions/latest"))
            .await
        {
            Ok(version) => version,
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
                    message: format!("Failed to read the latest version of secret '{full_name}'"),
                    resource_id: None,
                }))
            }
        };

        match version.state {
            // `name` resolves the `latest` alias to the concrete version
            // (`.../versions/3`), which changes on every new version.
            Some(SecretVersionState::Enabled) => Ok(SecretPresence::Present {
                version: version.name,
            }),
            Some(SecretVersionState::Disabled) => Ok(SecretPresence::Invalid {
                reason: format!("the latest version of secret '{full_name}' is disabled"),
            }),
            Some(SecretVersionState::Destroyed) => Ok(SecretPresence::Invalid {
                reason: format!("the latest version of secret '{full_name}' is destroyed"),
            }),
            Some(SecretVersionState::StateUnspecified) | None => Ok(SecretPresence::Invalid {
                reason: format!("the latest version of secret '{full_name}' has no state"),
            }),
        }
    }

    /// Get a secret value by name
    async fn get_secret(&self, secret_name: &str) -> Result<String> {
        let secret_resource = self.secret_resource_name(secret_name);

        // Get the latest version of the secret
        let response = self
            .client
            .access_secret_version(format!(
                "{}/versions/latest",
                self.full_secret_name(secret_name)
            ))
            .await
            .map_err(|error| {
                super::secret_read_error(
                    error,
                    &self.vault_prefix,
                    secret_name,
                    format!("Failed to access secret version '{}'", secret_resource),
                )
            })?;

        // Extract the payload from the response
        let payload = response.payload.ok_or_else(|| {
            alien_error::AlienError::new(ErrorData::CloudPlatformError {
                message: format!("Secret '{}' has no payload", secret_resource),
                resource_id: None,
            })
        })?;

        // Decode the base64-encoded payload
        let base64_data = payload.data.ok_or_else(|| {
            alien_error::AlienError::new(ErrorData::CloudPlatformError {
                message: format!("Secret '{}' has no data", secret_resource),
                resource_id: None,
            })
        })?;

        // Decode from base64 to bytes, then to UTF-8 string
        let data = base64_standard.decode(base64_data).map_err(|e| {
            alien_error::AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to decode base64 data for secret '{}': {}",
                    secret_resource, e
                ),
                resource_id: None,
            })
        })?;

        String::from_utf8(data).map_err(|e| {
            alien_error::AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Secret '{}' contains invalid UTF-8 data: {}",
                    secret_resource, e
                ),
                resource_id: None,
            })
        })
    }

    /// Set a secret value
    async fn set_secret(&self, secret_name: &str, value: &str) -> Result<()> {
        let secret_resource = self.secret_resource_name(secret_name);
        let full_secret_name = self.full_secret_name(secret_name);

        // Prepare the payload once
        let payload = SecretPayload::builder()
            .data(base64_standard.encode(value))
            .build();

        let request = AddSecretVersionRequest::builder()
            .payload(payload.clone())
            .build();

        // Try to add a new version to the secret
        match self
            .client
            .add_secret_version(full_secret_name.clone(), request)
            .await
        {
            Ok(_) => Ok(()),
            Err(e)
                if e.error
                    .as_ref()
                    .map(|err| {
                        matches!(
                            err,
                            alien_client_core::ErrorData::RemoteResourceNotFound { .. }
                        )
                    })
                    .unwrap_or(false) =>
            {
                // Secret doesn't exist, create it first with automatic replication
                let replication = Replication::builder()
                    .replication_policy(ReplicationPolicy::Automatic(
                        AutomaticReplication::builder().build(),
                    ))
                    .build();

                // Labels are set here, on create only. A later version add never touches them.
                let secret = Secret::builder()
                    .replication(replication)
                    .labels(self.marker_labels())
                    .build();

                // Create the secret
                self.client
                    .create_secret(full_secret_name.clone(), secret)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to create secret '{}'", secret_resource),
                        resource_id: None,
                    })?;

                // Now add the version with retry logic for potential race conditions
                let add_request = AddSecretVersionRequest::builder().payload(payload).build();

                // Retry adding the version with a small delay to handle race conditions
                let mut last_error = None;
                for attempt in 0..3 {
                    if attempt > 0 {
                        // Small delay between retries to allow GCP to propagate the secret creation
                        tokio::time::sleep(std::time::Duration::from_millis(
                            100 * (attempt as u64),
                        ))
                        .await;
                    }

                    debug!(
                        "Attempting to add secret version (attempt {}/3) for secret: {}",
                        attempt + 1,
                        full_secret_name
                    );
                    match self
                        .client
                        .add_secret_version(full_secret_name.clone(), add_request.clone())
                        .await
                    {
                        Ok(_) => {
                            debug!(
                                "Successfully added secret version for: {}",
                                full_secret_name
                            );
                            return Ok(());
                        }
                        Err(e) => {
                            warn!(
                                "Failed to add secret version (attempt {}/3) for {}: {:?}",
                                attempt + 1,
                                full_secret_name,
                                e
                            );
                            last_error = Some(e);
                            // Continue to retry unless it's the last attempt
                        }
                    }
                }

                // If we got here, all retries failed
                if let Some(e) = last_error {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to add version to secret '{}' after {} attempts",
                            secret_resource, 3
                        ),
                        resource_id: None,
                    }));
                }

                Ok(())
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: format!("Failed to set secret '{}'", secret_resource),
                resource_id: None,
            })),
        }
    }

    /// Creates the secret resource, which fails when one by this name exists, then writes its
    /// first version. An existing secret is left unchanged.
    async fn create_secret(&self, secret_name: &str, value: &str) -> Result<()> {
        let secret_resource = self.secret_resource_name(secret_name);
        let full_secret_name = self.full_secret_name(secret_name);
        let replication = Replication::builder()
            .replication_policy(ReplicationPolicy::Automatic(
                AutomaticReplication::builder().build(),
            ))
            .build();
        match self
            .client
            .create_secret(
                full_secret_name.clone(),
                Secret::builder().replication(replication).build(),
            )
            .await
        {
            Ok(_) => {}
            Err(e)
                if matches!(
                    e.error,
                    Some(alien_client_core::ErrorData::RemoteResourceConflict { .. })
                ) =>
            {
                return Err(AlienError::new(ErrorData::VaultSecretAlreadyExists {
                    vault: self.vault_prefix.clone(),
                    secret_name: secret_name.to_string(),
                }));
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create secret '{}'", secret_resource),
                    resource_id: None,
                }))
            }
        }
        let request = AddSecretVersionRequest::builder()
            .payload(
                SecretPayload::builder()
                    .data(base64_standard.encode(value))
                    .build(),
            )
            .build();
        let mut last_error = None;
        for attempt in 0..3u64 {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(100 * attempt)).await;
            }
            match self
                .client
                .add_secret_version(full_secret_name.clone(), request.clone())
                .await
            {
                Ok(_) => return Ok(()),
                Err(e) => last_error = Some(e),
            }
        }
        Err(last_error
            .expect("the loop ran")
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to add the first version to secret '{}'",
                    secret_resource
                ),
                resource_id: None,
            }))
    }

    /// Delete a secret
    async fn delete_secret(&self, secret_name: &str) -> Result<()> {
        let secret_resource = self.secret_resource_name(secret_name);

        match self
            .client
            .delete_secret(self.full_secret_name(secret_name))
            .await
        {
            Ok(_) => Ok(()),
            Err(e)
                if e.error
                    .as_ref()
                    .map(|err| {
                        matches!(
                            err,
                            alien_client_core::ErrorData::RemoteResourceNotFound { .. }
                        )
                    })
                    .unwrap_or(false) =>
            {
                // Secret doesn't exist - this is fine for delete operations (idempotent)
                debug!(
                    "Secret '{}' was not found during deletion - treating as success",
                    secret_resource
                );
                Ok(())
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete secret '{}'", secret_resource),
                resource_id: None,
            })),
        }
    }

    async fn list_secrets(&self) -> Result<Vec<String>> {
        // GCP Secret Manager list is projects.secrets.list. The
        // alien-gcp-clients SecretManagerApi wrapper does not expose it, so
        // this fails explicitly rather than guessing.
        Err(alien_error::AlienError::new(
            ErrorData::OperationNotSupported {
                operation: "vault.list_secrets".to_string(),
                reason: "GCP Secret Manager list is not exposed by the Secret Manager client"
                    .to_string(),
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::Vault as _;
    use alien_client_core::ErrorData as ClientErrorData;
    use alien_gcp_clients::secret_manager::{MockSecretManagerApi, SecretVersion};

    fn vault_with(client: MockSecretManagerApi, prefix: &str, id: &str) -> GcpSecretManagerVault {
        GcpSecretManagerVault::new(
            Arc::new(client),
            prefix.to_string(),
            "test-project".to_string(),
            id.to_string(),
        )
    }

    fn version() -> SecretVersion {
        SecretVersion::builder().build()
    }

    /// A secret created through the vault carries both markers as labels.
    #[tokio::test]
    async fn a_created_secret_is_labeled_with_its_vault_prefix_and_id() {
        let mut client = MockSecretManagerApi::new();
        let mut adds = 0;
        client.expect_add_secret_version().returning(move |_, _| {
            adds += 1;
            if adds == 1 {
                Err(alien_error::AlienError::new(
                    ClientErrorData::RemoteResourceNotFound {
                        resource_type: "Secret".to_string(),
                        resource_name: "x".to_string(),
                    },
                ))
            } else {
                Ok(version())
            }
        });
        client
            .expect_create_secret()
            .times(1)
            .withf(|secret_id, secret| {
                secret_id == "acme-1a2b-secrets-api-key"
                    && secret.labels
                        == Some(HashMap::from([
                            (
                                "alien-vault-prefix".to_string(),
                                "acme-1a2b-secrets".to_string(),
                            ),
                            ("alien-vault".to_string(), "secrets".to_string()),
                        ]))
            })
            .returning(|_, _| Ok(Secret::default()));

        vault_with(client, "acme-1a2b-secrets", "secrets")
            .set_secret("api-key", "v")
            .await
            .unwrap();
    }

    /// Adding a version to an existing secret creates nothing and changes no label.
    #[tokio::test]
    async fn updating_an_existing_secret_does_not_touch_its_labels() {
        let mut client = MockSecretManagerApi::new();
        client
            .expect_add_secret_version()
            .times(1)
            .returning(|_, _| Ok(version()));
        client.expect_create_secret().never();

        vault_with(client, "acme-1a2b-secrets", "secrets")
            .set_secret("api-key", "v2")
            .await
            .unwrap();
    }

    #[test]
    fn marker_labels_meet_the_label_rules_even_for_long_prefixes() {
        let labels = vault_with(
            MockSecretManagerApi::new(),
            &format!("{}-secrets", "a".repeat(60)),
            "secrets",
        )
        .marker_labels();
        assert_eq!(labels.len(), 2);
        for value in labels.values() {
            assert!((1..=63).contains(&value.len()), "{value}");
            assert!(value
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-'));
        }
    }
}
