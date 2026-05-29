use std::time::Duration;
use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{
    HeartbeatBackend, LocalStorageHeartbeatData, ObservedHealth, Platform, ProviderLifecycleState,
    ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs, ResourceStatus, Storage,
    StorageHeartbeatData, StorageHeartbeatStatus, StorageOutputs,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;
use chrono::{DateTime, Utc};

#[controller]
pub struct LocalStorageController {
    /// Path to the storage directory on the local filesystem
    pub(crate) storage_path: Option<String>,
}

#[controller]
impl LocalStorageController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Storage>()?;

        let storage_manager = ctx
            .service_provider
            .get_local_storage_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "storage_manager".to_string(),
                })
            })?;

        info!(storage_id=%config.id, "Creating local storage");

        // Create storage directory using the manager
        let storage_path = storage_manager.create_storage(&config.id).await.context(
            ErrorData::CloudPlatformError {
                message: format!("Failed to create storage directory for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            },
        )?;

        info!(
            storage_id=%config.id,
            path=%storage_path.display(),
            "Local storage created successfully"
        );

        self.storage_path = Some(storage_path.display().to_string());

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
        let config = ctx.desired_resource_config::<Storage>()?;

        // Verify storage still exists via service manager health check
        let storage_manager = ctx
            .service_provider
            .get_local_storage_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "storage_manager".to_string(),
                })
            })?;

        storage_manager
            .check_health(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Storage health check failed for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            })?;

        if let Some(storage_path) = &self.storage_path {
            emit_local_storage_heartbeat(ctx, &config.id, storage_path);
        }

        debug!(storage_id=%config.id, "Storage health check passed");

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
        let config = ctx.desired_resource_config::<Storage>()?;

        info!(storage_id=%config.id, "Updating local storage (no-op)");

        // For local storage, updates are typically no-op since the directory path doesn't change
        // The directory persists with its contents unchanged

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
        let config = ctx.desired_resource_config::<Storage>()?;

        info!(storage_id=%config.id, "Starting storage deletion");

        // Delete storage directory if storage_path is set
        if self.storage_path.is_some() {
            if let Some(storage_manager) = ctx.service_provider.get_local_storage_manager() {
                storage_manager.delete_storage(&config.id).await.context(
                    ErrorData::CloudPlatformError {
                        message: format!("Failed to delete storage directory for '{}'", config.id),
                        resource_id: Some(config.id.clone()),
                    },
                )?;

                info!(storage_id=%config.id, "Storage directory deleted");
            }
        } else {
            info!(storage_id=%config.id, "No storage directory to delete (creation failed early)");
        }

        self.storage_path = None;

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
        self.storage_path.as_ref().map(|path| {
            ResourceOutputs::new(StorageOutputs {
                bucket_name: path.clone(),
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, StorageBinding};

        if let Some(storage_path) = &self.storage_path {
            // Use file:// URL for the storage path
            let storage_url = format!("file://{}/", storage_path);

            let binding = StorageBinding::local(BindingValue::value(storage_url));
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

fn emit_local_storage_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    storage_path: &str,
) {
    let metadata = std::fs::metadata(storage_path).ok();
    let path_exists = metadata.is_some();
    let is_directory = metadata.as_ref().map(|metadata| metadata.is_dir());
    let readonly = metadata
        .as_ref()
        .map(|metadata| metadata.permissions().readonly());
    let modified_at = metadata
        .and_then(|metadata| metadata.modified().ok())
        .map(DateTime::<Utc>::from);
    let path_message = if path_exists {
        "Local storage backing path is reachable"
    } else {
        "Local storage manager is healthy, but backing path metadata is not reachable"
    };

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: Storage::RESOURCE_TYPE,
        controller_platform: Platform::Local,
        backend: HeartbeatBackend::Local,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Storage(StorageHeartbeatData::Local(
            LocalStorageHeartbeatData {
                status: StorageHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: Some(path_message.to_string()),
                    stale: false,
                    partial: !path_exists,
                    collection_issues: if path_exists {
                        vec![]
                    } else {
                        vec![alien_core::HeartbeatCollectionIssue {
                            source: "path-metadata".to_string(),
                            reason: alien_core::HeartbeatCollectionIssueReason::CollectionFailed,
                            severity: alien_core::HeartbeatIssueSeverity::Warning,
                            message: format!(
                                "Failed to read metadata for local storage backing path '{}'",
                                storage_path
                            ),
                        }]
                    },
                },
                path: storage_path.to_string(),
                path_exists,
                is_directory,
                readonly,
                modified_at,
                events: vec![],
            },
        )),
        raw: vec![],
    });
}

impl LocalStorageController {
    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(storage_path: &str) -> Self {
        Self {
            state: LocalStorageState::Ready,
            storage_path: Some(storage_path.to_string()),
            _internal_stay_count: None,
        }
    }
}
