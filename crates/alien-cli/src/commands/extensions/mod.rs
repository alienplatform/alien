//! `alien extensions`: build an extension's bundle, and test it.
//!
//! An extension is a Cargo workspace whose `[workspace.metadata.alien-extension]` table names its
//! hooks crate, and its controller and types crates when it has them. Both commands run locally,
//! with no manager or platform account.

mod build;
mod local_bundle;
pub(crate) mod release;
mod test;
mod typescript;
pub(crate) mod upload;

use std::path::{Path, PathBuf};
use std::process::Command;

use alien_core::hooks::HooksManifest;
use alien_core::{BinaryTarget, Platform};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_hooks_runtime::{HookModule, Runtime};
use clap::{Parser, Subcommand};
use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::{ErrorData, Result};

/// Where `alien extensions build` writes the bundle, under the extension's root.
pub(crate) const BUNDLE_DIR: &str = "target/alien-extension";

/// Where `alien extensions build` writes the module alien.ts imports, under the extension's root.
pub(crate) const GENERATED_DIR: &str = "generated";

pub use build::build_task;
pub use local_bundle::load_local_bundles;
pub use test::test_task;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Build and test an extension",
    long_about = "Build and test an extension.

Run these from the extension's Cargo workspace, whose Cargo.toml names its crates under
[workspace.metadata.alien-extension].

EXAMPLES:
    # Run the extension's tests, then the hooks crate's tests and each importer fixture natively
    # and in the sandbox Alien runs hooks in
    alien extensions test

    # Write the bundle to target/alien-extension/ and generated/index.ts for alien.ts
    alien extensions build

    # Build the controller for chosen targets
    alien extensions build --target linux-x64,linux-arm64
"
)]
pub struct ExtensionsArgs {
    #[command(subcommand)]
    pub action: ExtensionsAction,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ExtensionsAction {
    /// Build the hooks module and the controller into a bundle in target/alien-extension/, and
    /// write generated/index.ts, the module alien.ts imports.
    Build {
        /// Controller targets, comma-separated: linux-x64, linux-arm64, darwin-arm64,
        /// windows-x64. Defaults to every target this machine can build without installing
        /// anything.
        #[arg(long = "target", value_delimiter = ',', value_parser = parse_target)]
        targets: Vec<BinaryTarget>,
    },
    /// Run the extension's tests natively, then run the hooks crate's tests and each importer
    /// fixture natively and in the sandbox Alien runs hooks in, and fail where the two differ.
    Test,
}

pub async fn extensions_task(args: ExtensionsArgs) -> Result<()> {
    let directory =
        std::env::current_dir()
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read".to_string(),
                file_path: ".".to_string(),
                reason: "the current directory can't be read".to_string(),
            })?;
    match args.action {
        ExtensionsAction::Build { targets } => build_task(&directory, &targets).await,
        ExtensionsAction::Test => test_task(&directory).await,
    }
}

fn parse_target(value: &str) -> std::result::Result<BinaryTarget, String> {
    serde_json::from_value(Value::String(value.to_string())).map_err(|_| {
        format!(
            "`{value}` is not a target: use linux-x64, linux-arm64, darwin-arm64 or windows-x64"
        )
    })
}

/// An extension's Cargo workspace, as `cargo metadata` and its `alien-extension` table describe
/// it.
pub(crate) struct ExtensionWorkspace {
    /// The directory of the workspace's Cargo.toml.
    pub root: PathBuf,
    /// Cargo's target directory.
    pub target_dir: PathBuf,
    pub hooks: Member,
    pub controller: Option<Member>,
    pub controller_url: Option<String>,
    /// The hooks crate's `alien-extension-sdk`, as a Cargo dependency, or `None` when no
    /// workspace crate depends on the SDK.
    pub sdk_dependency: Option<String>,
}

/// A crate of the workspace.
pub(crate) struct Member {
    pub name: String,
    pub version: String,
    pub dir: PathBuf,
    /// Its binary targets' names.
    pub bins: Vec<String>,
}

/// `[workspace.metadata.alien-extension]`: each value is a crate's directory, relative to the
/// workspace root.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtensionTable {
    hooks: String,
    controller: Option<String>,
    types: Option<String>,
    #[serde(rename = "controller-url")]
    controller_url: Option<String>,
}

/// Whether `directory` holds an extension's Cargo workspace: its Cargo.toml has the
/// `[workspace.metadata.alien-extension]` table.
pub(crate) fn is_extension_workspace(directory: &Path) -> bool {
    std::fs::read_to_string(directory.join("Cargo.toml"))
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .is_some_and(|manifest| {
            manifest
                .get("workspace")
                .and_then(|workspace| workspace.get("metadata"))
                .and_then(|metadata| metadata.get("alien-extension"))
                .is_some()
        })
}

pub(crate) fn read_workspace(directory: &Path) -> Result<ExtensionWorkspace> {
    // Without `--no-deps` cargo resolves the dependencies and writes Cargo.lock when the workspace
    // has none, which the module build's `--locked` needs.
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .current_dir(directory)
        .output()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!(
                "could not run 'cargo metadata' in '{}'",
                directory.display()
            ),
        })?;
    if !output.status.success() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "'cargo metadata' failed in '{}':\n{}",
                directory.display(),
                String::from_utf8_lossy(&output.stderr)
            ),
        }));
    }
    let metadata: Value = serde_json::from_slice(&output.stdout)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "parse".to_string(),
            reason: "'cargo metadata' printed JSON this CLI can't read".to_string(),
        })?;
    workspace_from_metadata(&metadata)
}

fn workspace_from_metadata(metadata: &Value) -> Result<ExtensionWorkspace> {
    let path = |key: &str| {
        metadata[key].as_str().map(PathBuf::from).ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: format!("'cargo metadata' printed no {key}"),
            })
        })
    };
    let root = path("workspace_root")?;
    let target_dir = path("target_directory")?;
    let table = metadata["metadata"]
        .get("alien-extension")
        .cloned()
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: format!(
                    "'{}' is not an extension: its Cargo.toml has no [workspace.metadata.alien-extension] table naming the hooks crate",
                    root.join("Cargo.toml").display()
                ),
            })
        })?;
    let table: ExtensionTable = serde_json::from_value(table)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "[workspace.metadata.alien-extension] takes `hooks`, and optionally `controller`, `types` and `controller-url`".to_string(),
        })?;

    let packages = metadata["packages"].as_array().cloned().unwrap_or_default();
    let members: Vec<&Value> = metadata["workspace_members"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|id| packages.iter().find(|package| package["id"] == *id))
        .collect();
    let member = |key: &str, relative: &str| -> Result<Member> {
        let dir = root.join(relative);
        members
            .iter()
            .find(|package| {
                package["manifest_path"]
                    .as_str()
                    .and_then(|manifest| Path::new(manifest).parent())
                    == Some(dir.as_path())
            })
            .map(|package| Member {
                name: package["name"].as_str().unwrap_or_default().to_string(),
                version: package["version"].as_str().unwrap_or_default().to_string(),
                dir: dir.clone(),
                bins: package["targets"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|target| {
                        target["kind"]
                            .as_array()
                            .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
                    })
                    .filter_map(|target| target["name"].as_str().map(str::to_string))
                    .collect(),
            })
            .ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: format!(
                        "[workspace.metadata.alien-extension] names `{key} = \"{relative}\"`, and the workspace has no member crate in '{}'",
                        dir.display()
                    ),
                })
            })
    };
    let hooks = member("hooks", &table.hooks)?;
    let controller = table
        .controller
        .as_deref()
        .map(|relative| member("controller", relative))
        .transpose()?;
    if let Some(types) = &table.types {
        member("types", types)?;
    }

    Ok(ExtensionWorkspace {
        root,
        target_dir,
        hooks,
        controller,
        controller_url: table.controller_url,
        sdk_dependency: sdk_dependency(&packages)?,
    })
}

/// How `cargo metadata` names crates.io, whichever protocol fetched the index.
const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

fn unsupported(source: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ConfigurationError {
        message: format!(
            "alien-extension-sdk comes from '{source}', which `alien extensions test` can't build against"
        ),
    })
}

/// The extension SDK the workspace builds against, named as cargo resolved it, so the test
/// harness gets the same package.
fn sdk_dependency(packages: &[Value]) -> Result<Option<String>> {
    let Some(sdk) = packages
        .iter()
        .find(|package| package["name"] == "alien-extension-sdk")
    else {
        return Ok(None);
    };
    let quoted = |value: &str| toml::Value::String(value.to_string()).to_string();
    let source = match sdk["source"].as_str() {
        None => {
            let dir = sdk["manifest_path"]
                .as_str()
                .and_then(|manifest| Path::new(manifest).parent())
                .map(|dir| dir.display().to_string())
                .unwrap_or_default();
            format!("path = {}", quoted(&dir))
        }
        Some(CRATES_IO) => {
            format!(
                "version = {}",
                quoted(&format!("={}", sdk["version"].as_str().unwrap_or_default()))
            )
        }
        Some(source) if source.starts_with("git+") => {
            // `git+<url>[?<tag|branch|rev>=<value>]#<commit>`. Cargo tells git sources apart by
            // the URL and the reference as the manifest wrote it, so the harness must name the
            // same reference, not the commit it resolved to, or it builds a second copy of the
            // SDK.
            let rest = &source["git+".len()..];
            let rest = rest.split_once('#').map_or(rest, |(rest, _)| rest);
            let (url, query) = rest.split_once('?').unwrap_or((rest, ""));
            let mut dependency = format!("git = {}", quoted(url));
            for (kind, value) in url::form_urlencoded::parse(query.as_bytes()) {
                if !matches!(kind.as_ref(), "tag" | "branch" | "rev") {
                    return Err(unsupported(source));
                }
                dependency.push_str(&format!(", {kind} = {}", quoted(&value)));
            }
            dependency
        }
        Some(source) => return Err(unsupported(source)),
    };
    Ok(Some(format!("{{ {source} }}")))
}

/// The controller crate's one binary target.
pub(crate) fn controller_bin(controller: &Member) -> Result<&str> {
    match controller.bins.as_slice() {
        [bin] => Ok(bin),
        bins => Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "the controller crate {} must have exactly one binary target, and it has {}",
                controller.name,
                bins.len()
            ),
        })),
    }
}

/// Builds the controller for this machine, as `cargo build` does, and returns the binary: what
/// `--controllers` and the importer check run.
pub(crate) fn build_host_controller(
    workspace: &ExtensionWorkspace,
    controller: &Member,
) -> Result<PathBuf> {
    let bin = controller_bin(controller)?;
    let output = Command::new("cargo")
        .args(["build", "--message-format", "json", "--package"])
        .arg(&controller.name)
        .args(["--bin", bin])
        .current_dir(&workspace.root)
        .stderr(std::process::Stdio::inherit())
        .output()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "could not run 'cargo build' for the controller".to_string(),
        })?;
    if !output.status.success() {
        return Err(AlienError::new(ErrorData::ExtensionBuildFailed {
            message: format!("the controller {bin} did not build; its output is above"),
        }));
    }
    output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
        .filter(|message| {
            message["reason"] == "compiler-artifact" && message["target"]["name"] == bin
        })
        .find_map(|message| message["executable"].as_str().map(PathBuf::from))
        .ok_or_else(|| {
            AlienError::new(ErrorData::ExtensionBuildFailed {
                message: format!("cargo built the controller and reported no binary named {bin}"),
            })
        })
}

/// The resource types and platforms `run_controllers` registers, from `--controllers`.
pub(crate) fn controller_platforms(binary: &Path) -> Result<IndexMap<String, Vec<Platform>>> {
    let output = Command::new(binary)
        .arg("--controllers")
        .output()
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "run".to_string(),
            file_path: binary.display().to_string(),
            reason: "could not run it".to_string(),
        })?;
    if !output.status.success() {
        return Err(AlienError::new(ErrorData::ExtensionBuildFailed {
            message: format!(
                "the controller's `--controllers` failed (exit code {}): {}",
                output.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&output.stderr)
            ),
        }));
    }
    serde_json::from_slice(&output.stdout)
        .into_alien_error()
        .context(ErrorData::ExtensionBuildFailed {
            message:
                "the controller's `--controllers` printed something other than its controllers"
                    .to_string(),
        })
}

/// A hooks module, built from the workspace's hooks crate and loaded in Alien's hooks runtime.
pub(crate) struct HooksModule {
    pub wasm: Vec<u8>,
    pub sha256: String,
    pub manifest: HooksManifest,
    pub module: HookModule,
}

/// Builds the hooks crate with the profile hooks modules ship with, and loads the module as Alien
/// loads a released one.
pub(crate) fn build_hooks_module(workspace: &ExtensionWorkspace) -> Result<HooksModule> {
    let wasm = alien_hooks_runtime::build_module(
        &workspace.root.join("Cargo.toml"),
        &workspace.hooks.name,
        &workspace.target_dir,
    )
    .map_err(|message| AlienError::new(ErrorData::ExtensionBuildFailed { message }))?;
    let sha256 = hex::encode(Sha256::digest(&wasm));
    let module = Runtime::new()
        .and_then(|runtime| runtime.load(&wasm, &sha256))
        .context(ErrorData::ExtensionBuildFailed {
            message: format!(
                "the module built from {} did not load in Alien's hooks runtime",
                workspace.hooks.name
            ),
        })?;
    Ok(HooksModule {
        wasm,
        sha256,
        manifest: module.manifest.clone(),
        module,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn metadata(table: Value) -> Value {
        json!({
            "workspace_root": "/ext",
            "target_directory": "/ext/target",
            "metadata": { "alien-extension": table },
            "workspace_members": ["hooks-id", "controller-id"],
            "packages": [
                {
                    "id": "hooks-id",
                    "name": "acme-hooks",
                    "version": "0.2.0",
                    "manifest_path": "/ext/hooks/Cargo.toml",
                    "targets": [{ "name": "acme_hooks", "kind": ["cdylib", "rlib"] }],
                    "source": null
                },
                {
                    "id": "controller-id",
                    "name": "acme-controller",
                    "version": "0.2.0",
                    "manifest_path": "/ext/controller/Cargo.toml",
                    "targets": [{ "name": "acme-controller", "kind": ["bin"] }],
                    "source": null
                },
                {
                    "id": "sdk-id",
                    "name": "alien-extension-sdk",
                    "version": "3.3.54",
                    "manifest_path": "/alien/crates/alien-extension-sdk/Cargo.toml",
                    "targets": [],
                    "source": null
                }
            ]
        })
    }

    #[test]
    fn reads_the_crates_the_table_names() {
        let workspace = workspace_from_metadata(&metadata(json!({
            "hooks": "hooks",
            "controller": "controller",
            "controller-url": "https://controllers.acme.dev"
        })))
        .unwrap();
        assert_eq!(workspace.hooks.name, "acme-hooks");
        assert_eq!(workspace.hooks.version, "0.2.0");
        let controller = workspace.controller.unwrap();
        assert_eq!(controller.bins, vec!["acme-controller".to_string()]);
        assert_eq!(
            workspace.controller_url.as_deref(),
            Some("https://controllers.acme.dev")
        );
        assert_eq!(
            workspace.sdk_dependency.as_deref(),
            Some(r#"{ path = "/alien/crates/alien-extension-sdk" }"#)
        );
    }

    #[test]
    fn refuses_a_workspace_without_the_table() {
        let mut metadata = metadata(json!({}));
        metadata["metadata"] = Value::Null;
        let error = workspace_from_metadata(&metadata).err().unwrap();
        assert!(
            error
                .to_string()
                .contains("[workspace.metadata.alien-extension]"),
            "{error}"
        );
    }

    #[test]
    fn refuses_a_table_naming_a_crate_the_workspace_lacks() {
        let error =
            workspace_from_metadata(&metadata(json!({ "hooks": "hooks", "types": "types" })))
                .err()
                .unwrap();
        assert!(error.to_string().contains("`types = \"types\"`"), "{error}");
    }

    #[test]
    fn refuses_an_unknown_key() {
        let error = workspace_from_metadata(&metadata(json!({ "hooks": "hooks", "hook": "x" })))
            .err()
            .unwrap();
        assert!(error.to_string().contains("takes `hooks`"), "{error}");
    }

    #[test]
    fn names_the_sdk_as_the_workspace_resolved_it() {
        let named = |source: &str| {
            let packages =
                json!([{ "name": "alien-extension-sdk", "version": "3.4.0", "source": source }]);
            sdk_dependency(packages.as_array().unwrap())
                .unwrap()
                .unwrap()
        };
        let commit = "0123456789abcdef0123456789abcdef01234567";
        let cases = [
            (CRATES_IO.to_string(), r#"version = "=3.4.0""#),
            (
                format!("git+https://github.com/alienplatform/alien?tag=v3.4.0#{commit}"),
                r#"git = "https://github.com/alienplatform/alien", tag = "v3.4.0""#,
            ),
            (
                format!("git+https://github.com/alienplatform/alien?branch=main#{commit}"),
                r#"git = "https://github.com/alienplatform/alien", branch = "main""#,
            ),
            (
                format!("git+https://github.com/alienplatform/alien?rev={commit}#{commit}"),
                r#"git = "https://github.com/alienplatform/alien", rev = "0123456789abcdef0123456789abcdef01234567""#,
            ),
            (
                format!("git+https://github.com/alienplatform/alien?rev=0123abc#{commit}"),
                r#"git = "https://github.com/alienplatform/alien", rev = "0123abc""#,
            ),
            (
                format!("git+https://github.com/alienplatform/alien#{commit}"),
                r#"git = "https://github.com/alienplatform/alien""#,
            ),
            (
                format!("git+file:///src/alien?branch=feature%2Fx#{commit}"),
                r#"git = "file:///src/alien", branch = "feature/x""#,
            ),
        ];
        for (source, expected) in cases {
            assert_eq!(named(&source), format!("{{ {expected} }}"), "{source}");
        }
        let other = json!([{ "name": "alien-extension-sdk", "version": "3.4.0", "source": "registry+https://example.com/index" }]);
        assert!(sdk_dependency(other.as_array().unwrap()).is_err());
    }

    #[test]
    fn a_controller_crate_needs_one_binary() {
        let controller = |bins: &[&str]| Member {
            name: "acme-controller".to_string(),
            version: "0.1.0".to_string(),
            dir: PathBuf::from("/ext/controller"),
            bins: bins.iter().map(|bin| bin.to_string()).collect(),
        };
        assert_eq!(controller_bin(&controller(&["acme"])).unwrap(), "acme");
        assert!(controller_bin(&controller(&[])).is_err());
        assert!(controller_bin(&controller(&["a", "b"])).is_err());
    }

    #[test]
    fn reads_the_docs_target_names() {
        assert_eq!(
            parse_target("linux-arm64").unwrap(),
            BinaryTarget::LinuxArm64
        );
        assert_eq!(
            parse_target("windows-x64").unwrap(),
            BinaryTarget::WindowsX64
        );
        assert!(parse_target("linux-aarch64").is_err());
    }
}
