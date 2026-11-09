//! Records whether a built Container or Daemon has a command receiver that
//! reads its credential from `ALIEN_COMMANDS_CAPABILITY_FILE`.
//!
//! The receiver is app code: the app links `alien-commands` (Rust) or imports
//! `@alienplatform/commands` (TypeScript), and the build does not wrap or
//! replace it. The build therefore asks the dependency the app resolved. Both
//! packages publish `commandsCapabilityFile: true` in their own manifest
//! (`[package.metadata.alien]` in `Cargo.toml`, `alien` in `package.json`)
//! from the release that added the file support, so no version number is
//! compared and an older copy simply lacks the flag.
//!
//! Anything the build cannot inspect (a prebuilt image, a Dockerfile, Python,
//! a project whose dependencies are not installed) is unmarked. Unmarked is
//! the safe answer: deployers keep `ALIEN_COMMANDS_TOKEN` for that resource.

use alien_core::ToolchainConfig;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tokio::process::Command;
use tracing::{debug, info};

/// Manifest key set by the packages whose receiver reads the capability file.
const MARKER_KEY: &str = "commandsCapabilityFile";
const RUST_CRATE: &str = "alien-commands";
const TYPESCRIPT_PACKAGE: [&str; 2] = ["@alienplatform", "commands"];

/// Whether the receiver the app built from `src` reads the capability file.
pub(crate) async fn receiver_reads_capability_file(src: &str, toolchain: &ToolchainConfig) -> bool {
    let src = Path::new(src);
    let reads = match toolchain {
        ToolchainConfig::Rust { binary_name } => rust_app_links_marked_receiver(src, binary_name)
            .await
            .unwrap_or_else(|reason| {
                info!(src = %src.display(), "Command receiver left unmarked: {reason}");
                false
            }),
        ToolchainConfig::TypeScript { .. } => typescript_app_resolves_marked_receiver(src),
        // The build does not control what these images contain.
        ToolchainConfig::Python { .. } | ToolchainConfig::Docker { .. } => false,
    };
    debug!(src = %src.display(), reads, "Checked the command receiver for capability file support");
    reads
}

async fn rust_app_links_marked_receiver(src: &Path, binary_name: &str) -> Result<bool, String> {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .current_dir(src)
        .output()
        .await
        .map_err(|error| format!("cannot run cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err("cargo metadata failed".to_string());
    }
    let metadata: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("cannot read cargo metadata: {error}"))?;
    Ok(cargo_metadata_links_marked_receiver(&metadata, binary_name))
}

/// Whether the normal dependencies of the package that builds `binary_name`
/// include a `alien-commands` that carries the marker.
fn cargo_metadata_links_marked_receiver(metadata: &Value, binary_name: &str) -> bool {
    let packages = metadata["packages"].as_array().into_iter().flatten();
    let mut marked = HashSet::new();
    let mut root = None;
    for package in packages {
        let Some(id) = package["id"].as_str() else {
            continue;
        };
        if package["name"] == RUST_CRATE && package["metadata"]["alien"][MARKER_KEY] == true {
            marked.insert(id);
        }
        let builds_binary = package["targets"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|target| {
                target["name"] == binary_name
                    && target["kind"]
                        .as_array()
                        .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
            });
        if builds_binary {
            root = Some(id);
        }
    }
    let Some(root) = root else {
        return false;
    };

    let mut normal_dependencies: HashMap<&str, Vec<&str>> = HashMap::new();
    for node in metadata["resolve"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let (Some(id), Some(dependencies)) = (node["id"].as_str(), node["deps"].as_array()) else {
            continue;
        };
        let linked = dependencies
            .iter()
            .filter(|dependency| {
                dependency["dep_kinds"]
                    .as_array()
                    .is_some_and(|kinds| kinds.iter().any(|kind| kind["kind"].is_null()))
            })
            .filter_map(|dependency| dependency["pkg"].as_str())
            .collect();
        normal_dependencies.insert(id, linked);
    }

    let mut seen = HashSet::from([root]);
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        if marked.contains(id) {
            return true;
        }
        for next in normal_dependencies.get(id).into_iter().flatten() {
            if seen.insert(next) {
                pending.push(next);
            }
        }
    }
    false
}

/// Resolves `@alienplatform/commands` the way Node does from `src`: the
/// nearest `node_modules` up the tree that holds it.
fn typescript_app_resolves_marked_receiver(src: &Path) -> bool {
    let Ok(src) = src.canonicalize() else {
        return false;
    };
    for directory in src.ancestors() {
        let manifest = directory
            .join("node_modules")
            .join(TYPESCRIPT_PACKAGE[0])
            .join(TYPESCRIPT_PACKAGE[1])
            .join("package.json");
        let Ok(contents) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        return serde_json::from_str::<Value>(&contents)
            .is_ok_and(|package| package["alien"][MARKER_KEY] == true);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Writes a `alien-commands` stand-in crate. `marked` adds the manifest
    /// flag the real crate publishes.
    fn write_receiver_crate(dir: &Path, marked: bool) {
        fs::create_dir_all(dir.join("src")).unwrap();
        let metadata = if marked {
            "\n[package.metadata.alien]\ncommandsCapabilityFile = true\n"
        } else {
            ""
        };
        fs::write(
            dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"alien-commands\"\nversion = \"1.0.0\"\nedition = \"2021\"\n{metadata}"
            ),
        )
        .unwrap();
        fs::write(dir.join("src/lib.rs"), "").unwrap();
    }

    /// Writes an app crate that names `alien-commands` in the given
    /// dependency table (or not at all).
    fn write_app(dir: &Path, table: Option<&str>) {
        fs::create_dir_all(dir.join("src")).unwrap();
        let dependency = table
            .map(|table| format!("\n[{table}]\nalien-commands = {{ path = \"../receiver\" }}\n"))
            .unwrap_or_default();
        fs::write(
            dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{dependency}"
            ),
        )
        .unwrap();
        fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
    }

    async fn rust_receiver_marked(marked: bool, table: Option<&str>) -> bool {
        let root = tempdir().unwrap();
        write_receiver_crate(&root.path().join("receiver"), marked);
        write_app(&root.path().join("app"), table);
        receiver_reads_capability_file(
            root.path().join("app").to_str().unwrap(),
            &ToolchainConfig::Rust {
                binary_name: "app".to_string(),
            },
        )
        .await
    }

    #[tokio::test]
    async fn rust_app_linking_the_marked_receiver_is_marked() {
        assert!(rust_receiver_marked(true, Some("dependencies")).await);
    }

    #[tokio::test]
    async fn rust_app_linking_an_unmarked_receiver_is_not_marked() {
        // An older alien-commands has the same name and no flag.
        assert!(!rust_receiver_marked(false, Some("dependencies")).await);
    }

    #[tokio::test]
    async fn rust_app_without_a_receiver_is_not_marked() {
        assert!(!rust_receiver_marked(true, None).await);
    }

    #[tokio::test]
    async fn rust_app_using_the_receiver_only_in_tests_is_not_marked() {
        assert!(!rust_receiver_marked(true, Some("dev-dependencies")).await);
    }

    #[tokio::test]
    async fn rust_app_that_cargo_cannot_read_is_not_marked() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("Cargo.toml"), "not toml [").unwrap();
        let toolchain = ToolchainConfig::Rust {
            binary_name: "app".to_string(),
        };
        assert!(!receiver_reads_capability_file(root.path().to_str().unwrap(), &toolchain).await);
    }

    fn write_typescript_receiver(node_modules_parent: &Path, manifest: &str) {
        let package = node_modules_parent.join("node_modules/@alienplatform/commands");
        fs::create_dir_all(&package).unwrap();
        fs::write(package.join("package.json"), manifest).unwrap();
    }

    fn typescript() -> ToolchainConfig {
        ToolchainConfig::TypeScript { binary_name: None }
    }

    #[tokio::test]
    async fn typescript_app_resolving_the_marked_receiver_is_marked() {
        let root = tempdir().unwrap();
        let app = root.path().join("packages/app");
        fs::create_dir_all(&app).unwrap();
        // Hoisted to the workspace root, as a pnpm or npm workspace install does.
        write_typescript_receiver(
            root.path(),
            r#"{"name":"@alienplatform/commands","alien":{"commandsCapabilityFile":true}}"#,
        );
        assert!(receiver_reads_capability_file(app.to_str().unwrap(), &typescript()).await);
    }

    #[tokio::test]
    async fn typescript_app_resolving_an_older_receiver_is_not_marked() {
        let root = tempdir().unwrap();
        write_typescript_receiver(
            root.path(),
            r#"{"name":"@alienplatform/commands","version":"1.0.0"}"#,
        );
        assert!(
            !receiver_reads_capability_file(root.path().to_str().unwrap(), &typescript()).await
        );
    }

    #[tokio::test]
    async fn typescript_app_nearest_receiver_decides() {
        let root = tempdir().unwrap();
        let app = root.path().join("app");
        fs::create_dir_all(&app).unwrap();
        write_typescript_receiver(root.path(), r#"{"alien":{"commandsCapabilityFile":true}}"#);
        write_typescript_receiver(&app, r#"{"version":"1.0.0"}"#);
        assert!(!receiver_reads_capability_file(app.to_str().unwrap(), &typescript()).await);
    }

    #[tokio::test]
    async fn builds_the_receiver_cannot_be_inspected_for_are_not_marked() {
        let root = tempdir().unwrap();
        write_typescript_receiver(root.path(), r#"{"alien":{"commandsCapabilityFile":true}}"#);
        let src = root.path().to_str().unwrap();
        for toolchain in [
            ToolchainConfig::Python {
                python_version: None,
                package: None,
                command: vec!["python".to_string()],
            },
            ToolchainConfig::Docker {
                dockerfile: None,
                build_args: None,
                target: None,
            },
        ] {
            assert!(!receiver_reads_capability_file(src, &toolchain).await);
        }
    }
}
