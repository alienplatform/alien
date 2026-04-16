//! Local container manager using Docker via bollard.
//!
//! Manages containers on the local platform using Docker. Unlike cloud platforms
//! that use Horizon for orchestration, the Local platform uses Docker directly.
//!
//! # Features
//! - Creates a Docker network for inter-container communication
//! - Supports DNS aliases for service discovery (e.g., `postgres.svc`)
//! - Maps ports for publicly exposed containers
//! - Supports Docker volumes for persistent storage
//! - Streams container logs to dev command

use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use bollard::container::{
    Config, CreateContainerOptions, ListContainersOptions, LogOutput, LogsOptions,
    RemoveContainerOptions, StartContainerOptions, StopContainerOptions,
};
use bollard::models::{EndpointSettings, HostConfig, PortBinding};
use bollard::network::{CreateNetworkOptions, InspectNetworkOptions};
use bollard::volume::CreateVolumeOptions;
use bollard::Docker;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Default Docker network name for Alien containers.
const NETWORK_NAME: &str = "alien-network";

/// Allocates a host port, preferring a saved port if available.
///
/// This enables transparent recovery - when a container is recreated,
/// it tries to bind to the same port it had before. Only allocates a new random port
/// if the saved port is unavailable.
///
/// # Arguments
/// * `saved_port` - Previously allocated port (if any)
/// * `container_id` - Container ID for logging
///
/// # Returns
/// The allocated port number
fn allocate_host_port(saved_port: Option<u16>, container_id: &str) -> crate::error::Result<u16> {
    use alien_error::{Context, IntoAlienError};
    use std::net::TcpListener;

    if let Some(saved_port) = saved_port {
        // Try to bind to the saved port
        match TcpListener::bind(format!("127.0.0.1:{}", saved_port)) {
            Ok(socket) => {
                let port = socket
                    .local_addr()
                    .into_alien_error()
                    .context(ErrorData::DockerContainerError {
                        container: container_id.to_string(),
                        operation: "allocate_port".to_string(),
                        reason: "Failed to get saved port address".to_string(),
                    })?
                    .port();
                drop(socket); // Release for Docker to use
                tracing::info!(
                    container_id = %container_id,
                    port = port,
                    "Reusing saved host port (transparent recovery)"
                );
                return Ok(port);
            }
            Err(_) => {
                tracing::info!(
                    container_id = %container_id,
                    saved_port = saved_port,
                    "Saved host port unavailable, allocating new port"
                );
            }
        }
    }

    // No saved port or it's unavailable - allocate a new random port
    let port = port_check::free_local_port()
        .ok_or_else(|| AlienError::new(ErrorData::NoFreePortsAvailable))?;

    if saved_port.is_none() {
        tracing::info!(container_id = %container_id, port = port, "Allocated new host port");
    } else {
        tracing::info!(
            container_id = %container_id,
            old_port = saved_port,
            new_port = port,
            "Allocated new host port (saved port unavailable)"
        );
    }

    Ok(port)
}

/// Metadata stored for each container (for recovery).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerMetadata {
    /// Container resource ID
    pub container_id: String,
    /// Docker container ID (internal Docker ID)
    pub docker_container_id: String,
    /// Container image
    pub image: String,
    /// Container ports
    pub ports: Vec<u16>,
    /// Host port mapping (if exposed - maps first exposed port)
    pub host_port: Option<u16>,
    /// Whether this is a stateful container
    pub stateful: bool,
    /// Ordinal for stateful containers
    pub ordinal: Option<u32>,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Configuration for starting a container.
#[derive(Debug, Clone)]
pub struct ContainerConfig {
    /// Container image reference
    pub image: String,
    /// Container ports to expose internally
    pub ports: Vec<u16>,
    /// Whether to expose ports publicly (map to host ports)
    pub expose_public: bool,
    /// Environment variables
    pub env_vars: HashMap<String, String>,
    /// Whether this is a stateful container
    pub stateful: bool,
    /// Ordinal (for stateful containers)
    pub ordinal: Option<u32>,
    /// Volume mount path (for persistent storage)
    pub volume_mount: Option<String>,
    /// Volume size (for display only on local)
    pub volume_size: Option<String>,
    /// Bind mounts for linked resources (Storage, KV, Vault directories)
    /// The controller is responsible for rewriting binding env vars to use container paths.
    pub bind_mounts: Vec<BindMount>,
}

/// A bind mount for a linked resource directory.
///
/// Used for mounting host directories (Storage, KV, Vault) into containers.
/// The controller handles binding path rewriting; this is just mount metadata.
#[derive(Debug, Clone)]
pub struct BindMount {
    /// Host path (absolute path on the host machine)
    pub host_path: PathBuf,
    /// Container path (where to mount inside the container)
    pub container_path: String,
    /// Resource ID (for logging only)
    pub resource_id: String,
}

/// Result of starting a container.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerInfo {
    /// Container resource ID
    pub container_id: String,
    /// Docker container ID
    pub docker_container_id: String,
    /// Host port (if exposed publicly - uses first exposed port)
    pub host_port: Option<u16>,
    /// Container ports
    pub ports: Vec<u16>,
    /// Internal DNS name
    pub internal_dns: String,
}

/// Manager for local containers using Docker.
///
/// Uses the bollard crate to interact with the Docker daemon.
/// All containers are connected to a shared `alien-network` for DNS-based
/// service discovery.
#[derive(Debug)]
pub struct LocalContainerManager {
    docker: Docker,
    state_dir: PathBuf,
    /// Tracked containers (container_id → metadata)
    containers: Arc<RwLock<HashMap<String, ContainerMetadata>>>,
}

impl LocalContainerManager {
    /// Creates a new container manager.
    ///
    /// Attempts to connect to the local Docker daemon.
    ///
    /// # Arguments
    /// * `state_dir` - Base directory for container metadata
    pub fn new(state_dir: PathBuf) -> Result<Self> {
        let docker = Docker::connect_with_local_defaults()
            .into_alien_error()
            .context(ErrorData::DockerConnectionFailed {
                reason: "Failed to connect to Docker daemon. Is Docker running?".to_string(),
            })?;

        Ok(Self {
            docker,
            state_dir,
            containers: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Gets the tmp directory path for a container.
    ///
    /// Each container gets its own ephemeral tmp directory in the system temp location.
    /// This is mounted as /tmp in the container.
    ///
    /// Uses system temp (not state directory) because:
    /// - /tmp is ephemeral by definition (cleared on reboot)
    /// - System temp may be tmpfs (in-memory) for performance
    /// - State directory is for persistent state only
    /// - Matches cloud platform behavior (ephemeral ≠ persistent storage)
    pub fn get_container_tmp_dir(&self, container_id: &str) -> PathBuf {
        std::env::temp_dir()
            .join("alien-containers")
            .join(container_id)
    }

    /// Ensures the Docker network exists.
    ///
    /// Creates `alien-network` if it doesn't exist. This network is used
    /// for DNS-based service discovery between containers.
    pub async fn ensure_network(&self) -> Result<()> {
        // Check if network exists
        match self
            .docker
            .inspect_network(NETWORK_NAME, None::<InspectNetworkOptions<String>>)
            .await
        {
            Ok(_) => {
                debug!("Docker network '{}' already exists", NETWORK_NAME);
                return Ok(());
            }
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => {
                // Network doesn't exist, create it
            }
            Err(e) => {
                return Err(e)
                    .into_alien_error()
                    .context(ErrorData::DockerNetworkError {
                        network: NETWORK_NAME.to_string(),
                        operation: "inspect".to_string(),
                        reason: "Failed to inspect Docker network".to_string(),
                    });
            }
        }

        info!("Creating Docker network '{}'", NETWORK_NAME);

        let create_opts = CreateNetworkOptions {
            name: NETWORK_NAME,
            check_duplicate: true,
            driver: "bridge",
            internal: false,
            attachable: true,
            ingress: false,
            enable_ipv6: false,
            ..Default::default()
        };

        self.docker
            .create_network(create_opts)
            .await
            .into_alien_error()
            .context(ErrorData::DockerNetworkError {
                network: NETWORK_NAME.to_string(),
                operation: "create".to_string(),
                reason: "Failed to create Docker network".to_string(),
            })?;

        info!("✓ Docker network '{}' created", NETWORK_NAME);
        Ok(())
    }

    /// Resolves an image reference, loading from OCI tarball if it's a local path.
    ///
    /// If the image is a local file path that exists on disk, this method loads the
    /// OCI tarball into Docker and returns the loaded image tag.
    /// Otherwise, it returns the image reference as-is (for registry images).
    async fn resolve_image(&self, image: &str, container_id: &str) -> Result<String> {
        let path = Path::new(image);

        // Find the OCI tarball
        let tarball_path = if path.is_file() && image.ends_with(".tar") {
            // Direct path to tarball
            path.to_path_buf()
        } else if path.is_dir() {
            // Directory - look for *.oci.tar files
            Self::find_oci_tarball(path)?
        } else if !path.exists() {
            // Not a local path, treat as registry image
            debug!(image = %image, "Image is not a local path, treating as registry image");
            return Ok(image.to_string());
        } else {
            return Ok(image.to_string());
        };

        info!(
            tarball = %tarball_path.display(),
            container_id = %container_id,
            "Loading OCI image from local tarball"
        );

        // Use `docker load` instead of `import_image` to preserve CMD/ENTRYPOINT
        // docker import is for filesystem tarballs, docker load is for OCI image tarballs
        let output = tokio::process::Command::new("docker")
            .args(&["load", "-i", &tarball_path.to_string_lossy()])
            .output()
            .await
            .into_alien_error()
            .context(ErrorData::DockerContainerError {
                container: container_id.to_string(),
                operation: "docker_load".to_string(),
                reason: "Failed to execute docker load command".to_string(),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AlienError::new(ErrorData::DockerContainerError {
                container: container_id.to_string(),
                operation: "docker_load".to_string(),
                reason: format!("docker load failed: {}", stderr),
            }));
        }

        // Parse output to extract image tag
        // docker load output format: "Loaded image: <tag>" or "Loaded image ID: sha256:..."
        let stdout = String::from_utf8_lossy(&output.stdout);
        let loaded_image = stdout
            .lines()
            .find_map(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with("Loaded image:") {
                    Some(
                        trimmed
                            .trim_start_matches("Loaded image:")
                            .trim()
                            .to_string(),
                    )
                } else if trimmed.starts_with("Loaded image ID:") {
                    Some(
                        trimmed
                            .trim_start_matches("Loaded image ID:")
                            .trim()
                            .to_string(),
                    )
                } else {
                    None
                }
            })
            .unwrap_or_else(|| {
                // Fallback: generate a tag
                format!("alien-local/{}:latest", container_id)
            });

        info!(
            image_tag = %loaded_image,
            container_id = %container_id,
            tarball = %tarball_path.display(),
            "Successfully loaded OCI image with docker load"
        );

        Ok(loaded_image)
    }

    /// Finds an OCI tarball in a directory (searches *.oci.tar recursively up to 1 level deep).
    fn find_oci_tarball(dir: &Path) -> Result<PathBuf> {
        // First, look in the directory itself
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if name.ends_with(".oci.tar") {
                            return Ok(path);
                        }
                    }
                }
            }
        }

        // Then look one level deeper (e.g., {dir}/subdir/*.oci.tar)
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let subdir = entry.path();
                if subdir.is_dir() {
                    if let Ok(sub_entries) = std::fs::read_dir(&subdir) {
                        for sub_entry in sub_entries.flatten() {
                            let path = sub_entry.path();
                            if path.is_file() {
                                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                                    if name.ends_with(".oci.tar") {
                                        return Ok(path);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Err(AlienError::new(ErrorData::DockerContainerError {
            container: dir.display().to_string(),
            operation: "find_tarball".to_string(),
            reason: format!("No OCI tarball (*.oci.tar) found in {}", dir.display()),
        }))
    }

    /// Starts a container.
    ///
    /// Creates and starts a Docker container with the given configuration.
    /// The container is connected to `alien-network` with DNS aliases for
    /// service discovery.
    ///
    /// # Arguments
    /// * `container_id` - Alien resource ID for this container
    /// * `config` - Container configuration
    pub async fn start_container(
        &self,
        container_id: &str,
        config: ContainerConfig,
    ) -> Result<ContainerInfo> {
        // Ensure network exists
        self.ensure_network().await?;

        // Load existing metadata to check for saved host_port (for transparent recovery)
        let saved_host_port = {
            let metadata_file = self
                .state_dir
                .join("containers")
                .join(container_id)
                .join("metadata.json");
            if metadata_file.exists() {
                match tokio::fs::read_to_string(&metadata_file).await {
                    Ok(json) => serde_json::from_str::<ContainerMetadata>(&json)
                        .ok()
                        .and_then(|m| m.host_port),
                    Err(_) => None,
                }
            } else {
                None
            }
        };

        // Resolve image (load from OCI tarball if local path)
        let image = self.resolve_image(&config.image, container_id).await?;

        // Build DNS aliases
        let mut network_aliases = vec![container_id.to_string(), format!("{}.svc", container_id)];

        // Add ordinal-specific alias for stateful containers
        if config.stateful {
            let ordinal = config.ordinal.unwrap_or(0);
            network_aliases.push(format!("{}-{}.{}.svc", container_id, ordinal, container_id));
        }

        // Allocate host port if exposed publicly
        // This must be done BEFORE building env vars so we can inject the container binding
        // Try to reuse saved port for transparent recovery
        let host_port = if config.expose_public {
            Some(allocate_host_port(saved_host_port, container_id)?)
        } else {
            None
        };

        // Build environment variables
        // Note: The controller is responsible for rewriting binding paths to container paths.
        // We also need to rewrite localhost URLs to host.docker.internal for built-in
        // dev server URLs since containers run inside Docker and can't reach host via localhost.
        let mut env_vars = config.env_vars.clone();

        // Rewrite localhost URLs to host.docker.internal ONLY for known dev-server URLs
        // Don't rewrite user-provided env vars (they might intentionally use localhost)
        const DEV_SERVER_VARS: &[&str] = &[
            "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT",
            "ALIEN_COMMANDS_POLLING_URL",
        ];

        for key in DEV_SERVER_VARS {
            if let Some(value) = env_vars.get_mut(*key) {
                if value.contains("http://localhost:") || value.contains("https://localhost:") {
                    *value = value.replace("://localhost:", "://host.docker.internal:");
                }
            }
        }

        // Inject the current container binding so the container can discover its own URLs
        // This follows the same pattern as ALIEN_CURRENT_FUNCTION_BINDING_NAME for functions
        {
            use alien_core::bindings::{binding_env_var_name, BindingValue, ContainerBinding};

            // Internal URL uses Docker network DNS with first port
            let internal_dns = format!("{}.svc", container_id);
            let first_port = config.ports.first().copied().unwrap_or(8080);
            let internal_url = format!("http://{}:{}", internal_dns, first_port);

            // Public URL is the localhost-mapped port (if exposed publicly)
            let public_url = host_port.map(|p| format!("http://localhost:{}", p));

            let binding = if let Some(url) = public_url {
                ContainerBinding::local_with_public_url(
                    BindingValue::value(container_id.to_string()),
                    BindingValue::value(internal_url),
                    BindingValue::value(url),
                )
            } else {
                ContainerBinding::local(
                    BindingValue::value(container_id.to_string()),
                    BindingValue::value(internal_url),
                )
            };

            // Set ALIEN_CURRENT_CONTAINER_BINDING_NAME so AlienContext.get_current_container() works
            env_vars.insert(
                "ALIEN_CURRENT_CONTAINER_BINDING_NAME".to_string(),
                container_id.to_string(),
            );

            // Set the binding itself as ALIEN_{CONTAINER_ID}_BINDING
            if let Ok(binding_json) = serde_json::to_string(&binding) {
                let binding_key = binding_env_var_name(container_id);
                env_vars.insert(binding_key, binding_json);
            }
        }

        // Command polling is configured via environment variables (ALIEN_COMMANDS_POLLING_*)
        // The runtime CLI will read ALIEN_AGENT_ID, ALIEN_COMMANDS_POLLING_ENABLED, etc. from env
        // No need to inject here - it should come from the deployment configuration

        let env: Vec<String> = env_vars
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();

        // Build port bindings for all ports
        let (exposed_ports, port_bindings) = if config.expose_public && host_port.is_some() {
            let hp = host_port.unwrap();
            let mut exposed = HashMap::new();
            let mut bindings = HashMap::new();

            // Map first port to the assigned host port
            if let Some(&first_port) = config.ports.first() {
                let port_key = format!("{}/tcp", first_port);
                exposed.insert(port_key.clone(), HashMap::new());

                bindings.insert(
                    port_key,
                    Some(vec![PortBinding {
                        host_ip: Some("127.0.0.1".to_string()),
                        host_port: Some(hp.to_string()),
                    }]),
                );
            }

            (Some(exposed), Some(bindings))
        } else {
            (None, None)
        };

        // Build volume mounts (both persistent storage and linked storage)
        let mut binds = Vec::new();

        // Add persistent storage volume if configured
        if let Some(mount_path) = &config.volume_mount {
            let volume_name = format!("alien-{}-data", container_id);

            // Create Docker volume
            self.docker
                .create_volume(CreateVolumeOptions::<String> {
                    name: volume_name.clone(),
                    driver: "local".to_string(),
                    driver_opts: HashMap::new(),
                    labels: HashMap::new(),
                })
                .await
                .into_alien_error()
                .context(ErrorData::DockerVolumeError {
                    volume: volume_name.clone(),
                    operation: "create".to_string(),
                    reason: "Failed to create Docker volume".to_string(),
                })?;

            let bind = format!("{}:{}", volume_name, mount_path);
            binds.push(bind);
        }

        // Add bind mounts for linked resources (Storage, KV, Vault directories)
        for bind_mount in &config.bind_mounts {
            let bind = format!(
                "{}:{}",
                bind_mount.host_path.display(),
                bind_mount.container_path
            );
            binds.push(bind);

            info!(
                container_id = %container_id,
                resource_id = %bind_mount.resource_id,
                host_path = %bind_mount.host_path.display(),
                container_path = %bind_mount.container_path,
                "Mounting linked resource into container"
            );
        }

        let binds_option = if binds.is_empty() { None } else { Some(binds) };

        // Build network config
        let mut endpoints_config = HashMap::new();
        endpoints_config.insert(
            NETWORK_NAME.to_string(),
            EndpointSettings {
                aliases: Some(network_aliases.clone()),
                ..Default::default()
            },
        );

        // Build container config
        let container_config = Config {
            image: Some(image.clone()),
            hostname: Some(container_id.to_string()),
            env: Some(env),
            exposed_ports,
            host_config: Some(HostConfig {
                port_bindings,
                binds: binds_option,
                network_mode: Some(NETWORK_NAME.to_string()),
                // Add host.docker.internal mapping so containers can reach services on host
                // On Linux: maps to host gateway IP
                // On Mac/Windows: Docker Desktop provides this automatically, but explicit is fine
                extra_hosts: Some(vec!["host.docker.internal:host-gateway".to_string()]),
                ..Default::default()
            }),
            networking_config: Some(bollard::container::NetworkingConfig { endpoints_config }),
            ..Default::default()
        };

        // Generate a unique container name with timestamp to avoid conflicts
        let docker_name = format!("alien-{}", container_id);

        // Remove existing container with same name if it exists
        let _ = self
            .docker
            .remove_container(
                &docker_name,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;

        // Create container
        let response = self
            .docker
            .create_container(
                Some(CreateContainerOptions {
                    name: docker_name.clone(),
                    platform: None,
                }),
                container_config,
            )
            .await
            .into_alien_error()
            .context(ErrorData::DockerContainerError {
                container: container_id.to_string(),
                operation: "create".to_string(),
                reason: "Failed to create Docker container".to_string(),
            })?;

        // Start container
        self.docker
            .start_container(&response.id, None::<StartContainerOptions<String>>)
            .await
            .into_alien_error()
            .context(ErrorData::DockerContainerError {
                container: container_id.to_string(),
                operation: "start".to_string(),
                reason: "Failed to start Docker container".to_string(),
            })?;

        // Save metadata
        let metadata = ContainerMetadata {
            container_id: container_id.to_string(),
            docker_container_id: response.id.clone(),
            image,
            ports: config.ports.clone(),
            host_port,
            stateful: config.stateful,
            ordinal: config.ordinal,
            created_at: chrono::Utc::now(),
        };

        self.save_metadata(&metadata).await?;

        // Track in memory
        self.containers
            .write()
            .await
            .insert(container_id.to_string(), metadata);

        info!(
            container_id = %container_id,
            docker_id = %response.id,
            host_port = ?host_port,
            "Container started successfully"
        );

        Ok(ContainerInfo {
            container_id: container_id.to_string(),
            docker_container_id: response.id,
            host_port,
            ports: config.ports,
            internal_dns: format!("{}.svc", container_id),
        })
    }

    /// Stops a container.
    pub async fn stop_container(&self, container_id: &str) -> Result<()> {
        let docker_name = format!("alien-{}", container_id);

        self.docker
            .stop_container(&docker_name, Some(StopContainerOptions { t: 10 }))
            .await
            .into_alien_error()
            .context(ErrorData::DockerContainerError {
                container: container_id.to_string(),
                operation: "stop".to_string(),
                reason: "Failed to stop Docker container".to_string(),
            })?;

        debug!(container_id = %container_id, "Container stopped");
        Ok(())
    }

    /// Deletes a container (stop + remove).
    pub async fn delete_container(&self, container_id: &str) -> Result<()> {
        let docker_name = format!("alien-{}", container_id);

        // Stop and remove (force in case it's already stopped)
        match self
            .docker
            .remove_container(
                &docker_name,
                Some(RemoveContainerOptions {
                    force: true,
                    v: true, // Remove associated volumes
                    ..Default::default()
                }),
            )
            .await
        {
            Ok(_) => {
                info!(container_id = %container_id, "Container deleted");
            }
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => {
                debug!(container_id = %container_id, "Container already deleted");
            }
            Err(e) => {
                return Err(e)
                    .into_alien_error()
                    .context(ErrorData::DockerContainerError {
                        container: container_id.to_string(),
                        operation: "remove".to_string(),
                        reason: "Failed to remove Docker container".to_string(),
                    });
            }
        }

        // Remove from tracking
        self.containers.write().await.remove(container_id);

        // Remove metadata file
        let _ = self.delete_metadata(container_id).await;

        // Remove ephemeral tmp directory
        let tmp_dir = self.get_container_tmp_dir(container_id);
        if tmp_dir.exists() {
            let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
            debug!(
                container_id = %container_id,
                tmp_dir = %tmp_dir.display(),
                "Removed container tmp directory"
            );
        }

        Ok(())
    }

    /// Checks if a container is running.
    pub async fn is_running(&self, container_id: &str) -> bool {
        let docker_name = format!("alien-{}", container_id);

        let mut filters = HashMap::new();
        filters.insert("name".to_string(), vec![docker_name]);
        filters.insert("status".to_string(), vec!["running".to_string()]);

        match self
            .docker
            .list_containers(Some(ListContainersOptions {
                filters,
                ..Default::default()
            }))
            .await
        {
            Ok(containers) => !containers.is_empty(),
            Err(_) => false,
        }
    }

    /// Health check - verifies container is running.
    pub async fn check_health(&self, container_id: &str) -> Result<()> {
        if !self.is_running(container_id).await {
            return Err(AlienError::new(ErrorData::ContainerNotRunning {
                container_id: container_id.to_string(),
            }));
        }
        Ok(())
    }

    /// Gets the URL for an exposed container.
    pub async fn get_url(&self, container_id: &str) -> Result<Option<String>> {
        let containers = self.containers.read().await;
        if let Some(metadata) = containers.get(container_id) {
            if let Some(host_port) = metadata.host_port {
                return Ok(Some(format!("http://localhost:{}", host_port)));
            }
        }
        Ok(None)
    }

    /// Gets the binding configuration for a running container.
    ///
    /// This is used by the bindings provider to create a Container binding.
    pub async fn get_binding(
        &self,
        container_id: &str,
    ) -> Result<alien_core::bindings::ContainerBinding> {
        use alien_core::bindings::{BindingValue, ContainerBinding};

        let containers = self.containers.read().await;
        let metadata = containers.get(container_id).ok_or_else(|| {
            AlienError::new(ErrorData::ContainerNotRunning {
                container_id: container_id.to_string(),
            })
        })?;

        // Internal URL uses Docker network DNS
        let internal_dns = format!("{}.svc", container_id);
        let first_port = metadata.ports.first().copied().unwrap_or(8080);
        let internal_url = format!("http://{}:{}", internal_dns, first_port);

        // Public URL is the localhost-mapped port (if exposed publicly)
        let public_url = metadata
            .host_port
            .map(|p| format!("http://localhost:{}", p));

        let binding = if let Some(url) = public_url {
            ContainerBinding::local_with_public_url(
                BindingValue::value(container_id.to_string()),
                BindingValue::value(internal_url),
                BindingValue::value(url),
            )
        } else {
            ContainerBinding::local(
                BindingValue::value(container_id.to_string()),
                BindingValue::value(internal_url),
            )
        };

        Ok(binding)
    }

    /// Streams logs from a container.
    ///
    /// Returns a stream of log lines with their stream type (stdout/stderr).
    /// This is used by the dev server to capture container logs and send them to LogBuffer.
    pub async fn stream_logs(
        &self,
        container_id: &str,
    ) -> Result<impl futures_util::Stream<Item = (String, bool)> + Send + 'static> {
        let docker_name = format!("alien-{}", container_id);
        let container_id_for_warn = container_id.to_string();

        // Clone the docker client so the stream doesn't borrow self
        let docker = self.docker.clone();

        let log_options = LogsOptions::<String> {
            follow: true,
            stdout: true,
            stderr: true,
            timestamps: false,
            tail: "0".to_string(), // Start from beginning
            ..Default::default()
        };

        let log_stream = docker
            .logs(&docker_name, Some(log_options))
            .map(move |result| {
                match result {
                    Ok(LogOutput::StdOut { message }) => {
                        let line = String::from_utf8_lossy(&message).trim_end().to_string();
                        (line, true) // true = stdout
                    }
                    Ok(LogOutput::StdErr { message }) => {
                        let line = String::from_utf8_lossy(&message).trim_end().to_string();
                        (line, false) // false = stderr
                    }
                    Ok(LogOutput::Console { message }) => {
                        let line = String::from_utf8_lossy(&message).trim_end().to_string();
                        (line, true)
                    }
                    Ok(LogOutput::StdIn { .. }) => {
                        // Ignore stdin
                        (String::new(), true)
                    }
                    Err(e) => {
                        warn!(container_id = %container_id_for_warn, error = %e, "Error reading container logs");
                        (String::new(), true)
                    }
                }
            })
            .filter(|(line, _)| {
                let is_not_empty = !line.is_empty();
                futures_util::future::ready(is_not_empty)
            });

        Ok(log_stream)
    }

    // ─────────────── Metadata Persistence ───────────────────────────────────

    async fn save_metadata(&self, metadata: &ContainerMetadata) -> Result<()> {
        let metadata_dir = self
            .state_dir
            .join("containers")
            .join(&metadata.container_id);
        tokio::fs::create_dir_all(&metadata_dir)
            .await
            .into_alien_error()
            .context(ErrorData::LocalDirectoryError {
                path: metadata_dir.display().to_string(),
                operation: "create".to_string(),
                reason: "Failed to create container metadata directory".to_string(),
            })?;

        let metadata_file = metadata_dir.join("metadata.json");
        let json = serde_json::to_string_pretty(metadata)
            .into_alien_error()
            .context(ErrorData::LocalDirectoryError {
                path: metadata_file.display().to_string(),
                operation: "serialize".to_string(),
                reason: "Failed to serialize container metadata".to_string(),
            })?;

        tokio::fs::write(&metadata_file, json)
            .await
            .into_alien_error()
            .context(ErrorData::LocalDirectoryError {
                path: metadata_file.display().to_string(),
                operation: "write".to_string(),
                reason: "Failed to write container metadata".to_string(),
            })?;

        Ok(())
    }

    async fn delete_metadata(&self, container_id: &str) -> Result<()> {
        let metadata_dir = self.state_dir.join("containers").join(container_id);
        if metadata_dir.exists() {
            let _ = tokio::fs::remove_dir_all(&metadata_dir).await;
        }
        Ok(())
    }

    /// Loads existing container metadata from disk (for recovery).
    pub async fn load_metadata(&self) -> Result<Vec<ContainerMetadata>> {
        let containers_dir = self.state_dir.join("containers");
        if !containers_dir.exists() {
            return Ok(Vec::new());
        }

        let mut metadata_list = Vec::new();
        let mut entries = tokio::fs::read_dir(&containers_dir)
            .await
            .into_alien_error()
            .context(ErrorData::LocalDirectoryError {
                path: containers_dir.display().to_string(),
                operation: "read".to_string(),
                reason: "Failed to read containers directory".to_string(),
            })?;

        while let Some(entry) = entries.next_entry().await.into_alien_error().context(
            ErrorData::LocalDirectoryError {
                path: containers_dir.display().to_string(),
                operation: "iterate".to_string(),
                reason: "Failed to iterate containers directory".to_string(),
            },
        )? {
            let metadata_file = entry.path().join("metadata.json");
            if metadata_file.exists() {
                match tokio::fs::read_to_string(&metadata_file).await {
                    Ok(json) => match serde_json::from_str::<ContainerMetadata>(&json) {
                        Ok(metadata) => {
                            metadata_list.push(metadata);
                        }
                        Err(e) => {
                            warn!(
                                path = %metadata_file.display(),
                                error = %e,
                                "Failed to parse container metadata"
                            );
                        }
                    },
                    Err(e) => {
                        warn!(
                            path = %metadata_file.display(),
                            error = %e,
                            "Failed to read container metadata"
                        );
                    }
                }
            }
        }

        Ok(metadata_list)
    }
}
