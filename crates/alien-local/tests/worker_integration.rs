//! Integration tests for LocalWorkerManager
//!
//! CI runs these in their own job (`Worker integration` in ci-fast.yml), which prebuilds the
//! bindings addon. They need Bun and fail without it.
//!
//! These tests verify the complete worker lifecycle:
//! 1. Build TypeScript app using alien-build (the real build system)
//! 2. Extract the OCI image using worker manager
//! 3. Start the Worker (which registers through the Worker app protocol)
//! 4. Make HTTP requests to verify it works
//! 5. Stop the worker gracefully
//!
//! The test uses examples/basic-worker-ts — a real Alien app that:
//! - Exports a Hono app with a /health endpoint
//! - Registers a command handler (invoked only when a command is pushed)
//! - Serves HTTP requests through the Alien Worker Runtime

use alien_build::settings::{BuildSettings, PlatformBuildSettings};
use alien_core::permissions::{PermissionProfile, PermissionsConfig};
use alien_core::BinaryTarget;
use alien_core::{ResourceLifecycle, ToolchainConfig, Worker, WorkerCode};
use alien_local::LocalBindingsProvider;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

/// Path to the example app used as a test fixture, relative to workspace root.
const TEST_APP_PATH: &str = "examples/basic-worker-ts";

/// Builds the example app using alien-build and returns the path to the OCI tarball.
///
/// This uses the real build system:
/// - TypeScript toolchain detects bun and runs `bun build`
/// - Creates proper OCI image with CMD set correctly
/// - Returns path to the OCI tarball
async fn build_test_app_with_alien_build(output_dir: &std::path::Path) -> PathBuf {
    let workspace_root = workspace_root::get_workspace_root();
    let test_app_src = workspace_root.join(TEST_APP_PATH);

    // Create a worker with the test-app source
    let func = Worker::new("test-func".to_string())
        .code(WorkerCode::Source {
            src: test_app_src.to_str().unwrap().to_string(),
            toolchain: ToolchainConfig::TypeScript {
                binary_name: Some("app".to_string()),
            },
        })
        .memory_mb(512)
        .timeout_seconds(60)
        .expect("literal Worker timeout is within supported range")
        .environment(HashMap::new())
        .permissions("execution".to_string())
        .build();

    // Create permissions config with an "execution" profile (empty permissions for tests)
    let permissions = PermissionsConfig {
        profiles: [("execution".to_string(), PermissionProfile::default())]
            .iter()
            .cloned()
            .collect(),
        management: Default::default(),
    };

    // Create a stack with just this worker
    let stack = alien_core::Stack::new("test-stack".to_string())
        .add(func, ResourceLifecycle::Live)
        .permissions(permissions)
        .build();

    // Build settings for local platform
    let settings = BuildSettings {
        output_directory: output_dir.to_str().unwrap().to_string(),
        platform: PlatformBuildSettings::Local {},
        targets: Some(vec![BinaryTarget::current_os()]),
        cache_url: None,
        override_base_image: None,
        debug_mode: false,
        rebuild: false,
        pull_base_images: false,
    };

    // Build the stack
    let built_stack = alien_build::build_stack(stack, &settings)
        .await
        .expect("Failed to build example app with alien-build");

    // Find the built worker and get its image path
    for (_id, entry) in built_stack.resources() {
        if let Some(f) = entry.config.downcast_ref::<Worker>() {
            if f.id == "test-func" {
                if let WorkerCode::Image { image } = &f.code {
                    // The image path is the directory containing OCI tarballs
                    let image_dir = PathBuf::from(image);

                    // Find the OCI tarball in the directory
                    for entry in std::fs::read_dir(&image_dir).expect("Failed to read image dir") {
                        let entry = entry.expect("Failed to read dir entry");
                        let path = entry.path();
                        if path.extension().and_then(|s| s.to_str()) == Some("tar") {
                            return path;
                        }
                    }
                    panic!("No OCI tarball found in {}", image_dir.display());
                }
            }
        }
    }

    panic!("Built worker not found in stack");
}

/// Helper to create worker manager for tests using LocalBindingsProvider
fn create_test_provider(state_dir: PathBuf) -> Arc<LocalBindingsProvider> {
    LocalBindingsProvider::new(&state_dir).unwrap()
}

/// Wait for HTTP server to become ready
async fn wait_for_ready(url: &str, timeout: Duration) -> bool {
    let client = reqwest::Client::new();
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        match client.get(url).timeout(Duration::from_secs(1)).send().await {
            Ok(response) if response.status().is_success() => return true,
            _ => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    false
}

// =============================================================================
// TESTS
// =============================================================================

/// The TypeScript toolchain builds the worker with Bun, so these tests can't run without it.
fn require_bun() {
    let output = std::process::Command::new("bun")
        .arg("--version")
        .output()
        .expect("bun must be installed to run the worker integration tests");
    assert!(output.status.success(), "`bun --version` failed");
}

/// Builds the example app once and extracts it for each of `worker_ids` into a fresh state dir.
async fn extracted_worker(
    temp_dir: &TempDir,
    worker_ids: &[&str],
) -> (Arc<LocalBindingsProvider>, PathBuf, Vec<PathBuf>) {
    let oci_path = build_test_app_with_alien_build(temp_dir.path()).await;
    assert!(
        oci_path.exists(),
        "OCI tarball should exist at {}",
        oci_path.display()
    );

    let state_dir = temp_dir.path().join("state");
    std::fs::create_dir_all(&state_dir).unwrap();
    let provider = create_test_provider(state_dir);
    let mut extracted = Vec::new();
    for worker_id in worker_ids {
        let path = provider
            .worker_manager()
            .extract_image(worker_id, oci_path.to_str().unwrap(), None)
            .await
            .expect("Failed to extract image");
        assert!(path.exists());
        extracted.push(path);
    }
    (provider, oci_path, extracted)
}

/// GETs `/health` and checks the example app's response body.
async fn assert_healthy(url: &str) {
    let health_url = format!("{url}/health");
    assert!(
        wait_for_ready(&health_url, Duration::from_secs(30)).await,
        "worker at {url} should become ready within 30 seconds"
    );
    let response = reqwest::Client::new()
        .get(&health_url)
        .send()
        .await
        .expect("GET /health request failed");
    assert!(response.status().is_success());
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert!(
        body["timestamp"].is_string(),
        "Response should include a timestamp"
    );
}

/// One worker through its whole lifecycle: build → extract → start (twice, idempotent) →
/// binding → health → stop → restart → delete.
#[tokio::test]
async fn worker_lifecycle_on_one_image() {
    tracing_subscriber::fmt::try_init().ok();
    require_bun();

    let temp_dir = TempDir::new().unwrap();
    let (provider, _oci_path, extracted) = extracted_worker(&temp_dir, &["test-func"]).await;
    let manager = provider.worker_manager();

    assert!(
        manager.check_health("test-func").await.is_err(),
        "a worker that was never started is not healthy"
    );

    let url = manager
        .start_worker("test-func", HashMap::new(), Vec::new(), Vec::new())
        .await
        .expect("Failed to start worker");
    assert!(url.starts_with("http://localhost:"));
    assert!(manager.is_running("test-func").await);

    let again = manager
        .start_worker("test-func", HashMap::new(), Vec::new(), Vec::new())
        .await
        .expect("Starting a running worker again should succeed");
    assert_eq!(
        url, again,
        "Starting the same worker twice should return the same URL"
    );

    match manager.get_binding("test-func").await.unwrap() {
        alien_core::bindings::WorkerBinding::Local(config) => {
            let binding_url = config
                .worker_url
                .into_value("test-func", "worker_url")
                .unwrap();
            assert_eq!(
                binding_url, url,
                "the binding should point at the running worker"
            );
        }
        other => panic!("Expected Local binding variant, got {other:?}"),
    }

    assert_healthy(&url).await;
    manager
        .check_health("test-func")
        .await
        .expect("a running worker should pass its health check");

    manager
        .stop_worker("test-func")
        .await
        .expect("Failed to stop worker");
    assert!(!manager.is_running("test-func").await);
    assert!(manager.check_health("test-func").await.is_err());

    let restarted = manager
        .start_worker("test-func", HashMap::new(), Vec::new(), Vec::new())
        .await
        .expect("Failed to start worker after stop");
    assert_healthy(&restarted).await;

    manager.delete_worker("test-func").await.unwrap();
    assert!(!manager.is_running("test-func").await);
    assert!(
        !extracted[0].exists(),
        "Extracted directory should be deleted"
    );
    assert!(manager.get_worker_url("test-func").await.is_err());
}

/// Two workers from the same image, started at the same time. Both run the same Bun-compiled
/// binary, so this also covers processes sharing the binary's embedded bindings addon.
#[tokio::test]
async fn two_workers_from_one_image_run_side_by_side() {
    tracing_subscriber::fmt::try_init().ok();
    require_bun();

    let temp_dir = TempDir::new().unwrap();
    let (provider, _oci_path, _extracted) =
        extracted_worker(&temp_dir, &["worker-a", "worker-b"]).await;
    let manager = provider.worker_manager();

    let (a, b) = tokio::join!(
        manager.start_worker("worker-a", HashMap::new(), Vec::new(), Vec::new()),
        manager.start_worker("worker-b", HashMap::new(), Vec::new(), Vec::new()),
    );
    let (url_a, url_b) = (
        a.expect("worker-a should start"),
        b.expect("worker-b should start"),
    );
    assert_ne!(url_a, url_b, "workers should run on different ports");

    assert_healthy(&url_a).await;
    assert_healthy(&url_b).await;
    assert!(manager.is_running("worker-a").await);
    assert!(manager.is_running("worker-b").await);

    manager.stop_worker("worker-a").await.unwrap();
    manager.stop_worker("worker-b").await.unwrap();
}

/// Test stop on non-existent worker is idempotent
#[tokio::test]
async fn test_stop_nonexistent_is_idempotent() {
    let temp_dir = TempDir::new().unwrap();
    let provider = create_test_provider(temp_dir.path().to_path_buf());
    let manager = provider.worker_manager();

    // Should not error
    manager.stop_worker("nonexistent").await.unwrap();
}

/// Test get_worker_url fails for non-running worker
#[tokio::test]
async fn test_get_url_nonexistent_fails() {
    let temp_dir = TempDir::new().unwrap();
    let provider = create_test_provider(temp_dir.path().to_path_buf());
    let manager = provider.worker_manager();

    let result = manager.get_worker_url("nonexistent").await;
    assert!(result.is_err());
}

/// Test start_worker fails if image not extracted
#[tokio::test]
async fn test_start_without_extract_fails() {
    let temp_dir = TempDir::new().unwrap();
    let provider = create_test_provider(temp_dir.path().to_path_buf());
    let manager = provider.worker_manager();

    let result = manager
        .start_worker("no-image", HashMap::new(), Vec::new(), Vec::new())
        .await;
    assert!(result.is_err());
}
