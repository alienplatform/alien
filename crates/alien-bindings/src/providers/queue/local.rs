//! Local disk-persisted queue backed by SQLite (`localqueue.v1`), multi-process safe.
//!
//! # Connection strategy
//!
//! `rusqlite::Connection` is `Send` but **not** `Sync`, and every SQLite call is
//! blocking. We therefore never store a connection on `LocalQueue` (which must
//! be `Send + Sync` to live behind `Arc<dyn Queue>`): each operation runs inside
//! `tokio::task::spawn_blocking` and opens its **own** short-lived connection to
//! `<dataDir>/localqueue.sqlite`, then drops it. A connection is never held
//! across an `.await`, and there is no `Mutex<Connection>` anywhere — the two
//! footguns called out by the async/blocking boundary constraint simply cannot
//! occur.
//!
//! Correctness under concurrent access (multiple handles on one file, i.e.
//! multiple processes) comes from SQLite itself: every connection enables WAL
//! (concurrent readers alongside a single writer) and a `busy_timeout` (writers
//! wait for the write lock instead of returning `SQLITE_BUSY`).
//!
//! # Receive design: one `BEGIN IMMEDIATE` transaction per batch
//!
//! The pinned `localqueue.v1` receive is one atomic
//! `UPDATE ... SET receipt_handle = ?uuid ... RETURNING ...`. A single bound
//! parameter cannot mint a **distinct** UUID per claimed row, so this
//! implementation uses the sanctioned equivalent: one `BEGIN IMMEDIATE`
//! transaction per batch that selects the due ids, then runs that exact
//! per-row `UPDATE ... RETURNING` with a fresh UUID for each, and commits.
//! `IMMEDIATE` takes the write lock at `BEGIN`, so concurrent receivers (in
//! any process) serialize on the whole claim: a message is delivered to
//! exactly one receiver per visibility window.
//!
//! See `crates/alien-bindings/FORMAT.md` for the on-disk `localqueue.v1`
//! contract, including the `"{id}:{uuid}"` caller-facing receipt-handle format.
use crate::error::{ErrorData, Result};
use crate::providers::sqlite_store::{SqliteStore, StoreSpec};
use crate::traits::{
    Binding, MessagePayload, Queue, QueueMessage, LEASE_SECONDS, MAX_BATCH_SIZE, MAX_MESSAGE_BYTES,
};
use alien_core::bindings::LocalQueueBinding;
use alien_error::{AlienError, Context as _, IntoAlienError as _};
use async_trait::async_trait;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::path::PathBuf;
use std::time::Duration;

static QUEUE_SPEC: StoreSpec = StoreSpec {
    db_filename: "localqueue.sqlite",
    format_version: "localqueue.v1",
    binding_type: "local queue",
    schema_ddl: "CREATE TABLE IF NOT EXISTS messages (\
                     id             INTEGER PRIMARY KEY AUTOINCREMENT,\
                     payload_type   TEXT    NOT NULL,\
                     payload_data   TEXT    NOT NULL,\
                     enqueued_at    INTEGER NOT NULL,\
                     visible_at     INTEGER NOT NULL,\
                     attempt        INTEGER NOT NULL DEFAULT 0,\
                     receipt_handle TEXT\
                 );\
                 CREATE INDEX IF NOT EXISTS idx_messages_visible ON messages (visible_at, enqueued_at, id);",
};

/// Local disk-persisted queue on SQLite (`localqueue.v1`).
///
/// Implements the `Queue` trait (send/receive/ack) plus inherent `nack` and
/// `purge`. Messages survive process restarts, and multiple `LocalQueue`
/// handles — including handles in different OS processes — can safely share
/// one data directory.
#[derive(Debug)]
pub struct LocalQueue {
    store: SqliteStore,
}

/// Split a `MessagePayload` into its stored `(payload_type, payload_data)`
/// columns: `("json", <serialized JSON>)` or `("text", <raw string>)`.
fn encode_payload(payload: MessagePayload) -> Result<(&'static str, String)> {
    match payload {
        MessagePayload::Json(v) => {
            let data = serde_json::to_string(&v).into_alien_error().context(
                ErrorData::QueueOperationFailed {
                    operation: "send".to_string(),
                    reason: "failed to serialize JSON payload".to_string(),
                },
            )?;
            Ok(("json", data))
        }
        MessagePayload::Text(s) => Ok(("text", s)),
    }
}

/// Rebuild a `MessagePayload` from its stored columns.
fn decode_payload(payload_type: &str, payload_data: String) -> Result<MessagePayload> {
    match payload_type {
        "json" => {
            let value = serde_json::from_str(&payload_data)
                .into_alien_error()
                .context(ErrorData::QueueOperationFailed {
                    operation: "receive".to_string(),
                    reason: "failed to deserialize stored JSON payload".to_string(),
                })?;
            Ok(MessagePayload::Json(value))
        }
        "text" => Ok(MessagePayload::Text(payload_data)),
        other => Err(AlienError::new(ErrorData::QueueOperationFailed {
            operation: "receive".to_string(),
            reason: format!("unknown payload_type '{other}' in localqueue.v1 store"),
        })),
    }
}

impl LocalQueue {
    /// Create a new local queue store rooted at the given data directory.
    ///
    /// The directory is created if missing; the store lives at
    /// `<data_dir>/localqueue.sqlite`.
    pub async fn new(data_dir: PathBuf) -> Result<Self> {
        Ok(Self {
            store: SqliteStore::open(data_dir, &QUEUE_SPEC).await?,
        })
    }

    /// Create a LocalQueue from a LocalQueueBinding.
    pub async fn from_binding(binding: LocalQueueBinding) -> Result<Self> {
        let queue_path = binding
            .queue_path
            .into_value("queue", "queue_path")
            .context(ErrorData::config_invalid(
                "queue",
                "Failed to resolve queue_path from binding",
            ))?;

        Self::new(PathBuf::from(queue_path)).await
    }

    /// Get the data directory path (the directory that holds `localqueue.sqlite`).
    pub fn data_dir(&self) -> &PathBuf {
        self.store.data_dir()
    }

    /// Run a blocking closure with a freshly opened, WAL-configured connection.
    ///
    /// The closure gets a `&mut Connection` so it can open rusqlite transactions.
    async fn with_conn<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        self.store.with_conn(f).await
    }

    /// Split a caller-facing `"{id}:{uuid}"` receipt handle into its parts.
    ///
    /// Returns `None` for a handle this store could never have issued; ack and
    /// nack treat that exactly like an already-deleted message (idempotent Ok).
    fn parse_receipt_handle(receipt_handle: &str) -> Option<(i64, String)> {
        let (id, receipt) = receipt_handle.split_once(':')?;
        let id: i64 = id.parse().ok()?;
        if receipt.is_empty() {
            return None;
        }
        Some((id, receipt.to_string()))
    }

    /// Claim up to `max_messages` due messages with the given visibility
    /// timeout. This is `receive` minus the batch-size validation and with the
    /// timeout injectable, so tests can force fast redelivery.
    async fn receive_inner(
        &self,
        max_messages: usize,
        visibility: Duration,
    ) -> Result<Vec<QueueMessage>> {
        self.with_conn(move |conn| {
            let now = Utc::now().timestamp_millis();
            let visible_until =
                now.saturating_add(i64::try_from(visibility.as_millis()).unwrap_or(i64::MAX));
            let limit = i64::try_from(max_messages).unwrap_or(i64::MAX);

            // IMMEDIATE takes the write lock at BEGIN: the select-then-claim
            // below is one critical section across all handles and processes.
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .into_alien_error()
                .context(ErrorData::QueueOperationFailed {
                    operation: "receive".to_string(),
                    reason: "failed to begin immediate transaction".to_string(),
                })?;

            let ids: Vec<i64> = {
                let mut stmt = tx
                    .prepare(
                        "SELECT id FROM messages WHERE visible_at <= ?1 \
                         ORDER BY enqueued_at, id LIMIT ?2",
                    )
                    .into_alien_error()
                    .context(ErrorData::QueueOperationFailed {
                        operation: "receive".to_string(),
                        reason: "failed to prepare due-message scan".to_string(),
                    })?;
                let rows = stmt
                    .query_map(params![now, limit], |r| r.get::<_, i64>(0))
                    .into_alien_error()
                    .context(ErrorData::QueueOperationFailed {
                        operation: "receive".to_string(),
                        reason: "failed to scan due messages".to_string(),
                    })?;
                let mut ids = Vec::new();
                for row in rows {
                    ids.push(
                        row.into_alien_error()
                            .context(ErrorData::QueueOperationFailed {
                                operation: "receive".to_string(),
                                reason: "failed to read due-message row".to_string(),
                            })?,
                    );
                }
                ids
            };

            let mut messages = Vec::with_capacity(ids.len());
            for id in ids {
                // The pinned claim statement, with a fresh UUID per row.
                let receipt = uuid::Uuid::new_v4().to_string();
                let (payload_type, payload_data): (String, String) = tx
                    .query_row(
                        "UPDATE messages \
                         SET visible_at = ?1, attempt = attempt + 1, receipt_handle = ?2 \
                         WHERE id = ?3 \
                         RETURNING payload_type, payload_data",
                        params![visible_until, receipt, id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .into_alien_error()
                    .context(ErrorData::QueueOperationFailed {
                        operation: "receive".to_string(),
                        reason: format!("failed to claim message {id}"),
                    })?;
                messages.push(QueueMessage {
                    payload: decode_payload(&payload_type, payload_data)?,
                    receipt_handle: format!("{id}:{receipt}"),
                });
            }

            tx.commit()
                .into_alien_error()
                .context(ErrorData::QueueOperationFailed {
                    operation: "receive".to_string(),
                    reason: "failed to commit receive transaction".to_string(),
                })?;
            Ok(messages)
        })
        .await
    }

    /// Negative-acknowledge a message: make it immediately visible again.
    ///
    /// Same receipt rules as `ack`: a stale receipt (the message was
    /// redelivered and a newer receipt supersedes this one) is rejected; a
    /// receipt for an already-deleted message is an idempotent no-op.
    pub async fn nack(&self, _queue: &str, receipt_handle: &str) -> Result<()> {
        let Some((id, receipt)) = Self::parse_receipt_handle(receipt_handle) else {
            return Ok(());
        };

        self.with_conn(move |conn| {
            let now = Utc::now().timestamp_millis();
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .into_alien_error()
                .context(ErrorData::QueueOperationFailed {
                    operation: "nack".to_string(),
                    reason: "failed to begin immediate transaction".to_string(),
                })?;
            let updated = tx
                .execute(
                    "UPDATE messages SET visible_at = ?1 WHERE id = ?2 AND receipt_handle = ?3",
                    params![now, id, receipt],
                )
                .into_alien_error()
                .context(ErrorData::QueueOperationFailed {
                    operation: "nack".to_string(),
                    reason: format!("failed to nack message {id}"),
                })?;
            if updated == 0 && message_exists(&tx, id, "nack")? {
                return Err(stale_receipt_error(id, "nack"));
            }
            tx.commit()
                .into_alien_error()
                .context(ErrorData::QueueOperationFailed {
                    operation: "nack".to_string(),
                    reason: "failed to commit nack transaction".to_string(),
                })?;
            Ok(())
        })
        .await
    }

    /// Delete every message in the queue, visible or in flight.
    pub async fn purge(&self, _queue: &str) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM messages", [])
                .into_alien_error()
                .context(ErrorData::QueueOperationFailed {
                    operation: "purge".to_string(),
                    reason: "failed to purge queue".to_string(),
                })?;
            Ok(())
        })
        .await
    }
}

/// Does a message row with this id still exist (under the current transaction)?
fn message_exists(tx: &rusqlite::Transaction<'_>, id: i64, operation: &str) -> Result<bool> {
    let exists: Option<i64> = tx
        .query_row("SELECT 1 FROM messages WHERE id = ?1", params![id], |r| {
            r.get(0)
        })
        .optional()
        .into_alien_error()
        .context(ErrorData::QueueOperationFailed {
            operation: operation.to_string(),
            reason: format!("failed to check message {id} existence"),
        })?;
    Ok(exists.is_some())
}

fn stale_receipt_error(id: i64, operation: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::QueueOperationFailed {
        operation: operation.to_string(),
        reason: format!(
            "stale receipt handle for message {id}: the message was redelivered and a newer receipt supersedes this one"
        ),
    })
}

impl Binding for LocalQueue {}

#[async_trait]
impl Queue for LocalQueue {
    async fn send(&self, _queue: &str, message: MessagePayload) -> Result<()> {
        // Encode once, then measure the encoded bytes we will actually store.
        let (payload_type, payload_data) = encode_payload(message)?;
        if payload_data.len() > MAX_MESSAGE_BYTES {
            return Err(AlienError::new(ErrorData::BindingSetupFailed {
                binding_type: "queue.local".to_string(),
                reason: format!(
                    "Message size {} bytes exceeds limit of {} bytes",
                    payload_data.len(),
                    MAX_MESSAGE_BYTES
                ),
            }));
        }

        self.with_conn(move |conn| {
            let now = Utc::now().timestamp_millis();
            conn.execute(
                "INSERT INTO messages (payload_type, payload_data, enqueued_at, visible_at, attempt) \
                 VALUES (?1, ?2, ?3, ?3, 0)",
                params![payload_type, payload_data, now],
            )
            .into_alien_error()
            .context(ErrorData::QueueOperationFailed {
                operation: "send".to_string(),
                reason: "failed to insert message".to_string(),
            })?;
            Ok(())
        })
        .await
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

        self.receive_inner(max_messages, Duration::from_secs(LEASE_SECONDS))
            .await
    }

    async fn ack(&self, _queue: &str, receipt_handle: &str) -> Result<()> {
        // A handle this store never issued behaves like an already-deleted
        // message: idempotent Ok (preserves the historical ack contract).
        let Some((id, receipt)) = Self::parse_receipt_handle(receipt_handle) else {
            return Ok(());
        };

        self.with_conn(move |conn| {
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .into_alien_error()
                .context(ErrorData::QueueOperationFailed {
                    operation: "ack".to_string(),
                    reason: "failed to begin immediate transaction".to_string(),
                })?;
            let deleted = tx
                .execute(
                    "DELETE FROM messages WHERE id = ?1 AND receipt_handle = ?2",
                    params![id, receipt],
                )
                .into_alien_error()
                .context(ErrorData::QueueOperationFailed {
                    operation: "ack".to_string(),
                    reason: format!("failed to ack message {id}"),
                })?;
            if deleted == 0 && message_exists(&tx, id, "ack")? {
                // The row is still there but under a different (newer) receipt:
                // this caller lost its lease. Rejecting prevents a slow consumer
                // from deleting work that has been handed to someone else.
                return Err(stale_receipt_error(id, "ack"));
            }
            tx.commit()
                .into_alien_error()
                .context(ErrorData::QueueOperationFailed {
                    operation: "ack".to_string(),
                    reason: "failed to commit ack transaction".to_string(),
                })?;
            Ok(())
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::collections::BTreeSet;
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::time;

    fn payload_text(msg: &QueueMessage) -> String {
        match &msg.payload {
            MessagePayload::Text(s) => s.clone(),
            MessagePayload::Json(v) => v.to_string(),
        }
    }

    /// Parse the message id out of a `"{id}:{uuid}"` receipt handle.
    fn handle_id(receipt_handle: &str) -> i64 {
        receipt_handle
            .split_once(':')
            .expect("receipt handle must be '{id}:{uuid}'")
            .0
            .parse()
            .expect("receipt handle id must be an integer")
    }

    /// Open a raw connection to the store for white-box column inspection.
    fn raw_conn(queue: &LocalQueue) -> Connection {
        Connection::open(queue.data_dir().join("localqueue.sqlite")).expect("raw open")
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

        // Acking a receipt handle that never existed should succeed.
        queue.ack("q", "non-existent-handle").await.unwrap();

        // Acking an already-deleted message (double ack with the same, current
        // receipt) must also succeed: the row is gone, so the ack is a no-op.
        queue
            .send("q", MessagePayload::Text("msg".to_string()))
            .await
            .unwrap();
        let msgs = queue.receive("q", 1).await.unwrap();
        assert_eq!(msgs.len(), 1);
        queue.ack("q", &msgs[0].receipt_handle).await.unwrap();
        queue.ack("q", &msgs[0].receipt_handle).await.unwrap();
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

    #[tokio::test]
    async fn test_unknown_format_rejected_on_open() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let dir = temp_dir.path().join("queue");

        // Create a valid store, then rewrite its format marker to a future version.
        {
            let queue = LocalQueue::new(dir.clone()).await.expect("initial open");
            queue
                .send("q", MessagePayload::Text("m".to_string()))
                .await
                .unwrap();
        }
        {
            let conn = Connection::open(dir.join("localqueue.sqlite")).expect("raw open");
            conn.execute(
                "UPDATE meta SET value = 'localqueue.v2' WHERE key = 'format'",
                [],
            )
            .expect("format overwrite");
        }

        // Reopening must fail fast, naming both the found and expected formats.
        let err = LocalQueue::new(dir)
            .await
            .expect_err("unknown format must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("localqueue.v2"),
            "error must name the found format, got: {msg}"
        );
        assert!(
            msg.contains("localqueue.v1"),
            "error must name the expected format, got: {msg}"
        );
    }

    // ---- ALIEN-217 localqueue.v1 semantics proofs ----

    #[tokio::test]
    async fn test_visibility_timeout_redelivery_increments_attempt() {
        let (queue, _temp_dir) = create_test_queue().await;

        queue
            .send("q", MessagePayload::Text("retry-me".to_string()))
            .await
            .unwrap();

        // Receive with a short visibility timeout and do NOT ack.
        let first = queue
            .receive_inner(1, Duration::from_millis(100))
            .await
            .unwrap();
        assert_eq!(first.len(), 1);
        let id = handle_id(&first[0].receipt_handle);

        // While the message is in flight, it must not be redelivered.
        let hidden = queue.receive("q", 10).await.unwrap();
        assert!(hidden.is_empty(), "in-flight message must be hidden");

        // After the visibility timeout expires the message is redelivered ...
        time::sleep(Duration::from_millis(250)).await;
        let second = queue.receive("q", 10).await.unwrap();
        assert_eq!(second.len(), 1, "expired message must be redelivered");
        assert_eq!(payload_text(&second[0]), "retry-me");
        assert_eq!(
            handle_id(&second[0].receipt_handle),
            id,
            "redelivery must be the same message row"
        );
        assert_ne!(
            second[0].receipt_handle, first[0].receipt_handle,
            "each delivery must mint a fresh receipt handle"
        );

        // ... with attempt incremented once per delivery (1 then 2).
        let conn = raw_conn(&queue);
        let attempt: i64 = conn
            .query_row("SELECT attempt FROM messages WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .expect("attempt read");
        assert_eq!(attempt, 2, "two deliveries must mean attempt == 2");
    }

    #[tokio::test]
    async fn test_stale_receipt_rejected() {
        let (queue, _temp_dir) = create_test_queue().await;

        queue
            .send("q", MessagePayload::Text("contested".to_string()))
            .await
            .unwrap();

        // Handle A receives with a short visibility timeout and stalls.
        let a = queue
            .receive_inner(1, Duration::from_millis(100))
            .await
            .unwrap();
        assert_eq!(a.len(), 1);

        // The message expires and is redelivered to handle B.
        time::sleep(Duration::from_millis(250)).await;
        let b = queue.receive("q", 1).await.unwrap();
        assert_eq!(b.len(), 1);
        assert_ne!(a[0].receipt_handle, b[0].receipt_handle);

        // A's receipt is stale (B holds the current one): ack must be rejected.
        let err = queue
            .ack("q", &a[0].receipt_handle)
            .await
            .expect_err("stale receipt ack must be rejected");
        assert!(
            err.to_string().to_lowercase().contains("stale"),
            "error should identify the stale receipt, got: {err}"
        );

        // The message must still be there for B, whose current receipt works.
        queue.ack("q", &b[0].receipt_handle).await.unwrap();
        let remaining = queue.receive("q", 10).await.unwrap();
        assert!(remaining.is_empty(), "acked message must be gone");
    }

    #[tokio::test]
    async fn test_nack_makes_message_immediately_visible() {
        let (queue, _temp_dir) = create_test_queue().await;

        queue
            .send("q", MessagePayload::Text("try-again".to_string()))
            .await
            .unwrap();

        let msgs = queue.receive("q", 1).await.unwrap();
        assert_eq!(msgs.len(), 1);

        // In flight under the default 30s lease: hidden without a nack.
        assert!(queue.receive("q", 10).await.unwrap().is_empty());

        queue.nack("q", &msgs[0].receipt_handle).await.unwrap();

        // Immediately visible again — no waiting on the visibility timeout.
        let redelivered = queue.receive("q", 10).await.unwrap();
        assert_eq!(redelivered.len(), 1, "nacked message must be redelivered");
        assert_eq!(payload_text(&redelivered[0]), "try-again");
        assert_ne!(
            redelivered[0].receipt_handle, msgs[0].receipt_handle,
            "redelivery must mint a fresh receipt handle"
        );
    }

    #[tokio::test]
    async fn test_purge_empties_queue() {
        let (queue, _temp_dir) = create_test_queue().await;

        for i in 0..3 {
            queue
                .send("q", MessagePayload::Text(format!("m{i}")))
                .await
                .unwrap();
        }
        // Put one message in flight so purge covers both visible and leased rows.
        let in_flight = queue.receive("q", 1).await.unwrap();
        assert_eq!(in_flight.len(), 1);

        queue.purge("q").await.unwrap();

        assert!(queue.receive("q", 10).await.unwrap().is_empty());
        let conn = raw_conn(&queue);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))
            .expect("count read");
        assert_eq!(count, 0, "purge must delete every row, leased or not");
    }

    // ---- ALIEN-217 multi-process-safety proof ----

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_two_handle_concurrent_receive_no_double_delivery() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let dir = temp_dir.path().join("queue");
        // Two independent handles on the SAME data_dir == two processes sharing the file.
        let queue_a = Arc::new(LocalQueue::new(dir.clone()).await.expect("open handle a"));
        let queue_b = Arc::new(LocalQueue::new(dir.clone()).await.expect("open handle b"));

        let n = 30;
        for i in 0..n {
            queue_a
                .send("q", MessagePayload::Text(format!("msg-{i}")))
                .await
                .unwrap();
        }

        // Six concurrent receivers split across the two handles, each draining
        // in batches. All deliveries happen well inside the default 30s
        // visibility window, so ANY duplicate is a double delivery.
        let mut tasks = Vec::new();
        for t in 0..6 {
            let queue = if t % 2 == 0 {
                queue_a.clone()
            } else {
                queue_b.clone()
            };
            tasks.push(tokio::spawn(async move {
                let mut got: Vec<(i64, String)> = Vec::new();
                let mut consecutive_empty = 0;
                while consecutive_empty < 3 {
                    let batch = queue.receive("q", 5).await.expect("receive ok");
                    assert!(batch.len() <= 5, "batch must respect max_messages");
                    if batch.is_empty() {
                        consecutive_empty += 1;
                        time::sleep(Duration::from_millis(10)).await;
                        continue;
                    }
                    consecutive_empty = 0;
                    for msg in batch {
                        got.push((handle_id(&msg.receipt_handle), payload_text(&msg)));
                    }
                }
                got
            }));
        }

        let mut all: Vec<(i64, String)> = Vec::new();
        for task in tasks {
            all.extend(task.await.expect("task join"));
        }

        // Exactly N deliveries in total — this catches double delivery even
        // before any dedup: 31 deliveries of 30 messages must fail here.
        assert_eq!(
            all.len(),
            n,
            "total deliveries must equal messages sent (no double delivery)"
        );

        // No duplicate message ids ...
        let ids: BTreeSet<i64> = all.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids.len(), n, "every delivered message id must be unique");

        // ... and no duplicate payloads; the union covers every message.
        let payloads: BTreeSet<String> = all.iter().map(|(_, p)| p.clone()).collect();
        let expected: BTreeSet<String> = (0..n).map(|i| format!("msg-{i}")).collect();
        assert_eq!(
            payloads, expected,
            "union of deliveries must cover all messages exactly once"
        );
    }
}
