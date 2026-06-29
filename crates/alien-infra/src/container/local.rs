//! Local Container controller.
//!
//! On the Local platform, containers run via Docker directly.
//! The LocalContainerManager handles Docker API calls.

use std::time::Duration;
use tracing::{debug, info};

use crate::container::local_utils;
use crate::core::{environment_variables::EnvironmentVariableBuilder, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use alien_core::{
    Container, ContainerCode, ContainerHeartbeatData, ContainerOutputs, ContainerStatus,
    EnvironmentVariable, EnvironmentVariableType, HeartbeatBackend, Kv,
    LocalContainerHeartbeatData, LocalRuntimeUnitKind, LocalRuntimeUnitStatus, ObservedHealth,
    Platform, ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData,
    ResourceOutputs as CoreResourceOutputs, ResourceStatus, Storage, Vault,
    WorkloadHeartbeatStatus,
};
use alien_error::{AlienError, Context, IntoAlienError as _};
use alien_local::{ContainerConfig, ContainerInfo};
use alien_macros::controller;
use chrono::Utc;

fn matches_environment_target(resource_id: &str, target_resources: &Option<Vec<String>>) -> bool {
    match target_resources {
        None => true,
        Some(patterns) if patterns.is_empty() => false,
        Some(patterns) => patterns.iter().any(|pattern| {
            if let Some(prefix) = pattern.strip_suffix('*') {
                resource_id.starts_with(prefix)
            } else {
                resource_id == pattern
            }
        }),
    }
}

fn applicable_secret_environment_variables<'a>(
    resource_id: &str,
    variables: &'a [EnvironmentVariable],
) -> Vec<&'a EnvironmentVariable> {
    variables
        .iter()
        .filter(|var| var.var_type == EnvironmentVariableType::Secret)
        .filter(|var| matches_environment_target(resource_id, &var.target_resources))
        .collect()
}

/// Local Container controller.
///
/// Manages containerized workloads on the Local platform using Docker.
/// Unlike cloud platforms, containers run directly
/// via the Docker daemon.
#[controller]
pub struct LocalContainerController {
    /// Container info after starting
    pub(crate) container_info: Option<ContainerInfo>,
}

#[controller]
impl LocalContainerController {
    // ─────────────── CREATE FLOW ───────────────────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = StartingContainer,
        on_failure = ProvisionFailed,
        status = ResourceStatus::Provisioning
    )]
    async fn starting_container(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        info!(container_id = %config.id, "Starting container");

        // Get the container manager
        let container_mgr = ctx
            .service_provider
            .get_local_container_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "LocalContainerManager".to_string(),
                })
            })?;

        // Determine the image
        let image = match &config.code {
            ContainerCode::Image { image } => image.clone(),
            ContainerCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Local platform does not support building from source. Use pre-built images.".to_string(),
                    resource_id: Some(config.id.clone()),
                }));
            }
        };

        // Determine if this container should be exposed publicly.
        let expose_public = !config.public_endpoints.is_empty();

        // First, collect bind mounts for linked filesystem resources (Storage, KV, Vault)
        // We need to know the container paths before building env vars so we can rewrite bindings
        let mut bind_mounts = Vec::new();

        // Add /tmp mount for all containers
        // Docker on macOS doesn't have /tmp by default, but many tools (like tempfile) need it
        if let Some(container_mgr) = ctx.service_provider.get_local_container_manager() {
            let tmp_host_path = container_mgr.get_container_tmp_dir(&config.id);
            // Create the directory if it doesn't exist
            tokio::fs::create_dir_all(&tmp_host_path)
                .await
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create container tmp directory: {}",
                        tmp_host_path.display()
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            bind_mounts.push(alien_local::BindMount {
                host_path: tmp_host_path,
                container_path: "/tmp".to_string(),
                resource_id: "tmp".to_string(),
            });
        }

        for link in &config.links {
            let linked_resource_id = link.id();

            if let Some(resource_state) = ctx.state.resources.get(linked_resource_id) {
                let resource_config = &resource_state.config;

                // Check if it's a Storage resource
                if resource_config.downcast_ref::<Storage>().is_some() {
                    if let Some(storage_mgr) = ctx.service_provider.get_local_storage_manager() {
                        if let Ok(host_path) = storage_mgr.get_storage_path(linked_resource_id) {
                            bind_mounts.push(alien_local::BindMount {
                                host_path,
                                container_path: format!("/mnt/storage/{}", linked_resource_id),
                                resource_id: linked_resource_id.to_string(),
                            });
                        }
                    }
                }
                // Check if it's a Kv resource
                else if resource_config.downcast_ref::<Kv>().is_some() {
                    if let Some(kv_mgr) = ctx.service_provider.get_local_kv_manager() {
                        if let Ok(host_path) = kv_mgr.get_kv_path(linked_resource_id) {
                            bind_mounts.push(alien_local::BindMount {
                                host_path,
                                container_path: format!("/mnt/kv/{}", linked_resource_id),
                                resource_id: linked_resource_id.to_string(),
                            });
                        }
                    }
                }
                // Check if it's a Vault resource
                else if resource_config.downcast_ref::<Vault>().is_some() {
                    if let Some(vault_mgr) = ctx.service_provider.get_local_vault_manager() {
                        if let Ok(host_path) = vault_mgr.get_vault_path(linked_resource_id) {
                            bind_mounts.push(alien_local::BindMount {
                                host_path,
                                container_path: format!("/mnt/vault/{}", linked_resource_id),
                                resource_id: linked_resource_id.to_string(),
                            });
                        }
                    }
                }
            }
        }

        // Build environment variables using EnvironmentVariableBuilder
        // This populates binding env vars with HOST paths
        let mut env_vars = EnvironmentVariableBuilder::try_new(&config.environment)?
            .add_container_runtime_env_vars(ctx, &config.id)?
            .add_linked_resources(&config.links, ctx, &config.id)
            .await?
            .build();

        for var in applicable_secret_environment_variables(
            &config.id,
            &ctx.deployment_config.environment_variables.variables,
        ) {
            env_vars.insert(var.name.clone(), var.value.clone());
        }

        // Rewrite binding env vars to use container paths instead of host paths
        // This uses typed deserialization for type safety
        for bind_mount in &bind_mounts {
            local_utils::rewrite_binding_path_for_container(
                &mut env_vars,
                &bind_mount.resource_id,
                &bind_mount.container_path,
            )?;
        }

        // Rewrite localhost URLs to host.docker.internal for container networking
        // Containers cannot use "localhost" to reach services on the host machine
        local_utils::rewrite_localhost_urls_for_container(&mut env_vars)?;

        // Build the container config
        let ports: Vec<u16> = config.ports.iter().map(|p| p.port).collect();

        let container_config = ContainerConfig {
            image,
            command: config.command.clone(),
            ports,
            expose_public,
            env_vars,
            stateful: config.stateful,
            ordinal: None, // TODO: Handle ordinals for stateful containers
            volume_mount: config
                .persistent_storage
                .as_ref()
                .map(|ps| ps.mount_path.clone()),
            volume_size: config.persistent_storage.as_ref().map(|ps| ps.size.clone()),
            bind_mounts,
        };

        // Start the container
        let container_info = container_mgr
            .start_container(&config.id, container_config)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to start container".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.container_info = Some(container_info.clone());

        info!(
            container_id = %config.id,
            docker_id = %container_info.docker_container_id,
            url = ?container_info.host_port.map(|p| format!("http://localhost:{}", p)),
            "Container started successfully"
        );

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
        let config = ctx.desired_resource_config::<Container>()?;

        // Verify container is still running
        let container_mgr = ctx
            .service_provider
            .get_local_container_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "LocalContainerManager".to_string(),
                })
            })?;

        container_mgr
            .check_health(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Container health check failed for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            })?;

        // Query the CURRENT binding from the manager (in case recovery changed ports)
        // This ensures controller state stays in sync with runtime reality
        let current_binding =
            container_mgr
                .get_binding(&config.id)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to get container binding for '{}'", config.id),
                    resource_id: Some(config.id.clone()),
                })?;

        // Extract host_port from the binding's public URL if present
        let current_host_port = current_binding.get_public_url().and_then(|binding_value| {
            // Extract the actual URL string from BindingValue
            if let alien_core::bindings::BindingValue::Value(url) = binding_value {
                // Parse "http://localhost:12345" -> 12345
                url.strip_prefix("http://localhost:")
                    .and_then(|port_str| port_str.parse::<u16>().ok())
            } else {
                None
            }
        });

        // Update controller state if binding changed
        if let Some(info) = &mut self.container_info {
            if info.host_port != current_host_port {
                info!(
                    container_id = %config.id,
                    old_port = ?info.host_port,
                    new_port = ?current_host_port,
                    "Container port changed (likely due to auto-recovery), updating controller state"
                );
                info.host_port = current_host_port;
            }
        }

        emit_local_container_heartbeat(ctx, &config, self.container_info.as_ref(), true);

        debug!(container_id = %config.id, "Container health check passed");

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
        let config = ctx.desired_resource_config::<Container>()?;
        let container_mgr = ctx
            .service_provider
            .get_local_container_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "LocalContainerManager".to_string(),
                })
            })?;

        info!(container_id = %config.id, "Stopping container for update");

        // Delete the old container (will be recreated in StartingContainer)
        container_mgr.delete_container(&config.id).await.context(
            ErrorData::CloudPlatformError {
                message: "Failed to stop container for update".to_string(),
                resource_id: Some(config.id.clone()),
            },
        )?;

        self.container_info = None;

        Ok(HandlerAction::Continue {
            state: StartingContainer,
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
        let config = ctx.desired_resource_config::<Container>()?;
        let container_mgr = ctx
            .service_provider
            .get_local_container_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "LocalContainerManager".to_string(),
                })
            })?;

        info!(container_id = %config.id, "Deleting container");

        // Delete the container
        container_mgr.delete_container(&config.id).await.context(
            ErrorData::CloudPlatformError {
                message: "Failed to delete container".to_string(),
                resource_id: Some(config.id.clone()),
            },
        )?;

        info!(container_id = %config.id, "Container deleted successfully");

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
        self.container_info.as_ref().map(|info| {
            CoreResourceOutputs::new(ContainerOutputs {
                name: info.container_id.clone(),
                status: ContainerStatus::Running,
                current_replicas: 1,
                desired_replicas: 1,
                internal_dns: info.internal_dns.clone(),
                public_endpoints: info
                    .host_port
                    .map(|p| {
                        let url = format!("http://localhost:{p}");
                        std::collections::HashMap::from([(
                            "default".to_string(),
                            alien_core::PublicEndpointOutput {
                                host: alien_core::public_url_host(&url).unwrap_or_default(),
                                url,
                                wildcard_host: None,
                                load_balancer_endpoint: None,
                            },
                        )])
                    })
                    .unwrap_or_default(),
                replicas: Vec::new(), // TODO: Add replica status
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, ContainerBinding};

        let Some(info) = &self.container_info else {
            return Ok(None);
        };

        // Internal URL uses Docker network DNS with first port
        let first_port = info.ports.first().copied().unwrap_or(8080);
        let internal_url = format!("http://{}:{}", info.internal_dns, first_port);

        // Public URL is the localhost-mapped port (if exposed publicly)
        let public_url = info.host_port.map(|p| format!("http://localhost:{}", p));

        let binding = if let Some(url) = public_url {
            ContainerBinding::local_with_public_url(
                BindingValue::value(info.container_id.clone()),
                BindingValue::value(internal_url),
                BindingValue::value(url),
            )
        } else {
            ContainerBinding::local(
                BindingValue::value(info.container_id.clone()),
                BindingValue::value(internal_url),
            )
        };

        Ok(Some(
            serde_json::to_value(binding).into_alien_error().context(
                ErrorData::ResourceStateSerializationFailed {
                    resource_id: "binding".to_string(),
                    message: "Failed to serialize binding parameters".to_string(),
                },
            )?,
        ))
    }
}

fn emit_local_container_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    config: &Container,
    container_info: Option<&ContainerInfo>,
    runtime_reachable: bool,
) {
    let image = match &config.code {
        ContainerCode::Image { image } => Some(image.clone()),
        ContainerCode::Source { .. } => None,
    };
    let local_url = container_info
        .and_then(|info| info.host_port)
        .map(|port| format!("http://localhost:{port}"));

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: config.id.clone(),
        resource_type: Container::RESOURCE_TYPE,
        controller_platform: Platform::Local,
        backend: HeartbeatBackend::Local,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Container(ContainerHeartbeatData::Local(
            LocalContainerHeartbeatData {
                status: WorkloadHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: Some(format!("Local container '{}' is running", config.id)),
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                container_id: container_info.map(|info| info.docker_container_id.clone()),
                name: container_info.map(|info| info.container_id.clone()),
                image,
                runtime_status: Some("running".to_string()),
                restart_count: None,
                port_count: container_info
                    .map(|info| info.ports.len() as u32)
                    .unwrap_or(config.ports.len() as u32),
                bind_mount_count: config.links.len() as u32,
                local_url,
                runtime_reachable,
                cpu: None,
                memory: None,
                container_unit: container_info.map(|info| LocalRuntimeUnitStatus {
                    unit_id: info.docker_container_id.clone(),
                    name: info.container_id.clone(),
                    kind: LocalRuntimeUnitKind::Container,
                    ready: runtime_reachable,
                    phase: Some("running".to_string()),
                    pid: None,
                    restart_count: None,
                    cpu: None,
                    memory: None,
                }),
                events: vec![],
            },
        )),
        raw: vec![],
    });
}
