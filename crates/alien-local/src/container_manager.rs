//! Local container manager using Docker via bollard.
//!
//! Manages containers on the local platform using Docker. Unlike cloud platforms
//! that use managed container orchestration, the Local platform uses Docker directly.
//!
//! # Features
//! - Creates a Docker network for inter-container communication
//! - Supports DNS aliases for service discovery (e.g., `postgres.svc`)
//! - Maps ports for publicly exposed containers
//! - Supports Docker volumes for persistent storage
//! - Streams container logs to dev command

use crate::error::{ErrorData, Result};
use alien_core::{ExposeProtocol, ENV_ALIEN_COMMANDS_URL};
use alien_error::{AlienError, Context, IntoAlienError};
use bollard::container::{
    Config, CreateContainerOptions, ListContainersOptions, LogOutput, LogsOptions,
    RemoveContainerOptions, StartContainerOptions, StopContainerOptions,
};
use bollard::models::{EndpointSettings, HostConfig, PortBinding};
use bollard::network::{CreateNetworkOptions, InspectNetworkOptions};
use bollard::volume::{CreateVolumeOptions, RemoveVolumeOptions};
use bollard::Docker;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Default Docker network name for Alien containers.
const NETWORK_NAME: &str = "deployment-network";

/// Alien-injected dev-server URLs whose `localhost` must be rewritten to
/// `host.docker.internal` so a container running inside Docker can reach the
/// host. User-provided env vars are left untouched (they may intentionally use
/// localhost). Includes the command receiver base URL (`ALIEN_COMMANDS_URL`)
/// because it points at the local manager.
const DEV_SERVER_URL_VARS: &[&str] = &["OTEL_EXPORTER_OTLP_LOGS_ENDPOINT", ENV_ALIEN_COMMANDS_URL];

/// Rewrite `://localhost:` to `://host.docker.internal:` for the known
/// Alien-injected dev-server URL env vars only.
fn rewrite_dev_server_localhost_urls(env_vars: &mut HashMap<String, String>) {
    for key in DEV_SERVER_URL_VARS {
        if let Some(value) = env_vars.get_mut(*key) {
            if value.contains("http://localhost:") || value.contains("https://localhost:") {
                *value = value.replace("://localhost:", "://host.docker.internal:");
            }
        }
    }
}

fn endpoint_scheme(protocol: ExposeProtocol) -> &'static str {
    match protocol {
        ExposeProtocol::Http => "http",
        ExposeProtocol::Tcp => "tcp",
    }
}

type ExposedPorts = HashMap<String, HashMap<(), ()>>;
type PortBindings = HashMap<String, Option<Vec<PortBinding>>>;

fn loopback_port_bindings(
    public_endpoint: Option<&LocalPublicEndpoint>,
    host_port: Option<u16>,
    health_check_port: Option<u16>,
    health_host_port: Option<u16>,
) -> (Option<ExposedPorts>, Option<PortBindings>) {
    let mut exposed = HashMap::new();
    let mut bindings = HashMap::new();
    let mut insert = |container_port: u16, host_port: u16| {
        let key = format!("{container_port}/tcp");
        exposed.entry(key.clone()).or_insert_with(HashMap::new);
        bindings.entry(key).or_insert_with(|| {
            Some(vec![PortBinding {
                host_ip: Some("127.0.0.1".to_string()),
                host_port: Some(host_port.to_string()),
            }])
        });
    };
    if let (Some(endpoint), Some(host_port)) = (public_endpoint, host_port) {
        insert(endpoint.port, host_port);
    }
    if let (Some(container_port), Some(host_port)) = (health_check_port, health_host_port) {
        insert(container_port, host_port);
    }
    if exposed.is_empty() {
        (None, None)
    } else {
        (Some(exposed), Some(bindings))
    }
}

async fn probe_http_health(
    container_id: &str,
    host_port: u16,
    method: &str,
    path: &str,
    timeout: std::time::Duration,
) -> Result<()> {
    let method = reqwest::Method::from_bytes(method.as_bytes()).map_err(|error| {
        AlienError::new(ErrorData::DockerContainerError {
            container: container_id.to_string(),
            operation: "health_check".to_string(),
            reason: format!("Invalid HTTP method: {error}"),
        })
    })?;
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    let url = format!("http://127.0.0.1:{host_port}{path}");
    let response = reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .into_alien_error()
        .context(ErrorData::DockerContainerError {
            container: container_id.to_string(),
            operation: "health_check".to_string(),
            reason: "Failed to build HTTP health-check client".to_string(),
        })?
        .request(method, &url)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::DockerContainerError {
            container: container_id.to_string(),
            operation: "health_check".to_string(),
            reason: format!("Request to {url} failed"),
        })?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(AlienError::new(ErrorData::DockerContainerError {
            container: container_id.to_string(),
            operation: "health_check".to_string(),
            reason: format!("{url} returned HTTP {}", response.status()),
        }))
    }
}

#[derive(Debug, PartialEq, Eq)]
struct OciProcessOverride {
    entrypoint: Option<Vec<String>>,
    cmd: Option<Vec<String>>,
}

fn oci_process_override(command: Option<&[String]>) -> OciProcessOverride {
    OciProcessOverride {
        entrypoint: command
            .filter(|command| !command.is_empty())
            .map(|command| command.to_vec()),
        cmd: None,
    }
}

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

fn allocate_distinct_host_port(
    saved_port: Option<u16>,
    container_id: &str,
    already_allocated: Option<u16>,
) -> crate::error::Result<u16> {
    let mut candidate = allocate_host_port(saved_port, container_id)?;
    for _ in 0..10 {
        if Some(candidate) != already_allocated {
            return Ok(candidate);
        }
        candidate = allocate_host_port(None, container_id)?;
    }
    Err(AlienError::new(ErrorData::NoFreePortsAvailable))
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
    /// Loopback host port used for HTTP health checks.
    #[serde(default)]
    pub health_host_port: Option<u16>,
    /// Public endpoint served by the mapped host port.
    #[serde(default)]
    pub public_endpoint: Option<LocalPublicEndpoint>,
    /// Whether this is a stateful container
    pub stateful: bool,
    /// Ordinal for stateful containers
    pub ordinal: Option<u32>,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// The one backend endpoint published by the Local Docker runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalPublicEndpoint {
    /// Container port published on loopback.
    pub port: u16,
    /// Application protocol used to construct endpoint URLs.
    pub protocol: ExposeProtocol,
    /// Resource endpoint names resolved to this one backend.
    #[serde(default)]
    pub names: Vec<String>,
}

/// Configuration for starting a container.
#[derive(Debug, Clone)]
pub struct ContainerConfig {
    /// Container image reference
    pub image: String,
    /// Command override for the container image.
    pub command: Option<Vec<String>>,
    /// Container ports to expose internally
    pub ports: Vec<u16>,
    /// Backend endpoint to publish on a loopback host port.
    pub public_endpoint: Option<LocalPublicEndpoint>,
    /// Container port to publish on loopback for HTTP health checks.
    pub health_check_port: Option<u16>,
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
    /// Deployment token for authenticated pulls from the manager's registry
    /// proxy. Public-registry images (e.g. `postgres:16-alpine`) pull
    /// anonymously; when the anonymous pull is rejected and this token is
    /// present, the pull is retried as `deployment:<token>` basic auth —
    /// the same credential the local worker manager uses for proxy pulls.
    pub proxy_token: Option<String>,
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
    /// Whether a host-side Alien workload also opens files in this directory.
    /// When true, local containers must create files as the host operator user
    /// so native workloads can reopen them.
    pub shared_with_host_workloads: bool,
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
    /// Loopback host port used for HTTP health checks.
    #[serde(default)]
    pub health_host_port: Option<u16>,
    /// Public endpoint served by the mapped host port.
    #[serde(default)]
    pub public_endpoint: Option<LocalPublicEndpoint>,
    /// Container ports
    pub ports: Vec<u16>,
    /// Internal DNS name
    pub internal_dns: String,
}

/// Cheap Docker runtime status for local controller heartbeats.
#[derive(Debug, Clone)]
pub struct LocalRuntimeStatus {
    pub docker_version: Option<String>,
    pub docker_api_version: Option<String>,
    pub docker_os: Option<String>,
    pub docker_arch: Option<String>,
    pub tracked_containers: u32,
    pub running_containers: u32,
}

/// Manager for local containers using Docker.
///
/// Uses the bollard crate to interact with the Docker daemon.
/// All containers are connected to a shared `deployment-network` for DNS-based
/// service discovery.
#[derive(Debug)]
pub struct LocalContainerManager {
    docker: Docker,
    docker_host: String,
    state_dir: PathBuf,
    /// Tracked containers (container_id → metadata)
    containers: Arc<RwLock<HashMap<String, ContainerMetadata>>>,
    /// Serialize imports that share content or mutate the same registry reference.
    image_load_locks: tokio::sync::Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>,
}

impl LocalContainerManager {
    fn persistent_storage_dir(&self, container_id: &str) -> PathBuf {
        self.state_dir.join("container-volumes").join(container_id)
    }

    async fn persistent_storage_bind(
        &self,
        container_id: &str,
        mount_path: &str,
        run_as_host_user: bool,
    ) -> Result<String> {
        let volume_name = format!("alien-{container_id}-data");

        // Preserve data created by releases that always used Docker named
        // volumes. New Linux containers that run as the host operator use a
        // host-owned directory instead: Docker initializes an empty named
        // volume as root, so the explicitly non-root workload cannot write to
        // it. A directory created by this process has the exact uid/gid the
        // container is configured to use and requires no privileged init
        // container or helper image.
        match self.docker.inspect_volume(&volume_name).await {
            Ok(_) => return Ok(format!("{volume_name}:{mount_path}")),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => {}
            Err(error) => {
                return Err(error)
                    .into_alien_error()
                    .context(ErrorData::DockerVolumeError {
                        volume: volume_name,
                        operation: "inspect".to_string(),
                        reason: "Failed to inspect Docker volume".to_string(),
                    });
            }
        }

        #[cfg(target_os = "linux")]
        if run_as_host_user {
            let storage_dir = self.persistent_storage_dir(container_id);
            tokio::fs::create_dir_all(&storage_dir)
                .await
                .into_alien_error()
                .context(ErrorData::LocalDirectoryError {
                    path: storage_dir.display().to_string(),
                    operation: "create".to_string(),
                    reason: "Failed to create container persistent storage directory".to_string(),
                })?;
            return Ok(format!("{}:{mount_path}", storage_dir.display()));
        }

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

        Ok(format!("{volume_name}:{mount_path}"))
    }

    /// Creates a new container manager.
    ///
    /// Attempts to connect to the local Docker daemon.
    ///
    /// # Arguments
    /// * `state_dir` - Base directory for container metadata
    pub fn new(state_dir: PathBuf) -> Result<Self> {
        let (docker, docker_host) = crate::docker_connection::connect_docker_with_host()?;

        let containers = Self::load_metadata_from_disk(&state_dir)?
            .into_iter()
            .map(|metadata| (metadata.container_id.clone(), metadata))
            .collect();

        Ok(Self {
            docker,
            docker_host,
            state_dir,
            containers: Arc::new(RwLock::new(containers)),
            image_load_locks: tokio::sync::Mutex::new(HashMap::new()),
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
    /// Creates `deployment-network` if it doesn't exist. This network is used
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

    /// Reads cheap Docker runtime metadata without inspecting logs or host files.
    pub async fn runtime_status(&self) -> Result<LocalRuntimeStatus> {
        let version = self.docker.version().await.into_alien_error().context(
            ErrorData::DockerConnectionFailed {
                reason: "Failed to query Docker daemon version".to_string(),
            },
        )?;

        let tracked_container_ids: Vec<String> = {
            let containers = self.containers.read().await;
            containers.keys().cloned().collect()
        };
        let tracked_containers = tracked_container_ids.len() as u32;
        let mut running_containers = 0u32;

        for container_id in tracked_container_ids {
            if self.is_running(&container_id).await {
                running_containers += 1;
            }
        }

        Ok(LocalRuntimeStatus {
            docker_version: version.version,
            docker_api_version: version.api_version,
            docker_os: version.os,
            docker_arch: version.arch,
            tracked_containers,
            running_containers,
        })
    }

    /// Resolves an image reference, loading from OCI tarball if it's a local path.
    ///
    /// If the image is a local file path that exists on disk, this method loads the
    /// OCI tarball into Docker and returns the loaded image tag.
    /// Otherwise, it returns the image reference as-is (for registry images).
    async fn resolve_image(
        &self,
        image: &str,
        container_id: &str,
        proxy_token: Option<&str>,
    ) -> Result<String> {
        let path = Path::new(image);

        // Find the OCI tarball
        let tarball_path = if path.is_file() && image.ends_with(".tar") {
            // Direct path to tarball
            path.to_path_buf()
        } else if path.is_dir() {
            // Directory - look for *.oci.tar files
            Self::find_oci_tarball(path)?
        } else if !path.exists() {
            // Not a local path: a registry image. Pull it explicitly so the
            // later `create` never depends on an implicit anonymous pull —
            // source-built container images live behind the manager's
            // registry proxy, which requires deployment-token auth for GETs.
            // Public images (e.g. `postgres:16-alpine`) pull anonymously
            // first; only a rejected anonymous pull retries with the token.
            debug!(image = %image, "Image is not a local path, pulling registry image");
            return self
                .pull_registry_image(image, container_id, proxy_token)
                .await;
        } else {
            return Ok(image.to_string());
        };

        self.load_oci_tarball_into_docker(&tarball_path, container_id)
            .await
    }

    /// `docker load` an OCI tarball and return a reference the daemon can
    /// `create` from: the image's immutable ID on the active image store.
    async fn load_oci_tarball_into_docker(
        &self,
        tarball_path: &Path,
        container_id: &str,
    ) -> Result<String> {
        // Docker's containerd image store identifies the image by the manifest
        // digest listed in the archive's index.json; the classic store by the
        // config digest. Both are immutable, unlike the archive's tag, which a
        // previous load of the same tag may still point at.
        let archive_path = tarball_path.to_path_buf();
        let (candidates, mut lock_keys) =
            tokio::task::spawn_blocking(move || -> std::io::Result<([String; 2], Vec<String>)> {
                let (manifest_digest, references) = oci_archive_identity(&archive_path)?;
                let config_digest = dockdash::Image::from_tarball(&archive_path)
                    .map_err(std::io::Error::other)?
                    .config_digest()
                    .to_string();
                let mut lock_keys = references;
                lock_keys.push(format!("content:{config_digest}"));
                Ok(([manifest_digest, config_digest], lock_keys))
            })
            .await
            .into_alien_error()
            .context(ErrorData::DockerContainerError {
                container: container_id.to_string(),
                operation: "resolve_loaded_image".to_string(),
                reason: "Image archive reader task failed".to_string(),
            })?
            .into_alien_error()
            .context(ErrorData::DockerContainerError {
                container: container_id.to_string(),
                operation: "resolve_loaded_image".to_string(),
                reason: format!(
                    "Failed to read the image digests of '{}'",
                    tarball_path.display()
                ),
            })?;

        // Resolve keys before locking; independent archives can be read and imported
        // concurrently. Ordering prevents deadlock for archives with multiple tags.
        lock_keys.sort();
        lock_keys.dedup();
        let locks = {
            let mut locks = self.image_load_locks.lock().await;
            retain_image_load_locks(&mut locks, lock_keys)
        };
        let mut load_guards = Vec::with_capacity(locks.len());
        for lock in locks {
            load_guards.push(lock.lock_owned().await);
        }

        if let Some(image_id) = self
            .inspect_archive_image(&candidates, container_id)
            .await?
        {
            return Ok(image_id);
        }
        info!(
            tarball = %tarball_path.display(),
            container_id = %container_id,
            "Loading OCI image from local tarball"
        );

        // Use `docker load` instead of `import_image` to preserve CMD/ENTRYPOINT
        // docker import is for filesystem tarballs, docker load is for OCI image tarballs
        let output = crate::docker_connection::docker_command(&self.docker_host)
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

        if let Some(image_id) = self
            .inspect_archive_image(&candidates, container_id)
            .await?
        {
            return Ok(image_id);
        }
        Err(AlienError::new(ErrorData::DockerContainerError {
            container: container_id.to_string(),
            operation: "inspect_loaded_image".to_string(),
            reason: format!(
                "Docker did not load image {} (manifest) or {} (config)",
                candidates[0], candidates[1]
            ),
        }))
    }

    async fn inspect_archive_image(
        &self,
        candidates: &[String; 2],
        container_id: &str,
    ) -> Result<Option<String>> {
        for image_id in candidates {
            match self.docker.inspect_image(image_id).await {
                Ok(_) => return Ok(Some(image_id.clone())),
                Err(bollard::errors::Error::DockerResponseServerError {
                    status_code: 404, ..
                }) => continue,
                Err(error) => {
                    return Err(error).into_alien_error().context(
                        ErrorData::DockerContainerError {
                            container: container_id.to_string(),
                            operation: "inspect_loaded_image".to_string(),
                            reason: format!("Failed to inspect loaded image {image_id}"),
                        },
                    );
                }
            }
        }
        Ok(None)
    }

    /// Make a registry image available to the daemon and return a reference
    /// `create` can use. Three attempts, cheapest first:
    ///
    /// 1. Daemon-side anonymous pull — public images (`postgres:16-alpine`).
    /// 2. Daemon-side pull with `deployment:<token>` basic auth (the manager
    ///    registry proxy's pull credential) — proxies the daemon can reach
    ///    over HTTPS, e.g. the E2E harness's public manager URL.
    /// 3. Host-side pull via dockdash with the same credential (anonymous
    ///    when there is none), then `docker load` — the dev server's proxy
    ///    lives on the HOST's localhost, which the daemon cannot reach (and
    ///    would refuse as a plain-HTTP registry anyway). The operator process
    ///    CAN reach it, exactly like the local worker manager's image pulls.
    ///    Loopback registries skip steps 1 and 2.
    async fn pull_registry_image(
        &self,
        image: &str,
        container_id: &str,
        proxy_token: Option<&str>,
    ) -> Result<String> {
        use futures_util::TryStreamExt;

        let options = Some(bollard::image::CreateImageOptions {
            from_image: image.to_string(),
            ..Default::default()
        });

        // Docker Desktop's daemon cannot reach the manager's host loopback registry.
        let host_local = image.starts_with("127.0.0.1:")
            || image.starts_with("localhost:")
            || image.starts_with("[::1]:");
        if !host_local {
            // 1. Daemon-side, anonymous.
            if self
                .docker
                .create_image(options.clone(), None, None)
                .try_collect::<Vec<_>>()
                .await
                .is_ok()
            {
                return Ok(image.to_string());
            }

            let Some(token) = proxy_token else {
                return Err(AlienError::new(ErrorData::DockerContainerError {
                    container: container_id.to_string(),
                    operation: "pull_image".to_string(),
                    reason: format!(
                        "Anonymous pull of '{}' failed and no deployment token is available",
                        image
                    ),
                }));
            };

            // 2. Daemon-side, deployment-token auth.
            info!(
                image = %image,
                container_id = %container_id,
                "Anonymous pull rejected; retrying with deployment-token auth"
            );
            let credentials = bollard::auth::DockerCredentials {
                username: Some("deployment".to_string()),
                password: Some(token.to_string()),
                ..Default::default()
            };
            if self
                .docker
                .create_image(options, None, Some(credentials))
                .try_collect::<Vec<_>>()
                .await
                .is_ok()
            {
                return Ok(image.to_string());
            }
        }
        // A remote registry without a token already returned above, so only a loopback
        // registry pulls anonymously here; it may serve public images without credentials.
        let auth = match proxy_token {
            Some(token) => {
                dockdash::RegistryAuth::Basic("deployment".to_string(), token.to_string())
            }
            None => dockdash::RegistryAuth::Anonymous,
        };

        // 3. Host-side pull + docker load.
        info!(
            image = %image,
            container_id = %container_id,
            "Pulling on the host and loading into Docker"
        );
        let protocol = if host_local {
            dockdash::ClientProtocol::Http
        } else {
            dockdash::ClientProtocol::Https
        };
        let container_target = alien_core::BinaryTarget::linux_container_target();
        let arch = match container_target.oci_arch() {
            "arm64" => dockdash::Arch::ARM64,
            _ => dockdash::Arch::Amd64,
        };
        let (pulled, _diagnostics) = dockdash::Image::builder()
            .from(image)
            .pull_policy(dockdash::PullPolicy::Always)
            .protocol(protocol)
            .platform(container_target.oci_os(), &arch)
            .auth(auth)
            .build()
            .await
            .into_alien_error()
            .context(ErrorData::DockerContainerError {
                container: container_id.to_string(),
                operation: "pull_image".to_string(),
                reason: format!("Host-side registry pull of '{}' failed", image),
            })?;

        self.load_oci_tarball_into_docker(pulled.path(), container_id)
            .await
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
    /// The container is connected to `deployment-network` with DNS aliases for
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
        let (saved_host_port, saved_health_host_port) = {
            let metadata_file = self
                .state_dir
                .join("containers")
                .join(container_id)
                .join("metadata.json");
            if metadata_file.exists() {
                match tokio::fs::read_to_string(&metadata_file).await {
                    Ok(json) => serde_json::from_str::<ContainerMetadata>(&json)
                        .ok()
                        .map(|m| (m.host_port, m.health_host_port))
                        .unwrap_or((None, None)),
                    Err(_) => (None, None),
                }
            } else {
                (None, None)
            }
        };

        // Freeze registry references as well as archive images before creation.
        let image_reference = self
            .resolve_image(&config.image, container_id, config.proxy_token.as_deref())
            .await?;
        let image = self
            .docker
            .inspect_image(&image_reference)
            .await
            .into_alien_error()
            .context(ErrorData::DockerContainerError {
                container: container_id.to_string(),
                operation: "inspect_image".to_string(),
                reason: format!("Failed to resolve image identity for '{image_reference}'"),
            })?
            .id
            .ok_or_else(|| {
                AlienError::new(ErrorData::DockerContainerError {
                    container: container_id.to_string(),
                    operation: "inspect_image".to_string(),
                    reason: format!("Docker returned no image ID for '{image_reference}'"),
                })
            })?;

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
        let host_port = if config.public_endpoint.is_some() {
            Some(allocate_host_port(saved_host_port, container_id)?)
        } else {
            None
        };
        let health_host_port = match config.health_check_port {
            Some(health_port)
                if config
                    .public_endpoint
                    .as_ref()
                    .map(|endpoint| endpoint.port)
                    == Some(health_port) =>
            {
                host_port
            }
            Some(_) => Some(allocate_distinct_host_port(
                saved_health_host_port,
                container_id,
                host_port,
            )?),
            None => None,
        };

        // Build environment variables
        // Note: The controller is responsible for rewriting binding paths to container paths.
        // We also need to rewrite localhost URLs to host.docker.internal for built-in
        // dev server URLs since containers run inside Docker and can't reach host via localhost.
        let mut env_vars = config.env_vars.clone();
        rewrite_dev_server_localhost_urls(&mut env_vars);

        // Inject the current container binding so the container can discover its own URLs.
        {
            use alien_core::{
                bindings::{serialize_binding_as_env_var, BindingValue, ContainerBinding},
                ENV_ALIEN_CURRENT_CONTAINER_BINDING_NAME,
            };

            // Internal URL uses Docker network DNS with the public backend port.
            let internal_dns = format!("{}.svc", container_id);
            let endpoint = config.public_endpoint.as_ref();
            let internal_port = endpoint
                .map(|endpoint| endpoint.port)
                .or_else(|| config.ports.first().copied())
                .unwrap_or(8080);
            let scheme = endpoint.map_or("http", |endpoint| endpoint_scheme(endpoint.protocol));
            let internal_url = format!("{scheme}://{internal_dns}:{internal_port}");

            // Public URL is the localhost-mapped port (if exposed publicly)
            let public_url = host_port.map(|port| format!("{scheme}://localhost:{port}"));

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

            env_vars.insert(
                ENV_ALIEN_CURRENT_CONTAINER_BINDING_NAME.to_string(),
                container_id.to_string(),
            );

            let binding_env_vars =
                serialize_binding_as_env_var(container_id, &binding).map_err(|err| {
                    AlienError::new(ErrorData::Other {
                        message: err.to_string(),
                    })
                })?;
            env_vars.extend(binding_env_vars);
        }

        let env: Vec<String> = env_vars
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();

        // Build port bindings for all ports
        let (exposed_ports, port_bindings) = loopback_port_bindings(
            config.public_endpoint.as_ref(),
            host_port,
            config.health_check_port,
            health_host_port,
        );

        // Build volume mounts (both persistent storage and linked storage)
        let mut binds = Vec::new();

        // Add persistent storage volume if configured
        if let Some(mount_path) = &config.volume_mount {
            let run_as_host_user = shared_bind_mount_user(&config.bind_mounts).is_some();
            binds.push(
                self.persistent_storage_bind(container_id, mount_path, run_as_host_user)
                    .await?,
            );
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

        // Local file-backed bindings are shared with host-side workloads
        // (notably runtime-less Daemons) and with the operator's health
        // probes. Run a container that receives those bind mounts as the
        // operator's Unix uid/gid so every process creates SQLite/WAL and
        // storage files with the same ownership. Leaving the image's user in
        // place lets the first container process create files the host-side
        // daemon cannot reopen (EACCES), even though both were given the same
        // binding path.
        //
        // Containers without linked-resource bind mounts keep the image's
        // declared user unchanged.
        let user = shared_bind_mount_user(&config.bind_mounts);

        // `ContainerConfig::command` has Kubernetes `command` semantics: it
        // replaces the image ENTRYPOINT rather than becoming its CMD.
        let process_override = oci_process_override(config.command.as_deref());

        // Build container config
        let container_config = Config {
            image: Some(image.clone()),
            labels: Some(HashMap::from([
                ("alien.dev/resource".to_string(), container_id.to_string()),
                (
                    "alien.dev/image-reference".to_string(),
                    config.image.clone(),
                ),
                ("alien.dev/image-id".to_string(), image.clone()),
            ])),
            entrypoint: process_override.entrypoint,
            cmd: process_override.cmd,
            user,
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
                // Recover crashes, but preserve a manual stop across Docker daemon restarts.
                restart_policy: Some(bollard::models::RestartPolicy {
                    name: Some(bollard::models::RestartPolicyNameEnum::UNLESS_STOPPED),
                    maximum_retry_count: None,
                }),
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
            health_host_port,
            public_endpoint: config.public_endpoint.clone(),
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
            health_host_port,
            public_endpoint: config.public_endpoint,
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

    /// Deletes a container and the persistent storage owned by the resource.
    ///
    /// Container replacement uses [`Self::delete_container`] so data survives
    /// updates. Resource deletion uses this method so neither the current
    /// host-backed directory nor a named volume created by an older release is
    /// leaked.
    pub async fn delete_container_and_storage(&self, container_id: &str) -> Result<()> {
        self.delete_container(container_id).await?;

        let volume_name = format!("alien-{container_id}-data");
        match self
            .docker
            .remove_volume(&volume_name, Some(RemoveVolumeOptions { force: true }))
            .await
        {
            Ok(_) => {
                info!(volume = %volume_name, "Container persistent volume deleted");
            }
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => {}
            Err(error) => {
                return Err(error)
                    .into_alien_error()
                    .context(ErrorData::DockerVolumeError {
                        volume: volume_name,
                        operation: "delete".to_string(),
                        reason: "Failed to delete container persistent volume".to_string(),
                    });
            }
        }

        let storage_dir = self.persistent_storage_dir(container_id);
        match tokio::fs::remove_dir_all(&storage_dir).await {
            Ok(_) => {
                info!(
                    path = %storage_dir.display(),
                    "Container persistent storage directory deleted"
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .into_alien_error()
                    .context(ErrorData::LocalDirectoryError {
                        path: storage_dir.display().to_string(),
                        operation: "delete".to_string(),
                        reason: "Failed to delete container persistent storage directory"
                            .to_string(),
                    });
            }
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

    /// Reads the Docker restart count for a managed container.
    pub async fn restart_count(&self, container_id: &str) -> Result<u32> {
        let docker_name = format!("alien-{container_id}");
        let inspection = self
            .docker
            .inspect_container(&docker_name, None)
            .await
            .into_alien_error()
            .context(ErrorData::DockerContainerError {
                container: container_id.to_string(),
                operation: "inspect".to_string(),
                reason: "Failed to inspect Docker container restart count".to_string(),
            })?;
        Ok(inspection
            .restart_count
            .unwrap_or_default()
            .clamp(0, u32::MAX.into()) as u32)
    }

    /// Whether Docker still has the desired container, including a manually stopped one.
    pub async fn container_exists(&self, container_id: &str) -> Result<bool> {
        match self
            .docker
            .inspect_container(&format!("alien-{container_id}"), None)
            .await
        {
            Ok(_) => Ok(true),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(false),
            Err(error) => Err(error)
                .into_alien_error()
                .context(ErrorData::DockerContainerError {
                    container: container_id.to_string(),
                    operation: "inspect".to_string(),
                    reason: "Failed to check whether the container exists".to_string(),
                }),
        }
    }

    /// Verifies that the container process is running and, when configured,
    /// that its declared HTTP health endpoint returns a successful status.
    pub async fn check_health(
        &self,
        container_id: &str,
        method: Option<&str>,
        path: Option<&str>,
        timeout: std::time::Duration,
    ) -> Result<u32> {
        let docker_name = format!("alien-{container_id}");
        let inspection = self
            .docker
            .inspect_container(&docker_name, None)
            .await
            .into_alien_error()
            .context(ErrorData::DockerContainerError {
                container: container_id.to_string(),
                operation: "health_check".to_string(),
                reason: "Failed to inspect Docker container state".to_string(),
            })?;
        if !inspection
            .state
            .and_then(|state| state.running)
            .unwrap_or(false)
        {
            return Err(AlienError::new(ErrorData::ContainerNotRunning {
                container_id: container_id.to_string(),
            }));
        }
        let restart_count = inspection
            .restart_count
            .unwrap_or_default()
            .clamp(0, u32::MAX.into()) as u32;

        let health_host_port = self
            .containers
            .read()
            .await
            .get(container_id)
            .and_then(|metadata| metadata.health_host_port);
        let Some(health_host_port) = health_host_port else {
            return if method.is_none() && path.is_none() {
                Ok(restart_count)
            } else {
                Err(AlienError::new(ErrorData::DockerContainerError {
                    container: container_id.to_string(),
                    operation: "health_check".to_string(),
                    reason: "Declared health-check port is not published on loopback; redeploy the container to apply it".to_string(),
                }))
            };
        };
        probe_http_health(
            container_id,
            health_host_port,
            method.unwrap_or("GET"),
            path.unwrap_or("/health"),
            timeout,
        )
        .await?;
        Ok(restart_count)
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

        // Internal URL uses Docker network DNS.
        let internal_dns = format!("{}.svc", container_id);
        let internal_port = metadata
            .public_endpoint
            .as_ref()
            .map(|endpoint| endpoint.port)
            .or_else(|| metadata.ports.first().copied())
            .unwrap_or(8080);
        let scheme = metadata
            .public_endpoint
            .as_ref()
            .map_or("http", |endpoint| endpoint_scheme(endpoint.protocol));
        let internal_url = format!("{scheme}://{internal_dns}:{internal_port}");

        // Public URL is the localhost-mapped port (if exposed publicly).
        let public_url = metadata
            .host_port
            .map(|port| format!("{scheme}://localhost:{port}"));

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

    fn load_metadata_from_disk(state_dir: &Path) -> Result<Vec<ContainerMetadata>> {
        let containers_dir = state_dir.join("containers");
        if !containers_dir.exists() {
            return Ok(Vec::new());
        }

        let entries = std::fs::read_dir(&containers_dir)
            .into_alien_error()
            .context(ErrorData::LocalDirectoryError {
                path: containers_dir.display().to_string(),
                operation: "read".to_string(),
                reason: "Failed to read containers directory".to_string(),
            })?;
        let mut metadata_list = Vec::new();
        for entry in entries {
            let entry = entry
                .into_alien_error()
                .context(ErrorData::LocalDirectoryError {
                    path: containers_dir.display().to_string(),
                    operation: "iterate".to_string(),
                    reason: "Failed to iterate containers directory".to_string(),
                })?;
            let metadata_file = entry.path().join("metadata.json");
            if !metadata_file.exists() {
                continue;
            }
            match std::fs::read_to_string(&metadata_file) {
                Ok(json) => match serde_json::from_str::<ContainerMetadata>(&json) {
                    Ok(metadata) => metadata_list.push(metadata),
                    Err(error) => warn!(
                        path = %metadata_file.display(),
                        error = %error,
                        "Failed to parse container metadata"
                    ),
                },
                Err(error) => warn!(
                    path = %metadata_file.display(),
                    error = %error,
                    "Failed to read container metadata"
                ),
            }
        }
        Ok(metadata_list)
    }

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

    /// Returns the container metadata currently tracked by the manager.
    ///
    /// Persisted metadata is loaded during construction, so this includes
    /// containers recovered after a manager restart.
    pub async fn load_metadata(&self) -> Result<Vec<ContainerMetadata>> {
        Ok(self.containers.read().await.values().cloned().collect())
    }
}

/// Return the host identity a bind-mounted local workload must share with the
/// operator and native Daemons. Docker accepts numeric `uid:gid` values even
/// when the image has no matching passwd entry.
#[cfg(target_os = "linux")]
fn shared_bind_mount_user(bind_mounts: &[BindMount]) -> Option<String> {
    if !bind_mounts
        .iter()
        .any(|mount| mount.shared_with_host_workloads)
    {
        return None;
    }

    // SAFETY: geteuid/getegid are side-effect-free process identity queries.
    let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
    if uid == 0 {
        return None;
    }
    Some(format!("{uid}:{gid}"))
}

#[cfg(not(target_os = "linux"))]
fn shared_bind_mount_user(_bind_mounts: &[BindMount]) -> Option<String> {
    // Docker Desktop mediates bind mounts through its VM/file-sharing layer;
    // host uid/gid values do not identify the container user there.
    None
}

/// The OCI layout's `index.json`, reduced to what image identification needs.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OciIndex {
    manifests: Vec<OciDescriptor>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OciDescriptor {
    digest: String,
    #[serde(default)]
    annotations: HashMap<String, String>,
}

/// Retain only imports with active owners or waiters, reusing their locks atomically.
fn retain_image_load_locks(
    locks: &mut HashMap<String, Weak<tokio::sync::Mutex<()>>>,
    keys: Vec<String>,
) -> Vec<Arc<tokio::sync::Mutex<()>>> {
    locks.retain(|_, lock| lock.strong_count() > 0);
    keys.into_iter()
        .map(|key| {
            if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
                return lock;
            }
            let lock = Arc::new(tokio::sync::Mutex::new(()));
            locks.insert(key, Arc::downgrade(&lock));
            lock
        })
        .collect()
}

/// Digest of the first image an OCI archive's `index.json` lists: the same
/// image whose config digest `dockdash::Image::from_tarball` reads.
fn oci_archive_identity(tarball_path: &Path) -> std::io::Result<(String, Vec<String>)> {
    let mut archive = tar::Archive::new(std::fs::File::open(tarball_path)?);
    // Seeking skips over layer blobs instead of reading them.
    for entry in archive.entries_with_seek()? {
        let entry = entry?;
        let path = entry.path()?.into_owned();
        if path.strip_prefix(".").unwrap_or(&path) != Path::new("index.json") {
            continue;
        }
        let index: OciIndex = serde_json::from_reader(entry).map_err(std::io::Error::other)?;
        let digest = index
            .manifests
            .first()
            .ok_or_else(|| std::io::Error::other("index.json lists no images"))?
            .digest
            .clone();
        let references = index
            .manifests
            .iter()
            .filter_map(|manifest| {
                manifest
                    .annotations
                    .get("org.opencontainers.image.ref.name")
                    .map(|reference| format!("reference:{reference}"))
            })
            .collect();
        return Ok((digest, references));
    }
    Err(std::io::Error::other("archive has no index.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn image_import_locks_preserve_waiters_and_prune_finished_imports() {
        let mut locks = HashMap::new();
        let mut first = retain_image_load_locks(&mut locks, vec!["shared".to_string()]);
        let guard = first.pop().unwrap().lock_owned().await;
        let mut second = retain_image_load_locks(&mut locks, vec!["shared".to_string()]);
        let waiter = second.pop().unwrap().lock_owned();
        tokio::pin!(waiter);
        tokio::select! {
            biased;
            _ = &mut waiter => panic!("a waiting import must not bypass the active owner"),
            _ = tokio::task::yield_now() => {}
        }
        for index in 0..100 {
            let unrelated =
                retain_image_load_locks(&mut locks, vec![format!("independent-{index}")]);
            assert_eq!(locks.len(), 2, "finished image keys must not accumulate");
            assert!(locks["shared"].upgrade().unwrap().try_lock().is_err());
            drop(unrelated);
        }
        drop(guard);
        let waiting_guard = waiter.await;
        let shared = retain_image_load_locks(&mut locks, vec!["shared".to_string()]);
        assert_eq!(locks.len(), 1);
        assert!(
            shared[0].try_lock().is_err(),
            "the waiter still owns the same lock"
        );
        drop((waiting_guard, shared));
        let final_import = retain_image_load_locks(&mut locks, vec!["last".to_string()]);
        assert_eq!(locks.len(), 1);
        assert!(!locks.contains_key("shared"));
        assert!(final_import[0].try_lock().is_ok());
    }

    fn test_bind_mount(shared_with_host_workloads: bool) -> BindMount {
        BindMount {
            host_path: PathBuf::from("/tmp/alien-test-binding"),
            container_path: "/mnt/test".to_string(),
            resource_id: "test".to_string(),
            shared_with_host_workloads,
        }
    }

    #[test]
    fn tmp_only_bind_mount_preserves_the_image_user() {
        assert_eq!(shared_bind_mount_user(&[test_bind_mount(false)]), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn shared_bind_mount_uses_the_non_root_host_identity() {
        // SAFETY: geteuid/getegid are side-effect-free process identity queries.
        let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
        let expected = (uid != 0).then(|| format!("{uid}:{gid}"));

        assert_eq!(shared_bind_mount_user(&[test_bind_mount(true)]), expected);
    }

    #[test]
    fn rewrites_localhost_for_command_receiver_url() {
        let mut env = HashMap::new();
        env.insert(
            ENV_ALIEN_COMMANDS_URL.to_string(),
            "http://localhost:8080/v1".to_string(),
        );
        // A user var pointing at localhost must be left alone.
        env.insert(
            "USER_API_URL".to_string(),
            "http://localhost:9000".to_string(),
        );
        // A non-localhost receiver URL must be left as-is.
        env.insert(
            "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT".to_string(),
            "https://otel.example.test/v1/logs".to_string(),
        );

        rewrite_dev_server_localhost_urls(&mut env);

        assert_eq!(
            env[ENV_ALIEN_COMMANDS_URL],
            "http://host.docker.internal:8080/v1"
        );
        assert_eq!(env["USER_API_URL"], "http://localhost:9000");
        assert_eq!(
            env["OTEL_EXPORTER_OTLP_LOGS_ENDPOINT"],
            "https://otel.example.test/v1/logs"
        );
    }

    #[test]
    fn configured_command_replaces_the_image_entrypoint() {
        let command = vec!["/agent".to_string(), "--poll".to_string()];

        let process_override = oci_process_override(Some(&command));

        assert_eq!(
            process_override,
            OciProcessOverride {
                entrypoint: Some(command),
                cmd: None,
            }
        );
    }

    #[test]
    fn health_check_reuses_the_public_mapping_for_the_same_port() {
        let endpoint = LocalPublicEndpoint {
            port: 8080,
            protocol: ExposeProtocol::Http,
            names: vec![],
        };
        let (_, bindings) =
            loopback_port_bindings(Some(&endpoint), Some(41000), Some(8080), Some(41000));

        let bindings = bindings.expect("port should be published");
        assert_eq!(bindings.len(), 1);
        assert_eq!(
            bindings["8080/tcp"].as_ref().unwrap()[0]
                .host_port
                .as_deref(),
            Some("41000")
        );
    }

    #[test]
    fn private_health_check_gets_a_loopback_mapping() {
        let (_, bindings) = loopback_port_bindings(None, None, Some(9090), Some(41001));

        let bindings = bindings.expect("health port should be published");
        let binding = &bindings["9090/tcp"].as_ref().unwrap()[0];
        assert_eq!(binding.host_ip.as_deref(), Some("127.0.0.1"));
        assert_eq!(binding.host_port.as_deref(), Some("41001"));
    }

    #[tokio::test]
    async fn http_probe_requires_a_success_response() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            for status in [503, 204] {
                let (mut stream, _) = listener.accept().await.unwrap();
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut request = [0; 1024];
                let size = stream.read(&mut request).await.unwrap();
                assert!(String::from_utf8_lossy(&request[..size]).starts_with("HEAD /ready "));
                stream
                    .write_all(
                        format!("HTTP/1.1 {status} Test\r\nContent-Length: 0\r\n\r\n").as_bytes(),
                    )
                    .await
                    .unwrap();
            }
        });

        assert!(probe_http_health(
            "api",
            port,
            "HEAD",
            "ready",
            std::time::Duration::from_secs(1)
        )
        .await
        .is_err());
        probe_http_health(
            "api",
            port,
            "HEAD",
            "/ready",
            std::time::Duration::from_secs(1),
        )
        .await
        .unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn manager_restart_restores_container_metadata() {
        let state_dir = tempfile::tempdir().unwrap();
        let metadata_dir = state_dir.path().join("containers/api");
        std::fs::create_dir_all(&metadata_dir).unwrap();
        let metadata = ContainerMetadata {
            container_id: "api".to_string(),
            docker_container_id: "docker-api".to_string(),
            image: "example.test/api:latest".to_string(),
            ports: vec![8080, 9090],
            host_port: Some(41000),
            health_host_port: Some(41001),
            public_endpoint: Some(LocalPublicEndpoint {
                port: 8080,
                protocol: ExposeProtocol::Http,
                names: vec!["web".to_string()],
            }),
            stateful: false,
            ordinal: None,
            created_at: chrono::Utc::now(),
        };
        std::fs::write(
            metadata_dir.join("metadata.json"),
            serde_json::to_vec(&metadata).unwrap(),
        )
        .unwrap();

        let manager = LocalContainerManager::new(state_dir.path().to_path_buf()).unwrap();

        assert_eq!(
            manager.get_url("api").await.unwrap().as_deref(),
            Some("http://localhost:41000")
        );
        assert_eq!(
            manager
                .containers
                .read()
                .await
                .get("api")
                .and_then(|metadata| metadata.health_host_port),
            Some(41001)
        );
    }
}
