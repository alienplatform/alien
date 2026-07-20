#[cfg(test)]
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{debug, info};

use crate::core::{
    applicable_secret_environment_variables, direct_monitoring_auth_headers,
    environment_variables::EnvironmentVariableBuilder, ResourceControllerContext,
};
use crate::error::{ErrorData, Result};
use alien_core::{
    Daemon, DaemonCode, DaemonHeartbeatData, DaemonOutputs, HeartbeatBackend,
    LocalDaemonHeartbeatData, LocalRuntimeUnitKind, LocalRuntimeUnitStatus, ObservedHealth,
    Platform, Postgres, ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData,
    ResourceOutputs, ResourceStatus, WorkloadHeartbeatStatus,
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
    /// Public URL when the daemon declares or receives a public HTTP endpoint.
    pub(crate) public_url: Option<String>,
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

        // Source-built daemons are supported: `alien build` compiles the
        // source and rewrites `code` to an image whose binary this controller
        // extracts and runs. Reaching here with unbuilt source means the
        // build step was skipped.
        let image_ref = match &config.code {
            DaemonCode::Image { image } => image.clone(),
            DaemonCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message:
                        "Daemon still has unbuilt source code. Run 'alien build' first; it compiles the source into an image the controller can extract and run."
                            .to_string(),
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
            .add_daemon_runtime_env_vars(ctx)?
            .add_direct_monitoring_auth_headers(ctx)
            .add_current_resource_public_endpoint(ctx, &config.id)?
            .add_linked_resources(&config.links, ctx, &config.id)
            .await?;

        // Command-enabled Daemons no longer get `ALIEN_TRANSPORT=passthrough`.
        // Their receiver config (`ALIEN_COMMANDS_*`) is injected per-resource into
        // `config.environment` by the manager/operator snapshot and
        // flows in through `EnvironmentVariableBuilder::try_new(&config.environment)`.

        if let Some(endpoint) = config.public_endpoints.first() {
            env_builder = env_builder.add_env_var("PORT".to_string(), endpoint.port.to_string());
        }

        let mut env_vars = env_builder.build();

        // Resolve secret environment variables into plain values before the process starts. On the
        // local platform there is no `ALIEN_SECRETS` vault-load marker — the supervisor spawns the
        // app with these values already in its environment, so the app reads them like any env var.
        // Their NAMES are handed to the supervisor as runtime-only so the values never persist to
        // the daemon's on-disk metadata.
        let mut runtime_only_env_names = Vec::new();
        for var in applicable_secret_environment_variables(
            &config.id,
            &ctx.deployment_config.environment_variables.variables,
        ) {
            env_vars.insert(var.name.clone(), var.value.clone());
            runtime_only_env_names.push(var.name.clone());
        }
        // Monitoring credentials are controller-owned. Apply them after user
        // secrets so a same-name snapshot value cannot replace the credential
        // selected by DeploymentConfig.monitoring.
        let monitoring_headers = direct_monitoring_auth_headers(ctx);
        env_vars.extend(monitoring_headers.clone());
        runtime_only_env_names.extend(monitoring_headers.into_keys());
        runtime_only_env_names.sort();
        runtime_only_env_names.dedup();

        // Linked Postgres resources carry a runtime-only secret (the password). Name them so the
        // worker manager delivers the binding to the process but never persists it to metadata.
        let runtime_only_binding_names: Vec<String> = config
            .links
            .iter()
            .filter(|link| link.resource_type() == &Postgres::RESOURCE_TYPE)
            .map(|link| link.id().to_string())
            .collect();

        manager
            .start_daemon(
                &config.id,
                env_vars,
                alien_local::DaemonLaunchOptions {
                    runtime_only_binding_names,
                    runtime_only_env_names,
                    command_override: config.command.clone(),
                    stop_grace_period_seconds: config.stop_grace_period_seconds,
                },
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to start daemon runtime".to_string(),
                resource_id: Some(config.id.clone()),
            })?;
        self.daemon_name = Some(config.id.clone());
        self.public_url = local_daemon_public_url(ctx, &config);

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

        // Self-heal instead of erroring when the daemon simply isn't running:
        // after a manager restart, cold recovery intentionally skips daemons
        // whose env carried runtime-only secrets (their values are never
        // persisted), so the controller — resuming at Ready — is the ONLY
        // thing that can bring them back. Re-entering StartingProcess rebuilds
        // the full env (secrets freshly resolved) and starts the process;
        // erroring here would just cycle Ready→RefreshFailed→Ready forever.
        if !manager.is_daemon_running(&config.id).await {
            debug!(daemon_id=%config.id, "Daemon not running; re-entering start flow");
            return Ok(HandlerAction::Continue {
                state: StartingProcess,
                suggested_delay: None,
            });
        }

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
        self.public_url = None;

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
                public_endpoints: self
                    .public_url
                    .as_ref()
                    .map(|url| {
                        std::collections::HashMap::from([(
                            "default".to_string(),
                            alien_core::PublicEndpointOutput {
                                url: url.clone(),
                                host: alien_core::public_url_host(url).unwrap_or_default(),
                                protocol: alien_core::ExposeProtocol::Http,
                                port: alien_core::public_url_port(url).unwrap_or(80),
                                wildcard_host: None,
                                load_balancer_endpoint: None,
                            },
                        )])
                    })
                    .unwrap_or_default(),
            })
        })
    }
}

fn local_daemon_public_url(ctx: &ResourceControllerContext<'_>, config: &Daemon) -> Option<String> {
    local_daemon_public_url_from_config(ctx.deployment_config.public_endpoints.as_ref(), config)
}

fn local_daemon_public_url_from_config(
    public_endpoints: Option<&alien_core::PublicEndpointUrls>,
    config: &Daemon,
) -> Option<String> {
    public_endpoints
        .and_then(|resources| resources.get(&config.id))
        .and_then(|endpoints| {
            config
                .public_endpoints
                .first()
                .and_then(|endpoint| endpoints.get(&endpoint.name))
        })
        .cloned()
        .or_else(|| {
            config
                .public_endpoints
                .first()
                .map(|endpoint| format!("http://localhost:{}", endpoint.port))
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{DaemonCode, ExposeProtocol, PublicEndpoint};

    fn daemon_with_public_port() -> Daemon {
        Daemon::new("gateway".to_string())
            .code(DaemonCode::Image {
                image: "gateway:latest".to_string(),
            })
            .permissions("default".to_string())
            .public_endpoint(PublicEndpoint {
                name: "api".to_string(),
                port: 8080,
                protocol: ExposeProtocol::Http,
                host_label: None,
                wildcard_subdomains: false,
            })
            .build()
    }

    #[test]
    fn local_daemon_public_url_prefers_configured_resource_url() {
        let daemon = daemon_with_public_port();
        let public_endpoints = HashMap::from([(
            "gateway".to_string(),
            HashMap::from([(
                "api".to_string(),
                "https://gateway.example.test".to_string(),
            )]),
        )]);

        assert_eq!(
            local_daemon_public_url_from_config(Some(&public_endpoints), &daemon).as_deref(),
            Some("https://gateway.example.test")
        );
    }

    #[test]
    fn local_daemon_public_url_falls_back_to_localhost_for_declared_port() {
        let daemon = daemon_with_public_port();

        assert_eq!(
            local_daemon_public_url_from_config(None, &daemon).as_deref(),
            Some("http://localhost:8080")
        );
    }

    #[test]
    fn local_daemon_public_url_is_absent_without_config_or_public_port() {
        let daemon = Daemon::new("internal".to_string())
            .code(DaemonCode::Image {
                image: "internal:latest".to_string(),
            })
            .permissions("default".to_string())
            .build();

        assert_eq!(local_daemon_public_url_from_config(None, &daemon), None);
    }
}
