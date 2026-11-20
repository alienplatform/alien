use crate::error::{ErrorData, Result};
use alien_bindings::{
    providers::queue::{local::LocalQueue, local_http::QueueStats},
    traits::{Binding, MessagePayload, Queue, QueueAckResult, QueueMessage, QueueSendResult},
};
use alien_core::bindings::{BindingValue, LocalHttpQueueBinding, QueueBinding};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
};
use tokio::sync::Mutex;

/// The manager owns each queue handle. Clients never receive its filesystem path.
#[derive(Debug)]
pub struct LocalQueueManager {
    state_dir: PathBuf,
    endpoint: RwLock<Option<String>>,
    queues: Mutex<HashMap<String, Arc<ManagedQueue>>>,
    closed: AtomicBool,
}

/// A revocable handle: deletion waits for in-flight operations and closes the store
/// before unlinking it. Existing trigger handles cannot write into an unlinked database.
#[derive(Debug)]
pub struct ManagedQueue {
    queue: Mutex<Option<LocalQueue>>,
    /// Where messages that failed `max_attempts` deliveries move, when the queue has a
    /// dead-letter setting.
    dead_letter: RwLock<Option<DeadLetterTarget>>,
}

#[derive(Debug, Clone)]
struct DeadLetterTarget {
    queue: Arc<ManagedQueue>,
    max_attempts: u32,
}

/// The dead-letter setting persisted beside the queue data, so a restart keeps it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct DeadLetterFile {
    queue: String,
    max_attempts: u32,
}
fn unavailable() -> alien_bindings::error::ErrorData {
    alien_bindings::error::ErrorData::config_invalid("queue", "Queue has been deleted")
}
impl ManagedQueue {
    /// Moves messages that were delivered `max_attempts` times to the dead-letter target, before
    /// the next delivery. A failure leaves them in place to be retried on the next receive.
    async fn move_dead_letters(&self) {
        let Some(target) = self
            .dead_letter
            .read()
            .expect("dead-letter setting poisoned")
            .clone()
        else {
            return;
        };
        // Both stores are locked in one fixed order (by address), whichever is the source. A
        // setting that changes from A->B to B->A while receives wait would otherwise leave one
        // holding A for B and another holding B for A.
        let mine = self as *const ManagedQueue as usize;
        let theirs = Arc::as_ptr(&target.queue) as usize;
        if mine == theirs {
            return;
        }
        let (first, second) = if mine < theirs {
            (&self.queue, &target.queue.queue)
        } else {
            (&target.queue.queue, &self.queue)
        };
        let first = first.lock().await;
        let second = second.lock().await;
        let (source, destination) = if mine < theirs {
            (first, second)
        } else {
            (second, first)
        };
        // The setting may have changed while waiting for the locks: leave it to the next receive.
        let unchanged = self
            .dead_letter
            .read()
            .expect("dead-letter setting poisoned")
            .as_ref()
            .is_some_and(|now| {
                Arc::ptr_eq(&now.queue, &target.queue) && now.max_attempts == target.max_attempts
            });
        if !unchanged {
            return;
        }
        let Some(source) = source.as_ref() else {
            return;
        };
        let Some(destination) = destination.as_ref() else {
            tracing::warn!("dead-letter target queue is unavailable; messages stay in place");
            return;
        };
        loop {
            match source
                .move_dead_letters(destination, target.max_attempts, 100)
                .await
            {
                Ok(100) => continue,
                Ok(_) => break,
                Err(error) => {
                    tracing::warn!(%error, "moving dead letters failed; retrying on the next receive");
                    break;
                }
            }
        }
    }

    pub async fn stats(&self) -> alien_bindings::error::Result<QueueStats> {
        self.queue
            .lock()
            .await
            .as_ref()
            .ok_or_else(|| AlienError::new(unavailable()))?
            .stats()
            .await
    }
}
impl Binding for ManagedQueue {}
#[async_trait]
impl Queue for ManagedQueue {
    async fn send(&self, name: &str, message: MessagePayload) -> alien_bindings::error::Result<()> {
        self.queue
            .lock()
            .await
            .as_ref()
            .ok_or_else(|| AlienError::new(unavailable()))?
            .send(name, message)
            .await
    }
    async fn send_batch(
        &self,
        name: &str,
        messages: Vec<MessagePayload>,
    ) -> alien_bindings::error::Result<Vec<QueueSendResult>> {
        self.queue
            .lock()
            .await
            .as_ref()
            .ok_or_else(|| AlienError::new(unavailable()))?
            .send_batch(name, messages)
            .await
    }
    async fn receive(
        &self,
        name: &str,
        max: usize,
    ) -> alien_bindings::error::Result<Vec<QueueMessage>> {
        self.move_dead_letters().await;
        self.queue
            .lock()
            .await
            .as_ref()
            .ok_or_else(|| AlienError::new(unavailable()))?
            .receive(name, max)
            .await
    }
    async fn ack(&self, name: &str, receipt: &str) -> alien_bindings::error::Result<()> {
        self.queue
            .lock()
            .await
            .as_ref()
            .ok_or_else(|| AlienError::new(unavailable()))?
            .ack(name, receipt)
            .await
    }
    async fn ack_batch(
        &self,
        name: &str,
        receipts: &[String],
    ) -> alien_bindings::error::Result<Vec<QueueAckResult>> {
        self.queue
            .lock()
            .await
            .as_ref()
            .ok_or_else(|| AlienError::new(unavailable()))?
            .ack_batch(name, receipts)
            .await
    }
    async fn nack(&self, name: &str, receipt: &str) -> alien_bindings::error::Result<()> {
        self.queue
            .lock()
            .await
            .as_ref()
            .ok_or_else(|| AlienError::new(unavailable()))?
            .nack(name, receipt)
            .await
    }
    async fn renew_lease(
        &self,
        name: &str,
        receipt: &str,
        duration: u32,
    ) -> alien_bindings::error::Result<()> {
        self.queue
            .lock()
            .await
            .as_ref()
            .ok_or_else(|| AlienError::new(unavailable()))?
            .renew_lease(name, receipt, duration)
            .await
    }
    async fn purge(&self, name: &str) -> alien_bindings::error::Result<()> {
        self.queue
            .lock()
            .await
            .as_ref()
            .ok_or_else(|| AlienError::new(unavailable()))?
            .purge(name)
            .await
    }
}
/// Writes `bytes` to a temporary file beside `path`, flushes it, and renames it into place, so a
/// crash or a full disk leaves the previous file (or none) and never a partial one.
async fn write_atomically(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    let result = async {
        let mut file = tokio::fs::File::create(&temporary).await?;
        file.write_all(bytes).await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(&temporary, path).await
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temporary).await;
    }
    result
}

fn queue_error(id: &str, reason: impl Into<String>) -> ErrorData {
    ErrorData::BindingsStoreError {
        binding_id: id.into(),
        operation: "open queue".into(),
        reason: reason.into(),
    }
}

impl LocalQueueManager {
    pub fn new(state_dir: PathBuf) -> Self {
        Self {
            state_dir,
            endpoint: RwLock::new(None),
            queues: Mutex::new(HashMap::new()),
            closed: AtomicBool::new(false),
        }
    }

    /// Configure the existing manager listener before publishing bindings.
    pub fn set_endpoint(&self, endpoint: String) {
        *self.endpoint.write().expect("queue endpoint poisoned") = Some(endpoint);
    }

    fn path(&self, id: &str) -> Result<PathBuf> {
        if id.is_empty() || id == "." || id == ".." || id.contains(['/', '\\']) {
            return Err(AlienError::new(queue_error(id, "Invalid queue identifier")));
        }
        // Never open an old file while older SDKs may still hold a mount/handle.
        if self.state_dir.join("queue").join(id).exists() {
            return Err(AlienError::new(queue_error(id,
                "Legacy local queue requires offline migration. Stop the old manager and every queue client, preserve the queue directory, then migrate it to queue-owned before restarting with rebuilt SDKs. No legacy database was opened.")));
        }
        Ok(self.state_dir.join("queue-owned").join(id))
    }

    pub async fn create_queue(&self, id: &str) -> Result<PathBuf> {
        let mut queues = self.queues.lock().await;
        if self.closed.load(Ordering::Relaxed) {
            return Err(AlienError::new(queue_error(
                id,
                "Queue owner has shut down",
            )));
        }
        let path = self.path(id)?;
        tokio::fs::create_dir_all(&path)
            .await
            .into_alien_error()
            .context(queue_error(id, "Create queue directory"))?;
        self.load_locked(id, &mut queues).await?;
        Ok(path)
    }

    pub fn get_queue_path(&self, id: &str) -> Result<PathBuf> {
        let path = self.path(id)?;
        if !path.is_dir() {
            return Err(AlienError::new(ErrorData::ServiceResourceNotFound {
                resource_id: id.into(),
                resource_type: "queue".into(),
            }));
        }
        Ok(path)
    }

    pub async fn load_queue(&self, id: &str) -> Result<Arc<ManagedQueue>> {
        let mut queues = self.queues.lock().await;
        self.load_locked(id, &mut queues).await
    }

    /// Authentication and handle selection share deletion's lock. A credential
    /// from a deleted generation must never authorize its replacement.
    pub(crate) async fn authenticated_queue(
        &self,
        id: &str,
        token: &str,
    ) -> alien_bindings::error::Result<Arc<ManagedQueue>> {
        let mut queues = self.queues.lock().await;
        self.authorize(id, token)
            .context(alien_bindings::error::ErrorData::LocalQueueUnauthorized)?;
        self.load_locked(id, &mut queues).await.context(
            alien_bindings::error::ErrorData::config_invalid("queue", "Queue is unavailable"),
        )
    }

    async fn load_locked(
        &self,
        id: &str,
        queues: &mut HashMap<String, Arc<ManagedQueue>>,
    ) -> Result<Arc<ManagedQueue>> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(AlienError::new(queue_error(
                id,
                "Queue owner has shut down",
            )));
        }
        let path = self.get_queue_path(id)?;
        if let Some(queue) = queues.get(id) {
            return Ok(queue.clone());
        }
        self.token(id, true)?;
        let queue = Arc::new(ManagedQueue {
            queue: Mutex::new(Some(
                LocalQueue::single_process(path)
                    .await
                    .context(queue_error(id, "Open exclusively owned queue"))?,
            )),
            dead_letter: RwLock::new(None),
        });
        queues.insert(id.into(), queue.clone());
        // A restart keeps the setting: it was persisted when the controller set it.
        if let Some(setting) = self.read_dead_letter(id).await? {
            self.link_dead_letter(id, &setting, queues).await?;
        }
        Ok(queue)
    }

    fn dead_letter_path(&self, id: &str) -> PathBuf {
        self.state_dir
            .join("queue-dead-letter")
            .join(format!("{id}.json"))
    }

    async fn read_dead_letter(&self, id: &str) -> Result<Option<DeadLetterFile>> {
        let path = self.dead_letter_path(id);
        match tokio::fs::read(&path).await {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .into_alien_error()
                .context(queue_error(
                    id,
                    format!(
                        "The dead-letter setting file '{}' is not valid; remove it and deploy again",
                        path.display()
                    ),
                ))
                .map(Some),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(AlienError::new(queue_error(
                id,
                format!("Read the dead-letter setting '{}': {error}", path.display()),
            ))),
        }
    }

    fn link_dead_letter<'a>(
        &'a self,
        id: &'a str,
        setting: &'a DeadLetterFile,
        queues: &'a mut HashMap<String, Arc<ManagedQueue>>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let target = self.load_locked(&setting.queue, queues).await?;
            if let Some(source) = queues.get(id) {
                *source
                    .dead_letter
                    .write()
                    .expect("dead-letter setting poisoned") = Some(DeadLetterTarget {
                    queue: target,
                    max_attempts: setting.max_attempts,
                });
            }
            Ok(())
        })
    }

    /// Sets or clears where queue `id` moves messages that failed `max_attempts` deliveries. The
    /// setting is persisted, so it holds across restarts, and takes effect on the next receive.
    pub async fn set_dead_letter(&self, id: &str, target: Option<(&str, u32)>) -> Result<()> {
        let mut queues = self.queues.lock().await;
        let setting = target.map(|(queue, max_attempts)| DeadLetterFile {
            queue: queue.to_string(),
            max_attempts,
        });
        let path = self.dead_letter_path(id);
        let unchanged = self.read_dead_letter(id).await? == setting;
        match &setting {
            _ if unchanged => {}
            Some(setting) => {
                if let Some(parent) = path.parent() {
                    tokio::fs::create_dir_all(parent)
                        .await
                        .into_alien_error()
                        .context(queue_error(id, "Create the dead-letter directory"))?;
                }
                let bytes = serde_json::to_vec(setting)
                    .into_alien_error()
                    .context(queue_error(id, "Encode the dead-letter setting"))?;
                write_atomically(&path, &bytes)
                    .await
                    .into_alien_error()
                    .context(queue_error(id, "Persist the dead-letter setting"))?;
            }
            None => match tokio::fs::remove_file(&path).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(AlienError::new(queue_error(
                        id,
                        format!("Remove the dead-letter setting: {error}"),
                    )))
                }
            },
        }
        let source = self.load_locked(id, &mut queues).await?;
        match setting {
            Some(setting) => self.link_dead_letter(id, &setting, &mut queues).await?,
            None => {
                *source
                    .dead_letter
                    .write()
                    .expect("dead-letter setting poisoned") = None;
            }
        }
        Ok(())
    }

    /// Revoke outstanding handles without deleting durable data.
    pub(crate) async fn close(&self) {
        let mut queues = self.queues.lock().await;
        self.closed.store(true, Ordering::Relaxed);
        for (_, queue) in queues.drain() {
            queue.queue.lock().await.take();
        }
    }

    pub async fn delete_queue(&self, id: &str) -> Result<()> {
        if id.is_empty() || id == "." || id == ".." || id.contains(['/', '\\']) {
            return Err(AlienError::new(queue_error(id, "Invalid queue identifier")));
        }
        let path = self.state_dir.join("queue-owned").join(id);
        let mut queues = self.queues.lock().await;
        if let Some(queue) = queues.remove(id) {
            queue.queue.lock().await.take();
        }
        if path.exists() {
            tokio::fs::remove_dir_all(path)
                .await
                .into_alien_error()
                .context(queue_error(id, "Delete queue directory"))?;
        }
        let _ = tokio::fs::remove_file(self.dead_letter_path(id)).await;
        // Explicit resource deletion also removes unmigrated data, after dependents stop.
        let legacy = self.state_dir.join("queue").join(id);
        if legacy.exists() {
            tokio::fs::remove_dir_all(legacy)
                .await
                .into_alien_error()
                .context(queue_error(id, "Delete legacy queue directory"))?;
        }
        Ok(())
    }

    pub fn queue_exists(&self, id: &str) -> bool {
        self.get_queue_path(id).is_ok()
    }

    pub async fn check_health(&self, id: &str) -> Result<()> {
        self.load_queue(id)
            .await?
            .stats()
            .await
            .context(queue_error(id, "Read queue metrics"))?;
        Ok(())
    }

    fn token(&self, id: &str, create: bool) -> Result<String> {
        let path = self.get_queue_path(id)?.join("access-token");
        if create && !path.exists() {
            let token = format!(
                "ax_lq_{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            );
            let directory = self.get_queue_path(id)?;
            // NamedTempFile is private (0600). Publish only a complete, synced credential;
            // a crash before publication leaves no partially initialized final file.
            let mut temporary = tempfile::NamedTempFile::new_in(&directory)
                .into_alien_error()
                .context(queue_error(id, "Create private queue credential"))?;
            temporary
                .write_all(token.as_bytes())
                .into_alien_error()
                .context(queue_error(id, "Write queue credential"))?;
            temporary
                .as_file()
                .sync_all()
                .into_alien_error()
                .context(queue_error(id, "Sync queue credential"))?;
            match temporary.persist_noclobber(&path) {
                Ok(_) => {}
                Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(error.error)
                        .into_alien_error()
                        .context(queue_error(id, "Publish queue credential"))
                }
            }
            std::fs::File::open(directory)
                .and_then(|file| file.sync_all())
                .into_alien_error()
                .context(queue_error(id, "Sync queue credential directory"))?;
        }
        let token = std::fs::read_to_string(path)
            .into_alien_error()
            .context(queue_error(id, "Read queue credential"))?;
        if token.len() != 70 || !token.starts_with("ax_lq_") {
            return Err(AlienError::new(queue_error(
                id,
                "Invalid saved queue credential",
            )));
        }
        Ok(token)
    }

    pub(crate) fn authorize(&self, id: &str, token: &str) -> Result<()> {
        let expected = Sha256::digest(self.token(id, false)?.as_bytes());
        let actual = Sha256::digest(token.as_bytes());
        // Compare hashes, never retain or log a request's bearer credential.
        if expected
            .iter()
            .zip(actual.iter())
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            != 0
        {
            return Err(AlienError::new(queue_error(id, "Invalid queue credential")));
        }
        Ok(())
    }

    pub fn runtime_binding(&self, id: &str) -> Result<QueueBinding> {
        let mut binding = self.get_binding(id)?;
        if let QueueBinding::LocalHttp(config) = &mut binding {
            config.token = Some(self.token(id, false)?);
        }
        Ok(binding)
    }

    /// Non-secret binding topology. Credentials are injected only at process start.
    pub fn get_binding(&self, id: &str) -> Result<QueueBinding> {
        self.get_queue_path(id)?;
        let endpoint = self
            .endpoint
            .read()
            .expect("queue endpoint poisoned")
            .clone()
            .ok_or_else(|| {
                AlienError::new(queue_error(
                    id,
                    "Local queue HTTP endpoint is not configured",
                ))
            })?;
        Ok(QueueBinding::LocalHttp(LocalHttpQueueBinding {
            endpoint: BindingValue::value(format!(
                "{}/{}",
                endpoint.trim_end_matches('/'),
                urlencoding::encode(id)
            )),
            token: None,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    /// While the setting changes from A->B to B->A, receives that waited on the old target and
    /// receives that use the new one must not wait for each other's queue forever.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn receives_on_queues_that_dead_letter_to_each_other_do_not_deadlock() {
        let directory = tempfile::tempdir().unwrap();
        let manager = LocalQueueManager::new(directory.path().to_owned());
        manager.create_queue("a").await.unwrap();
        manager.create_queue("b").await.unwrap();
        let a = manager.load_queue("a").await.unwrap();
        let b = manager.load_queue("b").await.unwrap();
        for (from, to) in [(&a, &b), (&b, &a)] {
            *from.dead_letter.write().unwrap() = Some(DeadLetterTarget {
                queue: to.clone(),
                max_attempts: 5,
            });
        }
        let moves = async {
            let mut tasks = Vec::new();
            for _ in 0..500 {
                for queue in [a.clone(), b.clone()] {
                    tasks.push(tokio::spawn(async move { queue.move_dead_letters().await }));
                }
            }
            for task in tasks {
                task.await.unwrap();
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(30), moves)
            .await
            .expect("moves between queues that name each other deadlocked");
    }

    /// A message that keeps failing moves to the dead-letter queue with its payload, and the
    /// setting and the failed deliveries survive a restart of the owner.
    #[tokio::test]
    async fn exhausted_messages_move_to_the_dead_letter_queue_across_a_restart() {
        use alien_bindings::traits::MessagePayload;
        let directory = tempfile::tempdir().unwrap();
        let text = |message: &QueueMessage| match &message.payload {
            MessagePayload::Text(text) => text.clone(),
            MessagePayload::Json(value) => value.to_string(),
        };
        {
            let manager = LocalQueueManager::new(directory.path().to_owned());
            manager.create_queue("tasks").await.unwrap();
            manager.create_queue("failed").await.unwrap();
            manager
                .set_dead_letter("tasks", Some(("failed", 5)))
                .await
                .unwrap();
            let tasks = manager.load_queue("tasks").await.unwrap();
            tasks
                .send("tasks", MessagePayload::Text("poison".into()))
                .await
                .unwrap();
            // Four failed deliveries: nack returns the message at once.
            for expected in 1..=4 {
                let got = tasks.receive("tasks", 1).await.unwrap();
                assert_eq!(got[0].attempt, expected);
                tasks.nack("tasks", &got[0].receipt_handle).await.unwrap();
            }
            manager.close().await;
        }

        // A new owner over the same directory: the setting is read back from disk.
        let manager = LocalQueueManager::new(directory.path().to_owned());
        let tasks = manager.load_queue("tasks").await.unwrap();
        let failed = manager.load_queue("failed").await.unwrap();
        let fifth = tasks.receive("tasks", 1).await.unwrap();
        assert_eq!(fifth[0].attempt, 5, "five deliveries are allowed");
        tasks.nack("tasks", &fifth[0].receipt_handle).await.unwrap();

        // The sixth would be past maxAttempts: it moves instead, payload intact.
        assert!(tasks.receive("tasks", 1).await.unwrap().is_empty());
        let moved = failed.receive("failed", 10).await.unwrap();
        assert_eq!(moved.len(), 1);
        assert_eq!(text(&moved[0]), "poison");
        assert_eq!(tasks.stats().await.unwrap().total, 0, "tasks ends empty");

        // Removing the setting stops the move.
        manager.set_dead_letter("tasks", None).await.unwrap();
        tasks
            .send("tasks", MessagePayload::Text("again".into()))
            .await
            .unwrap();
        for _ in 0..7 {
            let got = tasks.receive("tasks", 1).await.unwrap();
            tasks.nack("tasks", &got[0].receipt_handle).await.unwrap();
        }
        assert_eq!(tasks.stats().await.unwrap().total, 1);
    }

    /// The setting is replaced by renaming a finished temporary file: a partial file left by an
    /// interrupted write is never read as the setting, and the next write replaces it.
    #[tokio::test]
    async fn the_dead_letter_setting_is_written_atomically() {
        let directory = tempfile::tempdir().unwrap();
        let manager = LocalQueueManager::new(directory.path().to_owned());
        manager.create_queue("tasks").await.unwrap();
        manager.create_queue("failed").await.unwrap();
        manager
            .set_dead_letter("tasks", Some(("failed", 5)))
            .await
            .unwrap();
        let path = manager.dead_letter_path("tasks");

        // An interrupted write: a truncated temporary file beside the good setting.
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, b"{\"queue\":\"fai").unwrap();
        let read = manager.read_dead_letter("tasks").await.unwrap().unwrap();
        assert_eq!((read.queue.as_str(), read.max_attempts), ("failed", 5));

        // The next write replaces the setting whole and leaves no temporary file.
        manager
            .set_dead_letter("tasks", Some(("failed", 9)))
            .await
            .unwrap();
        assert!(!temporary.exists());
        let read = manager.read_dead_letter("tasks").await.unwrap().unwrap();
        assert_eq!(read.max_attempts, 9);

        // A file that is already corrupt gives an error that names it.
        std::fs::write(&path, b"{\"queue\":\"fai").unwrap();
        let error = manager.read_dead_letter("tasks").await.unwrap_err();
        assert!(
            error.to_string().contains(&path.display().to_string())
                || format!("{error:?}").contains(&path.display().to_string()),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn waiting_open_cannot_recreate_a_deleted_queue() {
        let directory = tempfile::tempdir().unwrap();
        let manager = Arc::new(LocalQueueManager::new(directory.path().to_owned()));
        let path = manager.create_queue("jobs").await.unwrap();
        let mut queues = manager.queues.lock().await;
        let queue = queues.remove("jobs").unwrap();
        queue.queue.lock().await.take();
        let mut opening = std::pin::pin!(manager.load_queue("jobs"));
        assert!(
            futures::poll!(&mut opening).is_pending(),
            "open must wait for deletion's lock"
        );
        tokio::fs::remove_dir_all(&path).await.unwrap();
        drop(queues);
        assert!(opening.await.is_err());
        assert!(
            !path.exists(),
            "waiting open must not resurrect deleted data"
        );
    }

    #[tokio::test]
    async fn abandoned_temporary_credential_does_not_block_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("queue-owned/jobs");
        std::fs::create_dir_all(&path).unwrap();
        // Simulate a crash before the temporary credential was fully written or published.
        std::fs::write(path.join(".tmp-interrupted-token"), "ax_lq_partial").unwrap();
        let manager = LocalQueueManager::new(directory.path().to_owned());
        manager.create_queue("jobs").await.unwrap();
        manager.set_endpoint("http://localhost:1234/v1/local/queues/test".into());
        let QueueBinding::LocalHttp(binding) = manager.runtime_binding("jobs").unwrap() else {
            panic!("HTTP binding")
        };
        let token = binding.token.unwrap();
        assert_eq!(token.len(), 70);
        manager.authorize("jobs", &token).unwrap();
        #[cfg(unix)]
        {
            assert_eq!(
                std::fs::metadata(path.join("access-token"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        assert!(manager.authorize("jobs", "incorrect").is_err());
    }

    #[tokio::test]
    async fn waiting_old_credential_cannot_access_recreated_queue() {
        let directory = tempfile::tempdir().unwrap();
        let manager = LocalQueueManager::new(directory.path().to_owned());
        let path = manager.create_queue("jobs").await.unwrap();
        let old_token = manager.token("jobs", false).unwrap();
        let old_handle = manager.load_queue("jobs").await.unwrap();
        let mut queues = manager.queues.lock().await;
        let mut request = std::pin::pin!(manager.authenticated_queue("jobs", &old_token));
        assert!(futures::poll!(&mut request).is_pending());
        queues.remove("jobs").unwrap().queue.lock().await.take();
        std::fs::remove_dir_all(&path).unwrap();
        std::fs::create_dir_all(&path).unwrap();
        let replacement = manager.load_locked("jobs", &mut queues).await.unwrap();
        replacement
            .send("jobs", MessagePayload::Text("preserve".into()))
            .await
            .unwrap();
        drop(queues);
        let error = request.await.unwrap_err();
        assert_eq!(error.code, "LOCAL_QUEUE_UNAUTHORIZED");
        assert!(old_handle.purge("jobs").await.is_err());
        assert_eq!(replacement.stats().await.unwrap().total, 1);
        let new_token = manager.token("jobs", false).unwrap();
        assert_ne!(old_token, new_token);
        assert!(manager
            .authenticated_queue("jobs", &new_token)
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn unregister_revokes_handles_and_releases_store_for_restart() {
        let directory = tempfile::tempdir().unwrap();
        let manager = Arc::new(LocalQueueManager::new(directory.path().to_owned()));
        let service = crate::LocalQueueService::default();
        service.register("dep", manager.clone(), "http://localhost:1234");
        manager.create_queue("jobs").await.unwrap();
        let token = manager.token("jobs", false).unwrap();
        let handle = manager.authenticated_queue("jobs", &token).await.unwrap();
        handle
            .send("jobs", MessagePayload::Text("durable".into()))
            .await
            .unwrap();
        service.unregister("dep").await;
        assert!(handle.purge("jobs").await.is_err());
        assert!(manager.authenticated_queue("jobs", &token).await.is_err());
        let restarted = LocalQueueManager::new(directory.path().to_owned());
        let recovered = restarted.load_queue("jobs").await.unwrap();
        assert_eq!(recovered.stats().await.unwrap().total, 1);
        assert_eq!(restarted.token("jobs", false).unwrap(), token);
    }
}
