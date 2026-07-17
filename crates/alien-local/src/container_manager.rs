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
use alien_core::ENV_ALIEN_COMMANDS_URL;
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
    /// Command override for the container image.
    pub command: Option<Vec<String>>,
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
    /// `create` from, re-tagging by image ID when the containerd image store
    /// registered only the tar's literal annotation name.
    async fn load_oci_tarball_into_docker(
        &self,
        tarball_path: &Path,
        container_id: &str,
    ) -> Result<String> {
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

        // With Docker's containerd image store, `docker load` registers the
        // image under the tar's literal `io.containerd.image.name` annotation
        // (e.g. `worker:tag`), while every docker CLI/API lookup normalizes
        // the reference to `docker.io/library/worker:tag` — a name the load
        // did NOT register, so `create` fails with "No such image" even
        // though the content is present. Re-tagging by image ID registers
        // the normalized reference. Uses the same bollard client `create`
        // will use (a CLI `docker tag` could target a different daemon via
        // the active docker context). On the classic image store the initial
        // inspect succeeds and nothing else runs.
        if self.docker.inspect_image(&loaded_image).await.is_err() {
            let images = self
                .docker
                .list_images(None::<bollard::image::ListImagesOptions<String>>)
                .await
                .into_alien_error()
                .context(ErrorData::DockerContainerError {
                    container: container_id.to_string(),
                    operation: "list_images".to_string(),
                    reason: "Failed to list images to locate the loaded OCI image".to_string(),
                })?;
            // Compare with the `docker.io/library/` default-registry prefix
            // stripped from both sides: depending on the image store, the
            // daemon may report the tag in literal or normalized form.
            let normalize = |t: &str| {
                t.strip_prefix("docker.io/library/")
                    .unwrap_or(t)
                    .to_string()
            };
            let wanted = normalize(&loaded_image);
            let image_id = images
                .iter()
                .find(|img| img.repo_tags.iter().any(|t| normalize(t) == wanted))
                .map(|img| img.id.clone())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::DockerContainerError {
                        container: container_id.to_string(),
                        operation: "resolve_loaded_image".to_string(),
                        reason: format!(
                            "docker load reported image '{}' but the daemon can neither \
                             inspect it nor list it — the load did not register usable content",
                            loaded_image
                        ),
                    })
                })?;
            let (repo, tag) = loaded_image.rsplit_once(':').ok_or_else(|| {
                AlienError::new(ErrorData::DockerContainerError {
                    container: container_id.to_string(),
                    operation: "resolve_loaded_image".to_string(),
                    reason: format!("Loaded image reference '{}' has no tag", loaded_image),
                })
            })?;
            self.docker
                .tag_image(
                    &image_id,
                    Some(bollard::image::TagImageOptions { repo, tag }),
                )
                .await
                .into_alien_error()
                .context(ErrorData::DockerContainerError {
                    container: container_id.to_string(),
                    operation: "tag_image".to_string(),
                    reason: format!(
                        "Failed to tag loaded image {} as {}",
                        image_id, loaded_image
                    ),
                })?;
        }

        Ok(loaded_image)
    }

    /// Make a registry image available to the daemon and return a reference
    /// `create` can use. Three attempts, cheapest first:
    ///
    /// 1. Daemon-side anonymous pull — public images (`postgres:16-alpine`).
    /// 2. Daemon-side pull with `deployment:<token>` basic auth (the manager
    ///    registry proxy's pull credential) — proxies the daemon can reach
    ///    over HTTPS, e.g. the E2E harness's public manager URL.
    /// 3. Host-side pull via dockdash with the same credential, then
    ///    `docker load` — the dev server's proxy lives on the HOST's
    ///    localhost, which the daemon cannot reach (and would refuse as a
    ///    plain-HTTP registry anyway). The operator process CAN reach it,
    ///    exactly like the local worker manager's image pulls.
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

        // 3. Host-side pull + docker load.
        info!(
            image = %image,
            container_id = %container_id,
            "Daemon-side pulls failed; pulling on the host and loading into Docker"
        );
        let protocol = if image.starts_with("127.0.0.1") || image.starts_with("localhost") {
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
            .auth(dockdash::RegistryAuth::Basic(
                "deployment".to_string(),
                token.to_string(),
            ))
            .build()
            .await
            .into_alien_error()
            .context(ErrorData::DockerContainerError {
                container: container_id.to_string(),
                operation: "pull_image".to_string(),
                reason: format!(
                    "Pull of '{}' failed anonymously, with deployment-token auth via the \
                     daemon, and via the host-side registry client",
                    image
                ),
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
        let image = self
            .resolve_image(&config.image, container_id, config.proxy_token.as_deref())
            .await?;

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
        rewrite_dev_server_localhost_urls(&mut env_vars);

        // Inject the current container binding so the container can discover its own URLs.
        {
            use alien_core::{
                bindings::{serialize_binding_as_env_var, BindingValue, ContainerBinding},
                ENV_ALIEN_CURRENT_CONTAINER_BINDING_NAME,
            };

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
                // Restart exited containers like every managed platform does.
                // Without this a container that races its peers at startup —
                // e.g. nginx resolving an upstream before that service joined
                // the network — stays Exited forever, while in production it
                // would self-heal. ALWAYS (not ON_FAILURE) matches the
                // Kubernetes Deployment default and also covers entrypoints
                // that exit 0 on failure; Docker applies exponential backoff
                // between restarts, and a manual stop/rm still sticks.
                restart_policy: Some(bollard::models::RestartPolicy {
                    name: Some(bollard::models::RestartPolicyNameEnum::ALWAYS),
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
