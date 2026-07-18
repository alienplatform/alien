use std::path::PathBuf;
use std::time::Duration;
use tracing::{debug, error, info};

use crate::core::{
    environment_variables::{applicable_secret_environment_variables, EnvironmentVariableBuilder},
    ResourceControllerContext,
};
use crate::error::{ErrorData, Result};
use alien_core::{
    HeartbeatBackend, LocalRuntimeUnitKind, LocalRuntimeUnitStatus, LocalWorkerHeartbeatData,
    ObservedHealth, Platform, Postgres, ProviderLifecycleState, ResourceHeartbeat,
    ResourceHeartbeatData, ResourceOutputs as CoreResourceOutputs, ResourceStatus, Worker,
    WorkerCode, WorkerHeartbeatData, WorkerOutputs, WorkloadHeartbeatStatus,
    ENV_ALIEN_COMMANDS_TOKEN,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;
use chrono::Utc;

/// Shared trigger service shutdown handle. The controller is Clone+Serialize
/// (required by the macro), so we can't store JoinHandle/broadcast::Sender directly.
/// Instead, this static holds the shutdown sender keyed by worker ID.
static TRIGGER_SHUTDOWNS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, tokio::sync::broadcast::Sender<()>>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

#[controller]
pub struct LocalWorkerController {
    /// Path to the extracted OCI image directory
    pub(crate) extracted_image_path: Option<PathBuf>,
    /// URL where the worker is accessible
    pub(crate) worker_url: Option<String>,
    /// Whether the running Worker accepts command pushes.
    #[serde(default)]
    pub(crate) commands_enabled: bool,
}

#[controller]
impl LocalWorkerController {
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
        let config = ctx.desired_resource_config::<Worker>()?;

        info!(worker_id = %config.id, "Extracting worker OCI image");

        // Get the worker manager from the service provider
        let func_mgr = ctx
            .service_provider
            .get_local_worker_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "LocalWorkerManager".to_string(),
                })
            })?;

        // Determine the image reference from the worker code
        let image_ref = match &config.code {
            WorkerCode::Image { image } => image.clone(),
            WorkerCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Local platform does not support building from source code directly. Please build the image first and use WorkerCode::Image.".to_string(),
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
                message: "Failed to extract worker OCI image".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.extracted_image_path = Some(extracted_path);

        debug!(
            worker_id = %config.id,
            extracted_path = ?self.extracted_image_path,
            "Worker OCI image extracted successfully"
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
        let config = ctx.desired_resource_config::<Worker>()?;
        let func_mgr = ctx
            .service_provider
            .get_local_worker_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "LocalWorkerManager".to_string(),
                })
            })?;

        info!(worker_id = %config.id, "Starting worker runtime");

        // Build environment variables for the application
        //
        // IMPORTANT: config.environment already includes:
        // - User-defined variables
        // - OTLP configuration (OTEL_EXPORTER_OTLP_LOGS_ENDPOINT, etc.) from deployment loop
        // - ALIEN_AGENT_ID from deployment loop
        //
        // Runtime-owned names are added from the same core contract used by
        // cloud controllers and IaC renderers.
        let mut env_vars = EnvironmentVariableBuilder::try_new(&config.environment)?
            .add_worker_runtime_env_vars(ctx, &config.id, config.timeout_seconds)?
            .add_linked_resources(&config.links, ctx, &config.id)
            .await?
            .build();

        // The push token is control-plane material rather than an application secret. Resolve it
        // from the current desired snapshot only at the final process-launch boundary. Ordinary
        // application secrets continue through the existing ALIEN_SECRETS vault-load path.
        let runtime_control_env = worker_runtime_control_environment(
            &config,
            &ctx.deployment_config.environment_variables.variables,
        )?;
        let mut runtime_only_env_names = runtime_control_env.keys().cloned().collect::<Vec<_>>();
        runtime_only_env_names.sort();
        env_vars.extend(runtime_control_env);

        // Linked Postgres resources carry a runtime-only secret (the password). Name them so the
        // worker manager delivers the binding to the process but never persists it to metadata.
        let runtime_only_binding_names: Vec<String> = config
            .links
            .iter()
            .filter(|link| link.resource_type() == &Postgres::RESOURCE_TYPE)
            .map(|link| link.id().to_string())
            .collect();

        // Start the worker with complete environment
        let worker_url = func_mgr
            .start_worker(
                &config.id,
                env_vars,
                runtime_only_binding_names,
                runtime_only_env_names,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to start worker runtime".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.worker_url = Some(worker_url);
        self.commands_enabled = config.commands_enabled;

        info!(
            worker_id = %config.id,
            url = ?self.worker_url,
            "Worker runtime started successfully"
        );

        // Start trigger service if the worker has triggers configured.
        // This mirrors what cloud platforms do natively (SQS event source mapping,
        // Pub/Sub subscriptions, etc.) — delivering events to the worker externally.
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
                let worker_id = config.id.clone();

                // Store shutdown sender in static map (controller struct is Clone+Serialize)
                TRIGGER_SHUTDOWNS
                    .lock()
                    .unwrap()
                    .insert(worker_id.clone(), shutdown_tx);

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
                    worker_id = %worker_id,
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
        let config = ctx.desired_resource_config::<Worker>()?;

        // Verify worker is still running via service manager health check
        let func_mgr = ctx
            .service_provider
            .get_local_worker_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "LocalWorkerManager".to_string(),
                })
            })?;

        if let Some(state) = self.runtime_refresh_state(&config) {
            info!(
                worker_id = %config.id,
                running_commands_enabled = self.commands_enabled,
                desired_commands_enabled = config.commands_enabled,
                "Worker command capability changed; restarting with current desired environment"
            );
            return Ok(HandlerAction::Continue {
                state,
                suggested_delay: None,
            });
        }

        // Runtime-only control secrets are deliberately absent from recovery metadata. If the
        // manager was restarted or reaped a command-capable Worker, rebuild the launch from the
        // current desired snapshot instead of attempting metadata-only recovery.
        if !func_mgr.is_running(&config.id).await {
            info!(
                worker_id = %config.id,
                "Worker is not running; restarting with current desired environment"
            );
            return Ok(HandlerAction::Continue {
                state: LocalWorkerState::StartingProcess,
                suggested_delay: None,
            });
        }

        func_mgr
            .check_health(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Worker health check failed for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            })?;

        // Query the CURRENT URL from the manager (in case recovery changed the port)
        // This ensures controller state stays in sync with runtime reality
        let current_url =
            func_mgr
                .get_worker_url(&config.id)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to get worker URL for '{}'", config.id),
                    resource_id: Some(config.id.clone()),
                })?;

        // Update controller state if URL changed (e.g., after auto-recovery)
        if self.worker_url.as_ref() != Some(&current_url) {
            info!(
                worker_id = %config.id,
                old_url = ?self.worker_url,
                new_url = %current_url,
                "Worker URL changed (likely due to auto-recovery), updating controller state"
            );
            self.worker_url = Some(current_url);
        }

        emit_local_worker_heartbeat(
            ctx,
            &config,
            self.extracted_image_path.as_ref(),
            self.commands_enabled,
        );

        debug!(worker_id=%config.id, "Worker health check passed");

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
        let config = ctx.desired_resource_config::<Worker>()?;
        let func_mgr = ctx
            .service_provider
            .get_local_worker_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "LocalWorkerManager".to_string(),
                })
            })?;

        info!(worker_id = %config.id, "Stopping worker for update");

        // Stop the running worker
        func_mgr
            .stop_worker(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to stop worker for update".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        info!(worker_id = %config.id, "Worker stopped successfully");

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
        let config = ctx.desired_resource_config::<Worker>()?;

        // Stop trigger service before deleting worker
        if let Some(tx) = TRIGGER_SHUTDOWNS.lock().unwrap().remove(&config.id) {
            let _ = tx.send(());
        }

        let func_mgr = ctx
            .service_provider
            .get_local_worker_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "LocalWorkerManager".to_string(),
                })
            })?;

        info!(worker_id = %config.id, "Deleting worker");

        // Delete the worker (stops runtime and removes extracted image)
        func_mgr
            .delete_worker(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to delete worker".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        info!(worker_id = %config.id, "Worker deleted successfully");

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
        self.worker_url.as_ref().map(|url| {
            CoreResourceOutputs::new(WorkerOutputs {
                worker_name: String::new(), // Not applicable for local
                identifier: None,
                public_endpoints: std::collections::HashMap::from([(
                    "default".to_string(),
                    alien_core::PublicEndpointOutput {
                        url: url.clone(),
                        host: alien_core::public_url_host(url).unwrap_or_default(),
                        wildcard_host: None,
                        load_balancer_endpoint: None,
                    },
                )]),
                commands_push_target: self.commands_enabled.then(|| {
                    format!(
                        "{}{}",
                        url.trim_end_matches('/'),
                        alien_core::WORKER_COMMAND_PUSH_PATH
                    )
                }),
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, WorkerBinding};

        if let Some(worker_url) = &self.worker_url {
            let binding = WorkerBinding::local(BindingValue::value(worker_url.clone()));
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

impl LocalWorkerController {
    fn runtime_refresh_state(&self, config: &Worker) -> Option<LocalWorkerState> {
        (self.commands_enabled != config.commands_enabled)
            .then_some(LocalWorkerState::StoppingForUpdate)
    }
}

fn worker_runtime_control_environment(
    config: &Worker,
    variables: &[alien_core::EnvironmentVariable],
) -> Result<std::collections::HashMap<String, String>> {
    if !config.commands_enabled {
        return Ok(std::collections::HashMap::new());
    }

    let command_tokens = applicable_secret_environment_variables(&config.id, variables)
        .into_iter()
        .filter(|var| var.name == ENV_ALIEN_COMMANDS_TOKEN)
        .collect::<Vec<_>>();

    let token = match command_tokens.as_slice() {
        [] => {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "commands-enabled Worker '{}' is missing its runtime command token",
                    config.id
                ),
                resource_id: Some(config.id.clone()),
            }));
        }
        [token] if token.value.is_empty() => {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "commands-enabled Worker '{}' has an empty runtime command token",
                    config.id
                ),
                resource_id: Some(config.id.clone()),
            }));
        }
        [token] => token.value.clone(),
        [_, _, ..] => {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "commands-enabled Worker '{}' has multiple applicable runtime command tokens",
                    config.id
                ),
                resource_id: Some(config.id.clone()),
            }));
        }
    };

    Ok(std::collections::HashMap::from([(
        ENV_ALIEN_COMMANDS_TOKEN.to_string(),
        token,
    )]))
}

fn emit_local_worker_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    config: &Worker,
    extracted_image_path: Option<&PathBuf>,
    commands_enabled: bool,
) {
    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: config.id.clone(),
        resource_type: Worker::RESOURCE_TYPE,
        controller_platform: Platform::Local,
        backend: HeartbeatBackend::Local,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Worker(WorkerHeartbeatData::Local(LocalWorkerHeartbeatData {
            status: WorkloadHeartbeatStatus {
                health: ObservedHealth::Healthy,
                lifecycle: ProviderLifecycleState::Running,
                message: Some(format!("Local worker '{}' is running", config.id)),
                stale: false,
                partial: false,
                collection_issues: vec![],
            },
            pid: None,
            command_supported: commands_enabled,
            image_path_present: extracted_image_path
                .map(|path| path.exists())
                .unwrap_or(false),
            readiness_probe_ok: None,
            trigger_count: config.triggers.len() as u32,
            cpu: None,
            memory: None,
            process: extracted_image_path.map(|path| LocalRuntimeUnitStatus {
                unit_id: config.id.clone(),
                name: config.id.clone(),
                kind: LocalRuntimeUnitKind::Process,
                ready: path.exists(),
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

#[cfg(test)]
mod output_tests {
    use alien_core::{
        EnvironmentVariable, EnvironmentVariableType, Worker, WorkerCode, WorkerOutputs,
        ENV_ALIEN_COMMANDS_TOKEN,
    };

    use super::{worker_runtime_control_environment, LocalWorkerController, LocalWorkerState};

    fn controller(commands_enabled: bool) -> LocalWorkerController {
        LocalWorkerController {
            state: LocalWorkerState::Ready,
            extracted_image_path: None,
            worker_url: Some("http://127.0.0.1:8080/".to_string()),
            commands_enabled,
            _internal_stay_count: None,
        }
    }

    #[test]
    fn command_push_output_exists_only_when_commands_are_enabled() {
        let enabled = controller(true).build_outputs().expect("outputs");
        let enabled = enabled
            .downcast_ref::<WorkerOutputs>()
            .expect("Worker outputs");
        assert_eq!(
            enabled.commands_push_target.as_deref(),
            Some("http://127.0.0.1:8080/_alien/commands")
        );

        let disabled = controller(false).build_outputs().expect("outputs");
        let disabled = disabled
            .downcast_ref::<WorkerOutputs>()
            .expect("Worker outputs");
        assert!(disabled.commands_push_target.is_none());
    }

    #[test]
    fn legacy_ready_state_requires_restart_before_enabling_commands() {
        let controller: LocalWorkerController = serde_json::from_value(serde_json::json!({
            "extractedImagePath": null,
            "workerUrl": "http://127.0.0.1:8080",
            "state": "ready",
            "_internalStayCount": null
        }))
        .expect("legacy Local Worker controller state");
        assert!(!controller.commands_enabled);
        assert!(controller
            .build_outputs()
            .expect("legacy outputs")
            .downcast_ref::<WorkerOutputs>()
            .expect("Worker outputs")
            .commands_push_target
            .is_none());

        let config = Worker::new("worker".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();
        assert_eq!(
            controller.runtime_refresh_state(&config),
            Some(LocalWorkerState::StoppingForUpdate)
        );
        assert!(!controller.commands_enabled);
    }

    #[test]
    fn ready_state_requires_restart_before_disabling_commands() {
        let controller = controller(true);
        let config = Worker::new("worker".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(false)
            .build();

        assert_eq!(
            controller.runtime_refresh_state(&config),
            Some(LocalWorkerState::StoppingForUpdate)
        );
        assert!(controller.commands_enabled);
    }

    #[test]
    fn runtime_control_channel_receives_token_but_not_app_secrets() {
        let config = Worker::new("worker".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();
        let variables = vec![
            EnvironmentVariable {
                name: ENV_ALIEN_COMMANDS_TOKEN.to_string(),
                value: "runtime-token".to_string(),
                var_type: EnvironmentVariableType::Secret,
                target_resources: Some(vec!["worker".to_string()]),
            },
            EnvironmentVariable {
                name: "APP_SECRET".to_string(),
                value: "app-value".to_string(),
                var_type: EnvironmentVariableType::Secret,
                target_resources: Some(vec!["worker".to_string()]),
            },
        ];

        let env = worker_runtime_control_environment(&config, &variables).expect("runtime env");
        assert_eq!(
            env.get(ENV_ALIEN_COMMANDS_TOKEN),
            Some(&"runtime-token".to_string())
        );
        assert!(!env.contains_key("APP_SECRET"));
    }

    #[test]
    fn commands_disabled_does_not_receive_a_runtime_control_token() {
        let config = Worker::new("worker".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(false)
            .build();
        let variables = vec![EnvironmentVariable {
            name: ENV_ALIEN_COMMANDS_TOKEN.to_string(),
            value: "runtime-token".to_string(),
            var_type: EnvironmentVariableType::Secret,
            target_resources: Some(vec!["worker".to_string()]),
        }];

        let env =
            worker_runtime_control_environment(&config, &variables).expect("disabled commands");
        assert!(!env.contains_key(ENV_ALIEN_COMMANDS_TOKEN));
    }
}
