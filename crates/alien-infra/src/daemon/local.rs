use std::path::PathBuf;
use std::time::Duration;
use tracing::{debug, info};

use crate::core::{environment_variables::EnvironmentVariableBuilder, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use alien_core::{Daemon, DaemonCode, DaemonOutputs, ResourceOutputs, ResourceStatus};
use alien_error::{AlienError, Context};
use alien_macros::controller;

#[controller]
pub struct LocalDaemonController {
    /// Path to the extracted OCI image directory.
    pub(crate) extracted_image_path: Option<PathBuf>,
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

        let env_vars = EnvironmentVariableBuilder::try_new(&config.environment)?
            .add_standard_alien_env_vars(ctx)?
            .add_passthrough_transport_env_vars()
            .add_linked_resources(&config.links, ctx, &config.id)
            .await?
            .build();

        manager
            .start_daemon(&config.id, env_vars)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to start daemon runtime".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

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
        Some(ResourceOutputs::new(DaemonOutputs {
            daemon_name: String::new(),
            running: true,
        }))
    }
}
