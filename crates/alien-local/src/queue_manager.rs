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
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, RwLock},
};
use tokio::sync::Mutex;

/// The manager owns each queue handle. Clients never receive its filesystem path.
#[derive(Debug)]
pub struct LocalQueueManager {
    state_dir: PathBuf,
    endpoint: RwLock<Option<String>>,
    queues: Mutex<HashMap<String, Arc<ManagedQueue>>>,
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
        let path = self.path(id)?;
        tokio::fs::create_dir_all(&path)
            .await
            .into_alien_error()
            .context(queue_error(id, "Create queue directory"))?;
        self.load_queue(id).await?;
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
        let path = self.get_queue_path(id)?;
        let mut queues = self.queues.lock().await;
        if let Some(queue) = queues.get(id) {
            return Ok(queue.clone());
        }
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

    pub async fn delete_queue(&self, id: &str) -> Result<()> {
        let path = self.path(id)?;
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

    fn token(&self, id: &str) -> Result<String> {
        let path = self.get_queue_path(id)?.join("access-token");
        if !path.exists() {
            let token = format!(
                "ax_lq_{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            );
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            match options.open(&path) {
                Ok(mut file) => {
                    file.write_all(token.as_bytes())
                        .into_alien_error()
                        .context(queue_error(id, "Write queue credential"))?;
                    file.sync_all()
                        .into_alien_error()
                        .context(queue_error(id, "Sync queue credential"))?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => {
                    return Err(e)
                        .into_alien_error()
                        .context(queue_error(id, "Create queue credential"))
                }
            }
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
        let expected = Sha256::digest(self.token(id)?.as_bytes());
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
            config.token = Some(self.token(id)?);
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
