use alien_build::{
    build_stack,
    settings::{BuildSettings, PlatformBuildSettings},
};
use alien_core::{
    permissions::PermissionProfile, Function, FunctionCode, Ingress, ResourceLifecycle,
};
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::tempdir;
use tokio::fs;

// Load environment variables from .env.test for Docker credentials
fn load_test_env() {
    let root: PathBuf = workspace_root::get_workspace_root();
    dotenvy::from_path(root.join(".env.test")).ok(); // OK to fail if not present
    std::env::set_var("ALIEN_SKIP_DEPENDENCY_INSTALL", "1");
}

// Get the path to the @aliendotdev/bindings package in the workspace.
fn get_bindings_package_path() -> PathBuf {
    let root: PathBuf = workspace_root::get_workspace_root();
    root.join("packages/bindings")
}

// Install @aliendotdev/bindings into the project's node_modules by copying the pre-built dist.
// This avoids issues with workspace: protocol dependencies when using file: protocol.
async fn install_bindings_package(project_dir: &std::path::Path) {
    let bindings_src = get_bindings_package_path();
    let bindings_dest = project_dir.join("node_modules/@aliendotdev/bindings");

    // Create the destination directory
    fs::create_dir_all(&bindings_dest)
        .await
        .expect("Failed to create bindings directory in node_modules");

    // Copy package.json
    let package_json_content = std::fs::read_to_string(bindings_src.join("package.json"))
        .expect("Failed to read bindings package.json");
    fs::write(bindings_dest.join("package.json"), &package_json_content)
        .await
        .expect("Failed to write bindings package.json");

    // Copy dist folder
    let dist_src = bindings_src.join("dist");
    let dist_dest = bindings_dest.join("dist");
    fs::create_dir_all(&dist_dest)
        .await
        .expect("Failed to create dist directory");

    // Copy all files in dist
    let entries = std::fs::read_dir(&dist_src).expect("Failed to read bindings dist directory");
    for entry in entries {
        let entry = entry.expect("Failed to read dist entry");
        let file_name = entry.file_name();
        let src_path = entry.path();
        let dest_path = dist_dest.join(&file_name);

        let content = std::fs::read(&src_path).expect(&format!("Failed to read {:?}", src_path));
        fs::write(&dest_path, content)
            .await
            .expect(&format!("Failed to write {:?}", dest_path));
    }
}

fn stack_with_permissions(name: &str) -> alien_core::StackBuilder {
    alien_core::Stack::new(name.to_string()).permission("execution", PermissionProfile::new())
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

// Helper to create a minimal TypeScript project for testing
async fn create_test_typescript_project(base_dir: &std::path::Path, project_name: &str) -> PathBuf {
    let project_dir = base_dir.join(project_name);
    fs::create_dir_all(&project_dir)
        .await
        .expect("Failed to create project dir");

    // Create package.json with proper entry point
    let package_json = serde_json::json!({
        "name": project_name,
        "version": "1.0.0",
        "main": "./index.js"
    });

    let package_json_path = project_dir.join("package.json");
    fs::write(
        &package_json_path,
        serde_json::to_string_pretty(&package_json).unwrap(),
    )
    .await
    .expect("Failed to write package.json");

    // Create a simple index.js file with default export (required by bootstrap wrapper)
    let index_js_path = project_dir.join("index.js");
    fs::write(&index_js_path, "export default { name: 'test' };")
        .await
        .expect("Failed to write index.js");

    // Install @aliendotdev/bindings (required by bootstrap wrapper)
    install_bindings_package(&project_dir).await;

    project_dir
}

#[tokio::test]
async fn test_build_stack_with_missing_file_should_error() {
    load_test_env(); // Load Docker credentials if available
    tracing_subscriber::fmt::try_init().ok();

    // Setup: Create a temporary output directory for the test
    let temp_output_dir = tempdir().expect("Failed to create temp output dir");
    let output_dir_path = temp_output_dir.path().to_path_buf();

    let func_with_missing_file = create_test_function(
        "my-func-missing-file",
        FunctionCode::Source {
            src: "nonexistent/directory".to_string(), // This directory doesn't exist
            toolchain: alien_core::ToolchainConfig::TypeScript {
                binary_name: Some("app".to_string()),
            },
        },
    );

    let stack = alien_core::Stack::new("test-stack".to_string())
        .permission("execution", PermissionProfile::new())
        .add(func_with_missing_file, ResourceLifecycle::Frozen)
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

    // Should fail with InvalidResourceConfig error (toolchain validation happens first)
    assert!(
        result.is_err(),
        "Expected build_stack to fail with missing directory"
    );

    match result.unwrap_err().error {
        Some(alien_build::error::ErrorData::InvalidResourceConfig {
            resource_id,
            reason,
        }) => {
            assert_eq!(resource_id, "typescript-project");
            assert!(
                reason.contains("Source directory does not contain package.json")
                    || reason.contains("not found")
            );
        }
        other => panic!("Expected InvalidResourceConfig error, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_build_stack_with_glob_matching_no_files_should_succeed() {
    load_test_env(); // Load Docker credentials if available
    tracing_subscriber::fmt::try_init().ok();

    // Setup: Create a temporary output directory for the test
    let temp_output_dir = tempdir().expect("Failed to create temp output dir");
    let output_dir_path = temp_output_dir.path().to_path_buf();

    // Setup: Create a temporary source directory with at least one file so the build doesn't fail
    let temp_source_dir = tempdir().expect("Failed to create temp source dir");
    let project_dir =
        create_test_typescript_project(temp_source_dir.path(), "my-func-empty-glob").await;

    let func_with_empty_glob = create_test_function(
        "my-func-empty-glob",
        FunctionCode::Source {
            src: project_dir.to_str().unwrap().to_string(),
            toolchain: alien_core::ToolchainConfig::TypeScript {
                binary_name: Some("app".to_string()),
            },
        },
    );

    let stack = stack_with_permissions("test-stack")
        .add(func_with_empty_glob, ResourceLifecycle::Frozen)
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

    // Should succeed even though the glob pattern matches no files
    assert!(
        result.is_ok(),
        "Expected build_stack to succeed with empty glob pattern: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn test_build_stack_with_direct_file_path() {
    load_test_env(); // Load Docker credentials if available
    tracing_subscriber::fmt::try_init().ok();

    // Setup: Create a temporary output directory for the test
    let temp_output_dir = tempdir().expect("Failed to create temp output dir");
    let output_dir_path = temp_output_dir.path().to_path_buf();

    // Setup: Create a temporary source directory with a specific file
    let temp_source_dir = tempdir().expect("Failed to create temp source dir");
    let project_dir =
        create_test_typescript_project(temp_source_dir.path(), "my-func-direct-file").await;

    // Add an additional main.js file
    let main_js_path = project_dir.join("main.js");
    fs::write(&main_js_path, "console.log('main file');")
        .await
        .unwrap();

    let func_with_direct_file = create_test_function(
        "my-func-direct-file",
        FunctionCode::Source {
            src: project_dir.to_str().unwrap().to_string(),
            toolchain: alien_core::ToolchainConfig::TypeScript {
                binary_name: Some("app".to_string()),
            },
        },
    );

    let stack = stack_with_permissions("test-stack")
        .add(func_with_direct_file, ResourceLifecycle::Frozen)
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
        "Expected build_stack to succeed with direct file path: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn test_build_stack_with_direct_directory_path() {
    load_test_env(); // Load Docker credentials if available
    tracing_subscriber::fmt::try_init().ok();

    // Setup: Create a temporary output directory for the test
    let temp_output_dir = tempdir().expect("Failed to create temp output dir");
    let output_dir_path = temp_output_dir.path().to_path_buf();

    // Setup: Create a temporary source directory with files
    let temp_source_dir = tempdir().expect("Failed to create temp source dir");
    let project_dir =
        create_test_typescript_project(temp_source_dir.path(), "my-func-direct-dir").await;

    // Create a subdirectory with a file
    let sub_dir = project_dir.join("lib");
    fs::create_dir(&sub_dir).await.unwrap();
    let lib_js_path = sub_dir.join("utils.js");
    fs::write(&lib_js_path, "module.exports = {};")
        .await
        .unwrap();

    let func_with_direct_dir = create_test_function(
        "my-func-direct-dir",
        FunctionCode::Source {
            src: project_dir.to_str().unwrap().to_string(),
            toolchain: alien_core::ToolchainConfig::TypeScript {
                binary_name: Some("app".to_string()),
            },
        },
    );

    let stack = stack_with_permissions("test-stack")
        .add(func_with_direct_dir, ResourceLifecycle::Frozen)
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
        "Expected build_stack to succeed with direct directory path: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn test_build_stack_with_glob_patterns_matching_files() {
    load_test_env(); // Load Docker credentials if available
    tracing_subscriber::fmt::try_init().ok();

    // Setup: Create a temporary output directory for the test
    let temp_output_dir = tempdir().expect("Failed to create temp output dir");
    let output_dir_path = temp_output_dir.path().to_path_buf();

    // Setup: Create a temporary source directory with various files
    let temp_source_dir = tempdir().expect("Failed to create temp source dir");
    let project_dir =
        create_test_typescript_project(temp_source_dir.path(), "my-func-glob-files").await;

    // Create additional .js files
    let main_js_path = project_dir.join("main.js");
    fs::write(&main_js_path, "console.log('main');")
        .await
        .unwrap();

    // Create some .ts files
    let app_ts_path = project_dir.join("app.ts");
    fs::write(&app_ts_path, "console.log('app');")
        .await
        .unwrap();

    // Create a non-matching file
    let readme_path = project_dir.join("README.md");
    fs::write(&readme_path, "# README").await.unwrap();

    let func_with_glob_files = create_test_function(
        "my-func-glob-files",
        FunctionCode::Source {
            src: project_dir.to_str().unwrap().to_string(),
            toolchain: alien_core::ToolchainConfig::TypeScript {
                binary_name: Some("app".to_string()),
            },
        },
    );

    let stack = stack_with_permissions("test-stack")
        .add(func_with_glob_files, ResourceLifecycle::Frozen)
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
        "Expected build_stack to succeed with glob patterns matching files: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn test_build_stack_with_glob_patterns_matching_directories() {
    load_test_env(); // Load Docker credentials if available
    tracing_subscriber::fmt::try_init().ok();

    // Setup: Create a temporary output directory for the test
    let temp_output_dir = tempdir().expect("Failed to create temp output dir");
    let output_dir_path = temp_output_dir.path().to_path_buf();

    // Setup: Create a temporary source directory with subdirectories
    let temp_source_dir = tempdir().expect("Failed to create temp source dir");
    let project_dir =
        create_test_typescript_project(temp_source_dir.path(), "my-func-glob-dirs").await;

    // Create src directory with files
    let src_dir = project_dir.join("src");
    fs::create_dir(&src_dir).await.unwrap();
    let src_main_path = src_dir.join("main.js");
    fs::write(&src_main_path, "console.log('src main');")
        .await
        .unwrap();

    // Create lib directory with files
    let lib_dir = project_dir.join("lib");
    fs::create_dir(&lib_dir).await.unwrap();
    let lib_utils_path = lib_dir.join("utils.js");
    fs::write(&lib_utils_path, "module.exports = {};")
        .await
        .unwrap();

    // Create a non-matching directory
    let docs_dir = project_dir.join("docs");
    fs::create_dir(&docs_dir).await.unwrap();
    let docs_readme_path = docs_dir.join("README.md");
    fs::write(&docs_readme_path, "# Docs").await.unwrap();

    let func_with_glob_dirs = create_test_function(
        "my-func-glob-dirs",
        FunctionCode::Source {
            src: project_dir.to_str().unwrap().to_string(),
            toolchain: alien_core::ToolchainConfig::TypeScript {
                binary_name: Some("app".to_string()),
            },
        },
    );

    let stack = stack_with_permissions("test-stack")
        .add(func_with_glob_dirs, ResourceLifecycle::Frozen)
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
        "Expected build_stack to succeed with glob patterns matching directories: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn test_build_stack_with_missing_directory_should_error() {
    load_test_env(); // Load Docker credentials if available
    tracing_subscriber::fmt::try_init().ok();

    // Setup: Create a temporary output directory for the test
    let temp_output_dir = tempdir().expect("Failed to create temp output dir");
    let output_dir_path = temp_output_dir.path().to_path_buf();

    let func_with_missing_dir = create_test_function(
        "my-func-missing-dir",
        FunctionCode::Source {
            src: "nonexistent/directory".to_string(), // This directory doesn't exist
            toolchain: alien_core::ToolchainConfig::TypeScript {
                binary_name: Some("app".to_string()),
            },
        },
    );

    let stack = stack_with_permissions("test-stack")
        .add(func_with_missing_dir, ResourceLifecycle::Frozen)
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

    // Should fail with InvalidResourceConfig error (toolchain validation happens first)
    assert!(
        result.is_err(),
        "Expected build_stack to fail with missing directory"
    );

    match result.unwrap_err().error {
        Some(alien_build::error::ErrorData::InvalidResourceConfig {
            resource_id,
            reason,
        }) => {
            assert_eq!(resource_id, "typescript-project");
            assert!(
                reason.contains("Source directory does not contain package.json")
                    || reason.contains("not found")
            );
        }
        other => panic!("Expected InvalidResourceConfig error, got: {:?}", other),
    }
}
