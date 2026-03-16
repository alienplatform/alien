//! Integration tests for LocalFunctionManager
//!
//! These tests verify the complete function lifecycle:
//! 1. Build TypeScript app using alien-build (the real build system)
//! 2. Extract the OCI image using function manager
//! 3. Start the function (which registers with runtime via gRPC)
//! 4. Make HTTP requests to verify it works
//! 5. Stop the function gracefully
//!
//! The test uses packages/test-app which is a proper Alien app that:
//! - Starts HTTP server on random port using Bun.serve
//! - Registers with the Alien runtime via gRPC (registerHttpServer)
//! - Responds to HTTP requests with JSON

use alien_build::settings::{BuildSettings, PlatformBuildSettings};
use alien_core::permissions::{PermissionProfile, PermissionsConfig};
use alien_core::BinaryTarget;
use alien_core::{Function, FunctionCode, Ingress, ResourceLifecycle, ToolchainConfig};
use alien_local::LocalBindingsProvider;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

/// Path to the test-app package relative to workspace root
const TEST_APP_PATH: &str = "packages/test-app";

/// Builds the test-app using alien-build and returns the path to the OCI tarball.
///
/// This uses the real build system:
/// - TypeScript toolchain detects bun and runs `bun build`
/// - Creates proper OCI image with CMD set correctly
/// - Returns path to the OCI tarball
async fn build_test_app_with_alien_build(output_dir: &std::path::Path) -> PathBuf {
    let workspace_root = workspace_root::get_workspace_root();
    let test_app_src = workspace_root.join(TEST_APP_PATH);

    ensure_test_app_deps(&workspace_root, &test_app_src);

    // Create a function with the test-app source
    let func = Function::new("test-func".to_string())
        .code(FunctionCode::Source {
            src: test_app_src.to_str().unwrap().to_string(),
            toolchain: ToolchainConfig::TypeScript {
                binary_name: Some("app".to_string()),
            },
        })
        .memory_mb(512)
        .timeout_seconds(60)
        .environment(HashMap::new())
        .ingress(Ingress::Private)
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

    // Create a stack with just this function
    let stack = alien_core::Stack::new("test-stack".to_string())
        .add(func, ResourceLifecycle::Frozen)
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
    };

    // Build the stack
    let built_stack = alien_build::build_stack(stack, &settings)
        .await
        .expect("Failed to build test-app with alien-build");

    // Find the built function and get its image path
    for (_id, entry) in built_stack.resources() {
        if let Some(f) = entry.config.downcast_ref::<Function>() {
            if f.id == "test-func" {
                if let FunctionCode::Image { image } = &f.code {
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

    panic!("Built function not found in stack");
}

fn ensure_test_app_deps(workspace_root: &std::path::Path, test_app_src: &std::path::Path) {
    let root_bindings = workspace_root
        .join("node_modules")
        .join("@alienplatform")
        .join("bindings");
    let app_bindings = test_app_src
        .join("node_modules")
        .join("@alienplatform")
        .join("bindings");

    if root_bindings.exists() || app_bindings.exists() {
        return;
    }

    let status = std::process::Command::new("pnpm")
        .arg("install")
        .current_dir(workspace_root)
        .status()
        .expect("Failed to run pnpm install for test app dependencies");

    assert!(
        status.success(),
        "pnpm install failed for test app dependencies"
    );
}

/// Helper to create function manager for tests using LocalBindingsProvider
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

/// Full lifecycle: build app → extract → start → HTTP request → stop
#[tokio::test]
async fn test_function_full_lifecycle() {
    tracing_subscriber::fmt::try_init().ok();

    // Skip if Bun is not available (required by TypeScript toolchain)
    if std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: bun not available");
        return;
    }

    let temp_dir = TempDir::new().unwrap();

    // 1. Build the test-app using alien-build
    let oci_path = build_test_app_with_alien_build(temp_dir.path()).await;
    assert!(
        oci_path.exists(),
        "OCI tarball should exist at {}",
        oci_path.display()
    );

    // 2. Create function manager via LocalBindingsProvider
    let state_dir = temp_dir.path().join("state");
    std::fs::create_dir_all(&state_dir).unwrap();
    let provider = create_test_provider(state_dir);
    let manager = provider.function_manager();

    // 3. Extract image
    let extracted_path = manager
        .extract_image("test-func", oci_path.to_str().unwrap(), None)
        .await
        .expect("Failed to extract image");

    assert!(extracted_path.exists());

    // 4. Start function
    let url = manager
        .start_function("test-func", HashMap::new())
        .await
        .expect("Failed to start function");

    assert!(url.starts_with("http://localhost:"));
    assert!(manager.is_running("test-func").await);

    // 5. Wait for function to be ready (app needs to register with runtime)
    let ready = wait_for_ready(&url, Duration::from_secs(30)).await;
    assert!(ready, "Function should become ready within 30 seconds");

    // 6. Make HTTP GET request
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/test-path", url))
        .send()
        .await
        .expect("GET request failed");

    assert!(response.status().is_success());
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["method"], "GET");
    assert_eq!(body["path"], "/test-path");

    // 7. Make HTTP POST request
    let response = client
        .post(format!("{}/submit", url))
        .body("test-body")
        .send()
        .await
        .expect("POST request failed");

    assert!(response.status().is_success());
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["method"], "POST");
    assert_eq!(body["body_length"], 9);

    // 8. Stop function
    manager
        .stop_function("test-func")
        .await
        .expect("Failed to stop function");
    assert!(!manager.is_running("test-func").await);
}

/// Test that start_function is idempotent (returns same URL)
#[tokio::test]
async fn test_start_function_idempotent() {
    if std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: bun not available");
        return;
    }

    let temp_dir = TempDir::new().unwrap();
    let oci_path = build_test_app_with_alien_build(temp_dir.path()).await;

    let state_dir = temp_dir.path().join("state");
    std::fs::create_dir_all(&state_dir).unwrap();
    let provider = create_test_provider(state_dir);
    let manager = provider.function_manager();

    manager
        .extract_image("idempotent-func", oci_path.to_str().unwrap(), None)
        .await
        .unwrap();

    // Start twice - should return same URL
    let url1 = manager
        .start_function("idempotent-func", HashMap::new())
        .await
        .unwrap();
    let url2 = manager
        .start_function("idempotent-func", HashMap::new())
        .await
        .unwrap();

    assert_eq!(
        url1, url2,
        "Starting same function twice should return same URL"
    );

    manager.stop_function("idempotent-func").await.unwrap();
}

/// Test stop on non-existent function is idempotent
#[tokio::test]
async fn test_stop_nonexistent_is_idempotent() {
    let temp_dir = TempDir::new().unwrap();
    let provider = create_test_provider(temp_dir.path().to_path_buf());
    let manager = provider.function_manager();

    // Should not error
    manager.stop_function("nonexistent").await.unwrap();
}

/// Test get_function_url fails for non-running function
#[tokio::test]
async fn test_get_url_nonexistent_fails() {
    let temp_dir = TempDir::new().unwrap();
    let provider = create_test_provider(temp_dir.path().to_path_buf());
    let manager = provider.function_manager();

    let result = manager.get_function_url("nonexistent").await;
    assert!(result.is_err());
}

/// Test start_function fails if image not extracted
#[tokio::test]
async fn test_start_without_extract_fails() {
    let temp_dir = TempDir::new().unwrap();
    let provider = create_test_provider(temp_dir.path().to_path_buf());
    let manager = provider.function_manager();

    let result = manager.start_function("no-image", HashMap::new()).await;
    assert!(result.is_err());
}

/// Test delete_function removes extracted files and stops function
#[tokio::test]
async fn test_delete_function_cleanup() {
    if std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: bun not available");
        return;
    }

    let temp_dir = TempDir::new().unwrap();
    let oci_path = build_test_app_with_alien_build(temp_dir.path()).await;

    let state_dir = temp_dir.path().join("state");
    std::fs::create_dir_all(&state_dir).unwrap();
    let provider = create_test_provider(state_dir);
    let manager = provider.function_manager();

    // Extract and start
    let extracted_path = manager
        .extract_image("delete-test", oci_path.to_str().unwrap(), None)
        .await
        .unwrap();
    manager
        .start_function("delete-test", HashMap::new())
        .await
        .unwrap();

    // Wait for ready
    let url = manager.get_function_url("delete-test").await.unwrap();
    wait_for_ready(&url, Duration::from_secs(30)).await;

    // Delete should stop and remove files
    manager.delete_function("delete-test").await.unwrap();

    assert!(!manager.is_running("delete-test").await);
    assert!(
        !extracted_path.exists(),
        "Extracted directory should be deleted"
    );
}

/// Test get_binding returns correct FunctionBinding::Local variant
#[tokio::test]
async fn test_get_binding_returns_local_variant() {
    if std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: bun not available");
        return;
    }

    let temp_dir = TempDir::new().unwrap();
    let oci_path = build_test_app_with_alien_build(temp_dir.path()).await;

    let state_dir = temp_dir.path().join("state");
    std::fs::create_dir_all(&state_dir).unwrap();
    let provider = create_test_provider(state_dir);
    let manager = provider.function_manager();

    manager
        .extract_image("binding-test", oci_path.to_str().unwrap(), None)
        .await
        .unwrap();
    manager
        .start_function("binding-test", HashMap::new())
        .await
        .unwrap();

    let binding = manager.get_binding("binding-test").await.unwrap();

    // Verify it's the Local variant with a URL
    match binding {
        alien_core::bindings::FunctionBinding::Local(config) => {
            let url = config
                .function_url
                .into_value("binding-test", "function_url")
                .unwrap();
            assert!(url.starts_with("http://localhost:"));
        }
        _ => panic!("Expected Local binding variant"),
    }

    manager.stop_function("binding-test").await.unwrap();
}

/// Test health check works for running function
#[tokio::test]
async fn test_health_check() {
    if std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: bun not available");
        return;
    }

    let temp_dir = TempDir::new().unwrap();
    let oci_path = build_test_app_with_alien_build(temp_dir.path()).await;

    let state_dir = temp_dir.path().join("state");
    std::fs::create_dir_all(&state_dir).unwrap();
    let provider = create_test_provider(state_dir);
    let manager = provider.function_manager();

    // Health check should fail for non-existent function
    assert!(manager.check_health("nonexistent").await.is_err());

    // Extract and start
    manager
        .extract_image("health-test", oci_path.to_str().unwrap(), None)
        .await
        .unwrap();
    manager
        .start_function("health-test", HashMap::new())
        .await
        .unwrap();

    // Wait for ready first
    let url = manager.get_function_url("health-test").await.unwrap();
    wait_for_ready(&url, Duration::from_secs(30)).await;

    // Health check should pass
    manager.check_health("health-test").await.unwrap();

    // Stop and verify health check fails
    manager.stop_function("health-test").await.unwrap();
    assert!(manager.check_health("health-test").await.is_err());
}

/// Test multiple functions can run concurrently
#[tokio::test]
async fn test_multiple_functions_concurrent() {
    if std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: bun not available");
        return;
    }

    let temp_dir = TempDir::new().unwrap();
    let oci_path = build_test_app_with_alien_build(temp_dir.path()).await;

    let state_dir = temp_dir.path().join("state");
    std::fs::create_dir_all(&state_dir).unwrap();
    let provider = create_test_provider(state_dir);
    let manager = provider.function_manager();

    // Start 3 functions
    let mut urls = Vec::new();
    for i in 0..3 {
        let func_id = format!("concurrent-{}", i);
        manager
            .extract_image(&func_id, oci_path.to_str().unwrap(), None)
            .await
            .unwrap();
        let url = manager
            .start_function(&func_id, HashMap::new())
            .await
            .unwrap();
        urls.push((func_id, url));
    }

    // Wait for all to be ready
    for (_, url) in &urls {
        let ready = wait_for_ready(url, Duration::from_secs(30)).await;
        assert!(ready, "Function at {} should become ready", url);
    }

    // All should be running on different ports
    let ports: Vec<_> = urls
        .iter()
        .map(|(_, url)| url.split(':').last().unwrap())
        .collect();

    assert!(
        ports[0] != ports[1] && ports[1] != ports[2] && ports[0] != ports[2],
        "Functions should run on different ports"
    );

    // Make request to each
    let client = reqwest::Client::new();
    for (func_id, url) in &urls {
        let response = client.get(format!("{}/hello", url)).send().await.unwrap();
        assert!(
            response.status().is_success(),
            "Request to {} failed",
            func_id
        );
    }

    // Cleanup
    for (func_id, _) in &urls {
        manager.stop_function(func_id).await.unwrap();
    }
}

/// Test port allocation fallback - when saved port is unavailable, allocates new port
///
/// This verifies the graceful fallback behavior. If the saved port is still in TIME_WAIT
/// (common immediately after stop), the manager allocates a new port instead of failing.
#[tokio::test]
async fn test_port_allocation_fallback() {
    tracing_subscriber::fmt::try_init().ok();

    if std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: bun not available");
        return;
    }

    let temp_dir = TempDir::new().unwrap();
    let oci_path = build_test_app_with_alien_build(temp_dir.path()).await;

    let state_dir = temp_dir.path().join("state");
    std::fs::create_dir_all(&state_dir).unwrap();
    let provider = create_test_provider(state_dir);
    let manager = provider.function_manager();

    // 1. Extract and start function
    manager
        .extract_image("fallback-test", oci_path.to_str().unwrap(), None)
        .await
        .unwrap();

    let url_before = manager
        .start_function("fallback-test", HashMap::new())
        .await
        .unwrap();

    // Wait for function to be ready
    let ready = wait_for_ready(&url_before, Duration::from_secs(30)).await;
    assert!(ready, "Function should become ready");

    let port_before: u16 = url_before.split(':').last().unwrap().parse().unwrap();

    // 2. Stop function (simulates crash - metadata with port is preserved)
    manager.stop_function("fallback-test").await.unwrap();
    assert!(!manager.is_running("fallback-test").await);

    // With proper graceful shutdown (axum with_graceful_shutdown), the port is released
    // immediately when the shutdown signal is received. We just need a brief delay for
    // the OS to complete cleanup.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // 3. Start function again (port should be reused if properly released)
    let url_after = manager
        .start_function("fallback-test", HashMap::new())
        .await
        .expect("Failed to start function after stop");

    let port_after: u16 = url_after.split(':').last().unwrap().parse().unwrap();

    // Wait for function to be ready with new port
    let ready = wait_for_ready(&url_after, Duration::from_secs(30)).await;
    assert!(ready, "Function should become ready after recovery");

    // With 5 second delay, port SHOULD be reused (TIME_WAIT typically 1-5s on Linux, longer on macOS)
    // We log the ports to verify behavior, but don't assert equality due to OS differences
    eprintln!("Port before: {}, Port after: {}", port_before, port_after);
    if port_before == port_after {
        eprintln!("✓ Port reused (transparent recovery)");
    } else {
        eprintln!("⚠ Port changed (fallback due to TIME_WAIT)");
    }

    // Verify function works regardless of port
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/test", url_after))
        .send()
        .await
        .unwrap();
    assert!(
        response.status().is_success(),
        "Request should succeed after recovery"
    );

    // Cleanup
    manager.delete_function("fallback-test").await.unwrap();
}

/// Test that delete_function removes metadata (no recovery possible)
///
/// This verifies that delete (unlike stop) removes metadata, so the next
/// start will allocate a new random port instead of reusing the old one.
#[tokio::test]
async fn test_delete_prevents_port_reuse() {
    if std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: bun not available");
        return;
    }

    let temp_dir = TempDir::new().unwrap();
    let oci_path = build_test_app_with_alien_build(temp_dir.path()).await;

    let state_dir = temp_dir.path().join("state");
    std::fs::create_dir_all(&state_dir).unwrap();
    let provider = create_test_provider(state_dir);
    let manager = provider.function_manager();

    // Extract image once
    manager
        .extract_image("delete-test", oci_path.to_str().unwrap(), None)
        .await
        .unwrap();

    // Start function and get URL
    let url_before = manager
        .start_function("delete-test", HashMap::new())
        .await
        .unwrap();
    let port_before: u16 = url_before.split(':').last().unwrap().parse().unwrap();

    // Delete function (removes metadata)
    manager.delete_function("delete-test").await.unwrap();

    // Extract again (delete removed everything)
    manager
        .extract_image("delete-test", oci_path.to_str().unwrap(), None)
        .await
        .unwrap();

    // Start again - should get a DIFFERENT port (no saved metadata)
    let url_after = manager
        .start_function("delete-test", HashMap::new())
        .await
        .unwrap();
    let port_after: u16 = url_after.split(':').last().unwrap().parse().unwrap();

    // Ports should be different (random allocation, no saved port)
    assert_ne!(
        port_before, port_after,
        "After delete, function should get a new random port"
    );

    // Cleanup
    manager.delete_function("delete-test").await.unwrap();
}
