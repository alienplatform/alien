//! Persistent storage as the agent serves it: configured at boot, attached once by the binding
//! (claim already written, grant minted), then replicated until the sandbox ends or another
//! sandbox claims the key.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use alien_error::{AlienError, Context, IntoAlienError};
use alien_sandbox_storage::{
    read_manifest, restore, CommitOutcome, Committer, ErrorData as StorageError, ObjectName,
    ObjectVersion, StorageGrant,
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
pub const ENV_PERSISTENT_STORAGE: &str = alien_core::ENV_ALIEN_SANDBOX_PERSISTENT_STORAGE;

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
    /// HEAD's version after the claim.
    pub head_version: ObjectVersion,
    /// The commit HEAD named at the claim, restored before the image starts.
    #[serde(default)]
    pub commit: Option<ObjectName>,
    /// Rights to the key.
    pub grant: StorageGrant,
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
    /// Why the state is `behind` or `stopped`.
    pub last_error: Option<String>,
}

struct Live {
    replicator: Replicator,
    store: Arc<GrantStore>,
}

/// The agent's persistent storage.
pub struct PersistentStorage {
    config: StorageConfig,
    staging: std::path::PathBuf,
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

    /// Configured, staging commits in `staging` (agent-only; tests use a temporary one).
    pub fn with_staging(config: StorageConfig, staging: std::path::PathBuf) -> Self {
        Self {
            config,
            staging,
            live: tokio::sync::Mutex::new(None),
            status: Mutex::new(StorageStatus {
                key: None,
                state: ReplicationState::AwaitingRestore,
                last_synced_at: None,
                restored_from: None,
                head: None,
                last_error: None,
            }),
        }
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
        let store = Arc::new(
            GrantStore::new(request.grant).map_err(|e| storage_failed("use the key's grant", e))?,
        );
        let manifest = match &request.commit {
            Some(commit) => Some(
                read_manifest(store.as_ref(), commit)
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
            restore(store.as_ref(), manifest, &mut DirSink::new(open()?, root))
                .await
                .map_err(|e| storage_failed("restore the key's tree", e))?;
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
                request.head_version,
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
        live.store
            .renew(request.grant)
            .map_err(|e| storage_failed("renew the key's grant", e))?;
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
        match live.replicator.commit(live.store.as_ref()).await {
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
                let message = error.to_string();
                let status = self.update(|s| {
                    s.state = if stopped {
                        ReplicationState::Stopped
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
