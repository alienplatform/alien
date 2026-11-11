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

use alien_core::{BinaryTarget, ToolchainConfig};
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
pub(crate) async fn receiver_reads_capability_file(
    src: &str,
    toolchain: &ToolchainConfig,
    targets: &[BinaryTarget],
) -> bool {
    let src = Path::new(src);
    let reads = match toolchain {
        ToolchainConfig::Rust { binary_name } => {
            // The receiver counts only when every built target links a marked
            // one; with no target to check, or any doubt, it is unmarked.
            let mut all_marked = !targets.is_empty();
            for target in targets {
                let triple = target.rust_target_triple();
                match rust_app_links_marked_receiver(src, binary_name, triple).await {
                    Ok(true) => {}
                    Ok(false) => {
                        all_marked = false;
                        break;
                    }
                    Err(reason) => {
                        info!(src = %src.display(), "Command receiver left unmarked: {reason}");
                        all_marked = false;
                        break;
                    }
                }
            }
            all_marked
        }
        ToolchainConfig::TypeScript { .. } => typescript_app_resolves_marked_receiver(src),
        // The build does not control what these images contain.
        ToolchainConfig::Python { .. } | ToolchainConfig::Docker { .. } => false,
    };
    debug!(src = %src.display(), reads, "Checked the command receiver for capability file support");
    reads
}

/// Resolved for `triple` only, so a dependency declared for another platform
/// does not count.
async fn rust_app_links_marked_receiver(
    src: &Path,
    binary_name: &str,
    triple: &str,
) -> Result<bool, String> {
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--filter-platform",
            triple,
        ])
        .current_dir(src)
        .output()
        .await
        .map_err(|error| format!("cannot run cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err("cargo metadata failed".to_string());
    }
    let metadata: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("cannot read cargo metadata: {error}"))?;
    Ok(cargo_metadata_links_marked_receiver(
        &metadata,
        binary_name,
        src,
    ))
}

/// Whether the normal dependencies of the package that builds `binary_name`
/// from `src` include a `alien-commands` that carries the marker. In a
/// workspace several members can build a binary of that name; the one Cargo
/// builds from the resource's source directory is the package whose manifest
/// is there. When none is, only a package that is the single builder of the
/// binary counts, and otherwise the receiver is unmarked.
fn cargo_metadata_links_marked_receiver(metadata: &Value, binary_name: &str, src: &Path) -> bool {
    let manifest = src
        .canonicalize()
        .unwrap_or_else(|_| src.to_path_buf())
        .join("Cargo.toml");
    let packages = metadata["packages"].as_array().into_iter().flatten();
    let mut marked = HashSet::new();
    let mut builders = Vec::new();
    let mut from_source = None;
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
            builders.push(id);
            let at_source = package["manifest_path"].as_str().is_some_and(|path| {
                Path::new(path)
                    .canonicalize()
                    .unwrap_or_else(|_| Path::new(path).to_path_buf())
                    == manifest
            });
            if at_source {
                from_source = Some(id);
            }
        }
    }
    let root = match (from_source, builders.as_slice()) {
        (Some(id), _) => id,
        (None, [only]) => only,
        _ => return false,
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

    const LINUX: &[BinaryTarget] = &[BinaryTarget::LinuxX64];

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
            LINUX,
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

    /// Two workspace members can build a binary of the same name from different
    /// receiver versions. The package Cargo builds from the resource's source
    /// directory decides, not whichever member comes last.
    #[tokio::test]
    async fn the_package_built_from_the_source_directory_decides_the_marker() {
        let root = tempdir().unwrap();
        write_receiver_crate(&root.path().join("old"), false);
        let new = root.path().join("new");
        write_receiver_crate(&new, true);
        fs::write(
            new.join("Cargo.toml"),
            "[package]\nname = \"alien-commands\"\nversion = \"2.0.0\"\nedition = \"2021\"\n\n[package.metadata.alien]\ncommandsCapabilityFile = true\n",
        )
        .unwrap();
        let workspace = root.path().join("ws");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(
            workspace.join("Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = [\"a\", \"b\"]\n",
        )
        .unwrap();
        for (member, receiver) in [("a", "old"), ("b", "new")] {
            let dir = workspace.join(member);
            fs::create_dir_all(dir.join("src")).unwrap();
            fs::write(
                dir.join("Cargo.toml"),
                format!(
                    "[package]\nname = \"app-{member}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[[bin]]\nname = \"app\"\npath = \"src/main.rs\"\n\n[dependencies]\nalien-commands = {{ path = \"../../{receiver}\" }}\n"
                ),
            )
            .unwrap();
            fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
        }

        assert!(!rust_app_links_marked_receiver(
            &workspace.join("a"),
            "app",
            "x86_64-unknown-linux-musl"
        )
        .await
        .unwrap());
        assert!(rust_app_links_marked_receiver(
            &workspace.join("b"),
            "app",
            "x86_64-unknown-linux-musl"
        )
        .await
        .unwrap());
    }

    /// An app can link an older receiver on one platform and a marked one on
    /// another. Only the platforms actually built count, and with several,
    /// every one of them must link a marked receiver.
    #[tokio::test]
    async fn the_receiver_is_marked_only_when_every_built_target_links_a_marked_one() {
        let root = tempdir().unwrap();
        write_receiver_crate(&root.path().join("old"), false);
        let new = root.path().join("new");
        fs::create_dir_all(new.join("src")).unwrap();
        fs::write(
            new.join("Cargo.toml"),
            "[package]\nname = \"alien-commands\"\nversion = \"2.0.0\"\nedition = \"2021\"\n\n[package.metadata.alien]\ncommandsCapabilityFile = true\n",
        )
        .unwrap();
        fs::write(new.join("src/lib.rs"), "").unwrap();
        let app = root.path().join("app");
        fs::create_dir_all(app.join("src")).unwrap();
        fs::write(
            app.join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[target.'cfg(target_os = \"linux\")'.dependencies]\nlinux-receiver = { path = \"../old\", package = \"alien-commands\" }\n\n[target.'cfg(target_os = \"macos\")'.dependencies]\nmac-receiver = { path = \"../new\", package = \"alien-commands\" }\n",
        )
        .unwrap();
        fs::write(app.join("src/main.rs"), "fn main() {}").unwrap();
        let toolchain = ToolchainConfig::Rust {
            binary_name: "app".to_string(),
        };
        let src = app.to_str().unwrap();

        // Linux links the older receiver, so a Linux build is unmarked.
        assert!(!receiver_reads_capability_file(src, &toolchain, LINUX).await);
        // macOS links the marked one.
        assert!(
            receiver_reads_capability_file(src, &toolchain, &[BinaryTarget::DarwinArm64]).await
        );
        // Both built: every target must link a marked receiver.
        assert!(
            !receiver_reads_capability_file(
                src,
                &toolchain,
                &[BinaryTarget::LinuxX64, BinaryTarget::DarwinArm64]
            )
            .await
        );
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
        assert!(
            !receiver_reads_capability_file(root.path().to_str().unwrap(), &toolchain, LINUX).await
        );
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
        assert!(receiver_reads_capability_file(app.to_str().unwrap(), &typescript(), LINUX).await);
    }

    #[tokio::test]
    async fn typescript_app_resolving_an_older_receiver_is_not_marked() {
        let root = tempdir().unwrap();
        write_typescript_receiver(
            root.path(),
            r#"{"name":"@alienplatform/commands","version":"1.0.0"}"#,
        );
        assert!(
            !receiver_reads_capability_file(root.path().to_str().unwrap(), &typescript(), LINUX)
                .await
        );
    }

    #[tokio::test]
    async fn typescript_app_nearest_receiver_decides() {
        let root = tempdir().unwrap();
        let app = root.path().join("app");
        fs::create_dir_all(&app).unwrap();
        write_typescript_receiver(root.path(), r#"{"alien":{"commandsCapabilityFile":true}}"#);
        write_typescript_receiver(&app, r#"{"version":"1.0.0"}"#);
        assert!(!receiver_reads_capability_file(app.to_str().unwrap(), &typescript(), LINUX).await);
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
            assert!(!receiver_reads_capability_file(src, &toolchain, LINUX).await);
        }
    }
}
