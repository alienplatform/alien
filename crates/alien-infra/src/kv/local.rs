use std::time::Duration;
use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{Kv, KvOutputs, ResourceOutputs, ResourceStatus};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;

#[controller]
pub struct LocalKvController {
    /// Path to the KV database directory on the local filesystem
    pub(crate) kv_path: Option<String>,
}

#[controller]
impl LocalKvController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Kv>()?;

        let kv_manager = ctx.service_provider.get_local_kv_manager().ok_or_else(|| {
            AlienError::new(ErrorData::LocalServicesNotAvailable {
                service_name: "kv_manager".to_string(),
            })
        })?;

        info!(kv_id=%config.id, "Creating local KV");

        // Create KV database directory using the manager
        let kv_path =
            kv_manager
                .create_kv(&config.id)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create KV database for '{}'", config.id),
                    resource_id: Some(config.id.clone()),
                })?;

        info!(
            kv_id=%config.id,
            path=%kv_path.display(),
            "Local KV created successfully"
        );

        self.kv_path = Some(kv_path.display().to_string());

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
        let config = ctx.desired_resource_config::<Kv>()?;

        // Verify KV still exists via service manager health check
        let kv_manager = ctx.service_provider.get_local_kv_manager().ok_or_else(|| {
            AlienError::new(ErrorData::LocalServicesNotAvailable {
                service_name: "kv_manager".to_string(),
            })
        })?;

        kv_manager
            .check_health(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("KV health check failed for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            })?;

        debug!(kv_id=%config.id, "KV health check passed");

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
        let config = ctx.desired_resource_config::<Kv>()?;

        info!(kv_id=%config.id, "Updating local KV (no-op)");

        // For local KV, updates are typically no-op since the database path doesn't change
        // The sled database persists with its contents unchanged

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
        let config = ctx.desired_resource_config::<Kv>()?;

        info!(kv_id=%config.id, "Starting KV deletion");

        // Delete KV database if kv_path is set
        if self.kv_path.is_some() {
            if let Some(kv_manager) = ctx.service_provider.get_local_kv_manager() {
                kv_manager
                    .delete_kv(&config.id)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete KV database for '{}'", config.id),
                        resource_id: Some(config.id.clone()),
                    })?;

                info!(kv_id=%config.id, "KV database deleted");
            }
        } else {
            info!(kv_id=%config.id, "No KV database to delete (creation failed early)");
        }

        self.kv_path = None;

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
        self.kv_path.as_ref().map(|path| {
            ResourceOutputs::new(KvOutputs {
                store_name: path.clone(),
                identifier: Some(path.clone()),
                endpoint: None,
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, KvBinding};

        if let Some(kv_path) = &self.kv_path {
            let binding = KvBinding::local(BindingValue::value(kv_path.clone()));
            Ok(Some(
                serde_json::to_value(binding).into_alien_error().context(
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

impl LocalKvController {
    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(kv_path: &str) -> Self {
        Self {
            state: LocalKvState::Ready,
            kv_path: Some(kv_path.to_string()),
            _internal_stay_count: None,
        }
    }
}
