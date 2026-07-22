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
    bindings::{BindingValue, ContainerBinding},
    Container, ContainerCode, ContainerHeartbeatData, ContainerOutputs, ContainerStatus,
    EnvironmentVariable, EnvironmentVariableType, ExposeProtocol, HeartbeatBackend, Kv,
    LocalContainerHeartbeatData, LocalRuntimeUnitKind, LocalRuntimeUnitStatus, ObservedHealth,
    Platform, Postgres, ProviderLifecycleState, PublicEndpointOutput, Queue, ResourceHeartbeat,
    ResourceHeartbeatData, ResourceOutputs as CoreResourceOutputs, ResourceStatus, Storage, Vault,
    WorkloadHeartbeatStatus,
};
use alien_error::{AlienError, Context, IntoAlienError as _};
use alien_local::{
    BindMount, ContainerConfig, ContainerInfo, LocalPublicEndpoint, LocalQueueManager,
};
use alien_macros::controller;
use chrono::Utc;

/// Remove the vault-load secret pointers (`ALIEN_SECRETS` /
/// `ALIEN_RUNTIME_SECRETS`) from a projected environment.
///
/// Containers never receive `ALIEN_SECRETS` anymore
/// (`SecretDelivery::resolve(Local, Container)` is `NativeProjection` on every
/// platform), so this is defense in depth: it keeps a pointer minted by an
/// OLDER manager's env snapshot — or by any future regression — out of the
/// runtime-less container, whose secrets arrive as concrete env vars. The
/// local daemon path strips the same two vars before spawning its process;
/// this keeps the container path symmetric.
fn strip_vault_secret_pointers(env_vars: &mut std::collections::HashMap<String, String>) {
    env_vars.remove(alien_core::ENV_ALIEN_SECRETS);
    env_vars.remove(alien_core::ENV_ALIEN_RUNTIME_SECRETS);
}

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

fn local_queue_bind_mount(
    queue_manager: Option<&LocalQueueManager>,
    resource_id: &str,
) -> Result<BindMount> {
    let queue_manager = queue_manager.ok_or_else(|| {
        AlienError::new(ErrorData::LocalServicesNotAvailable {
            service_name: "LocalQueueManager".to_string(),
        })
    })?;
    let host_path = queue_manager.get_queue_path(resource_id).context(
        ErrorData::ResourceControllerConfigError {
            resource_id: resource_id.to_string(),
            message: "Failed to resolve local Queue path for container mount".to_string(),
        },
    )?;

    Ok(BindMount {
        host_path,
        container_path: format!("/mnt/queue/{resource_id}"),
        resource_id: resource_id.to_string(),
        shared_with_host_workloads: true,
    })
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

        // Determine the image. Source-built containers are supported: `alien
        // build` compiles the source and rewrites `code` to an image whose
        // compiled binary is the direct entrypoint. Reaching the controller
        // with unbuilt source means the build step was skipped.
        let image = match &config.code {
            ContainerCode::Image { image } => image.clone(),
            ContainerCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message:
                        "Container still has unbuilt source code. Run 'alien build' first; it compiles the source into an image the controller can run."
                            .to_string(),
                    resource_id: Some(config.id.clone()),
                }));
            }
        };

        // First, collect bind mounts for linked filesystem resources (Storage, KV, Queue, Vault)
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
                shared_with_host_workloads: false,
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
                                shared_with_host_workloads: true,
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
                                shared_with_host_workloads: true,
                            });
                        }
                    }
                }
                // Check if it's a Queue resource
                else if resource_config.downcast_ref::<Queue>().is_some() {
                    let queue_manager = ctx.service_provider.get_local_queue_manager();
                    bind_mounts.push(local_queue_bind_mount(
                        queue_manager.as_deref(),
                        linked_resource_id,
                    )?);
                }
                // Check if it's a Vault resource
                else if resource_config.downcast_ref::<Vault>().is_some() {
                    if let Some(vault_mgr) = ctx.service_provider.get_local_vault_manager() {
                        if let Ok(host_path) = vault_mgr.get_vault_path(linked_resource_id) {
                            bind_mounts.push(alien_local::BindMount {
                                host_path,
                                container_path: format!("/mnt/vault/{}", linked_resource_id),
                                resource_id: linked_resource_id.to_string(),
                                shared_with_host_workloads: true,
                            });
                        }
                    }
                }
            }
        }

        // Build environment variables using EnvironmentVariableBuilder.
        // Local containers run runtime-less (the app binary is the direct entrypoint), so they get
        // the same env surface as any linked workload — standard Alien vars, the public-endpoint
        // metadata, and linked-resource bindings (with HOST paths, rewritten to container paths
        // below). The container's own self-binding and binding-name var are injected by the
        // container manager at spawn, so there is no container "runtime" env plan here.
        let mut env_vars = EnvironmentVariableBuilder::try_new(&config.environment)?
            .add_standard_alien_env_vars(ctx)?
            .add_direct_monitoring_auth_headers(ctx)
            .add_current_resource_public_endpoint(ctx, &config.id)?
            .add_linked_resources(&config.links, ctx, &config.id)
            .await?
            .build();

        for var in applicable_secret_environment_variables(
            &config.id,
            &ctx.deployment_config.environment_variables.variables,
        ) {
            env_vars.insert(var.name.clone(), var.value.clone());
        }
        // Monitoring credentials are controller-owned and must win over a
        // same-name value from the deployment environment snapshot.
        env_vars.extend(crate::core::direct_monitoring_auth_headers(ctx));

        // A linked binding that carries a runtime-only secret (Postgres password, BYO-key AI) is
        // stripped from the synced copy by `get_binding_params`; re-resolve the full binding
        // straight into the container env, which is never persisted in Alien state. Unlike the
        // worker recover path, at container start the linked resource must exist — fail loud if
        // nothing resolves.
        for binding_ref in alien_local::RuntimeOnlyBindingRef::from_links(&config.links) {
            let bindings_provider = ctx
                .service_provider
                .get_local_bindings_provider()
                .ok_or_else(|| {
                    AlienError::new(ErrorData::LocalServicesNotAvailable {
                        service_name: "bindings_provider".to_string(),
                    })
                })?;
            let entry = bindings_provider
                .resolve_runtime_only_binding_env(&binding_ref.name, &binding_ref.resource_type)
                .await
                .context(ErrorData::ResourceControllerConfigError {
                    resource_id: binding_ref.name.clone(),
                    message: format!(
                        "Failed to resolve runtime-only binding '{}'",
                        binding_ref.name
                    ),
                })?
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceControllerConfigError {
                        resource_id: binding_ref.name.clone(),
                        message: format!(
                            "runtime-only binding '{}' resolved to nothing",
                            binding_ref.name
                        ),
                    })
                })?;
            env_vars.extend(entry);
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

        // Strip the vault-load pointer: a runtime-less local container has its
        // secrets delivered as concrete env vars, so `ALIEN_SECRETS` is dead
        // weight that would otherwise leak into the container env. Mirrors the
        // local daemon spawn path.
        strip_vault_secret_pointers(&mut env_vars);

        // Build the container config
        let ports: Vec<u16> = config.ports.iter().map(|p| p.port).collect();

        let container_config = ContainerConfig {
            image,
            command: config.command.clone(),
            ports,
            public_endpoint: config
                .public_endpoints
                .first()
                .map(|endpoint| LocalPublicEndpoint {
                    port: endpoint.port,
                    protocol: endpoint.protocol,
                    names: config
                        .public_endpoints
                        .iter()
                        .map(|endpoint| endpoint.name.clone())
                        .collect(),
                }),
            env_vars,
            stateful: config.stateful,
            ordinal: None, // TODO: Handle ordinals for stateful containers
            volume_mount: config
                .persistent_storage
                .as_ref()
                .map(|ps| ps.mount_path.clone()),
            volume_size: config.persistent_storage.as_ref().map(|ps| ps.size.clone()),
            bind_mounts,
            // For pulls from the manager's registry proxy (source-built
            // container images in pull deployments).
            proxy_token: ctx.deployment_config.deployment_token.clone(),
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

        // Extract host_port from the binding's public URL if present.
        let current_host_port = current_binding.get_public_url().and_then(|binding_value| {
            if let alien_core::bindings::BindingValue::Value(url) = binding_value {
                local_public_url_port(url)
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
        container_mgr
            .delete_container_and_storage(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to delete container".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

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
                public_endpoints: local_public_endpoint_outputs(info),
                replicas: Vec::new(), // TODO: Add replica status
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        let Some(info) = &self.container_info else {
            return Ok(None);
        };

        let binding = local_container_binding(info);

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

fn local_endpoint_scheme(protocol: ExposeProtocol) -> &'static str {
    match protocol {
        ExposeProtocol::Http => "http",
        ExposeProtocol::Tcp => "tcp",
    }
}

fn local_public_url_port(url: &str) -> Option<u16> {
    url.rsplit_once(':')?.1.parse().ok()
}

fn local_container_binding(info: &ContainerInfo) -> ContainerBinding {
    let endpoint = info.public_endpoint.as_ref();
    let protocol = endpoint.map_or(ExposeProtocol::Http, |endpoint| endpoint.protocol);
    let internal_port = endpoint
        .map(|endpoint| endpoint.port)
        .or_else(|| info.ports.first().copied())
        .unwrap_or(8080);
    let scheme = local_endpoint_scheme(protocol);
    let internal_url = format!("{scheme}://{}:{internal_port}", info.internal_dns);

    if let Some(host_port) = info.host_port {
        ContainerBinding::local_with_public_url(
            BindingValue::value(info.container_id.clone()),
            BindingValue::value(internal_url),
            BindingValue::value(format!("{scheme}://localhost:{host_port}")),
        )
    } else {
        ContainerBinding::local(
            BindingValue::value(info.container_id.clone()),
            BindingValue::value(internal_url),
        )
    }
}

fn local_public_endpoint_outputs(
    info: &ContainerInfo,
) -> std::collections::HashMap<String, PublicEndpointOutput> {
    let Some(host_port) = info.host_port else {
        return std::collections::HashMap::new();
    };
    let protocol = info
        .public_endpoint
        .as_ref()
        .map_or(ExposeProtocol::Http, |endpoint| endpoint.protocol);
    let scheme = local_endpoint_scheme(protocol);
    let url = format!("{scheme}://localhost:{host_port}");
    let names = info
        .public_endpoint
        .as_ref()
        .map(|endpoint| endpoint.names.as_slice())
        .filter(|names| !names.is_empty())
        .unwrap_or(&[]);
    let names: Vec<&str> = if names.is_empty() {
        vec!["default"]
    } else {
        names.iter().map(String::as_str).collect()
    };

    names
        .into_iter()
        .map(|name| {
            (
                name.to_string(),
                PublicEndpointOutput {
                    host: alien_core::public_url_host(&url).unwrap_or_default(),
                    protocol,
                    port: host_port,
                    url: url.clone(),
                    wildcard_host: None,
                    load_balancer_endpoint: None,
                },
            )
        })
        .collect()
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
    let local_url = container_info.and_then(|info| {
        let host_port = info.host_port?;
        let protocol = info
            .public_endpoint
            .as_ref()
            .map_or(ExposeProtocol::Http, |endpoint| endpoint.protocol);
        Some(format!(
            "{}://localhost:{host_port}",
            local_endpoint_scheme(protocol)
        ))
    });

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn tcp_binding_and_named_outputs_describe_the_published_endpoint() {
        let info = ContainerInfo {
            container_id: "database".to_string(),
            docker_container_id: "docker-id".to_string(),
            host_port: Some(49152),
            public_endpoint: Some(LocalPublicEndpoint {
                port: 5432,
                protocol: ExposeProtocol::Tcp,
                names: vec!["primary".to_string(), "direct".to_string()],
            }),
            ports: vec![8080, 5432],
            internal_dns: "database.svc".to_string(),
        };

        let binding =
            serde_json::to_value(local_container_binding(&info)).expect("binding should serialize");
        assert_eq!(binding["internalUrl"], "tcp://database.svc:5432");
        assert_eq!(binding["publicUrl"], "tcp://localhost:49152");

        let outputs = local_public_endpoint_outputs(&info);
        assert_eq!(outputs.len(), 2);
        for name in ["primary", "direct"] {
            let endpoint = outputs.get(name).expect("named endpoint should exist");
            assert_eq!(endpoint.protocol, ExposeProtocol::Tcp);
            assert_eq!(endpoint.port, 49152);
            assert_eq!(endpoint.host, "localhost");
            assert_eq!(endpoint.url, "tcp://localhost:49152");
        }
    }

    #[test]
    fn public_url_port_supports_http_and_tcp_bindings() {
        assert_eq!(local_public_url_port("http://localhost:49152"), Some(49152));
        assert_eq!(local_public_url_port("tcp://localhost:49153"), Some(49153));
        assert_eq!(local_public_url_port("tcp://localhost"), None);
    }

    #[test]
    fn old_local_container_state_defaults_public_endpoint_to_http() {
        let info: ContainerInfo = serde_json::from_value(serde_json::json!({
            "containerId": "web",
            "dockerContainerId": "docker-id",
            "hostPort": 49153,
            "ports": [8080],
            "internalDns": "web.svc"
        }))
        .expect("old controller state should deserialize");

        let binding =
            serde_json::to_value(local_container_binding(&info)).expect("binding should serialize");
        assert_eq!(binding["internalUrl"], "http://web.svc:8080");
        assert_eq!(binding["publicUrl"], "http://localhost:49153");
    }

    #[test]
    fn local_queue_mount_requires_queue_manager() {
        let error = local_queue_bind_mount(None, "jobs")
            .expect_err("a linked Queue must not silently skip its container mount");

        assert_eq!(error.code, "LOCAL_SERVICES_NOT_AVAILABLE");
        assert!(error.message.contains("LocalQueueManager"));
    }

    #[test]
    fn local_queue_mount_reports_missing_queue_path() {
        let state_dir = tempfile::tempdir().expect("tempdir");
        let queue_manager = LocalQueueManager::new(state_dir.path().to_path_buf());

        let error = local_queue_bind_mount(Some(&queue_manager), "jobs")
            .expect_err("a linked Queue without local state must fail before container startup");

        assert_eq!(error.code, "RESOURCE_CONTROLLER_CONFIG_ERROR");
        assert!(error.message.contains("jobs"));
        assert!(error.message.contains("Failed to resolve local Queue path"));
    }

    #[test]
    fn strips_vault_secret_pointers_but_keeps_delivered_secrets_and_plain_vars() {
        // A runtime-less local container env: the vault-load pointers plus the
        // concretely-delivered secrets and ordinary vars they exist alongside.
        let mut env_vars = HashMap::from([
            (
                alien_core::ENV_ALIEN_SECRETS.to_string(),
                "{\"keys\":[\"DB_PASSWORD\"],\"hash\":\"abc\"}".to_string(),
            ),
            (
                alien_core::ENV_ALIEN_RUNTIME_SECRETS.to_string(),
                "runtime-pointer".to_string(),
            ),
            ("DB_PASSWORD".to_string(), "s3cret".to_string()),
            ("APP_ENV".to_string(), "prod".to_string()),
        ]);

        strip_vault_secret_pointers(&mut env_vars);

        // The vault-load pointers must be gone — they are what leaks otherwise.
        assert!(
            !env_vars.contains_key(alien_core::ENV_ALIEN_SECRETS),
            "ALIEN_SECRETS must not reach the runtime-less container env"
        );
        assert!(
            !env_vars.contains_key(alien_core::ENV_ALIEN_RUNTIME_SECRETS),
            "ALIEN_RUNTIME_SECRETS must not reach the runtime-less container env"
        );
        // Concretely-delivered secrets and plain vars must survive untouched.
        assert_eq!(
            env_vars.get("DB_PASSWORD").map(String::as_str),
            Some("s3cret")
        );
        assert_eq!(env_vars.get("APP_ENV").map(String::as_str), Some("prod"));
        assert_eq!(env_vars.len(), 2);
    }
}
