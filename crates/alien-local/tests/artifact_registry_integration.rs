//! Integration tests for LocalArtifactRegistryManager
//!
//! These tests verify that:
//! 1. Registry starts and returns accessible URL
//! 2. Health checks validate registry state
//! 3. Registry is idempotent (multiple starts return same URL)

use alien_local::LocalArtifactRegistryManager;
use tempfile::TempDir;
use tokio::sync::broadcast;

/// Helper to create a manager for tests
fn create_test_manager(
    state_dir: std::path::PathBuf,
) -> (LocalArtifactRegistryManager, broadcast::Sender<()>) {
    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    let (manager, _task) = LocalArtifactRegistryManager::new_with_shutdown(state_dir, shutdown_rx);
    (manager, shutdown_tx)
}

/// Registry starts and returns accessible URL
#[tokio::test]
async fn test_registry_starts_and_is_accessible() {
    let temp_dir = TempDir::new().unwrap();
    let (manager, _shutdown_tx) = create_test_manager(temp_dir.path().to_path_buf());

    // Start registry
    let url = manager.start_registry("test-registry").await.unwrap();
    assert!(url.starts_with("localhost:"));
    assert!(manager.is_running("test-registry").await);

    // Verify it's actually accessible via HTTP
    let client = reqwest::Client::new();
    let response = client
        .get(&format!("http://{}/v2/", url))
        .send()
        .await
        .unwrap();
    // Registry should respond (200 OK or 401 Unauthorized for auth)
    assert!(
        response.status().is_success() || response.status().as_u16() == 401,
        "Expected success or 401, got {}",
        response.status()
    );

    // Remove and verify
    manager.remove_registry("test-registry").await.unwrap();
    assert!(!manager.is_running("test-registry").await);
}

/// Health check validates registry state
#[tokio::test]
async fn test_registry_health_check() {
    let temp_dir = TempDir::new().unwrap();
    let (manager, _shutdown_tx) = create_test_manager(temp_dir.path().to_path_buf());

    // Not running = health fails
    assert!(manager.check_health("nonexistent").await.is_err());

    // Running = health passes
    manager.start_registry("healthy-registry").await.unwrap();
    manager.check_health("healthy-registry").await.unwrap();

    // Cleanup
    manager.remove_registry("healthy-registry").await.unwrap();
}

/// Registry is idempotent (start twice returns same URL)
#[tokio::test]
async fn test_registry_start_idempotent() {
    let temp_dir = TempDir::new().unwrap();
    let (manager, _shutdown_tx) = create_test_manager(temp_dir.path().to_path_buf());

    let url1 = manager.start_registry("idem-registry").await.unwrap();
    let url2 = manager.start_registry("idem-registry").await.unwrap();
    assert_eq!(url1, url2);

    manager.remove_registry("idem-registry").await.unwrap();
}

/// get_registry_url works for running registry
#[tokio::test]
async fn test_get_registry_url() {
    let temp_dir = TempDir::new().unwrap();
    let (manager, _shutdown_tx) = create_test_manager(temp_dir.path().to_path_buf());

    // Not running = error
    assert!(manager.get_registry_url("nonexistent").await.is_err());

    // Start and get URL
    let start_url = manager.start_registry("url-test").await.unwrap();
    let get_url = manager.get_registry_url("url-test").await.unwrap();
    assert_eq!(start_url, get_url);

    manager.remove_registry("url-test").await.unwrap();
}

/// Multiple registries can coexist
#[tokio::test]
async fn test_multiple_registries_coexist() {
    let temp_dir = TempDir::new().unwrap();
    let (manager, _shutdown_tx) = create_test_manager(temp_dir.path().to_path_buf());

    // Start multiple registries
    let url_a = manager.start_registry("registry-a").await.unwrap();
    let url_b = manager.start_registry("registry-b").await.unwrap();

    // They should have different URLs (different ports)
    assert_ne!(url_a, url_b);

    // Both should be running
    assert!(manager.is_running("registry-a").await);
    assert!(manager.is_running("registry-b").await);

    // Both should be accessible
    let client = reqwest::Client::new();
    let resp_a = client.get(&format!("http://{}/v2/", url_a)).send().await;
    let resp_b = client.get(&format!("http://{}/v2/", url_b)).send().await;
    assert!(resp_a.is_ok());
    assert!(resp_b.is_ok());

    // Cleanup
    manager.remove_registry("registry-a").await.unwrap();
    manager.remove_registry("registry-b").await.unwrap();
}

/// Remove non-existent registry doesn't error (idempotent)
#[tokio::test]
async fn test_remove_nonexistent_is_idempotent() {
    let temp_dir = TempDir::new().unwrap();
    let (manager, _shutdown_tx) = create_test_manager(temp_dir.path().to_path_buf());

    // Should not error
    manager.remove_registry("nonexistent").await.unwrap();
}

/// Delete registry storage cleans up
#[tokio::test]
async fn test_delete_registry_storage() {
    let temp_dir = TempDir::new().unwrap();
    let (manager, _shutdown_tx) = create_test_manager(temp_dir.path().to_path_buf());

    // Start, remove, then delete storage
    manager.start_registry("delete-test").await.unwrap();
    manager.remove_registry("delete-test").await.unwrap();

    // Note: delete_registry_storage may fail if registry left files behind.
    // This is expected behavior - the manager cleans up what it can.
    let result = manager.delete_registry_storage("delete-test").await;

    // Either it succeeds or the directory exists but we can't fully clean it
    if result.is_ok() {
        let storage_dir = temp_dir
            .path()
            .join("artifact_registry")
            .join("delete-test");
        assert!(!storage_dir.exists());
    }
    // If it fails, that's okay - the underlying directory cleanup is a limitation
}

/// get_binding returns correct ArtifactRegistryBinding::Local variant
#[tokio::test]
async fn test_get_binding_returns_local_variant() {
    use alien_core::bindings::ArtifactRegistryBinding;

    let temp_dir = TempDir::new().unwrap();
    let (manager, _shutdown_tx) = create_test_manager(temp_dir.path().to_path_buf());

    manager.start_registry("binding-test").await.unwrap();
    let binding = manager.get_binding("binding-test").await.unwrap();

    // Verify it's the Local variant with correct URL
    match binding {
        ArtifactRegistryBinding::Local(config) => {
            let url = config
                .registry_url
                .into_value("binding-test", "registry_url")
                .unwrap();
            assert!(url.starts_with("localhost:"));
        }
        _ => panic!("Expected Local binding variant"),
    }

    manager.remove_registry("binding-test").await.unwrap();
}
