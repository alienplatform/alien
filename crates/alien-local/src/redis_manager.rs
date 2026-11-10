//! Local Redis manager.
//!
//! Runs the official Valkey image in Docker, through the same Docker connection the
//! container manager uses. Each server is one container named from the state directory
//! and the resource id. State for each server lives under `{state_dir}/redis/{id}/`
//! (connection metadata, 0600); data lives in a Docker volume only while persistence is
//! on. Docker's restart policy recovers crashes, and a `stop` on shutdown keeps the
//! container stopped until the next `alien dev` start recovers it.

use crate::{
    docker_connection::connect_docker,
    docker_network::bindable_docker_bridge_gateway,
    error::{ErrorData, Result},
};
use alien_core::bindings::RedisBinding;
use alien_error::{AlienError, Context, IntoAlienError};
use bollard::models::{
    ContainerCreateBody, ContainerStateStatusEnum, HostConfig, PortBinding, RestartPolicy,
    RestartPolicyNameEnum, VolumeCreateRequest,
};
use bollard::query_parameters::{
    CreateContainerOptions, CreateImageOptionsBuilder, RemoveContainerOptions, RemoveVolumeOptions,
    StopContainerOptionsBuilder, UploadToContainerOptionsBuilder,
};
use bollard::Docker;
use futures_util::TryStreamExt;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{broadcast, Mutex};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

/// The Valkey release line Local runs. The clouds run Valkey 9.0 too.
pub const VALKEY_VERSION: &str = "9.0";

/// The official image, pinned by digest so every machine runs the same bytes.
const VALKEY_IMAGE: &str =
    "valkey/valkey:9.0@sha256:7e1deda01b64ece3167d370ab5be3452d29734feec22cbcbc69b3a64f6859d6d";
const CONTAINER_PORT: &str = "6379/tcp";
/// The image's `valkey` user. The config file must be readable by it.
const VALKEY_UID: u64 = 999;
const CONFIG_PATH_IN_CONTAINER: &str = "/etc/alien-valkey.conf";
const READY_TIMEOUT: Duration = Duration::from_secs(30);

/// Settings a server is started with. A change recreates the container; data is kept by
/// the append-only file in the volume when persistence is on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedisSpec {
    pub persistence: bool,
    pub eviction_policy: String,
    /// Maximum memory in bytes; `None` leaves the server unlimited.
    pub max_memory_bytes: Option<u64>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RedisMetadata {
    resource_id: String,
    port: u16,
    password: String,
    spec: RedisSpec,
}

// Hand-written so the persisted password can never reach a `{:?}` log.
impl std::fmt::Debug for RedisMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisMetadata")
            .field("resource_id", &self.resource_id)
            .field("port", &self.port)
            .field("password", &"<redacted>")
            .field("spec", &self.spec)
            .finish()
    }
}

/// Manages local Valkey containers, keyed by resource id.
#[derive(Clone)]
pub struct LocalRedisManager {
    state_dir: PathBuf,
    /// Serializes start/stop/delete so two callers never race on one container name.
    lock: Arc<Mutex<()>>,
    ready_timeout: Duration,
}

impl std::fmt::Debug for LocalRedisManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalRedisManager")
            .field("state_dir", &self.state_dir)
            .finish()
    }
}

impl LocalRedisManager {
    pub fn new(state_dir: PathBuf) -> Self {
        Self {
            state_dir,
            lock: Arc::new(Mutex::new(())),
            ready_timeout: READY_TIMEOUT,
        }
    }

    /// Shortens the wait for a started server to answer (tests exercise the slow-start path).
    #[doc(hidden)]
    pub fn with_ready_timeout(mut self, timeout: Duration) -> Self {
        self.ready_timeout = timeout;
        self
    }

    /// Creates a manager and spawns a task that starts the servers recorded on disk and
    /// stops them on shutdown. Docker's restart policy handles crashes in between.
    pub fn new_with_shutdown(
        state_dir: PathBuf,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> (Self, JoinHandle<()>) {
        let manager = Self::new(state_dir);
        let task = manager.clone();
        let handle = tokio::spawn(async move {
            task.recover_all().await;
            let _ = shutdown_rx.recv().await;
            task.stop_all().await;
        });
        (manager, handle)
    }

    fn resource_dir(&self, id: &str) -> PathBuf {
        self.state_dir.join("redis").join(id)
    }

    fn metadata_path(&self, id: &str) -> PathBuf {
        self.resource_dir(id).join("metadata.json")
    }

    /// Container and volume name. The state directory is part of it so two Alien state
    /// directories sharing one Docker engine never touch each other's servers.
    fn docker_name(&self, id: &str) -> String {
        let scope = hex::encode(Sha256::digest(self.state_dir.to_string_lossy().as_bytes()));
        format!("alien-redis-{}-{id}", &scope[..8])
    }

    /// Starts the server for `id`, or applies a changed spec. Idempotent: the port and
    /// password are generated once and reused, so a restart keeps the same connection
    /// details.
    pub async fn start_redis(&self, id: &str, spec: RedisSpec) -> Result<()> {
        let _guard = self.lock.lock().await;
        self.start_redis_locked(id, Some(spec)).await
    }

    /// Starts a server from the settings already on disk. The metadata is read under the lock,
    /// so recovery can never write back settings a concurrent update has just replaced.
    async fn recover_redis(&self, id: &str) -> Result<()> {
        let _guard = self.lock.lock().await;
        self.start_redis_locked(id, None).await
    }

    async fn start_redis_locked(&self, id: &str, spec: Option<RedisSpec>) -> Result<()> {
        let docker = connect_redis_docker().await?;
        let metadata = match self.try_read_metadata(id)? {
            Some(mut metadata) => {
                if let Some(spec) = spec.filter(|spec| *spec != metadata.spec) {
                    metadata.spec = spec;
                    self.write_metadata(&metadata).await?;
                }
                metadata
            }
            None => {
                let Some(spec) = spec else {
                    return Ok(());
                };
                let metadata = RedisMetadata {
                    resource_id: id.to_string(),
                    port: allocate_port(id)?,
                    password: generate_password(),
                    spec,
                };
                self.write_metadata(&metadata).await?;
                metadata
            }
        };
        self.ensure_running(&docker, &metadata).await?;
        info!(redis_id = %id, "Local Redis started");
        Ok(())
    }

    /// Stops the server but keeps its data and metadata.
    pub async fn stop_redis(&self, id: &str) -> Result<()> {
        let _guard = self.lock.lock().await;
        let docker = connect_redis_docker().await?;
        self.stop_container(&docker, id).await
    }

    /// Removes the container, its data volume and its state directory.
    pub async fn delete_redis(&self, id: &str) -> Result<()> {
        let _guard = self.lock.lock().await;
        let docker = connect_redis_docker().await?;
        let name = self.docker_name(id);
        // Never remove a same-name container that is not ours.
        if let Some(existing) = inspect_optional(&docker, &name).await? {
            self.require_ours(&name, id, &existing)?;
            remove_container(&docker, &name).await?;
        }
        remove_volume(&docker, &name).await?;
        let dir = self.resource_dir(id);
        if dir.exists() {
            tokio::fs::remove_dir_all(&dir)
                .await
                .into_alien_error()
                .context(ErrorData::LocalDirectoryError {
                    path: dir.display().to_string(),
                    operation: "delete".to_string(),
                    reason: "Failed to delete Redis state directory".to_string(),
                })?;
        }
        debug!(redis_id = %id, "Local Redis deleted");
        Ok(())
    }

    /// Verifies the container is running. Never connects to the server.
    pub async fn check_health(&self, id: &str) -> Result<()> {
        let docker = connect_redis_docker().await?;
        let name = self.docker_name(id);
        let existing = inspect_optional(&docker, &name).await?.ok_or_else(|| {
            AlienError::new(ErrorData::ServiceResourceNotFound {
                resource_id: id.to_string(),
                resource_type: "redis".to_string(),
            })
        })?;
        self.require_ours(&name, id, &existing)?;
        let state = existing.state.unwrap_or_default();
        if state.running == Some(true) {
            Ok(())
        } else {
            Err(AlienError::new(container_error(
                &name,
                "health_check",
                &format!(
                    "Redis is not running (status {})",
                    state
                        .status
                        .map(|status| status.to_string())
                        .unwrap_or_else(|| "unknown".to_string())
                ),
            )))
        }
    }

    /// The binding for a server, read from metadata so it always has the current port
    /// and password.
    pub fn get_binding(&self, id: &str) -> Result<RedisBinding> {
        let metadata = self.try_read_metadata(id)?.ok_or_else(|| {
            AlienError::new(ErrorData::ServiceResourceNotFound {
                resource_id: id.to_string(),
                resource_type: "redis".to_string(),
            })
        })?;
        Ok(RedisBinding::local(
            "127.0.0.1",
            metadata.port,
            "default",
            metadata.password,
        ))
    }

    /// Like `get_binding`, but `Ok(None)` when no metadata exists for `id`.
    pub fn try_get_binding(&self, id: &str) -> Result<Option<RedisBinding>> {
        Ok(self.try_read_metadata(id)?.map(|metadata| {
            RedisBinding::local("127.0.0.1", metadata.port, "default", metadata.password)
        }))
    }

    // ───────────────────────── internals ─────────────────────────

    fn require_ours(
        &self,
        name: &str,
        id: &str,
        existing: &bollard::models::ContainerInspectResponse,
    ) -> Result<()> {
        let labels = existing
            .config
            .as_ref()
            .and_then(|config| config.labels.as_ref());
        if labels
            .and_then(|labels| labels.get(LABEL_OWNER))
            .map(String::as_str)
            == Some(self.owner_label(id).as_str())
        {
            return Ok(());
        }
        Err(AlienError::new(container_error(
            name,
            "adopt",
            "A Docker container with this name already exists and was not created by Alien for this Redis; remove or rename it",
        )))
    }

    fn owner_label(&self, id: &str) -> String {
        self.docker_name(id)
    }

    /// Brings the container to the state `metadata` describes: reuses a matching one,
    /// starts a stopped one, recreates one whose settings differ.
    async fn ensure_running(&self, docker: &Docker, metadata: &RedisMetadata) -> Result<()> {
        let id = &metadata.resource_id;
        let name = self.docker_name(id);
        let gateway = bindable_docker_bridge_gateway().await?;
        let desired = config_hash(metadata, gateway.map(|g| g.to_string()));

        if let Some(existing) = inspect_optional(docker, &name).await? {
            self.require_ours(&name, id, &existing)?;
            let current = existing
                .config
                .as_ref()
                .and_then(|config| config.labels.as_ref())
                .and_then(|labels| labels.get(LABEL_CONFIG))
                .cloned();
            // A container that was created but never started was interrupted before its config
            // was uploaded, although it already carries the final label: replace it.
            let never_started = existing.state.as_ref().and_then(|s| s.status)
                == Some(ContainerStateStatusEnum::CREATED);
            if current.as_deref() == Some(desired.as_str()) && !never_started {
                if existing.state.as_ref().and_then(|s| s.running) != Some(true) {
                    docker
                        .start_container(&name, None)
                        .await
                        .into_alien_error()
                        .context(container_error(
                            &name,
                            "start",
                            "Failed to start the container",
                        ))?;
                }
                return wait_until_ready(docker, &name, metadata, self.ready_timeout).await;
            }
            // SIGTERM first so Valkey flushes its append-only file; a forced removal could drop
            // the last second of acknowledged writes.
            self.stop_container(docker, id).await?;
            remove_container(docker, &name).await?;
        }
        // Data is only kept while persistence is on; turning it off drops the volume so a
        // later "on" never loads a stale append-only file.
        if !metadata.spec.persistence {
            remove_volume(docker, &name).await?;
        }

        ensure_image(docker, &name).await?;
        if metadata.spec.persistence {
            docker
                .create_volume(VolumeCreateRequest {
                    name: Some(name.clone()),
                    labels: Some(HashMap::from([(
                        LABEL_OWNER.to_string(),
                        self.owner_label(id),
                    )])),
                    ..Default::default()
                })
                .await
                .into_alien_error()
                .context(ErrorData::DockerVolumeError {
                    volume: name.clone(),
                    operation: "create".to_string(),
                    reason: "Failed to create the Redis data volume".to_string(),
                })?;
        }

        // Loopback plus the docker bridge gateway when present, so a same-stack local
        // container can reach it through `host.docker.internal`; never 0.0.0.0.
        let mut host_ips = vec!["127.0.0.1".to_string()];
        if let Some(gateway) = gateway {
            host_ips.push(gateway.to_string());
        }
        let bindings = host_ips
            .into_iter()
            .map(|ip| PortBinding {
                host_ip: Some(ip),
                host_port: Some(metadata.port.to_string()),
            })
            .collect();
        let body = ContainerCreateBody {
            image: Some(VALKEY_IMAGE.to_string()),
            cmd: Some(vec![CONFIG_PATH_IN_CONTAINER.to_string()]),
            labels: Some(HashMap::from([
                (LABEL_OWNER.to_string(), self.owner_label(id)),
                (LABEL_CONFIG.to_string(), desired),
            ])),
            exposed_ports: Some(vec![CONTAINER_PORT.to_string()]),
            host_config: Some(HostConfig {
                port_bindings: Some(HashMap::from([(
                    CONTAINER_PORT.to_string(),
                    Some(bindings),
                )])),
                binds: metadata
                    .spec
                    .persistence
                    .then(|| vec![format!("{name}:/data")]),
                // Docker recovers crashes and keeps an explicitly stopped container
                // stopped across daemon restarts.
                restart_policy: Some(RestartPolicy {
                    name: Some(RestartPolicyNameEnum::UNLESS_STOPPED),
                    maximum_retry_count: None,
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        docker
            .create_container(
                Some(CreateContainerOptions {
                    name: Some(name.clone()),
                    ..Default::default()
                }),
                body,
            )
            .await
            .into_alien_error()
            .context(container_error(
                &name,
                "create",
                "Failed to create the container",
            ))?;

        // The container carries its final config-hash label from creation, so a create that stops
        // before the config is uploaded and the server started must not be left behind: the next
        // attempt would take it for a finished one.
        let started = match self.configure_and_start(docker, &name, metadata).await {
            Ok(()) => wait_until_ready(docker, &name, metadata, self.ready_timeout).await,
            Err(error) => Err(error),
        };
        if let Err(error) = started {
            // A server that is still running only needs more time (a large append-only file can
            // take longer than the readiness wait to load); the next start finds the container
            // with its final config and waits again. Anything that stopped or never started is
            // removed so it is not mistaken for a finished one.
            let still_running = inspect_optional(docker, &name)
                .await
                .ok()
                .flatten()
                .and_then(|response| response.state)
                .is_some_and(|state| state.running == Some(true) && state.restarting != Some(true));
            if !still_running {
                if let Err(cleanup) = remove_container(docker, &name).await {
                    warn!(redis_id = %id, ?cleanup, "Failed to remove the unfinished Redis container");
                }
            }
            return Err(error);
        }
        Ok(())
    }

    /// Uploads the config (it holds the password, so it is not an argument or environment
    /// value where `docker inspect` and `ps` would show it) and starts the created container.
    async fn configure_and_start(
        &self,
        docker: &Docker,
        name: &str,
        metadata: &RedisMetadata,
    ) -> Result<()> {
        let archive = config_archive(&render_config(metadata))?;
        docker
            .upload_to_container(
                name,
                Some(
                    UploadToContainerOptionsBuilder::default()
                        .path("/etc")
                        .build(),
                ),
                bollard::body_full(archive.into()),
            )
            .await
            .into_alien_error()
            .context(container_error(
                name,
                "configure",
                "Failed to write the Redis config",
            ))?;

        docker
            .start_container(name, None)
            .await
            .into_alien_error()
            .context(container_error(
                name,
                "start",
                &format!(
                    "Failed to start the container. If another program took host port {}, stop it and retry",
                    metadata.port
                ),
            ))?;
        Ok(())
    }

    async fn stop_container(&self, docker: &Docker, id: &str) -> Result<()> {
        let name = self.docker_name(id);
        let Some(existing) = inspect_optional(docker, &name).await? else {
            return Ok(());
        };
        self.require_ours(&name, id, &existing)?;
        match docker
            .stop_container(
                &name,
                Some(StopContainerOptionsBuilder::default().t(10).build()),
            )
            .await
        {
            Ok(()) => Ok(()),
            // 304: already stopped. 404: removed since the inspect.
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 304 | 404,
                ..
            }) => Ok(()),
            Err(error) => Err(error).into_alien_error().context(container_error(
                &name,
                "stop",
                "Failed to stop the container",
            )),
        }
    }

    fn try_read_metadata(&self, id: &str) -> Result<Option<RedisMetadata>> {
        let path = self.metadata_path(id);
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error)
                    .into_alien_error()
                    .context(ErrorData::LocalDirectoryError {
                        path: path.display().to_string(),
                        operation: "read".to_string(),
                        reason: "Failed to read Redis metadata".to_string(),
                    })
            }
        };
        serde_json::from_str(&content)
            .into_alien_error()
            .context(ErrorData::LocalDirectoryError {
                path: path.display().to_string(),
                operation: "read".to_string(),
                reason: "Failed to parse Redis metadata".to_string(),
            })
            .map(Some)
    }

    /// Writes metadata 0600: it holds the generated password. The temp-file-and-rename
    /// keeps the only copy of the credentials intact if the write is interrupted.
    async fn write_metadata(&self, metadata: &RedisMetadata) -> Result<()> {
        let dir = self.resource_dir(&metadata.resource_id);
        tokio::fs::create_dir_all(&dir)
            .await
            .into_alien_error()
            .context(ErrorData::LocalDirectoryError {
                path: dir.display().to_string(),
                operation: "create".to_string(),
                reason: "Failed to create Redis state directory".to_string(),
            })?;
        let path = self.metadata_path(&metadata.resource_id);
        let data = serde_json::to_vec_pretty(metadata)
            .into_alien_error()
            .context(ErrorData::LocalDirectoryError {
                path: path.display().to_string(),
                operation: "write".to_string(),
                reason: "Failed to serialize Redis metadata".to_string(),
            })?;
        let temp_path = path.with_extension("json.tmp");
        let target = path.clone();
        tokio::task::spawn_blocking(move || {
            alien_core::file_utils::write_secret_file(&temp_path, &data)?;
            std::fs::rename(&temp_path, &target)
        })
        .await
        .into_alien_error()
        .context(ErrorData::LocalDirectoryError {
            path: path.display().to_string(),
            operation: "write".to_string(),
            reason: "Metadata write task panicked".to_string(),
        })?
        .into_alien_error()
        .context(ErrorData::LocalDirectoryError {
            path: path.display().to_string(),
            operation: "write".to_string(),
            reason: "Failed to write Redis metadata".to_string(),
        })
    }

    /// Starts every server recorded on disk. Failures are logged: one broken server must
    /// not keep the others down, and Docker being off is reported when a controller asks.
    async fn recover_all(&self) {
        let root = self.state_dir.join("redis");
        let Ok(mut entries) = tokio::fs::read_dir(&root).await else {
            return;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            if !entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let id = entry.file_name().to_string_lossy().to_string();
            if let Err(error) = self.recover_redis(&id).await {
                warn!(redis_id = %id, ?error, "Failed to recover local Redis");
            }
        }
    }

    async fn stop_all(&self) {
        let root = self.state_dir.join("redis");
        let Ok(mut entries) = tokio::fs::read_dir(&root).await else {
            return;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let id = entry.file_name().to_string_lossy().to_string();
            if let Err(error) = self.stop_redis(&id).await {
                warn!(redis_id = %id, ?error, "Failed to stop local Redis on shutdown");
            }
        }
    }
}

const LABEL_OWNER: &str = "alien.redis.owner";
const LABEL_CONFIG: &str = "alien.redis.config-hash";

fn container_error(container: &str, operation: &str, reason: &str) -> ErrorData {
    ErrorData::DockerContainerError {
        container: container.to_string(),
        operation: operation.to_string(),
        reason: reason.to_string(),
    }
}

/// Connects and pings, so a missing Docker engine fails here with a message that says what
/// to do, not later as an opaque socket error.
async fn connect_redis_docker() -> Result<Docker> {
    let docker = connect_docker().context(needs_docker())?;
    docker
        .ping()
        .await
        .into_alien_error()
        .context(needs_docker())?;
    Ok(docker)
}

fn needs_docker() -> ErrorData {
    ErrorData::DockerConnectionFailed {
        reason: "Redis on Local runs in Docker, and no Docker engine is reachable. Start Docker and retry".to_string(),
    }
}

async fn inspect_optional(
    docker: &Docker,
    name: &str,
) -> Result<Option<bollard::models::ContainerInspectResponse>> {
    match docker.inspect_container(name, None).await {
        Ok(response) => Ok(Some(response)),
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(None),
        Err(error) => Err(error).into_alien_error().context(container_error(
            name,
            "inspect",
            "Failed to inspect the container",
        )),
    }
}

async fn remove_container(docker: &Docker, name: &str) -> Result<()> {
    match docker
        .remove_container(
            name,
            Some(RemoveContainerOptions {
                force: true,
                v: true,
                ..Default::default()
            }),
        )
        .await
    {
        Ok(()) => Ok(()),
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(()),
        Err(error) => Err(error).into_alien_error().context(container_error(
            name,
            "remove",
            "Failed to remove the container",
        )),
    }
}

async fn remove_volume(docker: &Docker, name: &str) -> Result<()> {
    match docker
        .remove_volume(name, Some(RemoveVolumeOptions { force: true }))
        .await
    {
        Ok(()) => Ok(()),
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(()),
        Err(error) => Err(error)
            .into_alien_error()
            .context(ErrorData::DockerVolumeError {
                volume: name.to_string(),
                operation: "remove".to_string(),
                reason: "Failed to remove the Redis data volume".to_string(),
            }),
    }
}

/// Pulls the pinned image when the engine does not have it.
async fn ensure_image(docker: &Docker, name: &str) -> Result<()> {
    match docker.inspect_image(VALKEY_IMAGE).await {
        Ok(_) => return Ok(()),
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => {}
        Err(error) => {
            return Err(error).into_alien_error().context(container_error(
                name,
                "inspect_image",
                "Failed to inspect the Valkey image",
            ))
        }
    }
    info!(image = VALKEY_IMAGE, "Pulling the Valkey image");
    docker
        .create_image(
            Some(
                CreateImageOptionsBuilder::default()
                    .from_image(VALKEY_IMAGE)
                    .build(),
            ),
            None,
            None,
        )
        .try_collect::<Vec<_>>()
        .await
        .into_alien_error()
        .context(container_error(
            name,
            "pull_image",
            "Failed to pull the Valkey image",
        ))?;
    Ok(())
}

/// Hash of everything that decides how the container is built. A different hash means the
/// container is recreated.
fn config_hash(metadata: &RedisMetadata, gateway: Option<String>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(VALKEY_IMAGE.as_bytes());
    hasher.update(render_config(metadata).as_bytes());
    hasher.update(metadata.port.to_be_bytes());
    hasher.update(gateway.unwrap_or_default().as_bytes());
    hasher.update([metadata.spec.persistence as u8]);
    hex::encode(hasher.finalize())
}

/// The server config. `appendfsync everysec` bounds loss to about a second of writes.
fn render_config(metadata: &RedisMetadata) -> String {
    let spec = &metadata.spec;
    let mut config = format!(
        "bind 0.0.0.0\nport 6379\nprotected-mode yes\nrequirepass {}\ndir /data\n\
         maxmemory-policy {}\nsave \"\"\nappendonly {}\nappendfsync everysec\ndaemonize no\n",
        metadata.password,
        spec.eviction_policy,
        if spec.persistence { "yes" } else { "no" },
    );
    if let Some(bytes) = spec.max_memory_bytes {
        config.push_str(&format!("maxmemory {bytes}\n"));
    }
    config
}

/// A one-file tar for `/etc/alien-valkey.conf`, mode 0400, owned by the image's server user.
fn config_archive(config: &str) -> Result<Vec<u8>> {
    let mut header = tar::Header::new_gnu();
    header.set_size(config.len() as u64);
    header.set_mode(0o400);
    header.set_uid(VALKEY_UID);
    header.set_gid(VALKEY_UID);
    header.set_cksum();
    let mut builder = tar::Builder::new(Vec::new());
    builder
        .append_data(&mut header, "alien-valkey.conf", config.as_bytes())
        .into_alien_error()
        .context(container_error(
            "redis",
            "configure",
            "Failed to build the config archive",
        ))?;
    builder
        .into_inner()
        .into_alien_error()
        .context(container_error(
            "redis",
            "configure",
            "Failed to build the config archive",
        ))
}

/// Waits until the server answers an authenticated PING on the published port, failing early
/// when the container exits.
async fn wait_until_ready(
    docker: &Docker,
    name: &str,
    metadata: &RedisMetadata,
    timeout: Duration,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        // The restart policy keeps a crashing server "running" between attempts, so a restart in
        // progress counts as a failed start.
        let running = inspect_optional(docker, name)
            .await?
            .and_then(|response| response.state)
            .is_some_and(|state| state.running == Some(true) && state.restarting != Some(true));
        if !running {
            return Err(AlienError::new(container_error(
                name,
                "start",
                "The Valkey container exited during startup; check `docker logs`",
            )));
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(AlienError::new(container_error(
                name,
                "start",
                &format!(
                    "Valkey did not answer on port {} within {timeout:?}",
                    metadata.port
                ),
            )));
        }
        if ping(metadata.port, &metadata.password).await {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// One authenticated PING over a fresh connection.
async fn ping(port: u16, password: &str) -> bool {
    let attempt = async {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .ok()?;
        let request = format!(
            "*3\r\n$4\r\nAUTH\r\n$7\r\ndefault\r\n${}\r\n{password}\r\n*1\r\n$4\r\nPING\r\n",
            password.len()
        );
        stream.write_all(request.as_bytes()).await.ok()?;
        let mut reply = Vec::new();
        let mut buf = [0u8; 64];
        while !String::from_utf8_lossy(&reply).contains("+PONG") {
            let n = stream.read(&mut buf).await.ok()?;
            if n == 0 {
                return None;
            }
            reply.extend_from_slice(&buf[..n]);
        }
        Some(())
    };
    matches!(
        tokio::time::timeout(Duration::from_secs(2), attempt).await,
        Ok(Some(()))
    )
}

/// Generates a 32-character alphanumeric password (URL-safe).
fn generate_password() -> String {
    rand::rng()
        .sample_iter(rand::distr::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

/// Claims a free localhost port by binding and releasing it. The port is persisted, so
/// a restart reuses it.
fn allocate_port(id: &str) -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .into_alien_error()
        .context(ErrorData::NoFreePorts {
            service_name: format!("redis/{id}"),
        })?;
    Ok(listener
        .local_addr()
        .into_alien_error()
        .context(ErrorData::NoFreePorts {
            service_name: format!("redis/{id}"),
        })?
        .port())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(persistence: bool) -> RedisMetadata {
        RedisMetadata {
            resource_id: "cache".to_string(),
            port: 6400,
            password: "pw".to_string(),
            spec: RedisSpec {
                persistence,
                eviction_policy: "volatile-lru".to_string(),
                max_memory_bytes: Some(1024),
            },
        }
    }

    #[test]
    fn a_changed_setting_changes_the_container_hash() {
        let base = config_hash(&metadata(true), None);
        assert_eq!(base, config_hash(&metadata(true), None));
        assert_ne!(base, config_hash(&metadata(false), None));
        let mut other = metadata(true);
        other.spec.eviction_policy = "noeviction".to_string();
        assert_ne!(base, config_hash(&other, None));
        assert_ne!(
            base,
            config_hash(&metadata(true), Some("172.17.0.1".to_string()))
        );
    }

    #[test]
    fn debug_output_hides_the_password() {
        assert!(!format!("{:?}", metadata(true)).contains("pw\""));
    }
}
