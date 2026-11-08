//! Local Redis manager.
//!
//! Runs Valkey as a native `valkey-server` process, like the other local managers run
//! native processes. State for each server lives under `{state_dir}/redis/{id}/`; the
//! server binary is downloaded once into `{state_dir}/redis/_dist` and shared.
//!
//! The binary comes from Alien's release host (`releases.alien.dev/valkey`), pinned by
//! SHA-256. `ALIEN_VALKEY_RELEASES_URL` points at another host or a `file://` directory
//! (for testing a locally built binary); the checksum then comes from a `.sha256` file
//! next to the archive.

use crate::{
    docker_network::bindable_docker_bridge_gateway,
    error::{ErrorData, Result},
};
use alien_core::bindings::RedisBinding;
use alien_error::{AlienError, Context, IntoAlienError};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, Mutex};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

/// Valkey major.minor.patch that Local runs. The clouds run Valkey 9.0 too.
pub const VALKEY_VERSION: &str = "9.0.0";

const RELEASES_URL_ENV: &str = "ALIEN_VALKEY_RELEASES_URL";
const RELEASES_URL_DEFAULT: &str = "https://releases.alien.dev/valkey";
const MONITOR_INTERVAL: Duration = Duration::from_secs(30);
const READY_TIMEOUT: Duration = Duration::from_secs(20);

/// SHA-256 of `valkey-server.tar.gz` for each release target, filled in when a release is
/// published. An unpinned target fails with a clear error instead of running an
/// unverified binary.
const PINNED_SHA256: &[(&str, &str)] = &[];

/// Settings a server is started with. A change restarts the process; data is kept by
/// the append-only file when persistence is on.
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

/// Manages local Valkey servers as child processes, keyed by resource id.
#[derive(Clone)]
pub struct LocalRedisManager {
    state_dir: PathBuf,
    running: Arc<Mutex<HashMap<String, Child>>>,
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
            running: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Creates a manager and spawns the monitor loop: it restarts servers recorded on
    /// disk at startup and any that exit while running.
    pub fn new_with_shutdown(
        state_dir: PathBuf,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> (Self, JoinHandle<()>) {
        let manager = Self::new(state_dir);
        let monitor = manager.clone();
        let handle = tokio::spawn(async move {
            monitor.recover_all().await;
            let mut interval = tokio::time::interval(MONITOR_INTERVAL);
            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        monitor.stop_all().await;
                        break;
                    }
                    _ = interval.tick() => monitor.restart_exited().await,
                }
            }
        });
        (manager, handle)
    }

    fn resource_dir(&self, id: &str) -> PathBuf {
        self.state_dir.join("redis").join(id)
    }

    fn metadata_path(&self, id: &str) -> PathBuf {
        self.resource_dir(id).join("metadata.json")
    }

    /// Starts the server for `id`, or applies a changed spec. Idempotent: the port and
    /// password are generated once and reused, so a restart keeps the same connection
    /// details.
    pub async fn start_redis(&self, id: &str, spec: RedisSpec) -> Result<()> {
        let mut running = self.running.lock().await;
        let metadata = match self.try_read_metadata(id)? {
            Some(mut metadata) => {
                if metadata.spec != spec {
                    metadata.spec = spec;
                    self.write_metadata(&metadata).await?;
                    // Settings are read at process start: restart to apply them.
                    if let Some(mut child) = running.remove(id) {
                        stop_child(id, &mut child).await?;
                    }
                }
                metadata
            }
            None => {
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
        if let Some(child) = running.get_mut(id) {
            if child
                .try_wait()
                .into_alien_error()
                .context(process_error(
                    id,
                    "query",
                    "Failed to check the server process",
                ))?
                .is_none()
            {
                return Ok(());
            }
            running.remove(id);
        }
        let child = self.boot(&metadata).await?;
        running.insert(id.to_string(), child);
        info!(redis_id = %id, "Local Redis started");
        Ok(())
    }

    /// Stops the server but keeps its data and metadata.
    pub async fn stop_redis(&self, id: &str) -> Result<()> {
        let mut running = self.running.lock().await;
        if let Some(mut child) = running.remove(id) {
            stop_child(id, &mut child).await?;
        }
        Ok(())
    }

    /// Stops the server and removes its data directory.
    pub async fn delete_redis(&self, id: &str) -> Result<()> {
        self.stop_redis(id).await?;
        let dir = self.resource_dir(id);
        if dir.exists() {
            tokio::fs::remove_dir_all(&dir)
                .await
                .into_alien_error()
                .context(ErrorData::LocalDirectoryError {
                    path: dir.display().to_string(),
                    operation: "delete".to_string(),
                    reason: "Failed to delete Redis data directory".to_string(),
                })?;
        }
        debug!(redis_id = %id, "Local Redis deleted");
        Ok(())
    }

    /// Verifies the process is still running. Never connects to the server.
    pub async fn check_health(&self, id: &str) -> Result<()> {
        let mut running = self.running.lock().await;
        let child = running.get_mut(id).ok_or_else(|| {
            AlienError::new(ErrorData::ServiceResourceNotFound {
                resource_id: id.to_string(),
                resource_type: "redis".to_string(),
            })
        })?;
        match child.try_wait().into_alien_error().context(process_error(
            id,
            "health_check",
            "Failed to check the server process",
        ))? {
            None => Ok(()),
            Some(status) => Err(AlienError::new(process_error(
                id,
                "health_check",
                &format!("Redis is not running (exited with {status})"),
            ))),
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

    async fn boot(&self, metadata: &RedisMetadata) -> Result<Child> {
        let id = &metadata.resource_id;
        let binary = ensure_binary(&self.state_dir).await?;
        let dir = self.resource_dir(id);
        let data_dir = dir.join("data");
        tokio::fs::create_dir_all(&data_dir)
            .await
            .into_alien_error()
            .context(ErrorData::LocalDirectoryError {
                path: data_dir.display().to_string(),
                operation: "create".to_string(),
                reason: "Failed to create Redis data directory".to_string(),
            })?;

        // Loopback plus the docker bridge gateway when present, so a same-stack local
        // container can reach it; never 0.0.0.0.
        let bind = match bindable_docker_bridge_gateway().await? {
            Some(gateway) => format!("127.0.0.1 {gateway}"),
            None => "127.0.0.1".to_string(),
        };
        // The password goes in a 0600 config file, not on the command line where `ps`
        // would show it.
        let config = render_config(metadata, &bind, &data_dir);
        let config_path = dir.join("valkey.conf");
        let write_path = config_path.clone();
        tokio::task::spawn_blocking(move || {
            alien_core::file_utils::write_secret_file(&write_path, config.as_bytes())
        })
        .await
        .into_alien_error()
        .context(process_error(id, "configure", "Config write task panicked"))?
        .into_alien_error()
        .context(ErrorData::LocalDirectoryError {
            path: config_path.display().to_string(),
            operation: "write".to_string(),
            reason: "Failed to write the Redis config".to_string(),
        })?;

        let mut child = Command::new(&binary)
            .arg(&config_path)
            .current_dir(&data_dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .into_alien_error()
            .context(process_error(id, "start", "Failed to spawn valkey-server"))?;

        wait_until_listening(id, metadata.port, &mut child).await?;
        Ok(child)
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
            if id == "_dist" {
                continue;
            }
            match self.try_read_metadata(&id) {
                Ok(Some(metadata)) => {
                    if let Err(error) = self.start_redis(&id, metadata.spec).await {
                        warn!(redis_id = %id, ?error, "Failed to recover local Redis");
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    warn!(redis_id = %id, ?error, "Skipping local Redis: unreadable metadata")
                }
            }
        }
    }

    async fn restart_exited(&self) {
        let exited: Vec<String> = {
            let mut running = self.running.lock().await;
            running
                .iter_mut()
                .filter_map(|(id, child)| {
                    (!matches!(child.try_wait(), Ok(None))).then(|| id.clone())
                })
                .collect()
        };
        for id in exited {
            let spec = match self.try_read_metadata(&id) {
                Ok(Some(metadata)) => metadata.spec,
                _ => continue,
            };
            warn!(redis_id = %id, "Local Redis exited; restarting");
            if let Err(error) = self.start_redis(&id, spec).await {
                warn!(redis_id = %id, ?error, "Failed to restart local Redis");
            }
        }
    }

    async fn stop_all(&self) {
        let mut running = self.running.lock().await;
        for (id, child) in running.iter_mut() {
            if let Err(error) = stop_child(id, child).await {
                warn!(redis_id = %id, ?error, "Failed to stop local Redis on shutdown");
            }
        }
        running.clear();
    }
}

fn process_error(id: &str, operation: &str, reason: &str) -> ErrorData {
    ErrorData::LocalProcessError {
        process_id: id.to_string(),
        operation: operation.to_string(),
        reason: reason.to_string(),
    }
}

/// The server config. `appendfsync everysec` bounds loss to about a second of writes.
fn render_config(metadata: &RedisMetadata, bind: &str, data_dir: &Path) -> String {
    let spec = &metadata.spec;
    let mut config = format!(
        "bind {bind}\nport {}\nprotected-mode yes\nrequirepass {}\ndir {}\nmaxmemory-policy {}\n\
         save \"\"\nappendonly {}\nappendfsync everysec\ndaemonize no\n",
        metadata.port,
        metadata.password,
        data_dir.display(),
        spec.eviction_policy,
        if spec.persistence { "yes" } else { "no" },
    );
    if let Some(bytes) = spec.max_memory_bytes {
        config.push_str(&format!("maxmemory {bytes}\n"));
    }
    config
}

/// SIGTERM-equivalent stop: `kill` then reap. Valkey rewrites nothing on SIGKILL, but the
/// AOF with `everysec` keeps all but the last second of writes.
async fn stop_child(id: &str, child: &mut Child) -> Result<()> {
    child.kill().await.into_alien_error().context(process_error(
        id,
        "stop",
        "Failed to stop valkey-server",
    ))
}

async fn wait_until_listening(id: &str, port: u16, child: &mut Child) -> Result<()> {
    let deadline = tokio::time::Instant::now() + READY_TIMEOUT;
    loop {
        if let Some(status) = child.try_wait().into_alien_error().context(process_error(
            id,
            "start",
            "Failed to check the server process",
        ))? {
            return Err(AlienError::new(process_error(
                id,
                "start",
                &format!("valkey-server exited during startup with {status}"),
            )));
        }
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(AlienError::new(process_error(
                id,
                "start",
                &format!("valkey-server did not listen on port {port} within {READY_TIMEOUT:?}"),
            )));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The release target name for this host, e.g. `linux-x86_64`.
fn release_target() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// Returns the path of the cached `valkey-server`, downloading and verifying it first
/// when it is not there.
async fn ensure_binary(state_dir: &Path) -> Result<PathBuf> {
    let target = release_target();
    let dist = state_dir
        .join("redis")
        .join("_dist")
        .join(format!("v{VALKEY_VERSION}"))
        .join(&target);
    let binary = dist.join("valkey-server");
    if binary.exists() {
        return Ok(binary);
    }

    let base_override = std::env::var(RELEASES_URL_ENV).ok();
    let base = base_override
        .clone()
        .unwrap_or_else(|| RELEASES_URL_DEFAULT.to_string());
    let archive_url = format!("{base}/v{VALKEY_VERSION}/{target}/valkey-server.tar.gz");
    let fetch_error =
        |reason: String| AlienError::new(process_error("valkey", "download", &reason));

    let archive = fetch_bytes(&archive_url)
        .await
        .map_err(|reason| fetch_error(format!("Could not fetch {archive_url}: {reason}")))?;
    let expected = match &base_override {
        // A host the user pointed us at: trust its checksum file (e.g. a local build).
        Some(_) => {
            let text = fetch_bytes(&format!("{archive_url}.sha256"))
                .await
                .map_err(|reason| fetch_error(format!("Could not fetch the .sha256 file: {reason}")))?;
            String::from_utf8_lossy(&text)
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_string()
        }
        None => PINNED_SHA256
            .iter()
            .find(|(name, _)| *name == target)
            .map(|(_, sha)| sha.to_string())
            .ok_or_else(|| {
                fetch_error(format!(
                    "No pinned checksum for target '{target}' in this build; set {RELEASES_URL_ENV} to a host that serves valkey-server for it"
                ))
            })?,
    };
    let actual = hex::encode(Sha256::digest(&archive));
    if !actual.eq_ignore_ascii_case(expected.trim()) {
        return Err(fetch_error(format!(
            "Checksum mismatch for {archive_url}: expected {expected}, got {actual}"
        )));
    }

    let unpack_dir = dist.clone();
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        std::fs::create_dir_all(&unpack_dir)?;
        let decoder = flate2::read::GzDecoder::new(archive.as_slice());
        tar::Archive::new(decoder).unpack(&unpack_dir)
    })
    .await
    .into_alien_error()
    .context(process_error("valkey", "install", "Unpack task panicked"))?
    .into_alien_error()
    .context(process_error(
        "valkey",
        "install",
        "Failed to unpack the valkey-server archive",
    ))?;

    if !binary.exists() {
        return Err(fetch_error(
            "The archive did not contain a top-level 'valkey-server' file".to_string(),
        ));
    }
    Ok(binary)
}

async fn fetch_bytes(url: &str) -> std::result::Result<Vec<u8>, String> {
    if let Some(path) = url.strip_prefix("file://") {
        return tokio::fs::read(path).await.map_err(|e| e.to_string());
    }
    let response = reqwest::get(url).await.map_err(|e| e.to_string())?;
    let response = response.error_for_status().map_err(|e| e.to_string())?;
    response
        .bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| e.to_string())
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
