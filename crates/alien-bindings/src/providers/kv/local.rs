//! Local disk-persisted KV backed by turso (`localkv.v1`), multi-process safe.
//!
//! # Connection strategy
//!
//! turso is async-native and its `Connection` is `Send + Sync`, so there is no
//! `spawn_blocking` boundary and no `Mutex<Connection>` anywhere. `LocalKv`
//! holds one `turso::Database` handle on `<dataDir>/localkv.sqlite`; each
//! operation opens its **own** short-lived connection from it and drops it
//! when the operation completes, so no statement state leaks between
//! operations.
//!
//! Correctness under concurrent access (multiple handles on one file, i.e.
//! multiple processes) comes from turso's multi-process WAL mode — enabled
//! explicitly, experimental upstream, and gated by the multi-handle tests
//! below — plus a `busy_timeout` (writers wait for the write lock instead of
//! failing with `Busy`). The schema is created once in [`LocalKv::new`]. Reads
//! of live rows never take the write lock; a read that encounters an expired
//! row escalates to a short delete (see `get` and `exists`). Conditional puts
//! are a single atomic `INSERT ... ON CONFLICT DO UPDATE ... WHERE` so the
//! race is resolved by the database, not by application-level locking.
//!
//! See `crates/alien-bindings/FORMAT.md` for the on-disk `localkv.v1` contract.
use crate::error::{ErrorData, Result};
use crate::providers::local_store::{
    as_blob, as_i64, as_opt_i64, as_text, opt_i64_value, query_all, LocalStore, StoreSpec,
};
use crate::traits::{Binding, Kv, PutOptions, ScanResult};
use alien_error::{AlienError, Context as _, IntoAlienError as _};
use async_trait::async_trait;
use chrono::Utc;
use std::path::PathBuf;
use turso::Connection;

static KV_SPEC: StoreSpec = StoreSpec {
    db_filename: "localkv.sqlite",
    format_version: "localkv.v1",
    binding_type: "local KV",
    schema_ddl: "CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value BLOB NOT NULL, expires_at INTEGER);",
};

#[derive(Debug)]
pub struct LocalKv {
    store: LocalStore,
}

/// Build the standard KV operation error context.
fn kv_error(operation: &str, key: &str, reason: &str) -> ErrorData {
    ErrorData::KvOperationFailed {
        operation: operation.to_string(),
        key: key.to_string(),
        reason: reason.to_string(),
    }
}

/// Delete a row that is already expired (`expires_at <= now`), used by the
/// lazy-expiry paths of `get` and `exists`.
async fn delete_expired(conn: &Connection, operation: &str, key: &str, now: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM kv WHERE key = ?1 AND expires_at IS NOT NULL AND expires_at <= ?2",
        (key, now),
    )
    .await
    .into_alien_error()
    .context(kv_error(operation, key, "failed to delete expired row"))?;
    Ok(())
}

impl LocalKv {
    pub async fn new(data_dir: PathBuf) -> Result<Self> {
        Ok(Self {
            store: LocalStore::open(data_dir, &KV_SPEC).await?,
        })
    }

    /// Get the data directory path (the directory that holds `localkv.sqlite`).
    pub fn data_dir(&self) -> &PathBuf {
        self.store.data_dir()
    }

    /// Get the number of items currently stored (including expired items).
    /// Useful for testing.
    pub async fn len(&self) -> Result<usize> {
        self.store
            .with_conn(|conn| async move {
                let rows = query_all(&conn, "SELECT COUNT(*) FROM kv", ())
                    .await
                    .into_alien_error()
                    .context(kv_error("len", "*", "failed to count rows"))?;
                let count = rows
                    .first()
                    .and_then(|row| row.first())
                    .and_then(as_i64)
                    .ok_or_else(|| {
                        AlienError::new(kv_error("len", "*", "count query returned no value"))
                    })?;
                Ok(count as usize)
            })
            .await
    }

    /// Check if the store is empty (including expired items).
    /// Useful for testing.
    pub async fn is_empty(&self) -> Result<bool> {
        Ok(self.len().await? == 0)
    }

    /// Clear all data from the store.
    /// Useful for testing.
    pub async fn clear(&self) -> Result<()> {
        self.store
            .with_conn(|conn| async move {
                conn.execute("DELETE FROM kv", ())
                    .await
                    .into_alien_error()
                    .context(kv_error("clear", "*", "failed to clear local KV store"))?;
                Ok(())
            })
            .await
    }

    /// Get all keys currently in the store (including expired ones).
    /// Useful for testing and debugging.
    pub async fn keys(&self) -> Result<Vec<String>> {
        self.store
            .with_conn(|conn| async move {
                let rows = query_all(&conn, "SELECT key FROM kv", ())
                    .await
                    .into_alien_error()
                    .context(kv_error("keys", "*", "failed to scan keys"))?;
                let mut keys = Vec::with_capacity(rows.len());
                for row in &rows {
                    keys.push(row.first().and_then(as_text).ok_or_else(|| {
                        AlienError::new(kv_error("keys", "*", "failed to read key row"))
                    })?);
                }
                Ok(keys)
            })
            .await
    }

    /// Validate key constraints using global KV validation.
    fn validate_key(key: &str) -> Result<()> {
        crate::providers::kv::validate_key(key)
    }

    /// Validate value constraints using global KV validation.
    fn validate_value(value: &[u8]) -> Result<()> {
        crate::providers::kv::validate_value(value)
    }
}

impl Binding for LocalKv {}

#[async_trait]
impl Kv for LocalKv {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        Self::validate_key(key)?;

        self.store
            .with_conn(|conn| async move {
                let now = Utc::now().timestamp_millis();
                let rows = query_all(
                    &conn,
                    "SELECT value, expires_at FROM kv WHERE key = ?1",
                    (key,),
                )
                .await
                .into_alien_error()
                .context(kv_error("get", key, "failed to read value"))?;

                let Some(row) = rows.first() else {
                    return Ok(None);
                };
                let value = row.first().and_then(as_blob).ok_or_else(|| {
                    AlienError::new(kv_error("get", key, "stored value is not a blob"))
                })?;
                let expires_at = row.get(1).and_then(as_opt_i64).ok_or_else(|| {
                    AlienError::new(kv_error("get", key, "stored expires_at is not an integer"))
                })?;

                if matches!(expires_at, Some(exp) if exp <= now) {
                    // Lazily remove the expired row.
                    delete_expired(&conn, "get", key, now).await?;
                    Ok(None)
                } else {
                    Ok(Some(value))
                }
            })
            .await
    }

    async fn put(&self, key: &str, value: Vec<u8>, options: Option<PutOptions>) -> Result<bool> {
        Self::validate_key(key)?;
        Self::validate_value(&value)?;
        let options = options.unwrap_or_default();

        self.store
            .with_conn(|conn| async move {
                let now = Utc::now().timestamp_millis();
                let expires_at: Option<i64> = options
                    .ttl
                    .map(|d| now.saturating_add(i64::try_from(d.as_millis()).unwrap_or(i64::MAX)));

                if options.if_not_exists {
                    // One atomic statement: insert if absent, otherwise overwrite
                    // ONLY when the existing row is already expired. The changed
                    // row count (returned by `execute`) is 1 for the winner, 0
                    // for a loser.
                    let changed = conn
                        .execute(
                            "INSERT INTO kv (key, value, expires_at) VALUES (?1, ?2, ?3) \
                             ON CONFLICT(key) DO UPDATE SET value = ?2, expires_at = ?3 \
                             WHERE kv.expires_at IS NOT NULL AND kv.expires_at <= ?4",
                            (key, value, opt_i64_value(expires_at), now),
                        )
                        .await
                        .into_alien_error()
                        .context(kv_error("put", key, "failed conditional put"))?;
                    Ok(changed == 1)
                } else {
                    conn.execute(
                        "INSERT INTO kv (key, value, expires_at) VALUES (?1, ?2, ?3) \
                         ON CONFLICT(key) DO UPDATE SET value = ?2, expires_at = ?3",
                        (key, value, opt_i64_value(expires_at)),
                    )
                    .await
                    .into_alien_error()
                    .context(kv_error("put", key, "failed to upsert value"))?;
                    Ok(true)
                }
            })
            .await
    }

    async fn delete(&self, key: &str) -> Result<()> {
        Self::validate_key(key)?;

        self.store
            .with_conn(|conn| async move {
                conn.execute("DELETE FROM kv WHERE key = ?1", (key,))
                    .await
                    .into_alien_error()
                    .context(kv_error("delete", key, "failed to delete key"))?;
                Ok(())
            })
            .await
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        Self::validate_key(key)?;

        self.store
            .with_conn(|conn| async move {
                let now = Utc::now().timestamp_millis();
                let rows = query_all(&conn, "SELECT expires_at FROM kv WHERE key = ?1", (key,))
                    .await
                    .into_alien_error()
                    .context(kv_error("exists", key, "failed to check existence"))?;

                let Some(row) = rows.first() else {
                    return Ok(false);
                };
                let expires_at = row.first().and_then(as_opt_i64).ok_or_else(|| {
                    AlienError::new(kv_error(
                        "exists",
                        key,
                        "stored expires_at is not an integer",
                    ))
                })?;

                if matches!(expires_at, Some(exp) if exp <= now) {
                    // Lazily remove the expired row.
                    delete_expired(&conn, "exists", key, now).await?;
                    Ok(false)
                } else {
                    Ok(true)
                }
            })
            .await
    }

    async fn scan_prefix(
        &self,
        prefix: &str,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> Result<ScanResult> {
        Self::validate_key(prefix)?;

        // Parse cursor (simple offset-based pagination for local).
        let start_offset = if let Some(cursor_str) = cursor {
            cursor_str.parse::<usize>().map_err(|_| {
                AlienError::new(ErrorData::InvalidInput {
                    operation_context: "KV scan cursor parsing".to_string(),
                    details: format!("Invalid cursor format: {}", cursor_str),
                    field_name: Some("cursor".to_string()),
                })
            })?
        } else {
            0
        };

        // Collect matching, non-expired items in sorted key order.
        let matching: Vec<(String, Vec<u8>)> = self
            .store
            .with_conn(|conn| async move {
                let now = Utc::now().timestamp_millis();
                let rows = query_all(
                    &conn,
                    "SELECT key, value, expires_at FROM kv WHERE key >= ?1 ORDER BY key",
                    (prefix,),
                )
                .await
                .into_alien_error()
                .context(kv_error("scan_prefix", prefix, "failed to scan prefix"))?;

                let mut matching = Vec::new();
                for row in &rows {
                    let k = row.first().and_then(as_text).ok_or_else(|| {
                        AlienError::new(kv_error(
                            "scan_prefix",
                            prefix,
                            "failed to read scan row key",
                        ))
                    })?;
                    // Keys are ordered ascending starting at `prefix`; once a key
                    // stops matching the prefix, no later key can match either.
                    if !k.starts_with(prefix) {
                        break;
                    }
                    let v = row.get(1).and_then(as_blob).ok_or_else(|| {
                        AlienError::new(kv_error(
                            "scan_prefix",
                            prefix,
                            "stored value is not a blob",
                        ))
                    })?;
                    let exp = row.get(2).and_then(as_opt_i64).ok_or_else(|| {
                        AlienError::new(kv_error(
                            "scan_prefix",
                            prefix,
                            "stored expires_at is not an integer",
                        ))
                    })?;
                    if matches!(exp, Some(e) if e <= now) {
                        continue; // expired: treat as absent
                    }
                    matching.push((k, v));
                }
                Ok(matching)
            })
            .await?;

        // Apply offset-based pagination (results are already sorted by key).
        let total_items = matching.len();
        let end_offset = start_offset + limit.unwrap_or(total_items);

        let items = matching
            .into_iter()
            .skip(start_offset)
            .take(limit.unwrap_or(usize::MAX))
            .collect::<Vec<_>>();

        let next_cursor = if end_offset < total_items {
            Some(end_offset.to_string())
        } else {
            None
        };

        Ok(ScanResult { items, next_cursor })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::local_store::open_database;
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::time;

    async fn create_test_kv() -> (LocalKv, TempDir) {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let kv = LocalKv::new(temp_dir.path().join("kv.db"))
            .await
            .expect("Failed to create LocalKv");
        (kv, temp_dir)
    }

    #[tokio::test]
    async fn test_basic_operations() {
        let (kv, _temp_dir) = create_test_kv().await;

        assert!(kv
            .put("test_key", b"test_value".to_vec(), None)
            .await
            .unwrap());
        let value = kv.get("test_key").await.unwrap();
        assert_eq!(value, Some(b"test_value".to_vec()));

        assert!(kv.exists("test_key").await.unwrap());
        assert!(!kv.exists("nonexistent").await.unwrap());

        kv.delete("test_key").await.unwrap();
        assert!(!kv.exists("test_key").await.unwrap());
        assert_eq!(kv.get("test_key").await.unwrap(), None);
    }

    #[tokio::test]
    async fn test_conditional_put() {
        let (kv, _temp_dir) = create_test_kv().await;

        let options = Some(PutOptions {
            ttl: None,
            if_not_exists: true,
        });
        assert!(kv
            .put("key", b"value1".to_vec(), options.clone())
            .await
            .unwrap());

        assert!(!kv.put("key", b"value2".to_vec(), options).await.unwrap());

        assert_eq!(kv.get("key").await.unwrap(), Some(b"value1".to_vec()));

        assert!(kv.put("key", b"value3".to_vec(), None).await.unwrap());
        assert_eq!(kv.get("key").await.unwrap(), Some(b"value3".to_vec()));
    }

    #[tokio::test]
    async fn test_ttl_expiration() {
        let (kv, _temp_dir) = create_test_kv().await;

        let options = Some(PutOptions {
            ttl: Some(Duration::from_millis(500)),
            if_not_exists: false,
        });

        kv.put("expiring_key", b"value".to_vec(), options)
            .await
            .unwrap();

        assert!(kv.exists("expiring_key").await.unwrap());
        assert_eq!(
            kv.get("expiring_key").await.unwrap(),
            Some(b"value".to_vec())
        );

        time::sleep(Duration::from_millis(750)).await;

        assert!(!kv.exists("expiring_key").await.unwrap());
        assert_eq!(kv.get("expiring_key").await.unwrap(), None);
    }

    #[tokio::test]
    async fn test_prefix_scanning() {
        let (kv, _temp_dir) = create_test_kv().await;

        kv.put("prefix:key1", b"value1".to_vec(), None)
            .await
            .unwrap();
        kv.put("prefix:key2", b"value2".to_vec(), None)
            .await
            .unwrap();
        kv.put("prefix:key3", b"value3".to_vec(), None)
            .await
            .unwrap();
        kv.put("other:key", b"other".to_vec(), None).await.unwrap();

        let result = kv.scan_prefix("prefix:", None, None).await.unwrap();
        assert_eq!(result.items.len(), 3);
        assert!(result.next_cursor.is_none());

        assert_eq!(result.items[0].0, "prefix:key1");
        assert_eq!(result.items[1].0, "prefix:key2");
        assert_eq!(result.items[2].0, "prefix:key3");

        let result = kv.scan_prefix("prefix:", Some(2), None).await.unwrap();
        assert_eq!(result.items.len(), 2);
        assert!(result.next_cursor.is_some());

        let cursor = result.next_cursor.unwrap();
        let result = kv
            .scan_prefix("prefix:", Some(2), Some(cursor))
            .await
            .unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].0, "prefix:key3");
        assert!(result.next_cursor.is_none());
    }

    #[tokio::test]
    async fn test_persistence_across_reopens() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("kv.db");

        {
            let kv = LocalKv::new(db_path.clone())
                .await
                .expect("Failed to create LocalKv");
            kv.put("persistent_key", b"persistent_value".to_vec(), None)
                .await
                .unwrap();
        }

        {
            let kv = LocalKv::new(db_path)
                .await
                .expect("Failed to reopen LocalKv");
            let value = kv.get("persistent_key").await.unwrap();
            assert_eq!(value, Some(b"persistent_value".to_vec()));
        }
    }

    #[tokio::test]
    async fn test_key_validation() {
        let (kv, _temp_dir) = create_test_kv().await;

        assert!(kv.put("", b"value".to_vec(), None).await.is_err());
        assert!(kv.get("").await.is_err());

        let long_key = "a".repeat(513);
        assert!(kv.put(&long_key, b"value".to_vec(), None).await.is_err());

        assert!(kv
            .put("key with spaces", b"value".to_vec(), None)
            .await
            .is_err());
        assert!(kv
            .put("key\nwith\nnewlines", b"value".to_vec(), None)
            .await
            .is_err());
        assert!(kv
            .put("key/with/slashes", b"value".to_vec(), None)
            .await
            .is_err());

        assert!(kv
            .put("valid_key-123", b"value".to_vec(), None)
            .await
            .is_ok());
        assert!(kv
            .put("domain.com:8080", b"value".to_vec(), None)
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn test_value_validation() {
        let (kv, _temp_dir) = create_test_kv().await;

        let large_value = vec![0u8; 24_577];
        assert!(kv.put("key", large_value, None).await.is_err());

        let max_value = vec![0u8; 24_576];
        assert!(kv.put("key", max_value, None).await.is_ok());
    }

    #[tokio::test]
    async fn test_utility_methods() {
        let (kv, _temp_dir) = create_test_kv().await;

        assert!(kv.is_empty().await.unwrap());
        assert_eq!(kv.len().await.unwrap(), 0);
        assert_eq!(kv.keys().await.unwrap(), Vec::<String>::new());

        kv.put("key1", b"value1".to_vec(), None).await.unwrap();
        kv.put("key2", b"value2".to_vec(), None).await.unwrap();

        assert!(!kv.is_empty().await.unwrap());
        assert_eq!(kv.len().await.unwrap(), 2);

        let mut keys = kv.keys().await.unwrap();
        keys.sort();
        assert_eq!(keys, vec!["key1", "key2"]);

        kv.clear().await.unwrap();
        assert!(kv.is_empty().await.unwrap());
        assert_eq!(kv.len().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_unknown_format_rejected_on_open() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let dir = temp_dir.path().join("kv");

        // Create a valid store, then rewrite its format marker to a future version.
        {
            let kv = LocalKv::new(dir.clone()).await.expect("initial open");
            kv.put("k", b"v".to_vec(), None).await.unwrap();
        }
        {
            let db = open_database(&dir.join("localkv.sqlite"), "test")
                .await
                .expect("raw open");
            let conn = db.connect().expect("raw connect");
            conn.execute(
                "UPDATE meta SET value = 'localkv.v2' WHERE key = 'format'",
                (),
            )
            .await
            .expect("format overwrite");
        }

        // Reopening must fail fast, naming both the found and expected formats.
        let err = LocalKv::new(dir)
            .await
            .expect_err("unknown format must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("localkv.v2"),
            "error must name the found format, got: {msg}"
        );
        assert!(
            msg.contains("localkv.v1"),
            "error must name the expected format, got: {msg}"
        );
    }

    // ---- ALIEN-217 multi-process-safety proofs ----

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_conditional_put_atomicity_across_handles() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let dir = temp_dir.path().join("kv");
        // Two independent handles on the SAME data_dir == two processes sharing the file.
        let kv_a = Arc::new(LocalKv::new(dir.clone()).await.expect("open handle a"));
        let kv_b = Arc::new(LocalKv::new(dir.clone()).await.expect("open handle b"));

        let n = 16;
        let mut handles = Vec::new();
        for i in 0..n {
            let kv = if i % 2 == 0 {
                kv_a.clone()
            } else {
                kv_b.clone()
            };
            handles.push(tokio::spawn(async move {
                let val = format!("val-{i}").into_bytes();
                let opts = Some(PutOptions {
                    ttl: None,
                    if_not_exists: true,
                });
                let won = kv.put("race", val.clone(), opts).await.expect("put ok");
                (won, val)
            }));
        }

        let mut winners = Vec::new();
        for h in handles {
            let (won, val) = h.await.expect("task join");
            if won {
                winners.push(val);
            }
        }

        assert_eq!(
            winners.len(),
            1,
            "exactly one conditional put must win across both handles"
        );
        let stored = kv_a.get("race").await.unwrap().expect("key present");
        assert_eq!(stored, winners[0], "stored value must equal the winner");
        assert_eq!(
            kv_b.get("race").await.unwrap().expect("key present via b"),
            winners[0]
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_ttl_expiry_takeover_conditional_put() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let dir = temp_dir.path().join("kv");
        let kv_a = Arc::new(LocalKv::new(dir.clone()).await.expect("open handle a"));
        let kv_b = Arc::new(LocalKv::new(dir.clone()).await.expect("open handle b"));

        // Seed a short-lived key with a conditional put.
        assert!(kv_a
            .put(
                "k",
                b"initial".to_vec(),
                Some(PutOptions {
                    ttl: Some(Duration::from_millis(300)),
                    if_not_exists: true,
                }),
            )
            .await
            .unwrap());

        // While it is still live, a conditional put must lose.
        assert!(!kv_b
            .put(
                "k",
                b"early".to_vec(),
                Some(PutOptions {
                    ttl: None,
                    if_not_exists: true,
                }),
            )
            .await
            .unwrap());

        // Wait for the seeded key to expire.
        time::sleep(Duration::from_millis(450)).await;

        // Race the takeover: many conditional puts against the now-expired key.
        let n = 12;
        let mut handles = Vec::new();
        for i in 0..n {
            let kv = if i % 2 == 0 {
                kv_a.clone()
            } else {
                kv_b.clone()
            };
            handles.push(tokio::spawn(async move {
                let val = format!("takeover-{i}").into_bytes();
                let won = kv
                    .put(
                        "k",
                        val.clone(),
                        Some(PutOptions {
                            ttl: None,
                            if_not_exists: true,
                        }),
                    )
                    .await
                    .expect("put ok");
                (won, val)
            }));
        }

        let mut winners = Vec::new();
        for h in handles {
            let (won, val) = h.await.expect("task join");
            if won {
                winners.push(val);
            }
        }

        assert_eq!(
            winners.len(),
            1,
            "exactly one takeover conditional put must win after expiry"
        );
        let stored = kv_b.get("k").await.unwrap().expect("key present");
        assert_eq!(
            stored, winners[0],
            "stored value must equal the takeover winner"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_multi_handle_concurrent_smoke() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let dir = temp_dir.path().join("kv");
        let kv_a = Arc::new(LocalKv::new(dir.clone()).await.expect("open handle a"));
        let kv_b = Arc::new(LocalKv::new(dir.clone()).await.expect("open handle b"));

        let mut handles = Vec::new();
        for i in 0..50 {
            let kv = if i % 2 == 0 {
                kv_a.clone()
            } else {
                kv_b.clone()
            };
            handles.push(tokio::spawn(async move {
                let key = format!("key_{i}");
                let val = format!("v{i}").into_bytes();
                // No busy errors expected under multi-process WAL + busy_timeout.
                kv.put(&key, val.clone(), None).await.expect("put ok");
                let got = kv.get(&key).await.expect("get ok");
                assert_eq!(got, Some(val));
            }));
        }
        for h in handles {
            h.await.expect("task join");
        }

        assert_eq!(kv_a.len().await.unwrap(), 50, "handle a sees all keys");
        assert_eq!(kv_b.len().await.unwrap(), 50, "handle b sees all keys");
    }
}
