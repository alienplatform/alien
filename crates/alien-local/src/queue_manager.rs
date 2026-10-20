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
}
fn unavailable() -> alien_bindings::error::ErrorData {
    alien_bindings::error::ErrorData::config_invalid("queue", "Queue has been deleted")
}
impl ManagedQueue {
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
        });
        queues.insert(id.into(), queue.clone());
        Ok(queue)
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
