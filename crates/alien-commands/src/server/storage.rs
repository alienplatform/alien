use std::sync::Arc;
use std::time::Duration;

use alien_bindings::traits::Storage;
use alien_error::Context;
use chrono::{DateTime, Utc};

#[cfg(feature = "server")]
use object_store::path::Path as StoragePath;

use crate::error::{ErrorData, Result};

/// Helper functions for storage operations in ARC server
pub struct ArcStorageHelper {
    storage: Arc<dyn Storage>,
}

impl ArcStorageHelper {
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self { storage }
    }

    /// Generate a presigned PUT URL for command params upload
    pub async fn generate_params_put_url(
        &self,
        command_id: &str,
        expires_in: Duration,
    ) -> Result<String> {
        let path = StoragePath::from(format!("arc/commands/{}/params", command_id));
        let presigned = self
            .storage
            .presigned_put(&path, expires_in)
            .await
            .context(ErrorData::StorageOperationFailed {
                message: "Failed to create presigned PUT URL".to_string(),
                operation: Some("presigned_put".to_string()),
                path: Some(path.to_string()),
            })?;

        Ok(presigned.url())
    }

    /// Generate a presigned GET URL for command params download
    pub async fn generate_params_get_url(
        &self,
        command_id: &str,
        expires_in: Duration,
    ) -> Result<String> {
        let path = StoragePath::from(format!("arc/commands/{}/params", command_id));
        let presigned = self
            .storage
            .presigned_get(&path, expires_in)
            .await
            .context(ErrorData::StorageOperationFailed {
                message: "Failed to create presigned GET URL".to_string(),
                operation: Some("presigned_get".to_string()),
                path: Some(path.to_string()),
            })?;

        Ok(presigned.url())
    }

    /// Generate a presigned PUT URL for response body upload
    pub async fn generate_response_put_url(
        &self,
        command_id: &str,
        expires_in: Duration,
    ) -> Result<String> {
        let path = StoragePath::from(format!("arc/commands/{}/response", command_id));
        let presigned = self
            .storage
            .presigned_put(&path, expires_in)
            .await
            .context(ErrorData::StorageOperationFailed {
                message: "Failed to create response PUT URL".to_string(),
                operation: Some("presigned_put".to_string()),
                path: Some(path.to_string()),
            })?;

        Ok(presigned.url())
    }

    /// Generate a presigned GET URL for response body download
    pub async fn generate_response_get_url(
        &self,
        command_id: &str,
        expires_in: Duration,
    ) -> Result<String> {
        let path = StoragePath::from(format!("arc/commands/{}/response", command_id));
        let presigned = self
            .storage
            .presigned_get(&path, expires_in)
            .await
            .context(ErrorData::StorageOperationFailed {
                message: "Failed to create response GET URL".to_string(),
                operation: Some("presigned_get".to_string()),
                path: Some(path.to_string()),
            })?;

        Ok(presigned.url())
    }

    /// Clean up storage objects for a completed command
    pub async fn cleanup_command_storage(&self, command_id: &str) -> Result<()> {
        let params_path = StoragePath::from(format!("arc/commands/{}/params", command_id));
        let response_path = StoragePath::from(format!("arc/commands/{}/response", command_id));

        // Best effort cleanup - don't fail if objects don't exist
        if let Err(e) = self.storage.delete(&params_path).await {
            tracing::warn!("Failed to cleanup params for command {}: {}", command_id, e);
        }

        if let Err(e) = self.storage.delete(&response_path).await {
            tracing::warn!(
                "Failed to cleanup response for command {}: {}",
                command_id,
                e
            );
        }

        Ok(())
    }

    /// Get the base storage URL for this binding
    pub fn get_base_url(&self) -> String {
        self.storage.get_url().to_string()
    }
}

/// Configuration for storage URL generation
#[derive(Debug, Clone)]
pub struct StorageUrlConfig {
    /// Default expiration time for presigned URLs
    pub default_expires_in: Duration,
    /// Maximum expiration time allowed
    pub max_expires_in: Duration,
}

impl Default for StorageUrlConfig {
    fn default() -> Self {
        Self {
            default_expires_in: Duration::from_secs(3600), // 1 hour
            max_expires_in: Duration::from_secs(24 * 3600), // 24 hours
        }
    }
}

impl StorageUrlConfig {
    /// Validate and clamp expiration time to allowed range
    pub fn validate_expires_in(&self, requested: Duration) -> Duration {
        if requested > self.max_expires_in {
            self.max_expires_in
        } else if requested.is_zero() {
            self.default_expires_in
        } else {
            requested
        }
    }

    /// Calculate expiration time from a target datetime
    pub fn expires_in_until(&self, target: DateTime<Utc>) -> Duration {
        let now = Utc::now();
        if target <= now {
            Duration::from_secs(60) // Minimum 1 minute
        } else {
            let diff = target.signed_duration_since(now);
            let seconds = diff.num_seconds().max(60) as u64; // Minimum 1 minute
            self.validate_expires_in(Duration::from_secs(seconds))
        }
    }
}
