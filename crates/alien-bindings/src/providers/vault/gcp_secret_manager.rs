use crate::error::{ErrorData, Result};
use crate::traits::SecretPresence;
use alien_error::{Context, ContextError};
use alien_gcp_clients::secret_manager::{
    AddSecretVersionRequest, AutomaticReplication, Replication, ReplicationPolicy, Secret,
    SecretManagerApi, SecretManagerClient, SecretPayload, SecretVersionState,
};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as base64_standard, Engine as _};
use std::sync::Arc;
use tracing::{debug, warn};

/// GCP Secret Manager vault binding implementation
#[derive(Debug)]
pub struct GcpSecretManagerVault {
    client: Arc<SecretManagerClient>,
    vault_prefix: String,
    project_id: String,
}

impl GcpSecretManagerVault {
    /// Create a new GCP Secret Manager vault binding
    pub fn new(client: Arc<SecretManagerClient>, vault_prefix: String, project_id: String) -> Self {
        Self {
            client,
            vault_prefix,
            project_id,
        }
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

                let secret = Secret::builder().replication(replication).build();

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
