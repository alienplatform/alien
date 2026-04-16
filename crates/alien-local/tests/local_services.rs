//! Full integration tests for LocalBindingsProvider
//!
//! These tests verify the complete workflow:
//! 1. LocalBindingsProvider creates and manages all managers
//! 2. Correctly loads bindings from managers
//! 3. Graceful shutdown works correctly
//!
//! Note: KV is tested separately in kv_integration.rs because sled creates
//! the database directory on open, not during create_kv().

use alien_bindings::providers::kv::local::LocalKv;
use alien_bindings::traits::{BindingsProviderApi, PutOptions};
use alien_local::LocalBindingsProvider;
use bytes::Bytes;
use object_store::path::Path;
use tempfile::TempDir;

/// Full workflow: LocalBindingsProvider → managers → bindings
#[tokio::test]
async fn test_full_local_workflow() {
    let temp_dir = TempDir::new().unwrap();
    let provider = LocalBindingsProvider::new(temp_dir.path()).unwrap();

    // Create resources through managers
    provider
        .storage_manager()
        .create_storage("test-storage")
        .await
        .unwrap();
    provider
        .vault_manager()
        .create_vault("test-vault")
        .await
        .unwrap();

    // For KV, we need to open it first to create the database directory
    // (sled creates the directory, not the manager)
    let kv_path = provider.kv_manager().create_kv("test-kv").await.unwrap();
    {
        let _kv = LocalKv::new(kv_path).await.unwrap();
    }

    // Load bindings through provider (like runtime would)
    let storage = provider.load_storage("test-storage").await.unwrap();
    let kv = provider.load_kv("test-kv").await.unwrap();
    let vault = provider.load_vault("test-vault").await.unwrap();

    // Use the bindings
    storage
        .put(&Path::from("file.txt"), Bytes::from("data").into())
        .await
        .unwrap();
    kv.put("key", b"value".to_vec(), Some(PutOptions::default()))
        .await
        .unwrap();
    vault.set_secret("secret", "value").await.unwrap();

    // Verify data
    assert_eq!(
        storage
            .get(&Path::from("file.txt"))
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap(),
        Bytes::from("data")
    );
    assert_eq!(kv.get("key").await.unwrap(), Some(b"value".to_vec()));
    assert_eq!(vault.get_secret("secret").await.unwrap(), "value");
}

/// Provider errors correctly for missing resources
#[tokio::test]
async fn test_provider_errors_for_missing_resources() {
    let temp_dir = TempDir::new().unwrap();
    let provider = LocalBindingsProvider::new(temp_dir.path()).unwrap();

    // All should fail - resources don't exist
    assert!(provider.load_storage("nonexistent").await.is_err());
    assert!(provider.load_kv("nonexistent").await.is_err());
    assert!(provider.load_vault("nonexistent").await.is_err());
}

/// Graceful shutdown waits for background tasks
#[tokio::test]
async fn test_graceful_shutdown() {
    let temp_dir = TempDir::new().unwrap();
    let provider = LocalBindingsProvider::new(temp_dir.path()).unwrap();

    // Start a registry (has background task)
    provider
        .artifact_registry_manager()
        .start_registry("shutdown-test")
        .await
        .unwrap();

    // Shutdown should complete without hanging
    let shutdown_future = provider.shutdown();
    tokio::time::timeout(std::time::Duration::from_secs(5), shutdown_future)
        .await
        .expect("Shutdown should complete within 5 seconds");
}

/// LocalBindingsProvider clones share the same underlying managers
#[tokio::test]
async fn test_provider_clone_shares_state() {
    let temp_dir = TempDir::new().unwrap();
    let provider1 = LocalBindingsProvider::new(temp_dir.path()).unwrap();
    let provider2 = provider1.clone();

    // Create storage through provider1
    provider1
        .storage_manager()
        .create_storage("shared-storage")
        .await
        .unwrap();

    // Should be visible through provider2
    assert!(provider2.storage_manager().storage_exists("shared-storage"));
}

/// All managers are properly initialized
#[tokio::test]
async fn test_all_managers_initialized() {
    let temp_dir = TempDir::new().unwrap();
    let provider = LocalBindingsProvider::new(temp_dir.path()).unwrap();

    // Verify all managers work by creating resources
    provider
        .storage_manager()
        .create_storage("init-test")
        .await
        .unwrap();
    provider
        .vault_manager()
        .create_vault("init-test")
        .await
        .unwrap();
    provider
        .artifact_registry_manager()
        .start_registry("init-test")
        .await
        .unwrap();

    // For KV, we need to open it to create the database directory
    let kv_path = provider.kv_manager().create_kv("init-test").await.unwrap();
    {
        let _kv = LocalKv::new(kv_path).await.unwrap();
    }

    // All should exist
    assert!(provider.storage_manager().storage_exists("init-test"));
    assert!(provider.kv_manager().kv_exists("init-test"));
    assert!(provider.vault_manager().vault_exists("init-test"));
    assert!(
        provider
            .artifact_registry_manager()
            .is_running("init-test")
            .await
    );

    // Cleanup
    provider
        .artifact_registry_manager()
        .remove_registry("init-test")
        .await
        .unwrap();
}

/// Provider returns unsupported for unimplemented resources
#[tokio::test]
async fn test_provider_unsupported_resources() {
    let temp_dir = TempDir::new().unwrap();
    let provider = LocalBindingsProvider::new(temp_dir.path()).unwrap();

    // These should return "not supported" errors, not panics
    assert!(provider.load_build("any").await.is_err());
    assert!(provider.load_queue("any").await.is_err());
    assert!(provider.load_service_account("any").await.is_err());
}

/// Artifact registry through provider
#[tokio::test]
async fn test_artifact_registry_through_provider() {
    let temp_dir = TempDir::new().unwrap();
    let provider = LocalBindingsProvider::new(temp_dir.path()).unwrap();

    // Start registry through manager
    provider
        .artifact_registry_manager()
        .start_registry("provider-registry")
        .await
        .unwrap();

    // Load through provider
    let registry = provider
        .load_artifact_registry("provider-registry")
        .await
        .unwrap();

    // Use it
    let repo = registry.create_repository("test-repo").await.unwrap();
    assert!(!repo.name.is_empty());

    // Cleanup
    provider
        .artifact_registry_manager()
        .remove_registry("provider-registry")
        .await
        .unwrap();
}

/// Multiple resources workflow
#[tokio::test]
async fn test_multiple_resources_workflow() {
    let temp_dir = TempDir::new().unwrap();
    let provider = LocalBindingsProvider::new(temp_dir.path()).unwrap();

    // Create multiple of each resource type
    for i in 0..3 {
        provider
            .storage_manager()
            .create_storage(&format!("storage-{}", i))
            .await
            .unwrap();
        // For KV, we need to open it to create the database directory
        let kv_path = provider
            .kv_manager()
            .create_kv(&format!("kv-{}", i))
            .await
            .unwrap();
        {
            let _kv = LocalKv::new(kv_path).await.unwrap();
        }
        provider
            .vault_manager()
            .create_vault(&format!("vault-{}", i))
            .await
            .unwrap();
    }

    // Load and use each through provider
    for i in 0..3 {
        let storage = provider
            .load_storage(&format!("storage-{}", i))
            .await
            .unwrap();
        storage
            .put(
                &Path::from("test.txt"),
                Bytes::from(format!("data-{}", i)).into(),
            )
            .await
            .unwrap();

        let kv = provider.load_kv(&format!("kv-{}", i)).await.unwrap();
        kv.put(
            "key",
            format!("value-{}", i).into_bytes(),
            Some(PutOptions::default()),
        )
        .await
        .unwrap();

        let vault = provider.load_vault(&format!("vault-{}", i)).await.unwrap();
        vault
            .set_secret("secret", &format!("secret-{}", i))
            .await
            .unwrap();
    }

    // Verify isolation
    for i in 0..3 {
        let storage = provider
            .load_storage(&format!("storage-{}", i))
            .await
            .unwrap();
        let data = storage
            .get(&Path::from("test.txt"))
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        assert_eq!(data, Bytes::from(format!("data-{}", i)));

        let kv = provider.load_kv(&format!("kv-{}", i)).await.unwrap();
        let value = kv.get("key").await.unwrap().unwrap();
        assert_eq!(value, format!("value-{}", i).as_bytes());

        let vault = provider.load_vault(&format!("vault-{}", i)).await.unwrap();
        let secret = vault.get_secret("secret").await.unwrap();
        assert_eq!(secret, format!("secret-{}", i));
    }
}
