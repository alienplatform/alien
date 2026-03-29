use std::time::Duration;
use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{ArtifactRegistry, ArtifactRegistryOutputs, ResourceOutputs, ResourceStatus};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;

#[controller]
pub struct LocalArtifactRegistryController {
    /// URL where the registry is accessible (e.g., "localhost:5000")
    pub(crate) registry_url: Option<String>,
}

#[controller]
impl LocalArtifactRegistryController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        let registry_manager = ctx
            .service_provider
            .get_local_artifact_registry_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "artifact_registry_manager".to_string(),
                })
            })?;

        info!(registry_id=%config.id, "Starting local artifact registry");

        // Start the OCI registry server
        let registry_url = registry_manager.start_registry(&config.id).await.context(
            ErrorData::CloudPlatformError {
                message: format!("Failed to start artifact registry '{}'", config.id),
                resource_id: Some(config.id.clone()),
            },
        )?;

        info!(
            registry_id=%config.id,
            url=%registry_url,
            "Local artifact registry started successfully"
        );

        self.registry_url = Some(registry_url);

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
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        // Verify registry is still running via service manager health check
        let registry_manager = ctx
            .service_provider
            .get_local_artifact_registry_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "artifact_registry_manager".to_string(),
                })
            })?;

        registry_manager
            .check_health(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Registry health check failed for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            })?;

        // Query the CURRENT registry URL from the manager (in case auto-recovery changed ports)
        // This ensures controller state stays in sync with runtime reality
        let current_url = registry_manager
            .get_registry_url(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to get current registry URL for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            })?;

        // Update controller state if URL changed (e.g., due to process restart)
        if self.registry_url.as_ref() != Some(&current_url) {
            info!(
                registry_id=%config.id,
                old_url=?self.registry_url,
                new_url=%current_url,
                "Registry URL changed (likely due to auto-recovery), updating controller state"
            );
            self.registry_url = Some(current_url);
        }

        debug!(registry_id=%config.id, "Registry health check passed");

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
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        info!(registry_id=%config.id, "Updating local artifact registry");

        // Re-query the current registry URL from the manager.
        // This is important because:
        // 1. When the dev server process restarts, the artifact registry manager auto-recovers
        //    registries on potentially different ports
        // 2. The controller's registry_url field may have stale data from a previous run
        // 3. We need to update it to reflect the current runtime state
        let registry_manager = ctx
            .service_provider
            .get_local_artifact_registry_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "artifact_registry_manager".to_string(),
                })
            })?;

        // Get current URL from manager (may have changed due to auto-recovery)
        let current_url = registry_manager
            .get_registry_url(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to get current registry URL for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            })?;

        if self.registry_url.as_ref() != Some(&current_url) {
            info!(
                registry_id=%config.id,
                old_url=?self.registry_url,
                new_url=%current_url,
                "Registry URL changed (likely due to process restart)"
            );
            self.registry_url = Some(current_url);
        }

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
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        info!(registry_id=%config.id, "Stopping artifact registry");

        // Stop the registry and delete storage if registry_url is set
        if self.registry_url.is_some() {
            if let Some(registry_manager) =
                ctx.service_provider.get_local_artifact_registry_manager()
            {
                // Remove the registry (stops server and deletes metadata)
                registry_manager.remove_registry(&config.id).await.context(
                    ErrorData::CloudPlatformError {
                        message: format!("Failed to stop artifact registry '{}'", config.id),
                        resource_id: Some(config.id.clone()),
                    },
                )?;

                info!(registry_id=%config.id, "Artifact registry stopped");

                // Delete the registry storage (also deletes metadata)
                registry_manager
                    .delete_registry_storage(&config.id)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete artifact registry storage for '{}'",
                            config.id
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;

                info!(registry_id=%config.id, "Artifact registry storage deleted");
            }
        } else {
            info!(registry_id=%config.id, "No registry to stop (creation failed early)");
        }

        self.registry_url = None;

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
        self.registry_url.as_ref().map(|url| {
            ResourceOutputs::new(ArtifactRegistryOutputs {
                registry_id: url.clone(),
                registry_endpoint: url.clone(),
                pull_role: None,
                push_role: None,
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{ArtifactRegistryBinding, BindingValue};

        if let Some(registry_url) = &self.registry_url {
            let binding = ArtifactRegistryBinding::local(
                BindingValue::value(registry_url.clone()),
                BindingValue::value(None::<String>),
            );
            Ok(Some(serde_json::to_value(binding).into_alien_error().context(
                ErrorData::ResourceStateSerializationFailed {
                    resource_id: "binding".to_string(),
                    message: "Failed to serialize binding parameters".to_string(),
                },
            )?))
        } else {
            Ok(None)
        }
    }
}

impl LocalArtifactRegistryController {
    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(registry_url: &str) -> Self {
        Self {
            state: LocalArtifactRegistryState::Ready,
            registry_url: Some(registry_url.to_string()),
            _internal_stay_count: None,
        }
    }
}
