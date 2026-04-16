//! Integration tests for LocalStorageManager + LocalStorage binding
//!
//! These tests verify that:
//! 1. Manager creates directories usable by LocalStorage binding
//! 2. Health checks actually use object_store
//! 3. Data persists across manager recreations

use alien_bindings::providers::storage::local::LocalStorage;
use alien_local::LocalStorageManager;
use bytes::Bytes;
use object_store::path::Path;
use object_store::ObjectStore;
use tempfile::TempDir;

/// Manager creates storage that LocalStorage binding can actually use
#[tokio::test]
async fn test_manager_creates_usable_storage() {
    let temp_dir = TempDir::new().unwrap();
    let manager = LocalStorageManager::new(temp_dir.path().to_path_buf());

    // Manager creates storage
    manager.create_storage("my-storage").await.unwrap();

    // Get binding and extract storage path
    let binding = manager.get_binding("my-storage").unwrap();
    let storage_path = match binding {
        alien_core::bindings::StorageBinding::Local(local) => local
            .storage_path
            .into_value("my-storage", "storage_path")
            .unwrap(),
        _ => panic!("Expected Local storage binding"),
    };
    let storage = LocalStorage::new(storage_path).unwrap();

    // Actually use it - write and read
    let path = Path::from("test-file.txt");
    storage
        .put(&path, Bytes::from("hello world").into())
        .await
        .unwrap();
    let result = storage.get(&path).await.unwrap();
    assert_eq!(result.bytes().await.unwrap(), Bytes::from("hello world"));
}

/// Health check passes for healthy storage, fails for missing
#[tokio::test]
async fn test_health_check_validates_storage() {
    let temp_dir = TempDir::new().unwrap();
    let manager = LocalStorageManager::new(temp_dir.path().to_path_buf());

    // Missing storage fails health check
    assert!(manager.check_health("nonexistent").await.is_err());

    // Create and verify health passes
    manager.create_storage("healthy").await.unwrap();
    manager.check_health("healthy").await.unwrap();

    // Delete and verify health fails again
    manager.delete_storage("healthy").await.unwrap();
    assert!(manager.check_health("healthy").await.is_err());
}

/// Storage persists data across manager recreations (simulates CLI restart)
#[tokio::test]
async fn test_storage_data_persists_across_sessions() {
    let temp_dir = TempDir::new().unwrap();
    let path = Path::from("persistent.txt");
    let data = Bytes::from("persistent data");

    // Create and write with first manager instance
    {
        let manager = LocalStorageManager::new(temp_dir.path().to_path_buf());
        manager.create_storage("persist-test").await.unwrap();
        let binding = manager.get_binding("persist-test").unwrap();
        let storage_path = match binding {
            alien_core::bindings::StorageBinding::Local(local) => local
                .storage_path
                .into_value("persist-test", "storage_path")
                .unwrap(),
            _ => panic!("Expected Local storage binding"),
        };
        let storage = LocalStorage::new(storage_path).unwrap();
        storage.put(&path, data.clone().into()).await.unwrap();
    }

    // Read with fresh manager instance
    {
        let manager = LocalStorageManager::new(temp_dir.path().to_path_buf());
        // Storage should still exist (directory persists)
        assert!(manager.storage_exists("persist-test"));

        let binding = manager.get_binding("persist-test").unwrap();
        let storage_path = match binding {
            alien_core::bindings::StorageBinding::Local(local) => local
                .storage_path
                .into_value("persist-test", "storage_path")
                .unwrap(),
            _ => panic!("Expected Local storage binding"),
        };
        let storage = LocalStorage::new(storage_path).unwrap();
        let result = storage.get(&path).await.unwrap();
        assert_eq!(result.bytes().await.unwrap(), data);
    }
}

/// get_binding fails for non-existent storage
#[tokio::test]
async fn test_get_binding_fails_for_missing_storage() {
    let temp_dir = TempDir::new().unwrap();
    let manager = LocalStorageManager::new(temp_dir.path().to_path_buf());

    let result = manager.get_binding("nonexistent");
    assert!(result.is_err());
}

/// Multiple storages can coexist
#[tokio::test]
async fn test_multiple_storages_coexist() {
    let temp_dir = TempDir::new().unwrap();
    let manager = LocalStorageManager::new(temp_dir.path().to_path_buf());

    // Create multiple storages
    manager.create_storage("storage-a").await.unwrap();
    manager.create_storage("storage-b").await.unwrap();

    // Get bindings for both
    let binding_a = manager.get_binding("storage-a").unwrap();
    let binding_b = manager.get_binding("storage-b").unwrap();

    let storage_path_a = match binding_a {
        alien_core::bindings::StorageBinding::Local(local) => local
            .storage_path
            .into_value("storage-a", "storage_path")
            .unwrap(),
        _ => panic!("Expected Local storage binding"),
    };
    let storage_path_b = match binding_b {
        alien_core::bindings::StorageBinding::Local(local) => local
            .storage_path
            .into_value("storage-b", "storage_path")
            .unwrap(),
        _ => panic!("Expected Local storage binding"),
    };

    let storage_a = LocalStorage::new(storage_path_a).unwrap();
    let storage_b = LocalStorage::new(storage_path_b).unwrap();

    // Write to both
    let path = Path::from("test.txt");
    storage_a
        .put(&path, Bytes::from("data-a").into())
        .await
        .unwrap();
    storage_b
        .put(&path, Bytes::from("data-b").into())
        .await
        .unwrap();

    // Verify data is isolated
    let result_a = storage_a.get(&path).await.unwrap().bytes().await.unwrap();
    let result_b = storage_b.get(&path).await.unwrap().bytes().await.unwrap();

    assert_eq!(result_a, Bytes::from("data-a"));
    assert_eq!(result_b, Bytes::from("data-b"));
}

/// List operation works on storage
#[tokio::test]
async fn test_storage_list_operation() {
    use futures::TryStreamExt;

    let temp_dir = TempDir::new().unwrap();
    let manager = LocalStorageManager::new(temp_dir.path().to_path_buf());

    manager.create_storage("list-test").await.unwrap();
    let binding = manager.get_binding("list-test").unwrap();
    let storage_path = match binding {
        alien_core::bindings::StorageBinding::Local(local) => local
            .storage_path
            .into_value("list-test", "storage_path")
            .unwrap(),
        _ => panic!("Expected Local storage binding"),
    };
    let storage = LocalStorage::new(storage_path).unwrap();

    // Write multiple files
    storage
        .put(&Path::from("file1.txt"), Bytes::from("a").into())
        .await
        .unwrap();
    storage
        .put(&Path::from("file2.txt"), Bytes::from("b").into())
        .await
        .unwrap();
    storage
        .put(&Path::from("subdir/file3.txt"), Bytes::from("c").into())
        .await
        .unwrap();

    // List all
    let items: Vec<_> = storage.list(None).try_collect().await.unwrap();

    assert_eq!(items.len(), 3);
}
