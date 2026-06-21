use std::path::PathBuf;
use std::time::Duration;
use tracing::{debug, info};

use crate::core::{environment_variables::EnvironmentVariableBuilder, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use alien_core::{
    Daemon, DaemonCode, DaemonHeartbeatData, DaemonOutputs, HeartbeatBackend,
    LocalDaemonHeartbeatData, LocalRuntimeUnitKind, LocalRuntimeUnitStatus, ObservedHealth,
    Platform, ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus, WorkloadHeartbeatStatus,
};
use alien_error::{AlienError, Context};
use alien_macros::controller;
use chrono::Utc;

#[controller]
pub struct LocalDaemonController {
    /// Path to the extracted OCI image directory.
    pub(crate) extracted_image_path: Option<PathBuf>,
    /// Name used by the local daemon runtime.
    pub(crate) daemon_name: Option<String>,
}

#[controller]
impl LocalDaemonController {
    #[flow_entry(Create)]
    #[handler(
        state = ExtractingImage,
        on_failure = ProvisionFailed,
        status = ResourceStatus::Provisioning
    )]
    async fn extracting_image(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Daemon>()?;

        info!(daemon_id = %config.id, "Extracting daemon OCI image");

        let manager = ctx
            .service_provider
            .get_local_worker_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "LocalWorkerManager".to_string(),
                })
            })?;

        let image_ref = match &config.code {
            DaemonCode::Image { image } => image.clone(),
            DaemonCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Local platform does not support building daemon source code directly. Build the image first and use DaemonCode::Image.".to_string(),
                    resource_id: Some(config.id.clone()),
                }));
            }
        };

        let token = ctx
            .deployment_config
            .deployment_token
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "deployment_token is required to pull images from the registry proxy"
                        .to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;
        let extracted_path = manager
            .extract_daemon_image(&config.id, &image_ref, Some(token))
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to extract daemon OCI image".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.extracted_image_path = Some(extracted_path);
        self.daemon_name = Some(config.id.clone());

        Ok(HandlerAction::Continue {
            state: StartingProcess,
            suggested_delay: None,
        })
    }

    #[handler(
        state = StartingProcess,
        on_failure = ProvisionFailed,
        status = ResourceStatus::Provisioning
    )]
    async fn starting_process(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Daemon>()?;
        let manager = ctx
            .service_provider
            .get_local_worker_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "LocalWorkerManager".to_string(),
                })
            })?;

        info!(daemon_id = %config.id, "Starting daemon runtime");

        let mut env_builder = EnvironmentVariableBuilder::try_new(&config.environment)?
            .add_standard_alien_env_vars(ctx)?
            .add_linked_resources(&config.links, ctx, &config.id)
            .await?;

        if config.commands_enabled {
            env_builder = env_builder.add_passthrough_transport_env_vars();
        }

        let env_vars = env_builder.build();

        manager.start_daemon(&config.id, env_vars).await.context(
            ErrorData::CloudPlatformError {
                message: "Failed to start daemon runtime".to_string(),
                resource_id: Some(config.id.clone()),
            },
        )?;
        self.daemon_name = Some(config.id.clone());

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Daemon>()?;
        let manager = ctx
            .service_provider
            .get_local_worker_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "LocalWorkerManager".to_string(),
                })
            })?;

        manager
            .check_daemon_health(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Daemon health check failed for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            })?;

        debug!(daemon_id=%config.id, "Daemon health check passed");

        emit_local_daemon_heartbeat(ctx, &config, self.extracted_image_path.as_ref());

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = StoppingForUpdate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating
    )]
    async fn stopping_for_update(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Daemon>()?;
        let manager = ctx
            .service_provider
            .get_local_worker_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "LocalWorkerManager".to_string(),
                })
            })?;

        manager
            .stop_daemon(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to stop daemon for update".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        Ok(HandlerAction::Continue {
            state: ExtractingImage,
            suggested_delay: None,
        })
    }

    #[flow_entry(Delete)]
    #[handler(
        state = Deleting,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting
    )]
    async fn deleting(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Daemon>()?;
        let manager = ctx
            .service_provider
            .get_local_worker_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "LocalWorkerManager".to_string(),
                })
            })?;

        manager
            .delete_daemon(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to delete daemon".to_string(),
                resource_id: Some(config.id.clone()),
            })?;
        self.daemon_name = None;
        self.extracted_image_path = None;

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);
    terminal_state!(
        state = ProvisionFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        self.daemon_name.as_ref().map(|daemon_name| {
            ResourceOutputs::new(DaemonOutputs {
                daemon_name: daemon_name.clone(),
                running: true,
            })
        })
    }
}

fn emit_local_daemon_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    config: &Daemon,
    extracted_image_path: Option<&PathBuf>,
) {
    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: config.id.clone(),
        resource_type: Daemon::RESOURCE_TYPE,
        controller_platform: Platform::Local,
        backend: HeartbeatBackend::Local,
            source: Default::default(),
            alien_resource_id: None,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Daemon(DaemonHeartbeatData::Local(LocalDaemonHeartbeatData {
            status: WorkloadHeartbeatStatus {
                health: ObservedHealth::Healthy,
                lifecycle: ProviderLifecycleState::Running,
                message: Some(format!("Local daemon '{}' is running", config.id)),
                stale: false,
                partial: false,
                collection_issues: vec![],
            },
            daemon_name: config.id.clone(),
            runtime_id: config.id.clone(),
            pid: None,
            command_supported: config.commands_enabled,
            image_path_present: extracted_image_path
                .map(|path| path.exists())
                .unwrap_or(false),
            restart_count: None,
            exit_reason: None,
            daemon_instance: Some(LocalRuntimeUnitStatus {
                unit_id: config.id.clone(),
                name: config.id.clone(),
                kind: LocalRuntimeUnitKind::Daemon,
                ready: extracted_image_path
                    .map(|path| path.exists())
                    .unwrap_or(false),
                phase: Some("running".to_string()),
                pid: None,
                restart_count: None,
                cpu: None,
                memory: None,
            }),
            events: vec![],
        })),
        raw: vec![],
    });
}
