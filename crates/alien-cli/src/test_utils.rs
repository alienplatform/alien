use alien_core::permissions::PermissionSetReference;
use alien_core::{
    Function, FunctionCode, ManagementPermissions, PermissionProfile, PermissionsConfig,
    ResourceLifecycle, Stack, Storage,
};
use indexmap::IndexMap;
use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;
use tokio::sync::OnceCell;
use workspace_root::get_workspace_root;

static SHARED_NODE_MODULES: OnceCell<PathBuf> = OnceCell::const_new();

/// Returns a `node_modules` directory containing `@alienplatform/core`, suitable
/// for symlinking into test temp dirs so bun can resolve the package.
///
/// Creates a minimal `node_modules` with a direct symlink to `packages/core`.
/// Builds `packages/core` first if `dist/` is missing. This avoids a fresh
/// `bun install` in a temp dir, which fails because packages/core has
/// `workspace:^` devDependencies that bun cannot resolve outside the monorepo.
pub async fn shared_node_modules_path() -> &'static PathBuf {
    SHARED_NODE_MODULES
        .get_or_init(|| async {
            let temp_dir = TempDir::new().unwrap();
            let temp_path = temp_dir.path().to_path_buf();
            setup_node_modules(&temp_path);
            Box::leak(Box::new(temp_dir));
            temp_path.join("node_modules")
        })
        .await
}

/// Helper to create package.json content with absolute path to @alienplatform/core
pub fn create_package_json_content() -> String {
    let workspace_root = get_workspace_root();
    let core_path = workspace_root.join("packages/core");

    format!(
        r#"{{
  "name": "test-alien-app",
  "type": "module",
  "devDependencies": {{
    "@alienplatform/core": "file://{}"
  }}
}}"#,
        core_path.display()
    )
}

/// Helper to create alien.ts content without Functions (to avoid image building)
pub fn create_basic_alien_ts() -> String {
    r#"import * as alien from "@alienplatform/core";

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

/// Helper to create alien.ts content with Functions
pub fn create_full_alien_ts() -> String {
    r#"import * as alien from "@alienplatform/core";

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

/// Helper to create alien.js content
pub fn create_javascript_config_content() -> String {
    r#"import * as alien from "@alienplatform/core";

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

/// Helper to create alien.json content
pub fn create_json_config_content() -> String {
    let stack = create_sample_stack("test-stack-json");
    serde_json::to_string_pretty(&stack).unwrap()
}

/// Build `packages/core` if needed and create `node_modules/@alienplatform/core`
/// as a direct symlink to it in `dir`. This avoids `bun install` which fails
/// for `workspace:^` devDependencies outside the monorepo.
pub fn setup_node_modules(dir: &std::path::Path) {
    let workspace_root = get_workspace_root();
    let core_path = workspace_root.join("packages/core");

    if !core_path.join("dist").exists() {
        std::process::Command::new("pnpm")
            .args(["--filter", "@alienplatform/core", "build"])
            .current_dir(&workspace_root)
            .output()
            .expect("Failed to build @alienplatform/core");
    }

    let scope_dir = dir.join("node_modules/@alienplatform");
    fs::create_dir_all(&scope_dir).unwrap();
    std::os::unix::fs::symlink(&core_path, scope_dir.join("core")).unwrap();
}

/// Helper to create a temporary Alien app directory
pub fn create_temp_alien_app(config_content: &str) -> TempDir {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    // Create package.json
    let package_json = create_package_json_content();
    fs::write(temp_path.join("package.json"), package_json).unwrap();

    // Create alien.ts
    fs::write(temp_path.join("alien.ts"), config_content).unwrap();

    // Set up node_modules so bun can resolve @alienplatform/core
    setup_node_modules(temp_path);

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
            let config_content = create_full_alien_ts();
            fs::write(temp_path.join("alien.ts"), config_content).unwrap();
        }
        "js" => {
            let config_content = create_javascript_config_content();
            fs::write(temp_path.join("alien.js"), config_content).unwrap();
        }
        "json" => {
            let config_content = create_json_config_content();
            fs::write(temp_path.join("alien.json"), config_content).unwrap();
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
