use alien_core::permissions::PermissionSetReference;
use alien_core::{
    Function, FunctionCode, ManagementPermissions, PermissionProfile, PermissionsConfig,
    ResourceLifecycle, Stack, Storage,
};
use indexmap::IndexMap;
use std::fs;

use tempfile::TempDir;
use workspace_root::get_workspace_root;

/// Helper to create package.json content with absolute path to @aliendotdev/core
pub fn create_package_json_content() -> String {
    let workspace_root = get_workspace_root();
    let core_path = workspace_root.join("packages/core");

    format!(
        r#"{{
  "name": "test-alien-app",
  "type": "module",
  "devDependencies": {{
    "@aliendotdev/core": "file://{}"
  }}
}}"#,
        core_path.display()
    )
}

/// Helper to create alien.config.ts content without Functions (to avoid image building)
pub fn create_basic_alien_config_ts() -> String {
    r#"import * as alien from "@aliendotdev/core";

const storage = new alien.Storage("test-storage")
  .publicRead(true)
  .build();

const stack = new alien.Stack("test-stack")
  .add(storage, "frozen")
  .permissions({
    profiles: {},
    management: "auto"
  })
  .build();

export default stack;
"#
    .to_string()
}

/// Helper to create alien.config.ts content with Functions
pub fn create_full_alien_config_ts() -> String {
    r#"import * as alien from "@aliendotdev/core";

const storage = new alien.Storage("test-storage")
  .publicRead(true)
  .build();

const func = new alien.Function("test-function")
  .code({
    type: "image",
    image: "test:latest",
  })
  .permissions("execution")
  .link(storage)
  .build();

const stack = new alien.Stack("test-stack-ts")
  .add(storage, "frozen")
  .add(func, "live")
  .permissions({
    profiles: {
      execution: {
        "*": ["storage/data-read"],
        "test-storage": ["storage/data-write"]
      }
    },
    management: {
      extend: {
        "*": ["function/provision", "storage/management"]
      }
    }
  })
  .build();

export default stack;
"#
    .to_string()
}

/// Helper to create alien.config.js content
pub fn create_javascript_config_content() -> String {
    r#"import * as alien from "@aliendotdev/core";

const storage = new alien.Storage("test-storage")
  .publicRead(true)
  .build();

const func = new alien.Function("test-function")
  .code({
    type: "image",
    image: "test:latest",
  })
  .permissions("execution")
  .link(storage)
  .build();

const stack = new alien.Stack("test-stack-js")
  .add(storage, "frozen")
  .add(func, "live")
  .permissions({
    profiles: {
      execution: {
        "*": ["storage/data-read"],
        "test-storage": ["storage/data-write"]
      }
    },
    management: {
      extend: {
        "*": ["function/provision", "storage/management"]
      }
    }
  })
  .build();

export default stack;
"#
    .to_string()
}

/// Helper to create a sample Stack for testing
pub fn create_sample_stack(stack_id: &str) -> Stack {
    let storage = Storage::new("test-storage".to_string())
        .public_read(true)
        .build();

    let function = Function::new("test-function".to_string())
        .code(FunctionCode::Image {
            image: "test:latest".to_string(),
        })
        .permissions("execution".to_string())
        .link(&storage)
        .build();

    let mut permissions = IndexMap::new();

    // Create execution permission profile
    let mut execution_permissions = IndexMap::new();
    execution_permissions.insert(
        "*".to_string(),
        vec![PermissionSetReference::from_name("storage/data-read")],
    );
    execution_permissions.insert(
        "test-storage".to_string(),
        vec![PermissionSetReference::from_name("storage/data-write")],
    );

    permissions.insert(
        "execution".to_string(),
        PermissionProfile(execution_permissions),
    );

    // Create management permission profile
    let mut management_permissions = IndexMap::new();
    management_permissions.insert(
        "*".to_string(),
        vec![
            PermissionSetReference::from_name("function/provision"),
            PermissionSetReference::from_name("storage/management"),
        ],
    );

    permissions.insert(
        "management".to_string(),
        PermissionProfile(management_permissions),
    );

    Stack::new(stack_id.to_string())
        .add(storage, ResourceLifecycle::Frozen)
        .add(function, ResourceLifecycle::Live)
        .permissions(PermissionsConfig {
            profiles: permissions,
            management: ManagementPermissions::Auto,
        })
        .build()
}

/// Helper to create alien.config.json content
pub fn create_json_config_content() -> String {
    let stack = create_sample_stack("test-stack-json");
    serde_json::to_string_pretty(&stack).unwrap()
}

/// Helper to create a temporary Alien app directory
pub fn create_temp_alien_app(config_content: &str) -> TempDir {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    // Create package.json
    let package_json = create_package_json_content();
    fs::write(temp_path.join("package.json"), package_json).unwrap();

    // Create alien.config.ts
    fs::write(temp_path.join("alien.config.ts"), config_content).unwrap();

    // Note: Dependencies are not automatically installed for test helpers.
    // Tests that need dependencies should install them explicitly using
    // alien_build::dependencies::install_dependencies() if needed.

    temp_dir
}

/// Helper to create a temporary app directory with specific config file type
pub fn create_temp_app_dir(config_type: &str) -> TempDir {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    // Create package.json
    let package_json = create_package_json_content();
    fs::write(temp_path.join("package.json"), package_json).unwrap();

    // Create config file based on type
    match config_type {
        "ts" => {
            let config_content = create_full_alien_config_ts();
            fs::write(temp_path.join("alien.config.ts"), config_content).unwrap();
        }
        "js" => {
            let config_content = create_javascript_config_content();
            fs::write(temp_path.join("alien.config.js"), config_content).unwrap();
        }
        "json" => {
            let config_content = create_json_config_content();
            fs::write(temp_path.join("alien.config.json"), config_content).unwrap();
        }
        _ => panic!("Unknown config type: {}", config_type),
    }

    // Note: Dependencies are not automatically installed for test helpers.
    // Tests that need dependencies should install them explicitly using
    // alien_build::dependencies::install_dependencies() if needed.

    temp_dir
}

/// Get the path to the alien cli binary
pub fn get_alien_cli_binary() -> std::path::PathBuf {
    // For tests, we need to use runtime environment variable
    if let Ok(bin_path) = std::env::var("CARGO_BIN_EXE_alien") {
        std::path::PathBuf::from(bin_path)
    } else {
        // Fallback for development - assume binary is in target/debug or target/release
        let workspace_root = get_workspace_root();
        let debug_path = workspace_root.join("target/debug/alien");
        let release_path = workspace_root.join("target/release/alien");

        if debug_path.exists() {
            debug_path
        } else if release_path.exists() {
            release_path
        } else {
            // Final fallback - just the binary name and hope it's in PATH
            std::path::PathBuf::from("alien")
        }
    }
}

/// Get the runtime URL for tests
pub fn get_test_runtime_url() -> String {
    let workspace_root = get_workspace_root();
    format!("file://{}/target/", workspace_root.display())
}
