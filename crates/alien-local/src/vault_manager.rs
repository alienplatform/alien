use crate::error::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use std::path::PathBuf;
use tracing::{debug, info};

/// Manager for local vault resources.
///
/// Creates directories for vault storage. The binding handles storing and
/// retrieving secrets directly (encryption will be added later).
///
/// # State Scoping
/// All vault directories are created under `{state_dir}/vault/{resource_id}/`.
/// The `state_dir` should be scoped by agent ID (e.g., `~/.alien-cli/<agent_id>`)
/// to avoid conflicts between agents.
#[derive(Debug, Clone)]
pub struct LocalVaultManager {
    state_dir: PathBuf,
}

impl LocalVaultManager {
    /// Creates a new vault manager.
    ///
    /// # Arguments
    /// * `state_dir` - Base directory for all local platform state
    pub fn new(state_dir: PathBuf) -> Self {
        Self { state_dir }
    }

    /// Creates a vault directory for a resource.
    ///
    /// The binding will store secrets in this directory.
    ///
    /// # Arguments
    /// * `id` - Resource identifier
    ///
    /// # Returns
    /// Path to the vault directory
    ///
    /// # Note
    /// This is idempotent - can be called multiple times (e.g., during reconciliation).
    /// Uses `create_dir_all` which succeeds even if the directory already exists.
    pub async fn create_vault(&self, id: &str) -> Result<PathBuf> {
        let vault_dir = self.state_dir.join("vault").join(id);

        tokio::fs::create_dir_all(&vault_dir)
            .await
            .into_alien_error()
            .context(ErrorData::LocalDirectoryError {
                path: vault_dir.display().to_string(),
                operation: "create".to_string(),
                reason: "Failed to create vault directory".to_string(),
            })?;

        info!(
            vault_id = %id,
            path = %vault_dir.display(),
            "Vault directory created"
        );

        Ok(vault_dir)
    }

    /// Gets the path to a vault directory.
    ///
    /// Returns an error if the vault doesn't exist.
    pub fn get_vault_path(&self, id: &str) -> Result<PathBuf> {
        use alien_error::AlienError;

        let vault_path = self.state_dir.join("vault").join(id);

        if !vault_path.exists() {
            return Err(AlienError::new(ErrorData::ServiceResourceNotFound {
                resource_id: id.to_string(),
                resource_type: "vault".to_string(),
            }));
        }

        Ok(vault_path)
    }

    /// Deletes a vault directory and all its contents.
    ///
    /// # Arguments
    /// * `id` - Vault identifier
    ///
    /// # Note
    /// This is idempotent - succeeds even if the vault doesn't exist.
    pub async fn delete_vault(&self, id: &str) -> Result<()> {
        let vault_dir = self.state_dir.join("vault").join(id);

        if vault_dir.exists() {
            tokio::fs::remove_dir_all(&vault_dir)
                .await
                .into_alien_error()
                .context(ErrorData::LocalDirectoryError {
                    path: vault_dir.display().to_string(),
                    operation: "delete".to_string(),
                    reason: "Failed to delete vault directory".to_string(),
                })?;

            debug!(
                vault_id = %id,
                path = %vault_dir.display(),
                "Vault deleted"
            );
        } else {
            debug!(
                vault_id = %id,
                path = %vault_dir.display(),
                "Vault directory does not exist (already deleted)"
            );
        }

        Ok(())
    }

    /// Checks if a vault directory exists.
    pub fn vault_exists(&self, id: &str) -> bool {
        self.state_dir.join("vault").join(id).exists()
    }

    /// Verifies that a vault resource exists and is healthy by actually accessing it.
    ///
    /// This performs a real access operation similar to what bindings do, ensuring
    /// the vault directory is accessible and readable.
    ///
    /// # Arguments
    /// * `id` - Resource identifier
    ///
    /// # Returns
    /// Ok(()) if vault exists and is healthy, error otherwise
    pub async fn check_health(&self, id: &str) -> Result<()> {
        use alien_error::AlienError;

        let vault_path = self.state_dir.join("vault").join(id);

        if !vault_path.exists() {
            return Err(AlienError::new(ErrorData::ServiceResourceNotFound {
                resource_id: id.to_string(),
                resource_type: "vault".to_string(),
            }));
        }

        if !vault_path.is_dir() {
            return Err(AlienError::new(ErrorData::LocalDirectoryError {
                path: vault_path.display().to_string(),
                operation: "health_check".to_string(),
                reason: "Expected directory but found file".to_string(),
            }));
        }

        // Try to actually read the directory to ensure it's accessible
        std::fs::read_dir(&vault_path).into_alien_error().context(
            ErrorData::LocalDirectoryError {
                path: vault_path.display().to_string(),
                operation: "read".to_string(),
                reason: "Failed to read vault directory".to_string(),
            },
        )?;

        Ok(())
    }

    /// Gets the binding configuration for a vault resource.
    ///
    /// # Arguments
    /// * `id` - Resource identifier
    ///
    /// # Returns
    /// VaultBinding configured for this vault directory, or error if vault doesn't exist
    pub fn get_binding(&self, id: &str) -> Result<alien_core::bindings::VaultBinding> {
        use alien_core::bindings::VaultBinding;

        let vault_path = self.get_vault_path(id)?;

        Ok(VaultBinding::local(
            id,
            vault_path.to_string_lossy().to_string(),
        ))
    }
}
