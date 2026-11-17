//! `alien extensions`: build an extension's bundle, and test it.
//!
//! An extension is a Cargo workspace whose `[workspace.metadata.alien-extension]` table names its
//! hooks crate, and its controller and types crates when it has them. Both commands run locally,
//! with no manager or platform account.
//!
//! A controller crate's library builds into the controller module, which runs the controllers of
//! cloud platforms inside Alien. Its binary, when it has one, runs the controllers of
//! `Platform::Local` as a process under `alien dev`.

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
use alien_hooks_runtime::{Capabilities, ControllerModule, HookModule, Runtime, StepLimits};
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

    # Build the controller binary for chosen targets
    alien extensions build --target linux-x64,linux-arm64
"
)]
pub struct ExtensionsArgs {
    #[command(subcommand)]
    pub action: ExtensionsAction,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ExtensionsAction {
    /// Build the hooks module, the controller module and the controller binary into a bundle in
    /// target/alien-extension/, and write generated/index.ts, the module alien.ts imports.
    Build {
        /// Targets of the controller binary, comma-separated: linux-x64, linux-arm64,
        /// darwin-arm64, windows-x64. Defaults to every target this machine can build without
        /// installing anything.
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
    /// The origins (`https://host[:port]`) the controller module may call besides its cloud.
    pub egress: Vec<String>,
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
    /// Its library target's name, when it has one.
    pub library: Option<String>,
}

/// `[workspace.metadata.alien-extension]`: each value is a crate's directory, relative to the
/// workspace root.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtensionTable {
    hooks: String,
    controller: Option<String>,
    types: Option<String>,
    #[serde(default)]
    egress: Vec<String>,
}

/// Whether `directory` holds an extension's Cargo workspace: its Cargo.toml has the
/// `[workspace.metadata.alien-extension]` table. A directory with no Cargo.toml holds none; a
/// Cargo.toml that can't be read or parsed fails, naming it, rather than hide an extension.
pub(crate) fn is_extension_workspace(directory: &Path) -> Result<bool> {
    let path = directory.join("Cargo.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(false)
        }
        Err(error) => {
            return Err(error)
                .into_alien_error()
                .context(ErrorData::FileOperationFailed {
                    operation: "read".to_string(),
                    file_path: path.display().to_string(),
                    reason: "could not read it to see whether it is an extension workspace"
                        .to_string(),
                })
        }
    };
    let manifest =
        text.parse::<toml::Table>()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: format!(
                    "{} is not valid TOML, so whether it is an extension workspace is unknown",
                    path.display()
                ),
            })?;
    Ok(manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("metadata"))
        .and_then(|metadata| metadata.get("alien-extension"))
        .is_some())
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
            message: "[workspace.metadata.alien-extension] takes `hooks`, and optionally `controller`, `types` and `egress`".to_string(),
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
                library: package["targets"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|target| {
                        target["kind"].as_array().is_some_and(|kinds| {
                            kinds
                                .iter()
                                .any(|kind| kind == "lib" || kind == "rlib" || kind == "cdylib")
                        })
                    })
                    .and_then(|target| target["name"].as_str().map(str::to_string)),
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
        egress: origins(table.egress)?,
        sdk_dependency: sdk_dependency(&packages)?,
    })
}

/// `declared`, each checked to be an origin as the transport compares them: `https`, a host, an
/// optional port, and nothing after it.
fn origins(declared: Vec<String>) -> Result<Vec<String>> {
    for origin in &declared {
        let canonical = url::Url::parse(origin)
            .ok()
            .filter(|url| url.scheme() == "https")
            .map(|url| url.origin().ascii_serialization());
        if canonical.as_deref() != Some(origin.as_str()) {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: format!(
                    "[workspace.metadata.alien-extension] `egress` lists '{origin}', which is not an origin like 'https://api.example.com' or 'https://api.example.com:8443'"
                ),
            }));
        }
    }
    Ok(declared)
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

/// The controller crate's binary target, which runs its local controllers, or `None` for a
/// crate whose library is all it has.
pub(crate) fn controller_bin(controller: &Member) -> Result<Option<&str>> {
    match (controller.bins.as_slice(), &controller.library) {
        ([], Some(_)) => Ok(None),
        ([bin], _) => Ok(Some(bin)),
        ([], None) => Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "the controller crate {} has no library and no binary. Its library (src/lib.rs) registers the controllers of cloud platforms with `export_controllers!`, and a binary (src/main.rs) runs those of `Platform::Local` with `run_controllers`",
                controller.name
            ),
        })),
        (bins, _) => Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "the controller crate {} may have one binary target, and it has {}",
                controller.name,
                bins.len()
            ),
        })),
    }
}

/// Builds the controller binary for this machine, as `cargo build` does, and returns it: what
/// `--controllers` runs.
pub(crate) fn build_host_controller(
    workspace: &ExtensionWorkspace,
    controller: &Member,
    bin: &str,
) -> Result<PathBuf> {
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

/// The resource types and platforms a controller binary's `run_controllers` registers, from
/// `--controllers`. A binary runs the controllers of `Platform::Local` alone: Alien runs a cloud
/// platform's controller in the controller module.
pub(crate) fn local_controller_platforms(binary: &Path) -> Result<IndexMap<String, Vec<Platform>>> {
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
    let listed: IndexMap<String, Vec<Platform>> = serde_json::from_slice(&output.stdout)
        .into_alien_error()
        .context(ErrorData::ExtensionBuildFailed {
            message:
                "the controller's `--controllers` printed something other than its controllers"
                    .to_string(),
        })?;
    for (resource_type, platforms) in &listed {
        if let Some(cloud) = platforms
            .iter()
            .find(|platform| **platform != Platform::Local)
        {
            return Err(AlienError::new(ErrorData::ExtensionBuildFailed {
                message: format!(
                    "the controller binary registers `{resource_type}` on {cloud}. A binary runs the controllers of `Platform::Local`, for `alien dev`; register a cloud platform's controller in the crate's library, in the function `export_controllers!` names"
                ),
            }));
        }
    }
    Ok(listed)
}

/// A controller module, built from the controller crate's library and loaded in Alien's
/// runtime, with the controllers it lists.
pub(crate) struct BuiltControllerModule {
    pub wasm: Vec<u8>,
    pub sha256: String,
    pub module: ControllerModule,
    /// The platforms of each type's controllers, as the module's `controllers` op lists them.
    pub controllers: IndexMap<String, Vec<Platform>>,
}

/// Builds the controller crate's library into a controller module, loads it as Alien loads a
/// released one, and asks it for its controllers.
pub(crate) fn build_controller_module(
    workspace: &ExtensionWorkspace,
    controller: &Member,
    library: &str,
) -> Result<BuiltControllerModule> {
    let wasm = alien_hooks_runtime::build_controller_module(
        &workspace.root.join("Cargo.toml"),
        &controller.name,
        library,
        &workspace.target_dir,
    )
    .context(ErrorData::ExtensionBuildFailed {
        message: format!("{}'s controller module did not build", controller.name),
    })?;
    let sha256 = hex::encode(Sha256::digest(&wasm));
    let module = load_controller_module(&wasm, &sha256).context(ErrorData::ExtensionBuildFailed {
        message: format!(
            "the controller module built from {}'s library did not load in Alien's runtime. The library registers its controllers in a function it names with `alien_extension_sdk::export_controllers!`",
            controller.name
        ),
    })?;
    let controllers = module_controllers(&module)
        .map_err(|message| AlienError::new(ErrorData::ExtensionBuildFailed { message }))?;
    Ok(BuiltControllerModule {
        wasm,
        sha256,
        module,
        controllers,
    })
}

/// Loads a controller module as a loader does: checks its digest, its ABI and its imports.
pub(crate) fn load_controller_module(
    wasm: &[u8],
    sha256: &str,
) -> alien_hooks_runtime::Result<ControllerModule> {
    Runtime::new().and_then(|runtime| runtime.load_controller(wasm, sha256))
}

/// How long a module may take to list its controllers, which makes no call.
const LIST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// The reply to the `controllers` op of the step protocol.
#[derive(Deserialize)]
struct ControllersReply {
    controllers: IndexMap<String, Vec<Platform>>,
}

/// The resource types and platforms the module's controllers are registered for, from its
/// `controllers` op. A module runs the controllers of cloud platforms: `alien dev` runs a
/// `Platform::Local` controller as a process, from the crate's binary.
pub(crate) fn module_controllers(
    module: &ControllerModule,
) -> std::result::Result<IndexMap<String, Vec<Platform>>, String> {
    /// Listing controllers makes no call.
    struct NoCalls;
    impl Capabilities for NoCalls {
        fn cloud(&mut self, _: &[u8]) -> std::result::Result<Vec<u8>, String> {
            Err("refused: a module lists its controllers without a call".to_string())
        }
        fn http(&mut self, _: &[u8]) -> std::result::Result<Vec<u8>, String> {
            Err("refused: a module lists its controllers without a call".to_string())
        }
    }
    let request = serde_json::json!({
        "protocol": alien_infra::step_protocol::protocol::PROTOCOL_VERSION,
        "op": "controllers",
    });
    let reply = module
        .step(
            request.to_string().as_bytes(),
            &StepLimits::within(LIST_TIMEOUT),
            Box::new(NoCalls),
        )
        .map_err(|failure| {
            format!("the controller module could not list its controllers: {failure:?}")
        })?;
    let listed = serde_json::from_slice::<ControllersReply>(&reply)
        .map_err(|error| {
            format!("the controller module answered `controllers` with something else: {error}")
        })?
        .controllers;
    for (resource_type, platforms) in &listed {
        if platforms.contains(&Platform::Local) {
            return Err(format!(
                "the controller module registers `{resource_type}` on local. `alien dev` runs a local controller as a process: register it with `run_controllers` in the crate's binary (src/main.rs)"
            ));
        }
    }
    Ok(listed)
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
    let library = workspace.hooks.library.as_deref().ok_or_else(|| {
        AlienError::new(ErrorData::ExtensionBuildFailed {
            message: format!(
                "the hooks crate {} has no library; its library (src/lib.rs) builds into the hooks module",
                workspace.hooks.name
            ),
        })
    })?;
    let wasm = alien_hooks_runtime::build_module(
        &workspace.root.join("Cargo.toml"),
        &workspace.hooks.name,
        library,
        &workspace.target_dir,
    )
    .context(ErrorData::ExtensionBuildFailed {
        message: format!("{}'s hooks module did not build", workspace.hooks.name),
    })?;
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
            "controller": "controller"
        })))
        .unwrap();
        assert_eq!(workspace.hooks.name, "acme-hooks");
        assert_eq!(workspace.hooks.version, "0.2.0");
        let controller = workspace.controller.unwrap();
        assert_eq!(controller.bins, vec!["acme-controller".to_string()]);
        assert_eq!(controller.library, None);
        assert_eq!(
            workspace.sdk_dependency.as_deref(),
            Some(r#"{ path = "/alien/crates/alien-extension-sdk" }"#)
        );
    }

    #[test]
    fn egress_takes_origins_only() {
        let declared = json!(["https://api.acme.dev", "https://hooks.acme.dev:8443"]);
        let workspace =
            workspace_from_metadata(&metadata(json!({ "hooks": "hooks", "egress": declared })))
                .unwrap();
        assert_eq!(serde_json::to_value(&workspace.egress).unwrap(), declared);
        for wrong in [
            "http://api.acme.dev",
            "https://api.acme.dev/v1",
            "https://api.acme.dev/",
            "https://API.acme.dev",
            "https://user@api.acme.dev",
            "api.acme.dev",
        ] {
            let error =
                workspace_from_metadata(&metadata(json!({ "hooks": "hooks", "egress": [wrong] })))
                    .err()
                    .unwrap_or_else(|| panic!("{wrong} is accepted"));
            assert!(error.to_string().contains(wrong), "{error}");
        }
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

    /// The library is the controller module and the binary the local controller. A crate has
    /// either or both, and no second binary.
    #[test]
    fn a_controller_crate_has_a_library_a_binary_or_both() {
        let controller = |bins: &[&str], library: Option<&str>| Member {
            name: "acme-controller".to_string(),
            version: "0.1.0".to_string(),
            dir: PathBuf::from("/ext/controller"),
            bins: bins.iter().map(|bin| bin.to_string()).collect(),
            library: library.map(str::to_string),
        };
        let lib = Some("acme_controller");
        assert_eq!(
            controller_bin(&controller(&["acme"], None)).unwrap(),
            Some("acme")
        );
        assert_eq!(
            controller_bin(&controller(&["acme"], lib)).unwrap(),
            Some("acme")
        );
        assert_eq!(controller_bin(&controller(&[], lib)).unwrap(), None);
        let neither = controller_bin(&controller(&[], None)).unwrap_err();
        assert!(
            neither.to_string().contains("no library and no binary"),
            "{neither}"
        );
        assert!(controller_bin(&controller(&["a", "b"], lib)).is_err());
    }

    /// A workspace that still names the URL of a controller server is told the key is unknown.
    #[test]
    fn refuses_a_controller_url() {
        let error = workspace_from_metadata(&metadata(json!({
            "hooks": "hooks",
            "controller-url": "https://controllers.acme.dev"
        })))
        .err()
        .unwrap();
        assert!(error.to_string().contains("takes `hooks`"), "{error}");
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

    /// A directory with no Cargo.toml is no extension workspace, and a Cargo.toml that doesn't
    /// parse is an error naming it, not a "no".
    #[test]
    fn an_unparsable_cargo_toml_is_an_error_not_a_plain_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_extension_workspace(dir.path()).unwrap());
        assert!(!is_extension_workspace(&dir.path().join("missing")).unwrap());

        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\n[workspace.metadata.alien-extension]\nhooks = \"hooks\"\n",
        )
        .unwrap();
        assert!(is_extension_workspace(dir.path()).unwrap());

        std::fs::write(dir.path().join("Cargo.toml"), "[workspace\n").unwrap();
        let error = is_extension_workspace(dir.path()).unwrap_err();
        assert!(error.message.contains("Cargo.toml"), "{}", error.message);
    }
}
