//! Integration tests for LocalKvManager + LocalKv binding
//!
//! These tests verify that:
//! 1. Manager creates KV databases usable by LocalKv binding
//! 2. Health checks actually open sled databases
//! 3. Data persists across sessions

use alien_bindings::providers::kv::local::LocalKv;
use alien_bindings::traits::{Kv, PutOptions};
use alien_core::bindings::KvBinding;
use alien_local::LocalKvManager;
use std::time::Duration;
use tempfile::TempDir;

/// Manager creates KV database that LocalKv binding can actually use
#[tokio::test]
async fn test_manager_creates_usable_kv() {
    let temp_dir = TempDir::new().unwrap();
    let manager = LocalKvManager::new(temp_dir.path().to_path_buf());

    // Manager creates KV path (returns the path, but sled creates the actual DB)
    let kv_path = manager.create_kv("my-kv").await.unwrap();

    // LocalKv opens sled at that path (sled creates the directory)
    let kv = LocalKv::new(kv_path).await.unwrap();

    // Actually use it
    kv.put("key1", b"value1".to_vec(), Some(PutOptions::default()))
        .await
        .unwrap();
    let value = kv.get("key1").await.unwrap();
    assert_eq!(value, Some(b"value1".to_vec()));
}

/// Health check actually opens sled database
#[tokio::test]
async fn test_health_check_opens_database() {
    let temp_dir = TempDir::new().unwrap();
    let manager = LocalKvManager::new(temp_dir.path().to_path_buf());

    // Missing KV fails
    assert!(manager.check_health("nonexistent").await.is_err());

    // Create KV path and open database (sled creates the dir on open)
    let kv_path = manager.create_kv("healthy-kv").await.unwrap();
    {
        let _kv = LocalKv::new(kv_path).await.unwrap();
        // KV is dropped here, releasing the sled lock
    }

    // Now health check should pass (database exists and unlocked)
    manager.check_health("healthy-kv").await.unwrap();

    // Delete and verify health fails
    manager.delete_kv("healthy-kv").await.unwrap();
    assert!(manager.check_health("healthy-kv").await.is_err());
}

/// KV data persists across sessions (simulates CLI restart)
#[tokio::test]
async fn test_kv_data_persists_across_sessions() {
    let temp_dir = TempDir::new().unwrap();

    // Write with first KV instance
    {
        let manager = LocalKvManager::new(temp_dir.path().to_path_buf());
        let kv_path = manager.create_kv("persist-kv").await.unwrap();
        let kv = LocalKv::new(kv_path).await.unwrap();
        kv.put(
            "persistent-key",
            b"persistent-value".to_vec(),
            Some(PutOptions::default()),
        )
        .await
        .unwrap();
    }

    // Read with fresh instance
    {
        let manager = LocalKvManager::new(temp_dir.path().to_path_buf());
        // KV should still exist (sled created the directory)
        assert!(manager.kv_exists("persist-kv"));

        let kv_path = manager.get_kv_path("persist-kv").unwrap();
        let kv = LocalKv::new(kv_path).await.unwrap();
        let value = kv.get("persistent-key").await.unwrap();
        assert_eq!(value, Some(b"persistent-value".to_vec()));
    }
}

/// get_binding returns correct KvBinding::Local variant
#[tokio::test]
async fn test_get_binding_returns_local_variant() {
    let temp_dir = TempDir::new().unwrap();
    let manager = LocalKvManager::new(temp_dir.path().to_path_buf());

    // Create path and open with sled (so directory exists)
    let kv_path = manager.create_kv("binding-test").await.unwrap();
    let _kv = LocalKv::new(kv_path).await.unwrap();

    // Now get_binding should work (path exists)
    let binding = manager.get_binding("binding-test").unwrap();

    // Verify it's the Local variant
    match binding {
        KvBinding::Local(config) => {
            let data_dir = config
                .data_dir
                .into_value("binding-test", "data_dir")
                .unwrap();
            assert!(data_dir.contains("binding-test"));
        }
        _ => panic!("Expected Local binding variant"),
    }
}

/// get_binding fails for non-existent KV
#[tokio::test]
async fn test_get_binding_fails_for_missing_kv() {
    let temp_dir = TempDir::new().unwrap();
    let manager = LocalKvManager::new(temp_dir.path().to_path_buf());

    let result = manager.get_binding("nonexistent");
    assert!(result.is_err());
}

/// Multiple KV databases can coexist
#[tokio::test]
async fn test_multiple_kvs_coexist() {
    let temp_dir = TempDir::new().unwrap();
    let manager = LocalKvManager::new(temp_dir.path().to_path_buf());

    // Create multiple KV paths and open them
    let path_a = manager.create_kv("kv-a").await.unwrap();
    let path_b = manager.create_kv("kv-b").await.unwrap();

    let kv_a = LocalKv::new(path_a).await.unwrap();
    let kv_b = LocalKv::new(path_b).await.unwrap();

    // Write same key to both
    kv_a.put("key", b"value-a".to_vec(), Some(PutOptions::default()))
        .await
        .unwrap();
    kv_b.put("key", b"value-b".to_vec(), Some(PutOptions::default()))
        .await
        .unwrap();

    // Verify data is isolated
    assert_eq!(kv_a.get("key").await.unwrap(), Some(b"value-a".to_vec()));
    assert_eq!(kv_b.get("key").await.unwrap(), Some(b"value-b".to_vec()));
}

/// KV TTL functionality works
#[tokio::test]
async fn test_kv_ttl_functionality() {
    let temp_dir = TempDir::new().unwrap();
    let manager = LocalKvManager::new(temp_dir.path().to_path_buf());

    let kv_path = manager.create_kv("ttl-test").await.unwrap();
    let kv = LocalKv::new(kv_path).await.unwrap();

    // Put with short TTL
    let options = PutOptions {
        ttl: Some(Duration::from_millis(100)),
        ..Default::default()
    };
    kv.put("expiring-key", b"value".to_vec(), Some(options))
        .await
        .unwrap();

    // Should exist immediately
    assert!(kv.get("expiring-key").await.unwrap().is_some());

    // Wait for expiration
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Should be gone
    assert!(kv.get("expiring-key").await.unwrap().is_none());
}

/// KV scan_prefix works
#[tokio::test]
async fn test_kv_scan_prefix() {
    let temp_dir = TempDir::new().unwrap();
    let manager = LocalKvManager::new(temp_dir.path().to_path_buf());

    let kv_path = manager.create_kv("scan-test").await.unwrap();
    let kv = LocalKv::new(kv_path).await.unwrap();

    // Insert keys with different prefixes
    kv.put("user:1", b"alice".to_vec(), Some(PutOptions::default()))
        .await
        .unwrap();
    kv.put("user:2", b"bob".to_vec(), Some(PutOptions::default()))
        .await
        .unwrap();
    kv.put(
        "config:theme",
        b"dark".to_vec(),
        Some(PutOptions::default()),
    )
    .await
    .unwrap();

    // Scan user prefix
    let result = kv.scan_prefix("user:", None, None).await.unwrap();
    assert_eq!(result.items.len(), 2);
    assert!(result.items.iter().all(|(k, _)| k.starts_with("user:")));

    // Scan config prefix
    let result = kv.scan_prefix("config:", None, None).await.unwrap();
    assert_eq!(result.items.len(), 1);
}
