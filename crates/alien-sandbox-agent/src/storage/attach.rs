//! Persistent storage as the agent serves it: configured at boot, attached once by the binding
//! (claim already written, grant minted), then replicated until the sandbox ends or another
//! sandbox claims the key.
//!
//! Where the sandbox cannot reach object storage (GCP Agent Platform), the binding attaches
//! without a grant: the agent restores from and commits into its [`Spool`], which the binding
//! seeded beforehand and drains afterwards over the agent's inbound channel.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use alien_error::{AlienError, Context, IntoAlienError};
use alien_sandbox_storage::{
    read_manifest, restore, CommitOutcome, Committer, ErrorData as StorageError, ObjectName,
    ObjectStore, ObjectVersion, Spool, StorageGrant,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::freeze::Freezer;
use super::push::GrantStore;
use super::tree::{self, DirSink};
use super::{Replicator, STAGING_DIR};
use crate::error::{ErrorData, Result};
use crate::exec::ExecIdentity;
use crate::image_command::ImageProcess;

/// Set by setup on a sandbox that declares persistent storage: `{ path, exclude, maxSizeBytes }`.
pub const ENV_PERSISTENT_STORAGE: &str = "ALIEN_SANDBOX_PERSISTENT_STORAGE";

/// How often the directory is checked for changes. With one commit in flight at a time, a write
/// is committed within about two of these plus the upload.
const TICK: Duration = Duration::from_secs(1);

/// The declaration as the agent receives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StorageConfig {
    /// Absolute directory replicated.
    pub path: String,
    /// Paths relative to `path` never replicated.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Largest tree committed.
    #[serde(default)]
    pub max_size_bytes: Option<u64>,
}

impl StorageConfig {
    /// The declaration from the environment, if any.
    pub fn from_env() -> Result<Option<Self>> {
        let Ok(raw) = std::env::var(ENV_PERSISTENT_STORAGE) else {
            return Ok(None);
        };
        let config: Self = serde_json::from_str(&raw).map_err(|error| {
            AlienError::new(ErrorData::ConfigInvalid {
                setting: ENV_PERSISTENT_STORAGE.to_string(),
                reason: error.to_string(),
            })
        })?;
        if !config.path.starts_with('/') || config.path.contains("/../") {
            return Err(AlienError::new(ErrorData::ConfigInvalid {
                setting: ENV_PERSISTENT_STORAGE.to_string(),
                reason: "path must be absolute".to_string(),
            }));
        }
        Ok(Some(config))
    }
}

/// What the binding sends once, after claiming the key.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttachRequest {
    /// The key.
    pub key: String,
    /// This sandbox's writer id, as written into HEAD by the claim.
    pub writer: String,
    /// HEAD's version after the claim. Unused without a grant: the binding's puller holds it.
    pub head_version: ObjectVersion,
    /// The commit HEAD named at the claim, restored before the image starts.
    #[serde(default)]
    pub commit: Option<ObjectName>,
    /// Rights to the key. Without one the binding pulls: the restore reads what it seeded into
    /// the spool and commits go to the spool.
    #[serde(default)]
    pub grant: Option<StorageGrant>,
}

/// A renewed grant.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenewRequest {
    /// Rights to the key, replacing the previous ones.
    pub grant: StorageGrant,
}

/// Replication state as `sandbox.persistentStorage` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReplicationState {
    /// Not attached yet; the image command waits.
    AwaitingRestore,
    /// Every change seen so far is committed or in flight.
    Replicating,
    /// The last commit failed (store unreachable, `maxSize` exceeded); retried on the next change.
    Behind,
    /// Another sandbox claimed the key; nothing more is committed.
    Stopped,
    /// The grant expired or the store refused it; commits resume once the binding renews it.
    Denied,
}

/// The status a caller sees.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStatus {
    /// The key, once attached.
    pub key: Option<String>,
    /// Replication state.
    pub state: ReplicationState,
    /// When a commit last published (or found nothing to publish).
    pub last_synced_at: Option<DateTime<Utc>>,
    /// The commit restored at attach, if the key had one.
    pub restored_from: Option<ObjectName>,
    /// The commit HEAD names now.
    pub head: Option<u64>,
    /// Why the state is `behind`, `denied` or `stopped`.
    pub last_error: Option<String>,
    /// When the grant the agent holds expires; the binding renews it past half its life.
    pub grant_expires_at: Option<DateTime<Utc>>,
    /// When the agent received that grant: with `grant_expires_at`, its life.
    pub grant_issued_at: Option<DateTime<Utc>>,
}

/// Where commits go.
enum Store {
    /// The key itself, through a grant.
    Grant(Arc<GrantStore>),
    /// The spool the binding pulls from.
    Spool(Arc<Spool>),
}

impl Store {
    fn object_store(&self) -> &dyn ObjectStore {
        match self {
            Self::Grant(store) => store.as_ref(),
            Self::Spool(spool) => spool.as_ref(),
        }
    }

    /// Whether the store refused the grant. A spool holds no grant.
    fn denied(&self) -> bool {
        match self {
            Self::Grant(store) => store.denied(),
            Self::Spool(_) => false,
        }
    }
}

struct Live {
    replicator: Replicator,
    store: Store,
}

/// The agent's persistent storage.
pub struct PersistentStorage {
    config: StorageConfig,
    staging: std::path::PathBuf,
    spool_dir: std::path::PathBuf,
    spool: tokio::sync::OnceCell<Arc<Spool>>,
    live: tokio::sync::Mutex<Option<Live>>,
    status: Mutex<StorageStatus>,
}

fn failed(operation: &str, reason: impl Into<String>) -> ErrorData {
    ErrorData::OperationFailed {
        operation: operation.to_string(),
        reason: reason.into(),
    }
}

fn storage_failed(operation: &str, error: AlienError<StorageError>) -> AlienError<ErrorData> {
    AlienError::new(failed(operation, error.to_string()))
}

impl PersistentStorage {
    /// Configured, not attached.
    pub fn new(config: StorageConfig) -> Self {
        Self::with_staging(config, STAGING_DIR.into())
    }

    /// Configured, staging commits in `staging` and spooling them for a pulling binding in
    /// `<staging>.spool` (both agent-only; tests use a temporary one).
    pub fn with_staging(config: StorageConfig, staging: std::path::PathBuf) -> Self {
        let mut spool_dir = staging.clone().into_os_string();
        spool_dir.push(".spool");
        Self {
            config,
            staging,
            spool_dir: spool_dir.into(),
            spool: tokio::sync::OnceCell::new(),
            live: tokio::sync::Mutex::new(None),
            status: Mutex::new(StorageStatus {
                key: None,
                state: ReplicationState::AwaitingRestore,
                last_synced_at: None,
                restored_from: None,
                head: None,
                last_error: None,
                grant_expires_at: None,
                grant_issued_at: None,
            }),
        }
    }

    /// The spool a pulling binding seeds and drains, created on first use.
    pub async fn spool(&self) -> Result<Arc<Spool>> {
        self.spool
            .get_or_try_init(|| async {
                Spool::new(&self.spool_dir, ObjectVersion::etag("spool-0"))
                    .map(Arc::new)
                    .map_err(|e| storage_failed("create the storage spool", e))
            })
            .await
            .cloned()
    }

    /// The spool, if the binding has used one.
    pub fn existing_spool(&self) -> Option<Arc<Spool>> {
        self.spool.get().cloned()
    }

    /// The current status.
    pub fn status(&self) -> StorageStatus {
        self.status.lock().expect("not poisoned").clone()
    }

    fn update(&self, change: impl FnOnce(&mut StorageStatus)) -> StorageStatus {
        let mut status = self.status.lock().expect("not poisoned");
        change(&mut status);
        status.clone()
    }

    /// Restores the key's tree, opens the image gate and starts replicating. Once per boot.
    pub async fn attach(
        self: &Arc<Self>,
        request: AttachRequest,
        image: &ImageProcess,
        identity: ExecIdentity,
    ) -> Result<StorageStatus> {
        alien_sandbox_storage::validate_key(&request.key)
            .map_err(|e| storage_failed("attach persistent storage", e))?;
        let mut live = self.live.lock().await;
        if live.is_some() {
            return Err(AlienError::new(ErrorData::RequestInvalid {
                reason: "persistent storage is already attached".to_string(),
            }));
        }
        let path = std::path::Path::new(&self.config.path);
        if !path.exists() {
            std::fs::create_dir_all(path)
                .into_alien_error()
                .context(failed(
                    "create the storage directory",
                    self.config.path.clone(),
                ))?;
            std::os::unix::fs::chown(path, Some(identity.uid), Some(identity.gid))
                .into_alien_error()
                .context(failed(
                    "own the storage directory",
                    self.config.path.clone(),
                ))?;
        }
        let open = || {
            tree::open_root(path).into_alien_error().context(failed(
                "open the storage directory",
                self.config.path.clone(),
            ))
        };
        let (store, head_version, grant_expires_at) = match request.grant {
            Some(grant) => {
                let expires_at = grant.expires_at();
                (
                    Store::Grant(Arc::new(GrantStore::new(grant))),
                    request.head_version,
                    Some(expires_at),
                )
            }
            None => {
                let spool = self.spool().await?;
                let version = spool.head_version();
                (Store::Spool(spool), version, None)
            }
        };
        let manifest = match &request.commit {
            Some(commit) => Some(
                read_manifest(store.object_store(), commit)
                    .await
                    .map_err(|e| storage_failed("read the key's manifest", e))?,
            ),
            None => None,
        };
        tree::prune(&open()?, manifest.as_ref(), &self.config.exclude)
            .into_alien_error()
            .context(failed(
                "clear the storage directory",
                self.config.path.clone(),
            ))?;
        if let Some(manifest) = &manifest {
            // SAFETY: geteuid has no preconditions.
            let root = unsafe { libc::geteuid() } == 0;
            restore(
                store.object_store(),
                manifest,
                &mut DirSink::new(open()?, root),
            )
            .await
            .map_err(|e| storage_failed("restore the key's tree", e))?;
        }
        if let Store::Spool(spool) = &store {
            spool.clear_seeded();
        }
        let head = manifest.as_ref().map(|m| m.seq);
        let replicator = Replicator::new(
            request.key.clone(),
            open()?,
            self.config.exclude.clone(),
            self.staging.clone(),
            Freezer::guest(),
            Committer::new(
                request.writer,
                head_version,
                manifest,
                self.config.max_size_bytes,
            ),
        );
        *live = Some(Live { replicator, store });
        drop(live);
        let status = self.update(|s| {
            s.key = Some(request.key);
            s.state = ReplicationState::Replicating;
            s.restored_from = request.commit;
            s.head = head;
            s.last_synced_at = Some(Utc::now());
            s.grant_expires_at = grant_expires_at;
            s.grant_issued_at = grant_expires_at.map(|_| Utc::now());
        });
        image.restored()?;
        let this = Arc::clone(self);
        tokio::spawn(async move { this.replicate().await });
        Ok(status)
    }

    /// Replaces the grant before it expires.
    pub async fn renew(&self, request: RenewRequest) -> Result<()> {
        let live = self.live.lock().await;
        let live = live.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::RequestInvalid {
                reason: "persistent storage is not attached".to_string(),
            })
        })?;
        let Store::Grant(store) = &live.store else {
            return Err(AlienError::new(ErrorData::RequestInvalid {
                reason: "this key is pulled by the binding and holds no grant".to_string(),
            }));
        };
        let expires_at = request.grant.expires_at();
        store.renew(request.grant);
        self.update(|s| {
            s.grant_expires_at = Some(expires_at);
            s.grant_issued_at = Some(Utc::now());
            if s.state == ReplicationState::Denied {
                s.state = ReplicationState::Behind;
            }
        });
        Ok(())
    }

    /// Commits everything written so far and returns once it is durable (or why it is not).
    pub async fn sync(&self) -> Result<StorageStatus> {
        self.commit_now(true).await
    }

    async fn commit_now(&self, force: bool) -> Result<StorageStatus> {
        let mut live = self.live.lock().await;
        let Some(live) = live.as_mut() else {
            return Err(AlienError::new(ErrorData::RequestInvalid {
                reason: "persistent storage is not attached".to_string(),
            }));
        };
        if live.replicator.is_superseded() {
            return Ok(self.status());
        }
        if !force
            && !tokio::task::block_in_place(|| live.replicator.dirty())
                .into_alien_error()
                .context(failed("walk the storage directory", String::new()))?
        {
            return Ok(self.status());
        }
        match live.replicator.commit(live.store.object_store()).await {
            Ok(outcome) => {
                let head = live.replicator.head().map(|m| m.seq);
                Ok(self.update(|s| {
                    s.state = ReplicationState::Replicating;
                    s.last_synced_at = Some(Utc::now());
                    s.last_error = None;
                    s.head = head;
                    if let CommitOutcome::Committed { commit, .. } = outcome {
                        tracing::debug!(seq = commit.seq, "persistent storage committed");
                    }
                }))
            }
            Err(error) => {
                let stopped = live.replicator.is_superseded();
                let denied = live.store.denied();
                let message = error.to_string();
                let status = self.update(|s| {
                    s.state = if stopped {
                        ReplicationState::Stopped
                    } else if denied {
                        ReplicationState::Denied
                    } else {
                        ReplicationState::Behind
                    };
                    s.last_error = Some(message.clone());
                });
                tracing::warn!(error = %message, stopped, "persistent storage commit failed");
                if force {
                    return Err(storage_failed("sync persistent storage", error));
                }
                Ok(status)
            }
        }
    }

    async fn replicate(&self) {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            match self.commit_now(false).await {
                Ok(status) if status.state == ReplicationState::Stopped => return,
                Ok(_) => {}
                Err(error) => tracing::warn!(%error, "persistent storage check failed"),
            }
        }
    }
}
