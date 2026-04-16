use std::time::Duration;
use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{ResourceOutputs, ResourceStatus, Vault, VaultOutputs};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;

#[controller]
pub struct LocalVaultController {
    /// Path to the vault directory on the local filesystem
    pub(crate) vault_path: Option<String>,
}

#[controller]
impl LocalVaultController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Vault>()?;

        let vault_manager = ctx
            .service_provider
            .get_local_vault_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "vault_manager".to_string(),
                })
            })?;

        info!(vault_id=%config.id, "Creating local vault");

        // Create vault directory using the manager
        let vault_path = vault_manager.create_vault(&config.id).await.context(
            ErrorData::CloudPlatformError {
                message: format!("Failed to create vault directory for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            },
        )?;

        info!(
            vault_id=%config.id,
            path=%vault_path.display(),
            "Local vault created successfully"
        );

        self.vault_path = Some(vault_path.display().to_string());

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ────────────────────────────────
    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Vault>()?;

        // Verify vault still exists via service manager health check
        let vault_manager = ctx
            .service_provider
            .get_local_vault_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "vault_manager".to_string(),
                })
            })?;

        vault_manager
            .check_health(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Vault health check failed for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            })?;

        debug!(vault_id=%config.id, "Vault health check passed");

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Vault>()?;

        info!(vault_id=%config.id, "Updating local vault (no-op)");

        // For local vault, updates are typically no-op since the vault path doesn't change
        // The vault directory persists with its encrypted secrets unchanged

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────
    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Vault>()?;

        info!(vault_id=%config.id, "Starting vault deletion");

        // Delete vault directory if vault_path is set
        if self.vault_path.is_some() {
            if let Some(vault_manager) = ctx.service_provider.get_local_vault_manager() {
                vault_manager.delete_vault(&config.id).await.context(
                    ErrorData::CloudPlatformError {
                        message: format!("Failed to delete vault directory for '{}'", config.id),
                        resource_id: Some(config.id.clone()),
                    },
                )?;

                info!(vault_id=%config.id, "Vault directory deleted");
            }
        } else {
            info!(vault_id=%config.id, "No vault directory to delete (creation failed early)");
        }

        self.vault_path = None;

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── TERMINALS ────────────────────────────────
    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );

    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );

    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);

    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        self.vault_path.as_ref().map(|path| {
            ResourceOutputs::new(VaultOutputs {
                vault_id: path.clone(),
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::VaultBinding;

        if let Some(vault_path) = &self.vault_path {
            // Extract vault name from path (last component)
            let vault_name = std::path::Path::new(vault_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");

            let binding = VaultBinding::local(vault_name.to_string(), vault_path.clone());

            Ok(Some(
                serde_json::to_value(&binding).into_alien_error().context(
                    ErrorData::ResourceStateSerializationFailed {
                        resource_id: "binding".to_string(),
                        message: "Failed to serialize binding parameters".to_string(),
                    },
                )?,
            ))
        } else {
            Ok(None)
        }
    }
}

impl LocalVaultController {
    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(vault_path: &str) -> Self {
        Self {
            state: LocalVaultState::Ready,
            vault_path: Some(vault_path.to_string()),
            _internal_stay_count: None,
        }
    }
}
