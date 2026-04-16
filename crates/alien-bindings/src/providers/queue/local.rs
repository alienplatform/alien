use crate::error::{ErrorData, Result};
use crate::traits::{
    Binding, MessagePayload, Queue, QueueMessage, MAX_BATCH_SIZE, MAX_MESSAGE_BYTES,
};
use alien_core::bindings::LocalQueueBinding;
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

const LEASE_DURATION_SECS: i64 = 30;

/// Local disk-persisted queue implementation using sled embedded database.
///
/// This provides a persistent, thread-safe, disk-based message queue that implements
/// all Queue trait features including send, receive with visibility timeout, and ack.
/// Messages survive process restarts.
#[derive(Debug)]
pub struct LocalQueue {
    db: Arc<Mutex<sled::Db>>,
    data_dir: PathBuf,
}

/// Stored message format that avoids serde issues with `MessagePayload`'s internal tagging.
/// We store the payload as a raw JSON value and a discriminator tag.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredMessage {
    /// "json" or "text"
    payload_type: String,
    /// The raw payload content (JSON value for json type, string for text type)
    payload_data: serde_json::Value,
    enqueued_at: DateTime<Utc>,
}

impl StoredMessage {
    fn from_payload(payload: MessagePayload) -> Self {
        let (payload_type, payload_data) = match payload {
            MessagePayload::Json(v) => ("json".to_string(), v),
            MessagePayload::Text(s) => ("text".to_string(), serde_json::Value::String(s)),
        };
        Self {
            payload_type,
            payload_data,
            enqueued_at: Utc::now(),
        }
    }

    fn into_payload(self) -> MessagePayload {
        match self.payload_type.as_str() {
            "json" => MessagePayload::Json(self.payload_data),
            _ => match self.payload_data {
                serde_json::Value::String(s) => MessagePayload::Text(s),
                other => MessagePayload::Text(other.to_string()),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InFlightMessage {
    /// The sequence key in the messages tree (big-endian u64 bytes)
    seq_bytes: Vec<u8>,
    message: StoredMessage,
    leased_until: DateTime<Utc>,
}

impl LocalQueue {
    /// Create a new local queue store with the given data directory.
    pub async fn new(data_dir: PathBuf) -> Result<Self> {
        tracing::debug!(data_dir = %data_dir.display(), "Opening LocalQueue database");

        if let Some(parent) = data_dir.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .into_alien_error()
                .context(ErrorData::LocalFilesystemError {
                    path: parent.to_string_lossy().to_string(),
                    operation: "create_dir_all".to_string(),
                })?;
        }

        let db =
            sled::open(&data_dir)
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "local queue".to_string(),
                    reason: format!("Failed to open sled database at: {:?}", data_dir),
                })?;

        tracing::debug!(data_dir = %data_dir.display(), "LocalQueue database opened successfully");

        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            data_dir,
        })
    }

    /// Create a LocalQueue from a LocalQueueBinding.
    pub async fn from_binding(binding: LocalQueueBinding) -> Result<Self> {
        let queue_path = binding
            .queue_path
            .into_value("queue", "queue_path")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "queue".to_string(),
                reason: "Failed to resolve queue_path from binding".to_string(),
            })?;

        Self::new(PathBuf::from(queue_path)).await
    }

    /// Reclaim expired in-flight messages back to the messages tree.
    fn reclaim_expired_leases(db: &sled::Db) -> Result<()> {
        let in_flight_tree = db.open_tree("in_flight").into_alien_error().context(
            ErrorData::QueueOperationFailed {
                operation: "open in_flight tree".to_string(),
                reason: "Failed to open in_flight tree".to_string(),
            },
        )?;

        let messages_tree = db.open_tree("messages").into_alien_error().context(
            ErrorData::QueueOperationFailed {
                operation: "open messages tree".to_string(),
                reason: "Failed to open messages tree".to_string(),
            },
        )?;

        let now = Utc::now();
        let mut expired_handles = Vec::new();

        for result in in_flight_tree.iter() {
            let (handle_bytes, value_bytes) =
                result
                    .into_alien_error()
                    .context(ErrorData::QueueOperationFailed {
                        operation: "scan in_flight".to_string(),
                        reason: "Failed to iterate in-flight messages".to_string(),
                    })?;

            if let Ok(in_flight) = serde_json::from_slice::<InFlightMessage>(&value_bytes) {
                if now >= in_flight.leased_until {
                    // Re-enqueue the message with its original sequence key
                    let stored_bytes = serde_json::to_vec(&in_flight.message)
                        .into_alien_error()
                        .context(ErrorData::QueueOperationFailed {
                            operation: "serialize reclaimed message".to_string(),
                            reason: "Failed to serialize message".to_string(),
                        })?;

                    messages_tree
                        .insert(&in_flight.seq_bytes, stored_bytes)
                        .into_alien_error()
                        .context(ErrorData::QueueOperationFailed {
                            operation: "re-enqueue expired message".to_string(),
                            reason: "Failed to re-enqueue expired message".to_string(),
                        })?;

                    expired_handles.push(handle_bytes);
                }
            }
        }

        for handle in expired_handles {
            let _ = in_flight_tree.remove(&handle);
        }

        Ok(())
    }

    fn serialize_message(message: &StoredMessage) -> Result<Vec<u8>> {
        serde_json::to_vec(message)
            .into_alien_error()
            .context(ErrorData::QueueOperationFailed {
                operation: "serialize message".to_string(),
                reason: "Failed to serialize message to JSON".to_string(),
            })
    }

    fn message_size(payload: &MessagePayload) -> Result<usize> {
        match payload {
            MessagePayload::Json(v) => serde_json::to_string(v)
                .map(|s| s.len())
                .into_alien_error()
                .context(ErrorData::QueueOperationFailed {
                    operation: "measure message size".to_string(),
                    reason: "Failed to serialize JSON payload".to_string(),
                }),
            MessagePayload::Text(s) => Ok(s.len()),
        }
    }
}

impl Binding for LocalQueue {}

#[async_trait]
impl Queue for LocalQueue {
    async fn send(&self, _queue: &str, message: MessagePayload) -> Result<()> {
        let size = Self::message_size(&message)?;
        if size > MAX_MESSAGE_BYTES {
            return Err(AlienError::new(ErrorData::BindingSetupFailed {
                binding_type: "queue.local".to_string(),
                reason: format!(
                    "Message size {} bytes exceeds limit of {} bytes",
                    size, MAX_MESSAGE_BYTES
                ),
            }));
        }

        let stored = StoredMessage::from_payload(message);
        let serialized = Self::serialize_message(&stored)?;

        let db = self.db.lock().await;
        let messages_tree = db.open_tree("messages").into_alien_error().context(
            ErrorData::QueueOperationFailed {
                operation: "open messages tree".to_string(),
                reason: "Failed to open messages tree".to_string(),
            },
        )?;

        // Use generate_id for monotonically increasing sequence numbers
        let seq = db
            .generate_id()
            .into_alien_error()
            .context(ErrorData::QueueOperationFailed {
                operation: "generate sequence".to_string(),
                reason: "Failed to generate message sequence number".to_string(),
            })?;
        let seq_key = seq.to_be_bytes();

        messages_tree
            .insert(seq_key, serialized)
            .into_alien_error()
            .context(ErrorData::QueueOperationFailed {
                operation: "send".to_string(),
                reason: "Failed to insert message".to_string(),
            })?;

        messages_tree
            .flush_async()
            .await
            .into_alien_error()
            .context(ErrorData::QueueOperationFailed {
                operation: "flush".to_string(),
                reason: "Failed to flush message to disk".to_string(),
            })?;

        Ok(())
    }

    async fn receive(&self, _queue: &str, max_messages: usize) -> Result<Vec<QueueMessage>> {
        if max_messages == 0 || max_messages > MAX_BATCH_SIZE {
            return Err(AlienError::new(ErrorData::BindingSetupFailed {
                binding_type: "queue.local".to_string(),
                reason: format!(
                    "Batch size {} is invalid. Must be between 1 and {}",
                    max_messages, MAX_BATCH_SIZE
                ),
            }));
        }

        let db = self.db.lock().await;

        // Reclaim expired leases first
        Self::reclaim_expired_leases(&db)?;

        let messages_tree = db.open_tree("messages").into_alien_error().context(
            ErrorData::QueueOperationFailed {
                operation: "open messages tree".to_string(),
                reason: "Failed to open messages tree".to_string(),
            },
        )?;

        let in_flight_tree = db.open_tree("in_flight").into_alien_error().context(
            ErrorData::QueueOperationFailed {
                operation: "open in_flight tree".to_string(),
                reason: "Failed to open in_flight tree".to_string(),
            },
        )?;

        let now = Utc::now();
        let leased_until = now + chrono::Duration::seconds(LEASE_DURATION_SECS);
        let mut result = Vec::new();

        // Pop messages from the front (lowest sequence number)
        for item in messages_tree.iter() {
            if result.len() >= max_messages {
                break;
            }

            let (seq_key, value_bytes) =
                item.into_alien_error()
                    .context(ErrorData::QueueOperationFailed {
                        operation: "receive".to_string(),
                        reason: "Failed to iterate messages".to_string(),
                    })?;

            let stored: StoredMessage = match serde_json::from_slice(&value_bytes) {
                Ok(m) => m,
                Err(_) => continue, // Skip corrupted messages
            };

            // Generate a receipt handle
            let receipt_handle = uuid::Uuid::new_v4().to_string();

            // Move to in-flight
            let in_flight = InFlightMessage {
                seq_bytes: seq_key.to_vec(),
                message: stored.clone(),
                leased_until,
            };
            let in_flight_bytes = serde_json::to_vec(&in_flight).into_alien_error().context(
                ErrorData::QueueOperationFailed {
                    operation: "serialize in_flight".to_string(),
                    reason: "Failed to serialize in-flight message".to_string(),
                },
            )?;

            in_flight_tree
                .insert(receipt_handle.as_bytes(), in_flight_bytes)
                .into_alien_error()
                .context(ErrorData::QueueOperationFailed {
                    operation: "move to in_flight".to_string(),
                    reason: "Failed to move message to in-flight".to_string(),
                })?;

            // Remove from messages
            messages_tree.remove(&seq_key).into_alien_error().context(
                ErrorData::QueueOperationFailed {
                    operation: "remove from messages".to_string(),
                    reason: "Failed to remove message from queue".to_string(),
                },
            )?;

            result.push(QueueMessage {
                payload: stored.into_payload(),
                receipt_handle,
            });
        }

        // Flush both trees
        messages_tree
            .flush_async()
            .await
            .into_alien_error()
            .context(ErrorData::QueueOperationFailed {
                operation: "flush".to_string(),
                reason: "Failed to flush messages tree".to_string(),
            })?;
        in_flight_tree
            .flush_async()
            .await
            .into_alien_error()
            .context(ErrorData::QueueOperationFailed {
                operation: "flush".to_string(),
                reason: "Failed to flush in_flight tree".to_string(),
            })?;

        Ok(result)
    }

    async fn ack(&self, _queue: &str, receipt_handle: &str) -> Result<()> {
        let db = self.db.lock().await;
        let in_flight_tree = db.open_tree("in_flight").into_alien_error().context(
            ErrorData::QueueOperationFailed {
                operation: "open in_flight tree".to_string(),
                reason: "Failed to open in_flight tree".to_string(),
            },
        )?;

        // Remove the message (idempotent - missing key is OK)
        in_flight_tree
            .remove(receipt_handle.as_bytes())
            .into_alien_error()
            .context(ErrorData::QueueOperationFailed {
                operation: "ack".to_string(),
                reason: "Failed to acknowledge message".to_string(),
            })?;

        in_flight_tree
            .flush_async()
            .await
            .into_alien_error()
            .context(ErrorData::QueueOperationFailed {
                operation: "flush".to_string(),
                reason: "Failed to flush acknowledgment".to_string(),
            })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn payload_text(msg: &QueueMessage) -> String {
        match &msg.payload {
            MessagePayload::Text(s) => s.clone(),
            MessagePayload::Json(v) => v.to_string(),
        }
    }

    async fn create_test_queue() -> (LocalQueue, TempDir) {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let queue = LocalQueue::new(temp_dir.path().join("queue.db"))
            .await
            .expect("Failed to create LocalQueue");
        (queue, temp_dir)
    }

    #[tokio::test]
    async fn test_send_and_receive() {
        let (queue, _temp_dir) = create_test_queue().await;

        queue
            .send("q", MessagePayload::Text("hello".to_string()))
            .await
            .unwrap();
        queue
            .send("q", MessagePayload::Text("world".to_string()))
            .await
            .unwrap();

        let msgs = queue.receive("q", 10).await.unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(payload_text(&msgs[0]), "hello");
        assert_eq!(payload_text(&msgs[1]), "world");
    }

    #[tokio::test]
    async fn test_receive_empty_queue() {
        let (queue, _temp_dir) = create_test_queue().await;

        let msgs = queue.receive("q", 10).await.unwrap();
        assert!(msgs.is_empty());
    }

    #[tokio::test]
    async fn test_ack_removes_message() {
        let (queue, _temp_dir) = create_test_queue().await;

        queue
            .send("q", MessagePayload::Text("msg".to_string()))
            .await
            .unwrap();

        let msgs = queue.receive("q", 1).await.unwrap();
        assert_eq!(msgs.len(), 1);

        // Ack the message
        queue.ack("q", &msgs[0].receipt_handle).await.unwrap();

        // No messages should be available (acked, not expired)
        let msgs = queue.receive("q", 10).await.unwrap();
        assert!(msgs.is_empty());
    }

    #[tokio::test]
    async fn test_ack_idempotent() {
        let (queue, _temp_dir) = create_test_queue().await;

        // Acking a non-existent receipt handle should succeed
        queue.ack("q", "non-existent-handle").await.unwrap();
    }

    #[tokio::test]
    async fn test_receive_respects_max_messages() {
        let (queue, _temp_dir) = create_test_queue().await;

        for i in 0..5 {
            queue
                .send("q", MessagePayload::Text(format!("msg-{}", i)))
                .await
                .unwrap();
        }

        let msgs = queue.receive("q", 2).await.unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(payload_text(&msgs[0]), "msg-0");
        assert_eq!(payload_text(&msgs[1]), "msg-1");
    }

    #[tokio::test]
    async fn test_json_payload() {
        let (queue, _temp_dir) = create_test_queue().await;

        let payload = serde_json::json!({"key": "value", "num": 42});
        queue
            .send("q", MessagePayload::Json(payload.clone()))
            .await
            .unwrap();

        let msgs = queue.receive("q", 1).await.unwrap();
        assert_eq!(msgs.len(), 1);
        match &msgs[0].payload {
            MessagePayload::Json(v) => assert_eq!(v, &payload),
            _ => panic!("Expected JSON payload"),
        }
    }

    #[tokio::test]
    async fn test_message_size_validation() {
        let (queue, _temp_dir) = create_test_queue().await;

        let large = "x".repeat(MAX_MESSAGE_BYTES + 1);
        let result = queue.send("q", MessagePayload::Text(large)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_batch_size_validation() {
        let (queue, _temp_dir) = create_test_queue().await;

        assert!(queue.receive("q", 0).await.is_err());
        assert!(queue.receive("q", MAX_BATCH_SIZE + 1).await.is_err());
    }

    #[tokio::test]
    async fn test_persistence_across_reopens() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("queue.db");

        // Send a message and drop the queue
        {
            let queue = LocalQueue::new(db_path.clone()).await.unwrap();
            queue
                .send("q", MessagePayload::Text("persistent".to_string()))
                .await
                .unwrap();
        }

        // Reopen and verify message persists
        {
            let queue = LocalQueue::new(db_path).await.unwrap();
            let msgs = queue.receive("q", 1).await.unwrap();
            assert_eq!(msgs.len(), 1);
            assert_eq!(payload_text(&msgs[0]), "persistent");
        }
    }

    #[tokio::test]
    async fn test_fifo_ordering() {
        let (queue, _temp_dir) = create_test_queue().await;

        for i in 0..10 {
            queue
                .send("q", MessagePayload::Text(format!("{}", i)))
                .await
                .unwrap();
        }

        let msgs = queue.receive("q", 10).await.unwrap();
        for (i, msg) in msgs.iter().enumerate() {
            assert_eq!(payload_text(msg), format!("{}", i));
        }
    }
}
