use alien_build::{
    build_stack, push_stack,
    settings::{BuildSettings, PlatformBuildSettings, PushSettings},
};
use alien_core::{
    permissions::PermissionProfile, Function, FunctionCode, Ingress, Platform, ResourceLifecycle,
    Storage,
};
use dockdash::{test_utils::setup_local_registry, ClientProtocol, PushOptions, RegistryAuth};
use std::collections::HashMap;
use std::path::PathBuf as StdPathBuf;
use std::path::{Path, PathBuf};
use tempfile::tempdir;
use tokio::fs;
use workspace_root;

// Load environment variables from .env.test for Docker credentials
fn load_test_env() {
    let root: StdPathBuf = workspace_root::get_workspace_root();
    dotenvy::from_path(root.join(".env.test")).ok(); // OK to fail if not present
    std::env::set_var("ALIEN_SKIP_DEPENDENCY_INSTALL", "1");
}

// Get the path to the @alienplatform/sdk package in the workspace.
fn get_bindings_package_path() -> StdPathBuf {
    let root: StdPathBuf = workspace_root::get_workspace_root();
    root.join("packages/sdk")
}

// Install @alienplatform/sdk into the project's node_modules by copying the pre-built dist.
// This avoids issues with workspace: protocol dependencies when using file: protocol.
async fn install_bindings_package(project_dir: &Path) {
    let bindings_src = get_bindings_package_path();
    let bindings_dest = project_dir.join("node_modules/@alienplatform/sdk");

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

fn find_function_dir(base_dir: &Path, function_name: &str) -> PathBuf {
    let prefix = format!("{}-", function_name);
    let entries = std::fs::read_dir(base_dir)
        .unwrap_or_else(|_| panic!("Failed to read directory: {}", base_dir.display()));

    for entry in entries {
        if let Ok(entry) = entry {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name == function_name || name.starts_with(&prefix) {
                return path;
            }
        }
    }

    panic!(
        "Function directory with prefix '{}' not found in {}",
        prefix,
        base_dir.display()
    );
}

// Helper to create a minimal TypeScript project for testing
async fn create_test_typescript_project(base_dir: &Path, project_name: &str) -> PathBuf {
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

    // Create a simple index.js file as the entry point (with default export for bootstrap wrapper)
    let index_js_path = project_dir.join("index.js");
    fs::write(&index_js_path, "export default { name: 'test' };")
        .await
        .expect("Failed to write index.js");

    // Install @alienplatform/sdk (required by bootstrap wrapper)
    install_bindings_package(&project_dir).await;

    project_dir
}

// Helper function to create a TypeScript workspace for testing
async fn create_test_typescript_workspace(
    workspace_name: &str,
    package_manager: &str,
) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let workspace_dir = temp_dir.path().join(workspace_name);
    fs::create_dir(&workspace_dir)
        .await
        .expect("Failed to create workspace dir");

    // Create workspace package.json
    let workspace_package_json = match package_manager {
        "pnpm" => serde_json::json!({
            "name": workspace_name,
            "version": "1.0.0",
            "private": true,
            "workspaces": ["packages/*"],
            "devDependencies": {}
        }),
        _ => serde_json::json!({ // npm
            "name": workspace_name,
            "version": "1.0.0",
            "private": true,
            "workspaces": ["packages/*"],
            "devDependencies": {}
        }),
    };

    fs::write(
        workspace_dir.join("package.json"),
        serde_json::to_string_pretty(&workspace_package_json).unwrap(),
    )
    .await
    .expect("Failed to write workspace package.json");

    // Create pnpm-workspace.yaml for pnpm
    if package_manager == "pnpm" {
        let pnpm_workspace = "packages:\n  - 'packages/*'\n";
        fs::write(workspace_dir.join("pnpm-workspace.yaml"), pnpm_workspace)
            .await
            .expect("Failed to write pnpm-workspace.yaml");
    }

    // Create packages directory
    let packages_dir = workspace_dir.join("packages");
    fs::create_dir(&packages_dir)
        .await
        .expect("Failed to create packages dir");

    // Create member package directory
    let member_dir = packages_dir.join("my-app");
    fs::create_dir(&member_dir)
        .await
        .expect("Failed to create member dir");

    // Create member package.json with proper entry point
    let member_package_json = serde_json::json!({
        "name": "my-app",
        "version": "1.0.0",
        "main": "./src/index.js",
        "devDependencies": {}
    });

    fs::write(
        member_dir.join("package.json"),
        serde_json::to_string_pretty(&member_package_json).unwrap(),
    )
    .await
    .expect("Failed to write member package.json");

    // Create src directory and index.js
    let src_dir = member_dir.join("src");
    fs::create_dir(&src_dir)
        .await
        .expect("Failed to create src dir");

    // Create index.js with default export (required by bootstrap wrapper)
    let index_js_content = "export default { name: 'workspace-app' };";
    fs::write(src_dir.join("index.js"), index_js_content)
        .await
        .expect("Failed to write index.js");

    // Install @alienplatform/sdk (required by bootstrap wrapper)
    install_bindings_package(&member_dir).await;

    (temp_dir, workspace_dir, member_dir)
}

// Helper function to test workspace builds
async fn test_typescript_workspace_build(
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
            toolchain: alien_core::ToolchainConfig::TypeScript {
                binary_name: Some("app".to_string()),
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
        "Expected build_stack to succeed with TypeScript workspace project: {:?}",
        result.err()
    );

    let built_stack = result.unwrap();

    // Verify that the function was converted to an Image with local directory reference
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
                        let function_dir = find_function_dir(&test_output_dir, function_name);
                        assert!(
                            function_dir.exists(),
                            "Function directory should exist at: {}",
                            function_dir.display()
                        );
                    }
                    _ => panic!("Function should have been converted to Image"),
                }
            }
        }
    }
    assert!(
        func_found,
        "TypeScript workspace function '{}' was not found in the result stack",
        function_name
    );

    println!("✅ {}", success_message);
}

#[tokio::test]
async fn test_build_stack_with_source_code() {
    load_test_env(); // Load Docker credentials if available
    tracing_subscriber::fmt::try_init().ok();

    // Setup: Start local registry
    let (_registry_guard, local_registry_host) = setup_local_registry()
        .await
        .expect("Failed to set up local registry");

    // Setup: Create a temporary output directory for the test
    let temp_output_dir = tempdir().expect("Failed to create temp output dir");
    let output_dir_path = temp_output_dir.path().to_path_buf();

    // Setup: Create a temporary source directory for the test
    let temp_source_dir = tempdir().expect("Failed to create temp source dir");
    let project_dir =
        create_test_typescript_project(temp_source_dir.path(), "my-func-to-build").await;

    let func_to_build_resource = create_test_function(
        "my-func-to-build",
        FunctionCode::Source {
            src: project_dir.to_str().unwrap().to_string(),
            toolchain: alien_core::ToolchainConfig::TypeScript {
                binary_name: Some("app".to_string()),
            },
        },
    );
    let func_already_image_resource = create_test_function(
        "my-func-already-image",
        FunctionCode::Image {
            image: "existing/image:latest".to_string(),
        },
    );

    // Define a storage resource
    let storage_target = Storage::new("storage-target".to_string()).build();

    let stack = stack_with_permissions("test-stack")
        .add(storage_target.clone(), ResourceLifecycle::Frozen)
        .add(func_to_build_resource, ResourceLifecycle::Frozen)
        .add(func_already_image_resource, ResourceLifecycle::Live)
        .build();

    // Step 1: Build locally (no push)
    let build_settings = BuildSettings {
        output_directory: output_dir_path.to_str().unwrap().to_string(),
        platform: PlatformBuildSettings::Aws {
            managing_account_id: Some("123456789012".to_string()),
        },
        targets: None,
        cache_url: None,
        override_base_image: None, // No override needed - uses alien-base images
        debug_mode: false,
    };

    let result_stack = build_stack(stack, &build_settings).await;

    assert!(
        result_stack.is_ok(),
        "build_stack failed: {:?}",
        result_stack.err()
    );
    let built_stack = result_stack.unwrap();

    // Step 2: Push images to registry
    let push_settings = PushSettings {
        repository: format!("{}/test/build", local_registry_host),
        options: PushOptions {
            auth: RegistryAuth::Anonymous,
            protocol: ClientProtocol::Http,
            ..Default::default()
        },
    };

    let result_pushed = push_stack(built_stack, Platform::Aws, &push_settings).await;

    assert!(
        result_pushed.is_ok(),
        "push_stack failed: {:?}",
        result_pushed.err()
    );
    let built_stack = result_pushed.unwrap();

    // Verify stack.json was created in the AWS platform directory
    let platform_output_dir = output_dir_path.join("build").join(Platform::Aws.as_str());
    let stack_json_path = platform_output_dir.join("stack.json");
    assert!(
        stack_json_path.exists(),
        "stack.json was not created in the output directory: {}",
        stack_json_path.display()
    );
    let stack_json_content = fs::read_to_string(&stack_json_path)
        .await
        .expect("Failed to read stack.json");
    let _loaded_stack_from_json: alien_core::Stack =
        serde_json::from_str(&stack_json_content).expect("Failed to deserialize stack.json");

    let mut built_func_found = false;
    let mut image_func_untouched = false;

    for (_id, entry) in built_stack.resources() {
        if let Some(f) = entry.config.downcast_ref::<alien_core::Function>() {
            if f.id == "my-func-to-build" {
                built_func_found = true;
                match &f.code {
                    FunctionCode::Image { image } => {
                        // After push, image should be registry URL with format: {repo}:{function_name}-{tag}
                        assert!(image.starts_with(&format!("{}/test/build:", local_registry_host)) && image.contains("my-func-to-build"),
                            "Image URI '{}' should contain function name and be in registry format", image);
                        tracing::info!("Built and pushed function image URI: {}", image);

                        // Verify the OCI tarball exists in the build output directory
                        let function_dir = find_function_dir(&platform_output_dir, "my-func-to-build");
                        assert!(function_dir.exists(), "Function directory '{}' not found", function_dir.display());

                        // Check for at least one OCI tarball
                        let mut found_tarball = false;
                        if let Ok(mut entries) = std::fs::read_dir(&function_dir) {
                            while let Some(Ok(entry)) = entries.next() {
                                if entry.path().extension().and_then(|s| s.to_str()) == Some("tar") {
                                    found_tarball = true;
                                    break;
                                }
                            }
                        }
                        assert!(found_tarball, "No OCI tarballs found in {}", function_dir.display());
                    }
                    _ => panic!("Function '{}' was not converted to FunctionCode::Image with a pushed registry URI", f.id),
                }
            } else if f.id == "my-func-already-image" {
                image_func_untouched = true;
                match &f.code {
                    FunctionCode::Image { image } => {
                        assert_eq!(image, "existing/image:latest");
                    }
                    _ => panic!("Image function was unexpectedly modified"),
                }
            }
        }
    }
    assert!(
        built_func_found,
        "Built function was not found in the result stack"
    );
    assert!(
        image_func_untouched,
        "Pre-existing image function was not found or was modified"
    );
}

#[tokio::test]
async fn test_typescript_toolchain_invalid_project() {
    load_test_env(); // Load Docker credentials if available
    tracing_subscriber::fmt::try_init().ok();

    // Setup: Create a temporary directory without package.json
    let temp_source_dir = tempdir().expect("Failed to create temp source dir");
    fs::write(
        temp_source_dir.path().join("index.js"),
        "console.log('test');",
    )
    .await
    .unwrap();

    let func_with_invalid_ts = create_test_function(
        "invalid-ts-func",
        FunctionCode::Source {
            src: temp_source_dir.path().to_str().unwrap().to_string(),
            toolchain: alien_core::ToolchainConfig::TypeScript {
                binary_name: Some("app".to_string()),
            },
        },
    );

    let stack = stack_with_permissions("test-stack")
        .add(func_with_invalid_ts, ResourceLifecycle::Frozen)
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
        "Expected build_stack to fail with invalid TypeScript project"
    );

    match result.unwrap_err().error {
        Some(alien_build::error::ErrorData::InvalidResourceConfig {
            resource_id,
            reason,
        }) => {
            assert_eq!(resource_id, "typescript-project");
            assert!(reason.contains("package.json"));
        }
        other => panic!("Expected InvalidResourceConfig error, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_real_npm_init_project() {
    load_test_env(); // Load Docker credentials if available
    tracing_subscriber::fmt::try_init().ok();

    // Setup: Create a temporary directory with a valid npm project name
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let project_dir = temp_dir.path().join("test-npm-app");
    fs::create_dir(&project_dir)
        .await
        .expect("Failed to create project dir");

    // Initialize a real npm project
    let npm_init_output = tokio::process::Command::new("npm")
        .arg("init")
        .arg("-y") // Skip interactive prompts
        .current_dir(&project_dir)
        .output()
        .await
        .expect("Failed to run npm init");

    if !npm_init_output.status.success() {
        panic!(
            "npm init failed: {}",
            String::from_utf8_lossy(&npm_init_output.stderr)
        );
    }

    // Set main entry point in package.json
    let package_json_path = project_dir.join("package.json");
    let package_json_content = fs::read_to_string(&package_json_path)
        .await
        .expect("Failed to read package.json");

    let mut package_json: serde_json::Value =
        serde_json::from_str(&package_json_content).expect("Failed to parse package.json");

    // Set main entry point
    package_json["main"] = serde_json::Value::String("./index.js".to_string());

    fs::write(
        &package_json_path,
        serde_json::to_string_pretty(&package_json).unwrap(),
    )
    .await
    .expect("Failed to write updated package.json");

    // Create the entry point file with default export (required by bootstrap wrapper)
    fs::write(
        project_dir.join("index.js"),
        "export default { name: 'npm-app' };",
    )
    .await
    .expect("Failed to create index.js");

    // Install @alienplatform/sdk (required by bootstrap wrapper)
    install_bindings_package(&project_dir).await;

    let temp_output_dir = tempdir().expect("Failed to create temp output dir");
    let output_dir_path = temp_output_dir.path().to_path_buf();

    let func_with_npm_project = create_test_function(
        "my-npm-func",
        FunctionCode::Source {
            src: project_dir.to_str().unwrap().to_string(),
            toolchain: alien_core::ToolchainConfig::TypeScript {
                binary_name: Some("app".to_string()),
            },
        },
    );

    let stack = stack_with_permissions("test-stack")
        .add(func_with_npm_project, ResourceLifecycle::Frozen)
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
        "Expected build_stack to succeed with real npm project: {:?}",
        result.err()
    );

    let built_stack = result.unwrap();

    // Verify that the function was converted to an Image with local directory reference
    let mut npm_func_found = false;
    for (_id, entry) in built_stack.resources() {
        if let Some(f) = entry.config.downcast_ref::<alien_core::Function>() {
            if f.id == "my-npm-func" {
                npm_func_found = true;
                match &f.code {
                    FunctionCode::Image { image } => {
                        // After build, image should be a local directory path
                        let image_path = PathBuf::from(image);
                        assert!(
                            image_path.exists() && image_path.is_dir(),
                            "Image should be a local directory path, got: {}",
                            image
                        );
                        assert_image_dir_has_hash(image, "my-npm-func");

                        // Verify the directory contains OCI tarballs
                        let test_output_dir = output_dir_path.join("build").join("test");
                        let function_dir = find_function_dir(&test_output_dir, "my-npm-func");
                        assert!(
                            function_dir.exists(),
                            "Function directory should exist at: {}",
                            function_dir.display()
                        );
                    }
                    _ => panic!("Function should have been converted to Image"),
                }
            }
        }
    }
    assert!(
        npm_func_found,
        "npm function was not found in the result stack"
    );
}

// Only run pnpm test if pnpm is available
#[tokio::test]
async fn test_real_pnpm_init_project() {
    load_test_env(); // Load Docker credentials if available
    tracing_subscriber::fmt::try_init().ok();

    // Check if pnpm is available
    let pnpm_check = tokio::process::Command::new("pnpm")
        .arg("--version")
        .output()
        .await;

    if pnpm_check.is_err() {
        println!("Skipping pnpm test - pnpm not available");
        return;
    }

    // Setup: Create a temporary directory with a valid project name
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let project_dir = temp_dir.path().join("test-pnpm-app");
    fs::create_dir(&project_dir)
        .await
        .expect("Failed to create project dir");

    // Initialize a real pnpm project
    let pnpm_init_output = tokio::process::Command::new("pnpm")
        .arg("init")
        .current_dir(&project_dir)
        .output()
        .await
        .expect("Failed to run pnpm init");

    if !pnpm_init_output.status.success() {
        panic!(
            "pnpm init failed: {}",
            String::from_utf8_lossy(&pnpm_init_output.stderr)
        );
    }

    // Set main entry point in package.json
    let package_json_path = project_dir.join("package.json");
    let package_json_content = fs::read_to_string(&package_json_path)
        .await
        .expect("Failed to read package.json");

    let mut package_json: serde_json::Value =
        serde_json::from_str(&package_json_content).expect("Failed to parse package.json");

    // Set main entry point
    package_json["main"] = serde_json::Value::String("./index.js".to_string());

    fs::write(
        &package_json_path,
        serde_json::to_string_pretty(&package_json).unwrap(),
    )
    .await
    .expect("Failed to write updated package.json");

    // Create the entry point file with default export (required by bootstrap wrapper)
    fs::write(
        project_dir.join("index.js"),
        "export default { name: 'pnpm-app' };",
    )
    .await
    .expect("Failed to create index.js");

    // Install @alienplatform/sdk (required by bootstrap wrapper)
    install_bindings_package(&project_dir).await;

    let temp_output_dir = tempdir().expect("Failed to create temp output dir");
    let output_dir_path = temp_output_dir.path().to_path_buf();

    let func_with_pnpm_project = create_test_function(
        "my-pnpm-func",
        FunctionCode::Source {
            src: project_dir.to_str().unwrap().to_string(),
            toolchain: alien_core::ToolchainConfig::TypeScript {
                binary_name: Some("app".to_string()),
            },
        },
    );

    let stack = stack_with_permissions("test-stack")
        .add(func_with_pnpm_project, ResourceLifecycle::Frozen)
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
        "Expected build_stack to succeed with real pnpm project: {:?}",
        result.err()
    );

    let built_stack = result.unwrap();

    // Verify that the function was converted to an Image with local directory reference
    let mut pnpm_func_found = false;
    for (_id, entry) in built_stack.resources() {
        if let Some(f) = entry.config.downcast_ref::<alien_core::Function>() {
            if f.id == "my-pnpm-func" {
                pnpm_func_found = true;
                match &f.code {
                    FunctionCode::Image { image } => {
                        // After build, image should be a local directory path
                        let image_path = PathBuf::from(image);
                        assert!(
                            image_path.exists() && image_path.is_dir(),
                            "Image should be a local directory path, got: {}",
                            image
                        );
                        assert_image_dir_has_hash(image, "my-pnpm-func");

                        // Verify the directory contains OCI tarballs
                        let test_output_dir = output_dir_path.join("build").join("test");
                        let function_dir = find_function_dir(&test_output_dir, "my-pnpm-func");
                        assert!(
                            function_dir.exists(),
                            "Function directory should exist at: {}",
                            function_dir.display()
                        );
                    }
                    _ => panic!("Function should have been converted to Image"),
                }
            }
        }
    }
    assert!(
        pnpm_func_found,
        "pnpm function was not found in the result stack"
    );
}

// Only run bun test if bun is available
#[tokio::test]
async fn test_real_bun_init_project() {
    load_test_env(); // Load Docker credentials if available
    tracing_subscriber::fmt::try_init().ok();

    // Check if bun is available
    let bun_check = tokio::process::Command::new("bun")
        .arg("--version")
        .output()
        .await;

    if bun_check.is_err() {
        println!("Skipping bun test - bun not available");
        return;
    }

    // Setup: Create a temporary directory with a valid project name
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let project_dir = temp_dir.path().join("test-bun-app");
    fs::create_dir(&project_dir)
        .await
        .expect("Failed to create project dir");

    // Initialize a real bun project
    let bun_init_output = tokio::process::Command::new("bun")
        .arg("init")
        .arg("-y") // Skip interactive prompts
        .current_dir(&project_dir)
        .output()
        .await
        .expect("Failed to run bun init");

    if !bun_init_output.status.success() {
        panic!(
            "bun init failed: {}",
            String::from_utf8_lossy(&bun_init_output.stderr)
        );
    }

    // Add build script to package.json
    let package_json_path = project_dir.join("package.json");
    let package_json_content = fs::read_to_string(&package_json_path)
        .await
        .expect("Failed to read package.json");

    let mut package_json: serde_json::Value =
        serde_json::from_str(&package_json_content).expect("Failed to parse package.json");

    // Add build script
    if let Some(scripts) = package_json["scripts"].as_object_mut() {
        scripts.insert(
            "build".to_string(),
            serde_json::Value::String(
                "mkdir -p dist && echo 'console.log(\"Hello from bun!\");' > dist/index.js"
                    .to_string(),
            ),
        );
    } else {
        package_json["scripts"] = serde_json::json!({
            "build": "mkdir -p dist && echo 'console.log(\"Hello from bun!\");' > dist/index.js"
        });
    }

    fs::write(
        &package_json_path,
        serde_json::to_string_pretty(&package_json).unwrap(),
    )
    .await
    .expect("Failed to write updated package.json");

    // Update the index.ts file created by bun init to have a default export (required by bootstrap wrapper)
    fs::write(
        project_dir.join("index.ts"),
        "export default { name: 'bun-app' };",
    )
    .await
    .expect("Failed to update index.ts");

    // Install @alienplatform/sdk (required by bootstrap wrapper)
    install_bindings_package(&project_dir).await;

    let temp_output_dir = tempdir().expect("Failed to create temp output dir");
    let output_dir_path = temp_output_dir.path().to_path_buf();

    let func_with_bun_project = create_test_function(
        "my-bun-func",
        FunctionCode::Source {
            src: project_dir.to_str().unwrap().to_string(),
            toolchain: alien_core::ToolchainConfig::TypeScript {
                binary_name: Some("app".to_string()),
            },
        },
    );

    let stack = stack_with_permissions("test-stack")
        .add(func_with_bun_project, ResourceLifecycle::Frozen)
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
        "Expected build_stack to succeed with real bun project: {:?}",
        result.err()
    );

    let built_stack = result.unwrap();

    // Verify that the function was converted to an Image with local directory reference
    let mut bun_func_found = false;
    for (_id, entry) in built_stack.resources() {
        if let Some(f) = entry.config.downcast_ref::<alien_core::Function>() {
            if f.id == "my-bun-func" {
                bun_func_found = true;
                match &f.code {
                    FunctionCode::Image { image } => {
                        // After build, image should be a local directory path
                        let image_path = PathBuf::from(image);
                        assert!(
                            image_path.exists() && image_path.is_dir(),
                            "Image should be a local directory path, got: {}",
                            image
                        );
                        assert_image_dir_has_hash(image, "my-bun-func");

                        // Verify the directory contains OCI tarballs
                        let test_output_dir = output_dir_path.join("build").join("test");
                        let function_dir = find_function_dir(&test_output_dir, "my-bun-func");
                        assert!(
                            function_dir.exists(),
                            "Function directory should exist at: {}",
                            function_dir.display()
                        );
                    }
                    _ => panic!("Function should have been converted to Image"),
                }
            }
        }
    }
    assert!(
        bun_func_found,
        "bun function was not found in the result stack"
    );
}

#[tokio::test]
async fn test_npm_workspace_project() {
    load_test_env(); // Load Docker credentials if available
    tracing_subscriber::fmt::try_init().ok();

    let (_temp_dir, _workspace_dir, member_dir) =
        create_test_typescript_workspace("test-npm-workspace", "npm").await;

    test_typescript_workspace_build(
        "my-npm-workspace-func",
        "test-npm-workspace-stack",
        member_dir,
        "npm workspace build worked correctly",
    )
    .await;
}

#[tokio::test]
async fn test_pnpm_workspace_project() {
    load_test_env(); // Load Docker credentials if available
    tracing_subscriber::fmt::try_init().ok();

    // Check if pnpm is available
    let pnpm_check = tokio::process::Command::new("pnpm")
        .arg("--version")
        .output()
        .await;

    if pnpm_check.is_err() {
        println!("Skipping pnpm workspace test - pnpm not available");
        return;
    }

    let (_temp_dir, _workspace_dir, member_dir) =
        create_test_typescript_workspace("test-pnpm-workspace", "pnpm").await;

    test_typescript_workspace_build(
        "my-pnpm-workspace-func",
        "test-pnpm-workspace-stack",
        member_dir,
        "pnpm workspace build worked correctly",
    )
    .await;
}
