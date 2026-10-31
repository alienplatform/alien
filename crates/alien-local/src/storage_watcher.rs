//! Reconcile committed objects, persisting each destination's acknowledged versions.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use alien_error::{AlienError, Context, IntoAlienError};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{error, warn};

use crate::error::{ErrorData, Result};

/// Modification time and size identify the latest committed object version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObjectVersion {
    pub modified: SystemTime,
    pub size: u64,
}

pub(crate) struct StorageChange {
    pub key: String,
    pub version: Option<ObjectVersion>,
}

impl StorageChange {
    pub fn event_type(&self) -> &'static str {
        if self.version.is_some() {
            "created"
        } else {
            "deleted"
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Cursor {
    /// None means observed, but creation has not yet been acknowledged.
    objects: HashMap<String, Option<ObjectVersion>>,
}

/// A cursor belongs to one destination. Successful delivery to another destination
/// must never acknowledge this one's pending objects.
///
/// A destination without a cursor starts from the objects that exist when it opens, as
/// a new cloud notification or trigger only sees later writes. From then on the cursor
/// is durable, so a restart delivers what changed while stopped and replays nothing.
pub(crate) struct StorageWatcher {
    root: PathBuf,
    state_path: PathBuf,
    cursor: Cursor,
    _watcher: RecommendedWatcher,
    hints: mpsc::Receiver<()>,
}

impl StorageWatcher {
    pub async fn open(root: &Path, state_path: &Path) -> Result<Self> {
        let root = root
            .canonicalize()
            .into_alien_error()
            .context(storage_error(root, "Canonicalize storage root"))?;
        let cursor = match tokio::fs::read(state_path).await {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .into_alien_error()
                .context(storage_error(state_path, "Read storage cursor"))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let snapshot_root = root.clone();
                let existing =
                    tokio::task::spawn_blocking(move || storage_snapshot(&snapshot_root))
                        .await
                        .into_alien_error()
                        .context(storage_error(&root, "Join storage scan"))??;
                let cursor = Cursor {
                    objects: existing
                        .into_iter()
                        .map(|(key, version)| (key, Some(version)))
                        .collect(),
                };
                save_cursor(&cursor, state_path).await?;
                cursor
            }
            Err(error) => {
                return Err(error)
                    .into_alien_error()
                    .context(storage_error(state_path, "Read storage cursor"))
            }
        };
        let (sender, hints) = mpsc::channel(1);
        let mut watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
            match result {
                Ok(event)
                    if matches!(
                        event.kind,
                        EventKind::Create(_)
                            | EventKind::Remove(_)
                            | EventKind::Modify(
                                notify::event::ModifyKind::Name(_)
                                    | notify::event::ModifyKind::Data(_)
                            )
                    ) =>
                {
                    // Hints coalesce. The next scan reads all committed objects.
                    let _ = sender.try_send(());
                }
                Ok(_) => {}
                Err(error) => {
                    warn!(%error, "Storage watch hint failed; safety scan remains active")
                }
            }
        })
        .into_alien_error()
        .context(storage_error(&root, "Create storage watcher"))?;
        watcher
            .watch(&root, RecursiveMode::Recursive)
            .into_alien_error()
            .context(storage_error(&root, "Watch storage root"))?;
        Ok(Self {
            root,
            state_path: state_path.to_owned(),
            cursor,
            _watcher: watcher,
            hints,
        })
    }

    pub async fn run<F, Fut>(mut self, stop: &CancellationToken, mut deliver: F) -> Result<()>
    where
        F: FnMut(StorageChange) -> Fut,
        Fut: Future<Output = Result<bool>>,
    {
        // Watch before the initial scan so writes during startup also wake reconciliation.
        let mut scan = tokio::time::interval(Duration::from_secs(30));
        scan.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = stop.cancelled() => return Ok(()),
                _ = scan.tick() => {},
                _ = self.hints.recv() => {},
            }
            let _ = self.hints.try_recv();
            scan.reset();
            let root = self.root.clone();
            let current = match tokio::task::spawn_blocking(move || storage_snapshot(&root))
                .await
                .into_alien_error()
                .context(storage_error(&self.root, "Join storage scan"))?
            {
                Ok(current) => current,
                Err(error) => {
                    error!(%error, "Storage scan failed; pending deliveries retained");
                    scan.reset_after(Duration::from_secs(2));
                    continue;
                }
            };
            let mut observed = false;
            for key in current.keys() {
                if !self.cursor.objects.contains_key(key) {
                    self.cursor.objects.insert(key.clone(), None);
                    observed = true;
                }
            }
            // Preserve pending creation across restart, including a later deletion.
            if observed {
                save_cursor(&self.cursor, &self.state_path).await?;
            }
            let changes: Vec<_> = self
                .cursor
                .objects
                .iter()
                .filter_map(|(key, delivered)| match current.get(key) {
                    Some(version) if *delivered == Some(*version) => None,
                    version => Some(StorageChange {
                        key: key.clone(),
                        version: version.copied(),
                    }),
                })
                .collect();
            // The cursor advances only after the destination acknowledged, and is
            // checkpointed once per pass rather than once per object. A crash before the
            // checkpoint redelivers this pass (at-least-once); it can never lose an event.
            let mut acknowledged = false;
            for change in changes {
                let key = change.key.clone();
                let version = change.version;
                let success = tokio::select! {
                    // If completion and shutdown coincide, persist the completion first.
                    biased;
                    result = deliver(change) => result?,
                    _ = stop.cancelled() => break,
                };
                if !success {
                    scan.reset_after(Duration::from_secs(2));
                    continue;
                }
                match version {
                    Some(version) => {
                        self.cursor.objects.insert(key, Some(version));
                    }
                    None => {
                        self.cursor.objects.remove(&key);
                    }
                }
                acknowledged = true;
            }
            if acknowledged {
                save_cursor(&self.cursor, &self.state_path).await?;
            }
            if stop.is_cancelled() {
                return Ok(());
            }
        }
    }
}

async fn save_cursor(cursor: &Cursor, state_path: &Path) -> Result<()> {
    let bytes = serde_json::to_vec(cursor)
        .into_alien_error()
        .context(storage_error(state_path, "Serialize storage cursor"))?;
    let path = state_path.to_owned();
    tokio::task::spawn_blocking(move || {
        let parent = path.parent().ok_or_else(|| {
            AlienError::new(storage_error(&path, "Cursor has no parent directory"))
        })?;
        std::fs::create_dir_all(parent)
            .into_alien_error()
            .context(storage_error(&path, "Create cursor directory"))?;
        let mut file = tempfile::NamedTempFile::new_in(parent)
            .into_alien_error()
            .context(storage_error(&path, "Create cursor checkpoint"))?;
        std::io::Write::write_all(&mut file, &bytes)
            .into_alien_error()
            .context(storage_error(&path, "Write cursor checkpoint"))?;
        file.as_file()
            .sync_all()
            .into_alien_error()
            .context(storage_error(&path, "Sync cursor checkpoint"))?;
        file.persist(&path)
            .into_alien_error()
            .context(storage_error(&path, "Publish cursor checkpoint"))?;
        #[cfg(unix)]
        std::fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .into_alien_error()
            .context(storage_error(&path, "Sync cursor directory"))?;
        Ok(())
    })
    .await
    .into_alien_error()
    .context(storage_error(state_path, "Join cursor checkpoint"))?
}

fn storage_error(path: &Path, reason: &str) -> ErrorData {
    ErrorData::TriggerServiceError {
        trigger_type: "storage".into(),
        trigger_id: path.display().to_string(),
        reason: reason.into(),
    }
}
fn storage_snapshot(root: &Path) -> Result<HashMap<String, ObjectVersion>> {
    let mut objects = HashMap::new();
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error)
                    .into_alien_error()
                    .context(ErrorData::LocalDirectoryError {
                        path: directory.display().to_string(),
                        operation: "scan".to_string(),
                        reason: "Failed to reconcile storage objects".to_string(),
                    })
            }
        };
        for entry in entries {
            let entry = entry
                .into_alien_error()
                .context(ErrorData::LocalDirectoryError {
                    path: directory.display().to_string(),
                    operation: "scan".to_string(),
                    reason: "Failed to read storage entry".to_string(),
                })?;
            let path = entry.path();
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error)
                        .into_alien_error()
                        .context(ErrorData::LocalDirectoryError {
                            path: path.display().to_string(),
                            operation: "stat".to_string(),
                            reason: "Failed to inspect storage object".to_string(),
                        })
                }
            };
            if metadata.is_dir() {
                directories.push(path);
            } else if metadata.is_file() {
                // object_store stages key#N, then renames to the committed key.
                if path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.rsplit_once('#').is_some_and(|(_, suffix)| {
                            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
                        })
                    })
                {
                    continue;
                }
                let modified = metadata.modified().into_alien_error().context(
                    ErrorData::LocalDirectoryError {
                        path: path.display().to_string(),
                        operation: "stat".to_string(),
                        reason: "Failed to read object modification time".to_string(),
                    },
                )?;
                let key = path
                    .strip_prefix(root)
                    .into_alien_error()
                    .context(storage_error(root, "Object lies outside storage root"))?
                    .to_str()
                    .ok_or_else(|| {
                        AlienError::new(storage_error(&path, "Object key is not valid UTF-8"))
                    })?
                    .replace(std::path::MAIN_SEPARATOR, "/");
                objects.insert(
                    key,
                    ObjectVersion {
                        modified,
                        size: metadata.len(),
                    },
                );
            }
        }
    }
    Ok(objects)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn new_cursor_skips_existing_objects_and_pending_creation_survives_restart() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("objects");
        let state = directory.path().join("cursor.json");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("existing"), b"before").unwrap();
        let watcher = StorageWatcher::open(&root, &state).await.unwrap();
        std::fs::write(root.join("pending"), b"payload").unwrap();
        let stop = CancellationToken::new();
        let task_stop = stop.clone();
        let (sender, mut receiver) = mpsc::channel(1);
        let task = tokio::spawn(async move {
            watcher
                .run(&task_stop, |change| {
                    let sender = sender.clone();
                    async move {
                        sender.send(change).await.unwrap();
                        Ok(false)
                    }
                })
                .await
        });
        let created = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(created.key, "pending");
        assert_eq!(created.version.unwrap().size, 7);
        stop.cancel();
        task.await.unwrap().unwrap();

        std::fs::remove_file(root.join("pending")).unwrap();
        let watcher = StorageWatcher::open(&root, &state).await.unwrap();
        let stop = CancellationToken::new();
        let task_stop = stop.clone();
        let (sender, mut receiver) = mpsc::channel(1);
        let task = tokio::spawn(async move {
            watcher
                .run(&task_stop, |change| {
                    let sender = sender.clone();
                    async move {
                        sender.send(change).await.unwrap();
                        Ok(true)
                    }
                })
                .await
        });
        let deleted = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(deleted.key, "pending");
        assert!(deleted.version.is_none());
        stop.cancel();
        task.await.unwrap().unwrap();
        let recovered = StorageWatcher::open(&root, &state).await.unwrap();
        assert_eq!(
            recovered.cursor.objects.keys().collect::<Vec<_>>(),
            vec!["existing"],
            "completed deletion is durable and the baseline object was never pending"
        );
    }

    #[tokio::test]
    async fn corrupt_cursor_fails_instead_of_replaying_the_bucket() {
        let directory = tempfile::tempdir().unwrap();
        let state = directory.path().join("cursor.json");
        std::fs::write(&state, b"{broken").unwrap();
        let error = StorageWatcher::open(directory.path(), &state)
            .await
            .err()
            .expect("corrupt cursor must fail");
        assert_eq!(error.code, "TRIGGER_SERVICE_ERROR");
        assert!(error.to_string().contains("Read storage cursor"));
    }
}
