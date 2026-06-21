use std::time::Duration;
use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{
    HeartbeatBackend, LocalQueueHeartbeatData, ObservedHealth, Platform, ProviderLifecycleState,
    Queue, QueueHeartbeatData, QueueHeartbeatStatus, QueueOutputs, ResourceHeartbeat,
    ResourceHeartbeatData, ResourceOutputs, ResourceStatus,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;
use chrono::Utc;

#[controller]
pub struct LocalQueueController {
    /// Path to the queue database directory on the local filesystem
    pub(crate) queue_path: Option<String>,
}

#[controller]
impl LocalQueueController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Queue>()?;

        let queue_manager = ctx
            .service_provider
            .get_local_queue_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "queue_manager".to_string(),
                })
            })?;

        info!(queue_id=%config.id, "Creating local queue");

        let queue_path = queue_manager.create_queue(&config.id).await.context(
            ErrorData::CloudPlatformError {
                message: format!("Failed to create queue database for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            },
        )?;

        info!(
            queue_id=%config.id,
            path=%queue_path.display(),
            "Local queue created successfully"
        );

        self.queue_path = Some(queue_path.display().to_string());

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
        let config = ctx.desired_resource_config::<Queue>()?;

        let queue_manager = ctx
            .service_provider
            .get_local_queue_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "queue_manager".to_string(),
                })
            })?;

        queue_manager
            .check_health(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Queue health check failed for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            })?;

        let queue_path =
            queue_manager
                .get_queue_path(&config.id)
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to resolve queue path for '{}'", config.id),
                    resource_id: Some(config.id.clone()),
                })?;

        self.queue_path = Some(queue_path.display().to_string());

        emit_local_queue_heartbeat(ctx, &config.id, self.queue_path.as_deref());

        debug!(queue_id=%config.id, "Queue health check passed");

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
        let config = ctx.desired_resource_config::<Queue>()?;

        info!(queue_id=%config.id, "Updating local queue (no-op)");

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
        let config = ctx.desired_resource_config::<Queue>()?;

        info!(queue_id=%config.id, "Starting queue deletion");

        if self.queue_path.is_some() {
            if let Some(queue_manager) = ctx.service_provider.get_local_queue_manager() {
                queue_manager.delete_queue(&config.id).await.context(
                    ErrorData::CloudPlatformError {
                        message: format!("Failed to delete queue database for '{}'", config.id),
                        resource_id: Some(config.id.clone()),
                    },
                )?;

                info!(queue_id=%config.id, "Queue database deleted");
            }
        } else {
            info!(queue_id=%config.id, "No queue database to delete (creation failed early)");
        }

        self.queue_path = None;

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
        self.queue_path.as_ref().map(|path| {
            ResourceOutputs::new(QueueOutputs {
                queue_name: path.clone(),
                identifier: Some(path.clone()),
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, QueueBinding};

        if let Some(queue_path) = &self.queue_path {
            let binding = QueueBinding::local(BindingValue::value(queue_path.clone()));
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

fn emit_local_queue_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    queue_path: Option<&str>,
) {
    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: Queue::RESOURCE_TYPE,
        controller_platform: Platform::Local,
        backend: HeartbeatBackend::Local,
        source: Default::default(),
        alien_resource_id: None,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Queue(QueueHeartbeatData::Local(LocalQueueHeartbeatData {
            status: QueueHeartbeatStatus {
                health: ObservedHealth::Healthy,
                lifecycle: ProviderLifecycleState::Running,
                message: Some(format!("Local queue '{}' is reachable", resource_id)),
                stale: false,
                partial: false,
                collection_issues: vec![],
            },
            name: resource_id.to_string(),
            path: queue_path.map(ToString::to_string),
            service_status: Some("reachable".to_string()),
        })),
        raw: vec![],
    });
}
