//! Local Postgres manager.
//!
//! Runs Postgres as a native process from embedded binaries (via `postgresql_embedded`),
//! exactly like the other local service managers run native processes. State for each
//! database lives under `{state_dir}/postgres/{id}/`; the downloaded server binaries are
//! cached once in `{state_dir}/postgres/_dist` and shared across databases.
//!
//! pgvector is installed into each server at boot from Alien's own release host (see
//! [`install_pgvector`]), so `CREATE EXTENSION vector` works on Local without depending on
//! any third-party precompiled-extension repository.

use crate::error::{ErrorData, Result};
use alien_core::bindings::PostgresBinding;
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use postgresql_embedded::{PostgreSQL, Settings, Status, VersionReq, BOOTSTRAP_SUPERUSER};
use postgresql_extensions::repository::portal_corp::repository::PortalCorp;
use postgresql_extensions::repository::{registry, Repository};
use postgresql_extensions::{AvailableExtension, Version};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::{broadcast, Mutex};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

/// Admin user advertised in the binding. `postgresql_embedded` always names the bootstrap superuser
/// `BOOTSTRAP_SUPERUSER` ("postgres") and ignores `Settings.username`, so the binding must report that
/// exact role — any other name is a role that does not exist, and Postgres reports a missing role as
/// "password authentication failed" under password auth, which looks like (but isn't) a bad password.
const ADMIN_USER: &str = BOOTSTRAP_SUPERUSER;
/// How often the monitor loop checks that each server is still up.
const MONITOR_INTERVAL: Duration = Duration::from_secs(30);

/// pgvector install namespace. Reuses `postgresql_extensions`' "portal-corp" namespace (its
/// precompiled-zip layout matches our artifacts) but overrides the repository via
/// [`register_pgvector_repository`] to fetch from Alien's release host.
const PGVECTOR_NAMESPACE: &str = "portal-corp";
/// Extension name passed to the installer; the published archives are named `pgvector_compiled`.
const PGVECTOR_EXTENSION: &str = "pgvector_compiled";
/// pgvector version shipped on Alien's release host. Pinned so a server always installs a
/// known-good build; bump this in lockstep with the published artifacts.
const PGVECTOR_VERSION: &str = "0.8.1";
/// Environment variable that retargets the pgvector download to a different host (CI, a staging
/// mirror, or a local fixture). Deliberately NOT `postgresql_embedded`'s `POSTGRESQL_RELEASES_URL`
/// (which mirrors the server binaries): sharing that var would retarget pgvector to a layout an
/// internal mirror won't have and 404 every Local create. When unset,
/// [`ALIEN_PGVECTOR_RELEASES_URL_DEFAULT`] is used.
const PGVECTOR_RELEASES_URL_ENV: &str = "ALIEN_PGVECTOR_RELEASES_URL";
/// Default base URL for pgvector archives (Alien's release host). The installer requests
/// `{base}/v{PGVECTOR_VERSION}/{os}-{arch}/pg{major}/{name}.zip` — a static per-(target × PG-major)
/// object, so the path selects the build.
const ALIEN_PGVECTOR_RELEASES_URL_DEFAULT: &str =
    "https://releases.alien.dev/pgvector";

/// Registers the Alien-hosted pgvector repository exactly once for the whole process. The
/// `postgresql_extensions` registry is a process-global singleton, so registering on every boot
/// would be wasteful. The cached registration outcome (`Ok`, or the error string) keeps the override
/// idempotent across servers and restarts.
static REGISTER_PGVECTOR_REPOSITORY: OnceLock<std::result::Result<(), String>> = OnceLock::new();

/// Persisted state for one local Postgres database. Port and password are written once
/// so a restart (crash or CLI relaunch) reattaches to the same DSN.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PostgresMetadata {
    resource_id: String,
    /// Major engine version, e.g. "17".
    version: String,
    port: u16,
    username: String,
    password: String,
    /// Database created for this resource (named after the id).
    database: String,
    data_dir: PathBuf,
}

// Hand-written so the persisted password can never reach a `{:?}` log, matching the redacting `Debug`
// on the binding structs that carry the same secret.
impl std::fmt::Debug for PostgresMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresMetadata")
            .field("resource_id", &self.resource_id)
            .field("version", &self.version)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("database", &self.database)
            .field("data_dir", &self.data_dir)
            .finish()
    }
}

/// Manages local Postgres databases as native processes. Running servers are keyed by
/// resource id; their durable state lives on disk in `metadata.json`.
#[derive(Clone)]
pub struct LocalPostgresManager {
    state_dir: PathBuf,
    runtimes: Arc<Mutex<HashMap<String, PostgreSQL>>>,
}

impl std::fmt::Debug for LocalPostgresManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalPostgresManager")
            .field("state_dir", &self.state_dir)
            .finish()
    }
}

impl LocalPostgresManager {
    /// Creates a manager without a background monitor (tests).
    #[cfg(test)]
    pub fn new(state_dir: PathBuf) -> Self {
        Self {
            state_dir,
            runtimes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Creates a manager and spawns the monitor-and-recover loop: it restarts databases
    /// recorded on disk at startup and restarts any that exit while running.
    pub fn new_with_shutdown(
        state_dir: PathBuf,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> (Self, JoinHandle<()>) {
        let manager = Self {
            state_dir,
            runtimes: Arc::new(Mutex::new(HashMap::new())),
        };

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
                    _ = interval.tick() => {
                        monitor.restart_exited().await;
                    }
                }
            }
        });

        (manager, handle)
    }

    fn resource_dir(&self, id: &str) -> PathBuf {
        self.state_dir.join("postgres").join(id)
    }

    fn metadata_path(&self, id: &str) -> PathBuf {
        self.resource_dir(id).join("metadata.json")
    }

    fn install_cache_dir(&self) -> PathBuf {
        self.state_dir.join("postgres").join("_dist")
    }

    /// Starts (or, if already running, returns) the database for `id`. Idempotent:
    /// `initdb` runs once, and the password and port are generated once and reused.
    ///
    /// The lock is held across the whole boot so check-and-insert is atomic: the startup
    /// monitor's `recover_all` and a controller's `create_start` can call this for the same id
    /// concurrently, and a check-then-release-then-boot would let both pass the guard and boot two
    /// servers on one data dir and port. Holding it serialises per-id starts; the only contention
    /// is other lifecycle ops — never the SQL data path, which bypasses the manager — so the cost
    /// is cold-start latency, not throughput.
    pub async fn start_postgres(&self, id: &str, version: &str) -> Result<()> {
        let mut runtimes = self.runtimes.lock().await;
        if runtimes.contains_key(id) {
            return Ok(());
        }

        let metadata = self.load_or_init_metadata(id, version).await?;
        let postgres = self.boot(&metadata).await?;

        runtimes.insert(id.to_string(), postgres);
        info!(postgres_id = %id, "Local Postgres started");
        Ok(())
    }

    /// Stops the server but keeps its data and metadata so it can be recovered.
    ///
    /// Drops the tracking entry only after `stop()` succeeds, re-inserting the handle on
    /// failure. `boot()` runs with `temporary: false`, so a dropped handle leaves a live
    /// process; if it also left the id untracked, a `delete_postgres` retry would skip the
    /// stop and `remove_dir_all` the data dir out from under the running server.
    pub async fn stop_postgres(&self, id: &str) -> Result<()> {
        let mut runtimes = self.runtimes.lock().await;
        let Some(postgres) = runtimes.remove(id) else {
            return Ok(());
        };
        match postgres.stop().await {
            Ok(()) => Ok(()),
            Err(error) => {
                runtimes.insert(id.to_string(), postgres);
                Err(error).into_alien_error().context(ErrorData::LocalProcessError {
                    process_id: id.to_string(),
                    operation: "stop".to_string(),
                    reason: "Failed to stop local Postgres".to_string(),
                })
            }
        }
    }

    /// Stops the server and removes its data directory and metadata. Tolerates an already-gone
    /// database — `stop_postgres` is a no-op when the id isn't tracked (never created / already
    /// stopped), and the `dir.exists()` guard tolerates a missing data dir. But a genuine stop
    /// failure propagates: we must never delete the data directory out from under a live server.
    pub async fn delete_postgres(&self, id: &str) -> Result<()> {
        self.stop_postgres(id).await?;

        let dir = self.resource_dir(id);
        if dir.exists() {
            tokio::fs::remove_dir_all(&dir)
                .await
                .into_alien_error()
                .context(ErrorData::LocalDirectoryError {
                    path: dir.display().to_string(),
                    operation: "delete".to_string(),
                    reason: "Failed to delete Postgres data directory".to_string(),
                })?;
        }
        debug!(postgres_id = %id, "Local Postgres deleted");
        Ok(())
    }

    /// Verifies the server is up by checking process status — never connects to the database.
    /// Same-stack bindings are the only path that talks to the database, so the manager never
    /// speaks SQL; health is the process being up, not a query round-trip.
    pub async fn check_health(&self, id: &str) -> Result<()> {
        let runtimes = self.runtimes.lock().await;
        let postgres = runtimes.get(id).ok_or_else(|| {
            AlienError::new(ErrorData::ServiceResourceNotFound {
                resource_id: id.to_string(),
                resource_type: "postgres".to_string(),
            })
        })?;

        match postgres.status() {
            Status::Started => Ok(()),
            other => Err(AlienError::new(ErrorData::LocalProcessError {
                process_id: id.to_string(),
                operation: "health_check".to_string(),
                reason: format!("Postgres is not running (status: {:?})", other),
            })),
        }
    }

    /// Returns the binding for a running database. Reads persisted metadata so the
    /// caller always sees the current port and password.
    pub fn get_binding(&self, id: &str) -> Result<PostgresBinding> {
        let metadata = self.read_metadata(id)?;
        Ok(PostgresBinding::local(
            "127.0.0.1",
            metadata.port,
            metadata.database,
            metadata.username,
            metadata.password,
        ))
    }

    // ───────────────────────── internals ─────────────────────────

    /// Builds + starts a `PostgreSQL` from metadata and ensures the database exists.
    async fn boot(&self, metadata: &PostgresMetadata) -> Result<PostgreSQL> {
        let version = VersionReq::parse(&format!("^{}", metadata.version))
            .into_alien_error()
            .context(ErrorData::LocalProcessError {
                process_id: metadata.resource_id.clone(),
                operation: "configure".to_string(),
                reason: format!("Invalid Postgres major version '{}'", metadata.version),
            })?;

        let settings = Settings {
            version,
            // private-always applies to Local: bind loopback only, never 0.0.0.0.
            host: "127.0.0.1".to_string(),
            port: metadata.port,
            username: metadata.username.clone(),
            password: metadata.password.clone(),
            // keep the data directory across drops; recovery reattaches to it.
            temporary: false,
            data_dir: metadata.data_dir.clone(),
            installation_dir: self.install_cache_dir(),
            ..Default::default()
        };

        let mut postgres = PostgreSQL::new(settings);
        postgres
            .setup()
            .await
            .into_alien_error()
            .context(ErrorData::LocalProcessError {
                process_id: metadata.resource_id.clone(),
                operation: "setup".to_string(),
                reason: "Failed to download/initialise embedded Postgres".to_string(),
            })?;
        postgres
            .start()
            .await
            .into_alien_error()
            .context(ErrorData::LocalProcessError {
                process_id: metadata.resource_id.clone(),
                operation: "start".to_string(),
                reason: "Failed to start embedded Postgres".to_string(),
            })?;

        let exists = postgres
            .database_exists(&metadata.database)
            .await
            .into_alien_error()
            .context(ErrorData::LocalProcessError {
                process_id: metadata.resource_id.clone(),
                operation: "query".to_string(),
                reason: "Failed to check whether the database exists".to_string(),
            })?;
        if !exists {
            postgres
                .create_database(&metadata.database)
                .await
                .into_alien_error()
                .context(ErrorData::LocalProcessError {
                    process_id: metadata.resource_id.clone(),
                    operation: "create_database".to_string(),
                    reason: "Failed to create the database".to_string(),
                })?;
        }

        install_pgvector(postgres.settings().clone(), metadata.resource_id.clone()).await?;

        Ok(postgres)
    }

    /// Reads metadata from disk, generating it (password, port, paths) on first start.
    async fn load_or_init_metadata(&self, id: &str, version: &str) -> Result<PostgresMetadata> {
        if self.metadata_path(id).exists() {
            return self.read_metadata(id);
        }

        let metadata = PostgresMetadata {
            resource_id: id.to_string(),
            version: version.to_string(),
            port: allocate_port(id)?,
            username: ADMIN_USER.to_string(),
            password: generate_password(),
            database: id.to_string(),
            data_dir: self.resource_dir(id).join("data"),
        };
        self.write_metadata(&metadata).await?;
        Ok(metadata)
    }

    fn read_metadata(&self, id: &str) -> Result<PostgresMetadata> {
        let path = self.metadata_path(id);
        let read = std::fs::read_to_string(&path);
        // A genuinely-absent file is "not found"; any other IO error (e.g. permissions)
        // must surface as such, not be mislabelled 404.
        if let Err(error) = &read {
            if error.kind() == std::io::ErrorKind::NotFound {
                return Err(AlienError::new(ErrorData::ServiceResourceNotFound {
                    resource_id: id.to_string(),
                    resource_type: "postgres".to_string(),
                }));
            }
        }
        let content = read.into_alien_error().context(ErrorData::LocalDirectoryError {
            path: path.display().to_string(),
            operation: "read".to_string(),
            reason: "Failed to read Postgres metadata".to_string(),
        })?;
        serde_json::from_str(&content)
            .into_alien_error()
            .context(ErrorData::LocalDirectoryError {
                path: path.display().to_string(),
                operation: "read".to_string(),
                reason: "Failed to parse Postgres metadata".to_string(),
            })
    }

    /// Writes metadata 0600 — it holds the generated password, which must not be
    /// world-readable. (Local has no control-plane sync, so the password lives here;
    /// cloud controllers must keep only a secret identifier in their synced state.)
    async fn write_metadata(&self, metadata: &PostgresMetadata) -> Result<()> {
        let dir = self.resource_dir(&metadata.resource_id);
        tokio::fs::create_dir_all(&dir)
            .await
            .into_alien_error()
            .context(ErrorData::LocalDirectoryError {
                path: dir.display().to_string(),
                operation: "create".to_string(),
                reason: "Failed to create Postgres state directory".to_string(),
            })?;

        let path = self.metadata_path(&metadata.resource_id);
        let data = serde_json::to_vec_pretty(metadata)
            .into_alien_error()
            .context(ErrorData::LocalDirectoryError {
                path: path.display().to_string(),
                operation: "write".to_string(),
                reason: "Failed to serialize Postgres metadata".to_string(),
            })?;
        let write_path = path.clone();
        tokio::task::spawn_blocking(move || {
            alien_core::file_utils::write_secret_file(&write_path, &data)
        })
        .await
        .into_alien_error()
        .context(ErrorData::LocalDirectoryError {
            path: path.display().to_string(),
            operation: "write".to_string(),
            reason: "Failed to spawn metadata write".to_string(),
        })?
        .into_alien_error()
        .context(ErrorData::LocalDirectoryError {
            path: path.display().to_string(),
            operation: "write".to_string(),
            reason: "Failed to write Postgres metadata".to_string(),
        })
    }

    /// Restarts every database recorded on disk (called once at startup).
    async fn recover_all(&self) {
        let root = self.state_dir.join("postgres");
        let Ok(mut entries) = tokio::fs::read_dir(&root).await else {
            return;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let Ok(file_type) = entry.file_type().await else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().to_string();
            if id == "_dist" {
                continue;
            }
            let metadata = match self.read_metadata(&id) {
                Ok(metadata) => metadata,
                // No metadata.json in this dir: nothing recoverable, skip quietly.
                Err(error) if matches!(error.error, Some(ErrorData::ServiceResourceNotFound { .. })) => {
                    continue;
                }
                // Present but unreadable (permissions, corrupt JSON): a real DB would go dark
                // with no trace, so surface it instead of silently skipping.
                Err(error) => {
                    warn!(postgres_id = %id, ?error, "Skipping local Postgres: unreadable metadata");
                    continue;
                }
            };
            if let Err(error) = self.start_postgres(&id, &metadata.version).await {
                warn!(postgres_id = %id, ?error, "Failed to recover local Postgres");
            }
        }
    }

    /// Restarts any tracked server whose process has exited.
    async fn restart_exited(&self) {
        let mut runtimes = self.runtimes.lock().await;
        for (id, postgres) in runtimes.iter_mut() {
            if postgres.status() == Status::Started {
                continue;
            }
            warn!(postgres_id = %id, "Local Postgres exited; restarting");
            if let Err(error) = postgres.start().await {
                warn!(postgres_id = %id, ?error, "Failed to restart local Postgres");
            }
        }
    }

    async fn stop_all(&self) {
        let mut runtimes = self.runtimes.lock().await;
        for (id, postgres) in runtimes.iter_mut() {
            if let Err(error) = postgres.stop().await {
                warn!(postgres_id = %id, ?error, "Failed to stop local Postgres on shutdown");
            }
        }
        runtimes.clear();
    }
}

/// Installs pgvector into the running server so `CREATE EXTENSION vector` succeeds. Runs after the
/// server is up and the database exists (so `pg_config` resolves real directories); a missing
/// artifact errors rather than silently lacking the extension.
///
/// `postgresql_extensions::install` holds a `&dyn Settings` across an `.await`, so its future is not
/// `Send` and can't live in the spawned monitor task. We run it on a dedicated current-thread runtime
/// on a blocking thread, with an owned `Settings` clone so the future borrows nothing across the spawn.
async fn install_pgvector(settings: Settings, resource_id: String) -> Result<()> {
    register_pgvector_repository(&resource_id)?;

    let version = VersionReq::parse(&format!("={PGVECTOR_VERSION}"))
        .into_alien_error()
        .context(ErrorData::LocalProcessError {
            process_id: resource_id.clone(),
            operation: "install_pgvector".to_string(),
            reason: format!("Invalid pinned pgvector version '{PGVECTOR_VERSION}'"),
        })?;

    let install_resource_id = resource_id.clone();
    tokio::task::spawn_blocking(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .into_alien_error()
            .context(ErrorData::LocalProcessError {
                process_id: install_resource_id.clone(),
                operation: "install_pgvector".to_string(),
                reason: "Failed to build runtime for pgvector install".to_string(),
            })?
            .block_on(postgresql_extensions::install(
                &settings,
                PGVECTOR_NAMESPACE,
                PGVECTOR_EXTENSION,
                &version,
            ))
            .into_alien_error()
            .context(ErrorData::LocalProcessError {
                process_id: install_resource_id,
                operation: "install_pgvector".to_string(),
                reason: format!(
                    "Failed to install pgvector {PGVECTOR_VERSION} from the configured release host"
                ),
            })
    })
    .await
    .into_alien_error()
    .context(ErrorData::LocalProcessError {
        process_id: resource_id.clone(),
        operation: "install_pgvector".to_string(),
        reason: "pgvector install task panicked".to_string(),
    })??;

    info!(postgres_id = %resource_id, version = PGVECTOR_VERSION, "pgvector installed");
    Ok(())
}

/// Resolves the base URL pgvector archives are fetched from: the env override if set,
/// otherwise Alien's release host.
fn pgvector_releases_url() -> String {
    std::env::var(PGVECTOR_RELEASES_URL_ENV)
        .unwrap_or_else(|_| ALIEN_PGVECTOR_RELEASES_URL_DEFAULT.to_string())
}

/// Maps the running target to the `{os}-{arch}` path segment the release host serves under.
/// Only the four targets Alien actually builds are accepted; any other (e.g. an Intel Mac, for
/// which there is deliberately no build) fails loud here rather than 404-ing mid-download.
fn pgvector_target() -> postgresql_extensions::Result<(&'static str, &'static str)> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok(("linux", "x86_64")),
        ("linux", "aarch64") => Ok(("linux", "aarch64")),
        ("macos", "aarch64") => Ok(("darwin", "aarch64")),
        ("windows", "x86_64") => Ok(("windows", "x86_64")),
        (os, arch) => Err(postgresql_extensions::Error::IoError(format!(
            "pgvector: no published build for {os}-{arch}; the release host serves \
             linux-x86_64, linux-aarch64, darwin-aarch64, windows-x86_64"
        ))),
    }
}

/// Overrides the `postgresql_extensions` "portal-corp" namespace to download pgvector from Alien's
/// release host. `get_or_init` registers at most once across concurrent first-boots. Failures are
/// propagated (not swallowed — keeping the upstream repository would fetch from the wrong host) and
/// cached, since registering the process-global registry is not transient.
fn register_pgvector_repository(resource_id: &str) -> Result<()> {
    REGISTER_PGVECTOR_REPOSITORY
        .get_or_init(|| {
            registry::register(PGVECTOR_NAMESPACE, Box::new(|| Ok(Box::new(AlienPgvectorRepository))))
                .map_err(|e| e.to_string())
        })
        .clone()
        .map_err(|reason| {
            AlienError::new(ErrorData::LocalProcessError {
                process_id: resource_id.to_string(),
                operation: "install_pgvector".to_string(),
                reason: format!("Failed to register Alien pgvector repository: {reason}"),
            })
        })
}

/// A `postgresql_extensions` repository that serves pgvector from Alien's release host.
///
/// It only customises *where the archive comes from*: [`AlienPgvectorRepository::get_archive`]
/// downloads the per-(target × PG-major) zip over HTTP, then extraction is delegated to the
/// upstream `PortalCorp` repository, whose zip layout our published artifacts mirror.
#[derive(Debug)]
struct AlienPgvectorRepository;

#[async_trait]
impl Repository for AlienPgvectorRepository {
    fn name(&self) -> &str {
        PGVECTOR_NAMESPACE
    }

    async fn get_available_extensions(
        &self,
    ) -> postgresql_extensions::Result<Vec<AvailableExtension>> {
        Ok(vec![AvailableExtension::new(
            self.name(),
            PGVECTOR_EXTENSION,
            "pgvector built and published by Alien",
        )])
    }

    async fn get_archive(
        &self,
        postgresql_version: &str,
        name: &str,
        _version: &VersionReq,
    ) -> postgresql_extensions::Result<(Version, Vec<u8>)> {
        let base = pgvector_releases_url();
        let (os, arch) = pgvector_target()?;
        // The embedded server reports a full version (e.g. "17.2.0"); the artifacts are
        // keyed by major, the only axis that changes pgvector's ABI.
        let major = postgresql_version.split('.').next().unwrap_or(postgresql_version);
        let url = format!("{base}/v{PGVECTOR_VERSION}/{os}-{arch}/pg{major}/{name}.zip");
        let response = reqwest::get(&url)
            .await
            .map_err(|error| postgresql_extensions::Error::IoError(error.to_string()))?
            .error_for_status()
            .map_err(|error| postgresql_extensions::Error::IoError(error.to_string()))?;
        let bytes = response
            .bytes()
            .await
            .map_err(|error| postgresql_extensions::Error::IoError(error.to_string()))?;

        let version = Version::parse(PGVECTOR_VERSION)
            .map_err(|error| postgresql_extensions::Error::IoError(error.to_string()))?;
        Ok((version, bytes.to_vec()))
    }

    async fn install(
        &self,
        name: &str,
        library_dir: PathBuf,
        extension_dir: PathBuf,
        archive: &[u8],
    ) -> postgresql_extensions::Result<Vec<PathBuf>> {
        // The published archive is a zip with the same layout as portal-corp's, so the
        // extraction logic is identical; delegate rather than duplicate it.
        let upstream = PortalCorp::new()?;
        upstream
            .install(name, library_dir, extension_dir, archive)
            .await
    }
}

/// Generates a 24-character alphanumeric password (URL-safe, so the connection string
/// needs no special handling for it).
fn generate_password() -> String {
    rand::rng()
        .sample_iter(rand::distr::Alphanumeric)
        .take(24)
        .map(char::from)
        .collect()
}

/// Claims a free localhost port by binding and releasing it. There is a small race
/// between release and Postgres binding; the port is persisted so recovery reuses it.
fn allocate_port(id: &str) -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .into_alien_error()
        .context(ErrorData::NoFreePorts {
            service_name: format!("postgres/{}", id),
        })?;
    let port = listener
        .local_addr()
        .into_alien_error()
        .context(ErrorData::NoFreePorts {
            service_name: format!("postgres/{}", id),
        })?
        .port();
    Ok(port)
}
