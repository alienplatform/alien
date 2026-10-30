//! Storage-owned notification tasks, independent of Worker lifetimes.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use alien_bindings::traits::{MessagePayload, Queue};
use alien_core::{Storage, StorageEvent, StorageEventType, StorageNotification};
use alien_error::{AlienError, Context, IntoAlienError};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;
use tracing::error;

use crate::error::{ErrorData, Result};
use crate::storage_watcher::StorageWatcher;
use crate::LocalQueueManager;

#[derive(Debug, Default)]
pub(crate) struct NotificationServices {
    services: HashMap<String, NotificationService>,
    closed: bool,
}

#[derive(Debug)]
struct NotificationService {
    rules: Vec<StorageNotification>,
    stop: CancellationToken,
    task: Option<JoinHandle<Result<()>>>,
}

impl Drop for NotificationService {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

impl NotificationService {
    async fn join(mut self) -> Result<()> {
        self.stop.cancel();
        if let Some(task) = self.task.take() {
            task.await
                .into_alien_error()
                .context(notification_error("Join notification service"))??;
        }
        Ok(())
    }
}

impl NotificationServices {
    pub async fn ensure(
        &mut self,
        storage: &Storage,
        root: &Path,
        state: &Path,
        queues: &LocalQueueManager,
    ) -> Result<()> {
        if self.closed {
            return Err(AlienError::new(notification_error(
                "Storage notification owner has shut down",
            )));
        }
        if self.services.get(&storage.id).is_some_and(|service| {
            service.rules == storage.notifications
                && service
                    .task
                    .as_ref()
                    .is_some_and(|task| !task.is_finished())
        }) {
            return Ok(());
        }
        self.stop(&storage.id).await?;
        if storage.notifications.is_empty() {
            return Ok(());
        }

        // Resolve every queue and load every cursor before spawning any delivery.
        // Missing dependencies or invalid checkpoints fail the controller immediately.
        // Cursors are keyed by rule content: a reordered rule keeps its cursor, and a new
        // or edited rule starts from the objects that exist now, as it would in the cloud.
        // Removing every rule removes the cursors too (see `remove_notifications`).
        // Identical rules are separate destinations, so each occurrence keeps its own cursor.
        let mut cursor_files: Vec<String> = Vec::new();
        for (index, rule) in storage.notifications.iter().enumerate() {
            let occurrence = storage.notifications[..index]
                .iter()
                .filter(|earlier| *earlier == rule)
                .count();
            cursor_files.push(rule_cursor_file(rule, occurrence)?);
        }
        remove_stale_cursors(state, &cursor_files).await?;
        let mut deliveries = Vec::new();
        for (rule, cursor_file) in storage.notifications.iter().zip(&cursor_files) {
            let queue = queues.load_queue(&rule.queue.id).await?;
            let watcher = StorageWatcher::open(root, &state.join(cursor_file)).await?;
            deliveries.push((rule.clone(), queue, watcher));
        }
        let stop = CancellationToken::new();
        let service_stop = stop.clone();
        let bucket = storage.id.clone();
        let task = tokio::spawn(async move {
            let mut tasks = JoinSet::new();
            for (rule, queue, watcher) in deliveries {
                let stop = service_stop.clone();
                let bucket = bucket.clone();
                tasks.spawn(async move {
                    watcher.run(&stop, |change| {
                        let queue = Arc::clone(&queue);
                        let queue_id = rule.queue.id.clone();
                        let bucket = bucket.clone();
                        let version = change.version.filter(|_| rule.matches_key(&change.key));
                        async move {
                            let Some(version) = version else {
                                return Ok(true);
                            };
                            let event = StorageEvent {
                                event_type: StorageEventType::Created,
                                bucket_name: bucket,
                                object_key: change.key,
                                timestamp: DateTime::<Utc>::from(version.modified),
                                size: Some(version.size),
                                etag: None, content_type: None, metadata: HashMap::new(),
                                copy_source: None, previous_tier: None, current_tier: None,
                                region: None, version_id: None,
                            };
                            let payload = serde_json::to_value(event).into_alien_error()
                                .context(notification_error("Serialize storage notification"))?;
                            match queue.send(&queue_id, MessagePayload::Json(payload)).await {
                                Ok(()) => Ok(true),
                                Err(error) => {
                                    error!(%error, queue = %queue_id, "Storage notification failed; cursor remains pending");
                                    Ok(false)
                                }
                            }
                        }
                    }).await
                });
            }
            while let Some(joined) = tasks.join_next().await {
                joined
                    .into_alien_error()
                    .context(notification_error("Join storage rule"))??;
            }
            Ok(())
        });
        self.services.insert(
            storage.id.clone(),
            NotificationService {
                rules: storage.notifications.clone(),
                stop,
                task: Some(task),
            },
        );
        Ok(())
    }

    pub async fn stop(&mut self, id: &str) -> Result<()> {
        if let Some(service) = self.services.remove(id) {
            service.join().await?;
        }
        Ok(())
    }

    pub async fn close(&mut self) -> Result<()> {
        self.closed = true;
        let mut failure = None;
        for (_, service) in self.services.drain() {
            if let Err(error) = service.join().await {
                if failure.is_none() {
                    failure = Some(error);
                }
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

fn rule_cursor_file(rule: &StorageNotification, occurrence: usize) -> Result<String> {
    let rule = serde_json::to_vec(rule)
        .into_alien_error()
        .context(notification_error("Serialize storage notification rule"))?;
    let digest = Sha256::digest(&rule);
    let name: String = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(format!("rule-{name}-{occurrence}.json"))
}

/// Cursors of removed or edited rules would otherwise accumulate forever.
async fn remove_stale_cursors(state: &Path, keep: &[String]) -> Result<()> {
    let mut entries = match tokio::fs::read_dir(state).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .into_alien_error()
                .context(notification_error("Read storage notification cursors"))
        }
    };
    while let Some(entry) = entries
        .next_entry()
        .await
        .into_alien_error()
        .context(notification_error("Read storage notification cursors"))?
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("rule-") && name.ends_with(".json") && !keep.contains(&name) {
            tokio::fs::remove_file(entry.path())
                .await
                .into_alien_error()
                .context(notification_error(
                    "Remove stale storage notification cursor",
                ))?;
        }
    }
    Ok(())
}

fn notification_error(reason: &str) -> ErrorData {
    ErrorData::TriggerServiceError {
        trigger_type: "storage notification".into(),
        trigger_id: String::new(),
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use alien_bindings::providers::storage::local::LocalStorage;
    use alien_core::ResourceRef;
    use bytes::Bytes;
    use object_store::{path::Path as ObjectPath, ObjectStore};

    use super::*;
    use crate::LocalStorageManager;

    fn storage() -> Storage {
        Storage::new("objects".into())
            .notifications(vec![
                StorageNotification::builder()
                    .queue(ResourceRef::new(alien_core::Queue::RESOURCE_TYPE, "jobs"))
                    .prefix("in/".into())
                    .suffix(".json".into())
                    .build(),
                StorageNotification::builder()
                    .queue(ResourceRef::new(alien_core::Queue::RESOURCE_TYPE, "jobs"))
                    .prefix("other/".into())
                    .suffix(".data".into())
                    .build(),
            ])
            .build()
    }

    async fn put(store: &LocalStorage, key: &str, data: &'static str) {
        store
            .put(
                &ObjectPath::parse(key).unwrap(),
                Bytes::from_static(data.as_bytes()).into(),
            )
            .await
            .unwrap();
    }

    async fn collect(queues: &LocalQueueManager, expected: usize) -> BTreeMap<String, u64> {
        let queue = queues.load_queue("jobs").await.unwrap();
        let mut events = BTreeMap::new();
        tokio::time::timeout(Duration::from_secs(10), async {
            while events.len() < expected {
                for message in queue.receive("jobs", 10).await.unwrap() {
                    let payload = match message.payload {
                        MessagePayload::Json(value) => value,
                        MessagePayload::Text(text) => serde_json::from_str(&text).unwrap(),
                    };
                    let event: StorageEvent = serde_json::from_value(payload).unwrap();
                    assert_eq!(event.event_type, StorageEventType::Created);
                    assert_eq!(event.bucket_name, "objects");
                    assert!(event.timestamp <= Utc::now());
                    assert!(event.etag.is_none());
                    assert!(
                        events
                            .insert(event.object_key, event.size.unwrap())
                            .is_none(),
                        "no duplicate delivery"
                    );
                    queue.ack("jobs", &message.receipt_handle).await.unwrap();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("all matching events");
        events
    }

    async fn assert_empty(queues: &LocalQueueManager) {
        let queue = queues.load_queue("jobs").await.unwrap();
        assert!(queue.receive("jobs", 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn notifications_filter_restart_update_and_delete_with_real_bindings() {
        let directory = tempfile::tempdir().unwrap();
        let manager = LocalStorageManager::new(directory.path().to_owned());
        let queues = LocalQueueManager::new(directory.path().to_owned());
        queues.create_queue("jobs").await.unwrap();
        let root = manager.create_storage("objects").await.unwrap();
        let store = LocalStorage::new_from_url(&format!("file://{}/", root.display())).unwrap();
        let mut config = storage();
        manager
            .ensure_notifications(&config, &queues)
            .await
            .unwrap();
        for key in [
            "in/a.json",
            "in/quote\"é*%.json",
            "other/b.data",
            "in/no.txt",
            "other/no.json",
            "IN/no.json",
        ] {
            put(&store, key, "payload").await;
        }
        let got = collect(&queues, 3).await;
        assert_eq!(
            got,
            BTreeMap::from([
                ("in/a.json".into(), 7),
                ("in/quote\"é*%.json".into(), 7),
                ("other/b.data".into(), 7),
            ])
        );
        // No-op reconciliation must reuse tasks, not restart or replay cursors.
        manager
            .ensure_notifications(&config, &queues)
            .await
            .unwrap();
        manager.shutdown_notifications().await.unwrap();
        assert_empty(&queues).await;
        queues.close().await;
        drop(manager);
        drop(queues);

        put(&store, "in/offline.json", "offline").await;
        put(&store, "in/a.json", "overwritten").await;
        put(&store, "other/offline.txt", "excluded").await;
        let manager = LocalStorageManager::new(directory.path().to_owned());
        let queues = LocalQueueManager::new(directory.path().to_owned());
        manager
            .ensure_notifications(&config, &queues)
            .await
            .unwrap();
        assert_eq!(
            collect(&queues, 2).await,
            BTreeMap::from([("in/a.json".into(), 11), ("in/offline.json".into(), 7),])
        );
        // Editing the rule stops old tasks before installing the new filters.
        config.notifications[0].prefix = Some("changed/".into());
        manager
            .ensure_notifications(&config, &queues)
            .await
            .unwrap();
        put(&store, "changed/new.json", "new").await;
        put(&store, "in/old.json", "old").await;
        put(&store, "other/new.data", "other").await;
        assert_eq!(
            collect(&queues, 2).await,
            BTreeMap::from([("changed/new.json".into(), 3), ("other/new.data".into(), 5),])
        );
        manager.stop_notifications("objects").await.unwrap();
        assert_empty(&queues).await;
        put(&store, "changed/retained.json", "retained").await;
        assert_empty(&queues).await;
        manager.delete_storage("objects").await.unwrap();
        assert!(!root.exists());
        assert!(!directory
            .path()
            .join("storage-notifications/objects")
            .exists());
        queues.delete_queue("jobs").await.unwrap();
        assert!(!queues.queue_exists("jobs"));
    }

    #[tokio::test]
    async fn identical_rules_keep_their_own_cursor_and_removed_rules_start_over() {
        let directory = tempfile::tempdir().unwrap();
        let manager = LocalStorageManager::new(directory.path().to_owned());
        let queues = LocalQueueManager::new(directory.path().to_owned());
        queues.create_queue("jobs").await.unwrap();
        let root = manager.create_storage("objects").await.unwrap();
        let store = LocalStorage::new_from_url(&format!("file://{}/", root.display())).unwrap();
        let rule = storage().notifications[0].clone();
        let mut config = Storage::new("objects".into())
            .notifications(vec![rule.clone(), rule.clone()])
            .build();
        manager
            .ensure_notifications(&config, &queues)
            .await
            .unwrap();
        put(&store, "in/a.json", "payload").await;

        // Each identical rule is its own destination: two deliveries, two durable cursors.
        let queue = queues.load_queue("jobs").await.unwrap();
        let mut deliveries = Vec::new();
        tokio::time::timeout(Duration::from_secs(10), async {
            while deliveries.len() < 2 {
                for message in queue.receive("jobs", 10).await.unwrap() {
                    let MessagePayload::Json(payload) = message.payload else {
                        panic!("storage events are JSON");
                    };
                    let event: StorageEvent = serde_json::from_value(payload).unwrap();
                    deliveries.push(event.object_key);
                    queue.ack("jobs", &message.receipt_handle).await.unwrap();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("one delivery per identical rule");
        assert_eq!(deliveries, ["in/a.json", "in/a.json"]);
        let cursors = directory.path().join("storage-notifications/objects");
        let mut names: Vec<_> = std::fs::read_dir(&cursors)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(
            names.len(),
            2,
            "each identical rule checkpoints separately: {names:?}"
        );
        assert_ne!(names[0], names[1]);

        // Removing every rule forgets the cursors, so a rule added later only sees later
        // writes, as a new cloud rule does.
        config.notifications.clear();
        manager.remove_notifications("objects").await.unwrap();
        assert!(!cursors.exists());
        put(&store, "in/while-removed.json", "payload").await;
        config.notifications = vec![rule];
        manager
            .ensure_notifications(&config, &queues)
            .await
            .unwrap();
        put(&store, "in/after.json", "payload").await;
        assert_eq!(
            collect(&queues, 1).await,
            BTreeMap::from([("in/after.json".into(), 7)])
        );
        manager.shutdown_notifications().await.unwrap();
        assert_empty(&queues).await;
        queues.close().await;
    }

    #[tokio::test]
    async fn notification_destination_must_exist_before_start() {
        let directory = tempfile::tempdir().unwrap();
        let manager = LocalStorageManager::new(directory.path().to_owned());
        let queues = LocalQueueManager::new(directory.path().to_owned());
        manager.create_storage("objects").await.unwrap();
        let error = manager
            .ensure_notifications(&storage(), &queues)
            .await
            .unwrap_err();
        assert_eq!(error.code, "SERVICE_RESOURCE_NOT_FOUND");
        assert!(!directory.path().join("storage-notifications").exists());
    }
}
