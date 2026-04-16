//! Shared test utilities for alien-runtime integration tests.
//!
//! This module provides common functionality needed across multiple test files,
//! with special focus on ensuring the alien-test-app binary is built only once
//! across all tests for performance optimization.

use anyhow::Context;
use std::{path::PathBuf, process::Command as StdCommand, sync::Once};
use tracing::{debug, info};
use workspace_root::get_workspace_root;

/// Global state to ensure alien-test-app is built only once across all tests
static ALIEN_TEST_APP_BUILD: Once = Once::new();

/// Build result stored globally after first successful build
static mut ALIEN_TEST_APP_PATH: Option<PathBuf> = None;

/// Ensures the alien-test-app binary is built and returns its path.
///
/// This function uses `std::sync::Once` to ensure the binary is compiled only once
/// across all test runs, significantly improving test performance when running
/// multiple integration tests.
///
/// # Returns
///
/// Returns the absolute path to the built alien-test-app binary.
///
/// # Errors
///
/// Returns an error if:
/// - The cargo build command fails
/// - The built binary cannot be found in the expected location
/// - Workspace root cannot be determined
pub fn ensure_alien_test_app_built() -> anyhow::Result<PathBuf> {
    ALIEN_TEST_APP_BUILD.call_once(|| {
        let result = build_alien_test_app_impl();
        match result {
            Ok(path) => {
                info!(
                    ?path,
                    "Successfully built alien-test-app binary (first time)"
                );
                // SAFETY: This is safe because call_once ensures this code runs exactly once
                // and no other thread can access ALIEN_TEST_APP_PATH until this completes
                unsafe {
                    ALIEN_TEST_APP_PATH = Some(path);
                }
            }
            Err(e) => {
                // Store the error in the path for later retrieval
                // We can't return the error from call_once, so we'll check later
                tracing::error!("Failed to build alien-test-app: {}", e);
                unsafe {
                    ALIEN_TEST_APP_PATH = None;
                }
            }
        }
    });

    // SAFETY: This is safe because call_once guarantees that the initialization
    // above has completed before we reach this point
    unsafe {
        ALIEN_TEST_APP_PATH
            .clone()
            .context("alien-test-app build failed during initialization")
    }
}

/// Internal implementation of the binary building logic.
/// This is separated to make the Once usage cleaner.
fn build_alien_test_app_impl() -> anyhow::Result<PathBuf> {
    info!("Building alien-test-app binary (once for all tests)...");

    let build_output = StdCommand::new("cargo")
        .arg("build")
        .arg("-p")
        .arg("alien-test-app")
        .arg("--bin")
        .arg("alien-test-app")
        // .arg("--all-features")
        .current_dir(&get_workspace_root())
        .output()
        .context("Failed to execute cargo build for alien-test-app")?;

    if !build_output.status.success() {
        let stderr = String::from_utf8_lossy(&build_output.stderr);
        let stdout = String::from_utf8_lossy(&build_output.stdout);
        anyhow::bail!(
            "Failed to build alien-test-app binary. Status: {}, stdout: {}, stderr: {}",
            build_output.status,
            stdout,
            stderr
        );
    }

    let workspace_root = get_workspace_root();
    let test_app_path = workspace_root
        .join("target")
        .join(if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        })
        .join("alien-test-app");

    debug!(?test_app_path, "Resolved test app path");

    if !test_app_path.exists() {
        anyhow::bail!(
            "Test app binary not found at {:?}. Ensure alien-test-app is built.",
            test_app_path
        );
    }

    Ok(test_app_path)
}

/// Gets the test app path, building it if necessary.
///
/// This is a convenience function that calls `ensure_alien_test_app_built()`
/// and can be used as a drop-in replacement for the old build functions.
///
/// # Returns
///
/// Returns the absolute path to the alien-test-app binary.
pub fn get_test_app_path() -> anyhow::Result<PathBuf> {
    ensure_alien_test_app_built()
}
