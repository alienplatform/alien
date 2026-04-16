use std::path::PathBuf;
use std::time::Duration;
use tracing::{debug, error, info};

use crate::core::{environment_variables::EnvironmentVariableBuilder, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use alien_core::{
    Function, FunctionCode, FunctionOutputs, ResourceOutputs as CoreResourceOutputs, ResourceStatus,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;

/// Shared trigger service shutdown handle. The controller is Clone+Serialize
/// (required by the macro), so we can't store JoinHandle/broadcast::Sender directly.
/// Instead, this static holds the shutdown sender keyed by function ID.
static TRIGGER_SHUTDOWNS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, tokio::sync::broadcast::Sender<()>>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

#[controller]
pub struct LocalFunctionController {
    /// Path to the extracted OCI image directory
    pub(crate) extracted_image_path: Option<PathBuf>,
    /// URL where the function is accessible
    pub(crate) function_url: Option<String>,
}

#[controller]
impl LocalFunctionController {
    // ─────────────── CREATE FLOW ───────────────────────────────────────────

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
        let config = ctx.desired_resource_config::<Function>()?;

        info!(function_id = %config.id, "Extracting function OCI image");

        // Get the function manager from the service provider
        let func_mgr = ctx
            .service_provider
            .get_local_function_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "LocalFunctionManager".to_string(),
                })
            })?;

        // Determine the image reference from the function code
        let image_ref = match &config.code {
            FunctionCode::Image { image } => image.clone(),
            FunctionCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Local platform does not support building from source code directly. Please build the image first and use FunctionCode::Image.".to_string(),
                    resource_id: Some(config.id.clone()),
                }));
            }
        };

        // Extract the image. The deployment token is required for proxy pull auth
        // (Basic auth with the manager's /v2/ endpoint).
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
        let extracted_path = func_mgr
            .extract_image(&config.id, &image_ref, Some(token))
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to extract function OCI image".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.extracted_image_path = Some(extracted_path);

        debug!(
            function_id = %config.id,
            extracted_path = ?self.extracted_image_path,
            "Function OCI image extracted successfully"
        );

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
        let config = ctx.desired_resource_config::<Function>()?;
        let func_mgr = ctx
            .service_provider
            .get_local_function_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "LocalFunctionManager".to_string(),
                })
            })?;

        info!(function_id = %config.id, "Starting function runtime");

        // Build environment variables for the application
        //
        // IMPORTANT: config.environment already includes:
        // - User-defined variables
        // - OTLP configuration (OTEL_EXPORTER_OTLP_LOGS_ENDPOINT, etc.) from deployment loop
        // - ALIEN_AGENT_ID from deployment loop
        //
        // Note: We DON'T add ALIEN_RUNTIME_SEND_OTLP here because:
        // - For local functions, alien-runtime runs embedded (tokio task)
        // - It uses RuntimeConfig.send_otlp (set by LocalFunctionManager), not env vars
        // - The env vars here go to the child process (the app), not alien-runtime itself
        //
        // For cloud platforms (AWS/GCP/Azure/Kubernetes):
        // - alien-runtime runs standalone (PID 1)
        // - It reads config from CLI args + env vars
        // - So cloud controllers DO add ALIEN_RUNTIME_SEND_OTLP=true to env vars
        let env_vars = EnvironmentVariableBuilder::new(&config.environment)
            .add_standard_alien_env_vars(ctx)
            .add_function_transport_env_vars(ctx.platform)
            .add_linked_resources(&config.links, ctx, &config.id)
            .await?
            .build();

        // Start the function with complete environment
        let function_url = func_mgr
            .start_function(&config.id, env_vars)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to start function runtime".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.function_url = Some(function_url);

        info!(
            function_id = %config.id,
            url = ?self.function_url,
            "Function runtime started successfully"
        );

        // Start trigger service if the function has triggers configured.
        // This mirrors what cloud platforms do natively (SQS event source mapping,
        // Pub/Sub subscriptions, etc.) — delivering events to the function externally.
        if !config.triggers.is_empty() {
            if let Some(local_bindings) = ctx.service_provider.get_local_bindings_provider() {
                let state_dir = if let alien_core::ClientConfig::Local {
                    state_directory, ..
                } = &ctx.client_config
                {
                    PathBuf::from(state_directory)
                } else {
                    PathBuf::from(".alien")
                };

                let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);
                let triggers = config.triggers.clone();
                let function_id = config.id.clone();

                // Store shutdown sender in static map (controller struct is Clone+Serialize)
                TRIGGER_SHUTDOWNS
                    .lock()
                    .unwrap()
                    .insert(function_id.clone(), shutdown_tx);

                let service = alien_local::trigger_service::LocalTriggerService::new(
                    triggers.clone(),
                    local_bindings,
                    state_dir,
                    shutdown_rx,
                );

                tokio::spawn(async move {
                    if let Err(e) = service.run().await {
                        error!(error = %e, "Local trigger service error");
                    }
                });

                info!(
                    function_id = %function_id,
                    trigger_count = triggers.len(),
                    "Local trigger service started"
                );
            }
        }

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
        let config = ctx.desired_resource_config::<Function>()?;

        // Verify function is still running via service manager health check
        let func_mgr = ctx
            .service_provider
            .get_local_function_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "LocalFunctionManager".to_string(),
                })
            })?;

        func_mgr
            .check_health(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Function health check failed for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            })?;

        // Query the CURRENT URL from the manager (in case recovery changed the port)
        // This ensures controller state stays in sync with runtime reality
        let current_url =
            func_mgr
                .get_function_url(&config.id)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to get function URL for '{}'", config.id),
                    resource_id: Some(config.id.clone()),
                })?;

        // Update controller state if URL changed (e.g., after auto-recovery)
        if self.function_url.as_ref() != Some(&current_url) {
            info!(
                function_id = %config.id,
                old_url = ?self.function_url,
                new_url = %current_url,
                "Function URL changed (likely due to auto-recovery), updating controller state"
            );
            self.function_url = Some(current_url);
        }

        debug!(function_id=%config.id, "Function health check passed");

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────────────────

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
        let config = ctx.desired_resource_config::<Function>()?;
        let func_mgr = ctx
            .service_provider
            .get_local_function_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "LocalFunctionManager".to_string(),
                })
            })?;

        info!(function_id = %config.id, "Stopping function for update");

        // Stop the running function
        func_mgr
            .stop_function(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to stop function for update".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        info!(function_id = %config.id, "Function stopped successfully");

        Ok(HandlerAction::Continue {
            state: ExtractingImage,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────────────────

    #[flow_entry(Delete)]
    #[handler(
        state = Deleting,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting
    )]
    async fn deleting(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Function>()?;

        // Stop trigger service before deleting function
        if let Some(tx) = TRIGGER_SHUTDOWNS.lock().unwrap().remove(&config.id) {
            let _ = tx.send(());
        }

        let func_mgr = ctx
            .service_provider
            .get_local_function_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "LocalFunctionManager".to_string(),
                })
            })?;

        info!(function_id = %config.id, "Deleting function");

        // Delete the function (stops runtime and removes extracted image)
        func_mgr
            .delete_function(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to delete function".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        info!(function_id = %config.id, "Function deleted successfully");

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── TERMINAL STATES ──────────────────────────────────────

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

    // ─────────────── HELPER METHODS ──────────────────────────────────────

    fn build_outputs(&self) -> Option<CoreResourceOutputs> {
        self.function_url.as_ref().map(|url| {
            CoreResourceOutputs::new(FunctionOutputs {
                function_name: String::new(), // Not applicable for local
                url: Some(url.clone()),
                identifier: None,
                load_balancer_endpoint: None, // Local functions don't have load balancers
                commands_push_target: None,   // Local uses polling
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, FunctionBinding};

        if let Some(function_url) = &self.function_url {
            let binding = FunctionBinding::local(BindingValue::value(function_url.clone()));
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
