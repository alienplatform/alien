use crate::error::{ErrorData, Result};
use crate::traits::{Binding, Kv, PutOptions, ScanResult};
use alien_error::{
    AlienError, Context as _, ContextError as _, IntoAlienError as _, IntoAlienErrorDirect,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// Local disk-persisted KV implementation using sled embedded database
///
/// This provides a persistent, thread-safe, disk-based key-value store that implements
/// all KV trait features including TTL, conditional puts, and prefix scanning.
/// Perfect for local development and testing that needs data persistence across restarts.
#[derive(Debug)]
pub struct LocalKv {
    db: Arc<Mutex<sled::Db>>,
    data_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredValue {
    value: Vec<u8>,
    expires_at: Option<DateTime<Utc>>,
}

impl StoredValue {
    fn new(value: Vec<u8>, ttl: Option<Duration>) -> Self {
        let expires_at = ttl
            .map(|duration| Utc::now() + chrono::Duration::from_std(duration).unwrap_or_default());

        Self { value, expires_at }
    }

    fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            Utc::now() >= expires_at
        } else {
            false
        }
    }
}

impl LocalKv {
    /// Create a new local KV store with the given data directory
    pub async fn new(data_dir: PathBuf) -> Result<Self> {
        tracing::debug!(data_dir = %data_dir.display(), "Opening LocalKv database");

        // Ensure the data directory exists
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
                    binding_type: "local KV".to_string(),
                    reason: format!("Failed to open sled database at: {:?}", data_dir),
                })?;

        tracing::debug!(data_dir = %data_dir.display(), "LocalKv database opened successfully");

        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            data_dir,
        })
    }

    /// Get the data directory path
    pub fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    /// Get the number of items currently stored (including expired items)
    /// Useful for testing
    pub async fn len(&self) -> Result<usize> {
        let db = self.db.lock().await;
        Ok(db.len())
    }

    /// Check if the store is empty (including expired items)
    /// Useful for testing
    pub async fn is_empty(&self) -> Result<bool> {
        let db = self.db.lock().await;
        Ok(db.is_empty())
    }

    /// Clear all data from the store
    /// Useful for testing
    pub async fn clear(&self) -> Result<()> {
        let db = self.db.lock().await;
        db.clear()
            .into_alien_error()
            .context(ErrorData::KvOperationFailed {
                operation: "clear".to_string(),
                key: "*".to_string(),
                reason: "Failed to clear local KV store".to_string(),
            })?;
        Ok(())
    }

    /// Get all keys currently in the store (including expired ones)
    /// Useful for testing and debugging
    pub async fn keys(&self) -> Result<Vec<String>> {
        let db = self.db.lock().await;
        let mut keys = Vec::new();

        for result in db.iter() {
            let (key, _) = result
                .into_alien_error()
                .context(ErrorData::KvOperationFailed {
                    operation: "scan keys".to_string(),
                    key: "<unknown>".to_string(),
                    reason: "Failed to iterate over keys".to_string(),
                })?;

            let key_str = String::from_utf8(key.to_vec()).into_alien_error().context(
                ErrorData::KvOperationFailed {
                    operation: "decode key".to_string(),
                    key: "<invalid UTF-8>".to_string(),
                    reason: "Invalid UTF-8 in stored key".to_string(),
                },
            )?;

            keys.push(key_str);
        }

        Ok(keys)
    }

    /// Validate key constraints using global KV validation
    fn validate_key(key: &str) -> Result<()> {
        crate::providers::kv::validate_key(key)
    }

    /// Validate value constraints using global KV validation
    fn validate_value(value: &[u8]) -> Result<()> {
        crate::providers::kv::validate_value(value)
    }

    /// Serialize a stored value to bytes
    fn serialize_value(stored_value: &StoredValue) -> Result<Vec<u8>> {
        serde_json::to_vec(stored_value)
            .into_alien_error()
            .context(ErrorData::KvOperationFailed {
                operation: "serialize value".to_string(),
                key: "<unknown>".to_string(),
                reason: "Failed to serialize value to JSON".to_string(),
            })
    }

    /// Deserialize bytes to a stored value
    fn deserialize_value(bytes: &[u8]) -> Result<StoredValue> {
        serde_json::from_slice(bytes)
            .into_alien_error()
            .context(ErrorData::KvOperationFailed {
                operation: "deserialize value".to_string(),
                key: "<unknown>".to_string(),
                reason: "Failed to deserialize value from JSON".to_string(),
            })
    }
}

impl Binding for LocalKv {}

#[async_trait]
impl Kv for LocalKv {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        Self::validate_key(key)?;

        let db = self.db.lock().await;

        let value_bytes = match db.get(key.as_bytes()) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return Ok(None),
            Err(e) => {
                return Err(e.into_alien_error().context(ErrorData::KvOperationFailed {
                    operation: "get".to_string(),
                    key: key.to_string(),
                    reason: "Failed to retrieve value from sled database".to_string(),
                }));
            }
        };

        let stored_value = Self::deserialize_value(&value_bytes)?;

        if stored_value.is_expired() {
            // Lazily remove expired items
            let _ = db.remove(key.as_bytes());
            Ok(None)
        } else {
            Ok(Some(stored_value.value))
        }
    }

    async fn put(&self, key: &str, value: Vec<u8>, options: Option<PutOptions>) -> Result<bool> {
        Self::validate_key(key)?;
        Self::validate_value(&value)?;

        let db = self.db.lock().await;
        let options = options.unwrap_or_default();

        // Handle conditional put (if_not_exists)
        if options.if_not_exists {
            if let Some(existing_bytes) =
                db.get(key.as_bytes())
                    .into_alien_error()
                    .context(ErrorData::KvOperationFailed {
                        operation: "conditional put check".to_string(),
                        key: key.to_string(),
                        reason: "Failed to check existing key".to_string(),
                    })?
            {
                // Check if existing value is expired
                if let Ok(existing_stored) = Self::deserialize_value(&existing_bytes) {
                    if !existing_stored.is_expired() {
                        return Ok(false); // Key exists and is not expired
                    }
                }
                // If we can't deserialize or it's expired, we can overwrite
            }
        }

        let stored_value = StoredValue::new(value, options.ttl);
        let serialized = Self::serialize_value(&stored_value)?;

        db.insert(key.as_bytes(), serialized)
            .into_alien_error()
            .context(ErrorData::KvOperationFailed {
                operation: "put".to_string(),
                key: key.to_string(),
                reason: "Failed to insert value into sled database".to_string(),
            })?;

        // Ensure data is persisted to disk without blocking the Tokio thread
        db.flush_async()
            .await
            .into_alien_error()
            .context(ErrorData::KvOperationFailed {
                operation: "flush".to_string(),
                key: key.to_string(),
                reason: "Failed to flush data to disk".to_string(),
            })?;

        tracing::info!(key = %key, data_dir = %self.data_dir.display(), "LocalKv::put completed successfully and flushed");

        Ok(true)
    }

    async fn delete(&self, key: &str) -> Result<()> {
        Self::validate_key(key)?;

        let db = self.db.lock().await;
        db.remove(key.as_bytes())
            .into_alien_error()
            .context(ErrorData::KvOperationFailed {
                operation: "delete".to_string(),
                key: key.to_string(),
                reason: "Failed to remove key from sled database".to_string(),
            })?;

        // Ensure deletion is persisted to disk without blocking the Tokio thread
        db.flush_async()
            .await
            .into_alien_error()
            .context(ErrorData::KvOperationFailed {
                operation: "flush".to_string(),
                key: key.to_string(),
                reason: "Failed to flush deletion to disk".to_string(),
            })?;

        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        Self::validate_key(key)?;

        let db = self.db.lock().await;

        match db.get(key.as_bytes()) {
            Ok(Some(bytes)) => {
                let stored_value = Self::deserialize_value(&bytes)?;
                if stored_value.is_expired() {
                    // Lazily remove expired items
                    let _ = db.remove(key.as_bytes());
                    Ok(false)
                } else {
                    Ok(true)
                }
            }
            Ok(None) => Ok(false),
            Err(e) => Err(e.into_alien_error().context(ErrorData::KvOperationFailed {
                operation: "exists".to_string(),
                key: key.to_string(),
                reason: "Failed to check key existence in sled database".to_string(),
            })),
        }
    }

    async fn scan_prefix(
        &self,
        prefix: &str,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> Result<ScanResult> {
        Self::validate_key(prefix)?;

        let db = self.db.lock().await;

        // Parse cursor if provided (simple offset-based pagination for local)
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

        // Collect matching, non-expired keys
        let mut matching_items: Vec<(String, Vec<u8>)> = Vec::new();

        for result in db.scan_prefix(prefix.as_bytes()) {
            let (key_bytes, value_bytes) =
                result
                    .into_alien_error()
                    .context(ErrorData::KvOperationFailed {
                        operation: "scan_prefix".to_string(),
                        key: prefix.to_string(),
                        reason: "Failed to scan prefix in sled database".to_string(),
                    })?;

            let key = String::from_utf8(key_bytes.to_vec())
                .into_alien_error()
                .context(ErrorData::KvOperationFailed {
                    operation: "decode key".to_string(),
                    key: prefix.to_string(),
                    reason: "Invalid UTF-8 in stored key during scan".to_string(),
                })?;

            if let Ok(stored_value) = Self::deserialize_value(&value_bytes) {
                if !stored_value.is_expired() {
                    matching_items.push((key, stored_value.value));
                }
            }
        }

        // Sort for deterministic behavior
        matching_items.sort_by(|a, b| a.0.cmp(&b.0));

        // Apply pagination
        let total_items = matching_items.len();
        let end_offset = start_offset + limit.unwrap_or(total_items);

        let items = matching_items
            .into_iter()
            .skip(start_offset)
            .take(limit.unwrap_or(usize::MAX))
            .collect::<Vec<_>>();

        // Generate next cursor if there are more items
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

        // Test put and get
        assert!(kv
            .put("test_key", b"test_value".to_vec(), None)
            .await
            .unwrap());
        let value = kv.get("test_key").await.unwrap();
        assert_eq!(value, Some(b"test_value".to_vec()));

        // Test exists
        assert!(kv.exists("test_key").await.unwrap());
        assert!(!kv.exists("nonexistent").await.unwrap());

        // Test delete
        kv.delete("test_key").await.unwrap();
        assert!(!kv.exists("test_key").await.unwrap());
        assert_eq!(kv.get("test_key").await.unwrap(), None);
    }

    #[tokio::test]
    async fn test_conditional_put() {
        let (kv, _temp_dir) = create_test_kv().await;

        // First put should succeed
        let options = Some(PutOptions {
            ttl: None,
            if_not_exists: true,
        });
        assert!(kv
            .put("key", b"value1".to_vec(), options.clone())
            .await
            .unwrap());

        // Second put should fail due to if_not_exists
        assert!(!kv.put("key", b"value2".to_vec(), options).await.unwrap());

        // Value should still be the original
        assert_eq!(kv.get("key").await.unwrap(), Some(b"value1".to_vec()));

        // Regular put should succeed
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

        // Should exist immediately after put completes
        assert!(kv.exists("expiring_key").await.unwrap());
        assert_eq!(
            kv.get("expiring_key").await.unwrap(),
            Some(b"value".to_vec())
        );

        // Wait for expiration
        time::sleep(Duration::from_millis(750)).await;

        // Should be expired now
        assert!(!kv.exists("expiring_key").await.unwrap());
        assert_eq!(kv.get("expiring_key").await.unwrap(), None);
    }

    #[tokio::test]
    async fn test_prefix_scanning() {
        let (kv, _temp_dir) = create_test_kv().await;

        // Insert test data
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

        // Scan with prefix
        let result = kv.scan_prefix("prefix:", None, None).await.unwrap();
        assert_eq!(result.items.len(), 3);
        assert!(result.next_cursor.is_none());

        // Check items are sorted
        assert_eq!(result.items[0].0, "prefix:key1");
        assert_eq!(result.items[1].0, "prefix:key2");
        assert_eq!(result.items[2].0, "prefix:key3");

        // Test with limit
        let result = kv.scan_prefix("prefix:", Some(2), None).await.unwrap();
        assert_eq!(result.items.len(), 2);
        assert!(result.next_cursor.is_some());

        // Test pagination
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

        // Create KV, add data, and drop it
        {
            let kv = LocalKv::new(db_path.clone())
                .await
                .expect("Failed to create LocalKv");
            kv.put("persistent_key", b"persistent_value".to_vec(), None)
                .await
                .unwrap();
        }

        // Reopen and verify data persists
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

        // Empty key should fail
        assert!(kv.put("", b"value".to_vec(), None).await.is_err());
        assert!(kv.get("").await.is_err());

        // Key too long should fail
        let long_key = "a".repeat(513);
        assert!(kv.put(&long_key, b"value".to_vec(), None).await.is_err());

        // Invalid characters should fail
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

        // Valid keys should succeed
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

        // Value too large should fail
        let large_value = vec![0u8; 24_577]; // Just over 24 KiB
        assert!(kv.put("key", large_value, None).await.is_err());

        // Maximum size value should succeed
        let max_value = vec![0u8; 24_576]; // Exactly 24 KiB
        assert!(kv.put("key", max_value, None).await.is_ok());
    }

    #[tokio::test]
    async fn test_utility_methods() {
        let (kv, _temp_dir) = create_test_kv().await;

        // Initially empty
        assert!(kv.is_empty().await.unwrap());
        assert_eq!(kv.len().await.unwrap(), 0);
        assert_eq!(kv.keys().await.unwrap(), Vec::<String>::new());

        // Add some data
        kv.put("key1", b"value1".to_vec(), None).await.unwrap();
        kv.put("key2", b"value2".to_vec(), None).await.unwrap();

        assert!(!kv.is_empty().await.unwrap());
        assert_eq!(kv.len().await.unwrap(), 2);

        let mut keys = kv.keys().await.unwrap();
        keys.sort();
        assert_eq!(keys, vec!["key1", "key2"]);

        // Clear
        kv.clear().await.unwrap();
        assert!(kv.is_empty().await.unwrap());
        assert_eq!(kv.len().await.unwrap(), 0);
    }
}
