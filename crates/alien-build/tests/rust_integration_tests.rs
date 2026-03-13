use alien_build::{
    build_stack,
    settings::{BuildSettings, PlatformBuildSettings},
};
use alien_core::{
    permissions::PermissionProfile, Function, FunctionCode, Ingress, ResourceLifecycle,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::path::{Path, PathBuf as StdPathBuf};
use tempfile::{tempdir, TempDir};
use tokio::fs;
use workspace_root;

// Load environment variables from .env.test for Docker credentials
fn load_test_env() {
    let root: StdPathBuf = workspace_root::get_workspace_root();
    dotenvy::from_path(root.join(".env.test")).ok(); // OK to fail if not present
}

// Helper to create a basic function for testing
fn create_test_function(name: &str, code: FunctionCode) -> Function {
    Function::new(name.to_string())
        .code(code)
        .memory_mb(512)
        .timeout_seconds(60)
        .environment(HashMap::new())
        .ingress(Ingress::Private)
        .permissions("execution".to_string())
        .build()
}

fn stack_with_permissions(name: &str) -> alien_core::StackBuilder {
    alien_core::Stack::new(name.to_string()).permission("execution", PermissionProfile::new())
}

fn assert_image_dir_has_hash(image: &str, function_name: &str) {
    let dir_name = Path::new(image)
        .file_name()
        .and_then(|name| name.to_str())
        .expect("Image path should end with a directory name");

    let prefix = format!("{}-", function_name);
    assert!(
        dir_name.starts_with(&prefix),
        "Image path should start with {}: got {}",
        prefix,
        dir_name
    );

    let hash = dir_name.strip_prefix(&prefix).unwrap_or_default();
    assert_eq!(
        hash.len(),
        8,
        "Image path should end with 8-char hash suffix, got {}",
        dir_name
    );
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "Hash suffix should be hex, got {}",
        hash
    );
}

// Helper function to create a Rust workspace for testing
async fn create_test_rust_workspace(
    workspace_name: &str,
    message: &str,
) -> (TempDir, PathBuf, PathBuf) {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let workspace_dir = temp_dir.path().join(workspace_name);
    fs::create_dir(&workspace_dir)
        .await
        .expect("Failed to create workspace dir");

    // Create workspace Cargo.toml
    let workspace_cargo_toml = r#"[workspace]
members = ["my-app"]
resolver = "2"
"#;
    fs::write(workspace_dir.join("Cargo.toml"), workspace_cargo_toml)
        .await
        .expect("Failed to write workspace Cargo.toml");

    // Create member crate directory
    let member_dir = workspace_dir.join("my-app");
    fs::create_dir(&member_dir)
        .await
        .expect("Failed to create member dir");

    // Create member Cargo.toml
    let member_cargo_toml = r#"[package]
name = "my-app"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "my-app"
path = "src/main.rs"
"#;
    fs::write(member_dir.join("Cargo.toml"), member_cargo_toml)
        .await
        .expect("Failed to write member Cargo.toml");

    // Create src directory and main.rs
    let src_dir = member_dir.join("src");
    fs::create_dir(&src_dir)
        .await
        .expect("Failed to create src dir");

    let main_rs_content = format!(
        r#"fn main() {{
    println!("{message}");
}}
"#
    );
    fs::write(src_dir.join("main.rs"), main_rs_content)
        .await
        .expect("Failed to write main.rs");

    (temp_dir, workspace_dir, member_dir)
}

// Helper function to test workspace builds
async fn test_rust_workspace_build(
    function_name: &str,
    stack_name: &str,
    src_dir: PathBuf,
    success_message: &str,
) {
    let temp_output_dir = tempdir().expect("Failed to create temp output dir");
    let output_dir_path = temp_output_dir.path().to_path_buf();

    let func_with_workspace = create_test_function(
        function_name,
        FunctionCode::Source {
            src: src_dir.to_str().unwrap().to_string(),
            toolchain: alien_core::ToolchainConfig::Rust {
                binary_name: "my-app".to_string(),
            },
        },
    );

    let stack = stack_with_permissions(stack_name)
        .add(func_with_workspace, ResourceLifecycle::Frozen)
        .build();

    let settings = BuildSettings {
        output_directory: output_dir_path.to_str().unwrap().to_string(),
        platform: PlatformBuildSettings::Test {},
        targets: None,
        cache_url: None,
        override_base_image: None,
        debug_mode: false,
    };

    let result = build_stack(stack, &settings).await;

    assert!(
        result.is_ok(),
        "Expected build_stack to succeed with workspace Rust project: {:?}",
        result.err()
    );

    let built_stack = result.unwrap();

    // Verify that the function was converted to an Image
    let mut func_found = false;
    for (_id, entry) in built_stack.resources() {
        if let Some(f) = entry.config.downcast_ref::<alien_core::Function>() {
            if f.id == function_name {
                func_found = true;
                match &f.code {
                    FunctionCode::Image { image } => {
                        // After build, image should be a local directory path
                        let image_path = PathBuf::from(image);
                        assert!(
                            image_path.exists() && image_path.is_dir(),
                            "Image should be a local directory path, got: {}",
                            image
                        );
                        assert_image_dir_has_hash(image, function_name);

                        // Verify the directory contains OCI tarballs
                        let test_output_dir = output_dir_path.join("build").join("test");
                        assert!(
                            image_path.starts_with(&test_output_dir),
                            "Image path should live under build/test, got: {}",
                            image_path.display()
                        );
                    }
                    _ => panic!("Function should have been converted to Image"),
                }
            }
        }
    }
    assert!(
        func_found,
        "Workspace function '{}' was not found in the result stack",
        function_name
    );

    println!("✅ {}", success_message);
}

#[tokio::test]
async fn test_rust_toolchain_invalid_project() {
    load_test_env(); // Load Docker credentials if available
    tracing_subscriber::fmt::try_init().ok();

    // Setup: Create a temporary directory without Cargo.toml
    let temp_source_dir = tempdir().expect("Failed to create temp source dir");
    fs::write(temp_source_dir.path().join("main.rs"), "fn main() {}")
        .await
        .unwrap();

    let func_with_invalid_rust = create_test_function(
        "invalid-rust-func",
        FunctionCode::Source {
            src: temp_source_dir.path().to_str().unwrap().to_string(),
            toolchain: alien_core::ToolchainConfig::Rust {
                binary_name: "test-app".to_string(),
            },
        },
    );

    let stack = stack_with_permissions("test-stack")
        .add(func_with_invalid_rust, ResourceLifecycle::Frozen)
        .build();

    let temp_output_dir = tempdir().expect("Failed to create temp output dir");

    let settings = BuildSettings {
        output_directory: temp_output_dir.path().to_str().unwrap().to_string(),
        platform: PlatformBuildSettings::Test {},
        targets: None,
        cache_url: None,
        override_base_image: None,
        debug_mode: false,
    };

    let result = build_stack(stack, &settings).await;

    assert!(
        result.is_err(),
        "Expected build_stack to fail with invalid Rust project"
    );

    match result.unwrap_err().error {
        Some(alien_build::error::ErrorData::InvalidResourceConfig {
            resource_id,
            reason,
        }) => {
            assert_eq!(resource_id, "test-app");
            assert!(reason.contains("Cargo.toml"));
        }
        other => panic!("Expected InvalidResourceConfig error, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_real_cargo_init_project() {
    load_test_env(); // Load Docker credentials if available
    tracing_subscriber::fmt::try_init().ok();

    // Setup: Create a temporary directory with a valid project name
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let project_dir = temp_dir.path().join("test-cargo-app");
    fs::create_dir(&project_dir)
        .await
        .expect("Failed to create project dir");

    // Initialize a real Rust project with cargo
    let cargo_init_output = tokio::process::Command::new("cargo")
        .arg("init")
        .arg("--name")
        .arg("test-cargo-app")
        .arg("--bin")
        .current_dir(&project_dir)
        .output()
        .await
        .expect("Failed to run cargo init");

    if !cargo_init_output.status.success() {
        panic!(
            "cargo init failed: {}",
            String::from_utf8_lossy(&cargo_init_output.stderr)
        );
    }

    let temp_output_dir = tempdir().expect("Failed to create temp output dir");
    let output_dir_path = temp_output_dir.path().to_path_buf();

    let func_with_cargo_project = create_test_function(
        "my-cargo-func",
        FunctionCode::Source {
            src: project_dir.to_str().unwrap().to_string(),
            toolchain: alien_core::ToolchainConfig::Rust {
                binary_name: "test-cargo-app".to_string(),
            },
        },
    );

    let stack = stack_with_permissions("test-stack")
        .add(func_with_cargo_project, ResourceLifecycle::Frozen)
        .build();

    let settings = BuildSettings {
        output_directory: output_dir_path.to_str().unwrap().to_string(),
        platform: PlatformBuildSettings::Test {},
        targets: None,
        cache_url: None,
        override_base_image: None,
        debug_mode: false,
    };

    let result = build_stack(stack, &settings).await;

    assert!(
        result.is_ok(),
        "Expected build_stack to succeed with real cargo project: {:?}",
        result.err()
    );

    let built_stack = result.unwrap();

    // Verify that the function was converted to an Image
    let mut cargo_func_found = false;
    for (_id, entry) in built_stack.resources() {
        if let Some(f) = entry.config.downcast_ref::<alien_core::Function>() {
            if f.id == "my-cargo-func" {
                cargo_func_found = true;
                match &f.code {
                    FunctionCode::Image { image } => {
                        // After build, image should be a local directory path
                        let image_path = PathBuf::from(image);
                        assert!(
                            image_path.exists() && image_path.is_dir(),
                            "Image should be a local directory path, got: {}",
                            image
                        );
                        assert_image_dir_has_hash(image, "my-cargo-func");

                        // Verify the directory contains OCI tarballs
                        let test_output_dir = output_dir_path.join("build").join("test");
                        assert!(
                            image_path.starts_with(&test_output_dir),
                            "Image path should live under build/test, got: {}",
                            image_path.display()
                        );
                    }
                    _ => panic!("Function should have been converted to Image"),
                }
            }
        }
    }
    assert!(
        cargo_func_found,
        "Cargo function was not found in the result stack"
    );
}

#[tokio::test]
async fn test_rust_workspace_project() {
    load_test_env(); // Load Docker credentials if available
    tracing_subscriber::fmt::try_init().ok();

    let (_temp_dir, _workspace_dir, member_dir) =
        create_test_rust_workspace("test-workspace", "Hello from workspace binary!").await;

    test_rust_workspace_build(
        "my-workspace-func",
        "test-workspace-stack",
        member_dir,
        "Workspace target directory detection worked correctly",
    )
    .await;
}

#[tokio::test]
async fn test_rust_workspace_from_root() {
    load_test_env(); // Load Docker credentials if available
    tracing_subscriber::fmt::try_init().ok();

    let (_temp_dir, workspace_dir, _member_dir) = create_test_rust_workspace(
        "test-workspace-root",
        "Hello from workspace binary built from root!",
    )
    .await;

    test_rust_workspace_build(
        "my-workspace-root-func",
        "test-workspace-root-stack",
        workspace_dir,
        "Workspace target directory detection from root worked correctly",
    )
    .await;
}
