//! `alien extensions build`: the bundle every loader runs, and the module alien.ts imports.
//!
//! The bundle in `target/alien-extension/` is a core zip (the manifest and the hooks module) and
//! one controller binary per built target beside it. The zip's digest is not inside the zip; the
//! generated module carries it, and the stack pins it.

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use alien_build::toolchain::rust::RustToolchain;
use alien_build::toolchain::{Toolchain, ToolchainContext, WorkloadKind};
use alien_core::{
    BinaryTarget, BuildHost, CargoBuildStrategy, ControllerBinary, ExtensionBundleManifest,
    ExtensionManifest, ExtensionProtocols, Platform, ResourceLifecycle, ResourceType,
    ResourceTypeDescriptor, EXTENSION_BUNDLE_MANIFEST, EXTENSION_HOOKS_MODULE,
};
use alien_error::{AlienError, Context, IntoAlienError};
use indexmap::IndexMap;
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;

use super::{
    build_hooks_module, build_host_controller, controller_bin, controller_platforms,
    read_workspace, typescript, ExtensionWorkspace, Member, BUNDLE_DIR, GENERATED_DIR,
};
use crate::error::{ErrorData, Result};
use crate::ui::{accent, dim_label, success_line};

/// The hooks module ABI the hooks runtime loads. A module that loaded speaks it.
pub(crate) const HOOKS_PROTOCOL: u32 = 1;

pub async fn build_task(directory: &Path, targets: &[BinaryTarget]) -> Result<()> {
    let workspace = read_workspace(directory)?;
    let bundle = build(&workspace, targets).await?;

    println!("{}", success_line("Extension built."));
    println!(
        "{} {}",
        dim_label("Extension"),
        accent(&bundle.manifest.name)
    );
    println!("{} {}", dim_label("Bundle   "), bundle.dir.display());
    for (target, binary) in &bundle.controllers {
        let built = if binary.is_some() {
            "built"
        } else {
            "not built"
        };
        println!("{} {target}: {built}", dim_label("Controller"));
    }
    println!("{} {}", dim_label("Module   "), bundle.index_ts.display());
    Ok(())
}

/// What a build wrote.
pub(crate) struct Bundle {
    dir: PathBuf,
    manifest: ExtensionManifest,
    controllers: IndexMap<BinaryTarget, Option<ControllerBinary>>,
    index_ts: PathBuf,
}

pub(crate) async fn build(
    workspace: &ExtensionWorkspace,
    targets: &[BinaryTarget],
) -> Result<Bundle> {
    let module = build_hooks_module(workspace)?;
    let name = module.manifest.extension.clone();
    check_name(&name)?;
    let mut resource_types = module.manifest.resource_types.clone();
    if let Some(controller) = &workspace.controller {
        let listed = controller_platforms(&build_host_controller(workspace, controller)?)?;
        add_controller_platforms(&mut resource_types, listed)?;
    }
    check_lifecycles(&resource_types)?;

    let mut controllers = IndexMap::new();
    let mut binaries = Vec::new();
    if let Some(controller) = &workspace.controller {
        let bin = controller_bin(controller)?;
        let targets = if targets.is_empty() {
            buildable_targets(&workspace.root)?
        } else {
            targets.to_vec()
        };
        for target in BinaryTarget::ALL {
            if !targets.contains(target) {
                controllers.insert(*target, None);
                continue;
            }
            let bytes = build_controller(workspace, controller, bin, *target).await?;
            controllers.insert(
                *target,
                Some(ControllerBinary {
                    sha256: hex::encode(Sha256::digest(&bytes)),
                    size: bytes.len() as u64,
                }),
            );
            binaries.push((format!("{name}-controller-{target}"), bytes));
        }
    }

    let bundle_manifest = ExtensionBundleManifest {
        name: name.clone(),
        version: workspace.hooks.version.clone(),
        protocol: ExtensionProtocols {
            hooks: HOOKS_PROTOCOL,
            controller: workspace
                .controller
                .as_ref()
                .map(|_| alien_infra::step_protocol::protocol::PROTOCOL_VERSION),
        },
        controller_url: workspace.controller_url.clone(),
        hooks_sha256: module.sha256.clone(),
        controllers: controllers.clone(),
        resource_types,
    };
    let core_zip = core_zip(&bundle_manifest, &module.wasm)?;

    let dir = workspace.root.join(BUNDLE_DIR);
    // A binary from an earlier build for a target this build skipped must not sit beside a
    // manifest that lists that target as missing.
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .into_alien_error()
            .context(file_error("remove", &dir))?;
    }
    std::fs::create_dir_all(&dir)
        .into_alien_error()
        .context(file_error("create", &dir))?;
    write(&dir.join(format!("{name}-core.zip")), &core_zip)?;
    for (file, bytes) in &binaries {
        let path = dir.join(file);
        write(&path, bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .into_alien_error()
                .context(file_error("make executable", &path))?;
        }
    }

    let manifest = ExtensionManifest {
        name,
        version: bundle_manifest.version,
        digest: format!("sha256:{}", hex::encode(Sha256::digest(&core_zip))),
        resource_types: bundle_manifest.resource_types,
    };
    let generated = workspace.root.join(GENERATED_DIR);
    std::fs::create_dir_all(&generated)
        .into_alien_error()
        .context(file_error("create", &generated))?;
    let index_ts = generated.join("index.ts");
    let source = typescript::index_ts(&manifest)
        .map_err(|message| AlienError::new(ErrorData::ExtensionBuildFailed { message }))?;
    write(&index_ts, source.as_bytes())?;

    Ok(Bundle {
        dir,
        manifest,
        controllers,
        index_ts,
    })
}

/// Refuses a name no stack could use, by the rule the SDK's `export!` applies too
/// (`alien_core::extension_name_problem`).
pub(crate) fn check_name(name: &str) -> Result<()> {
    match alien_core::extension_name_problem(name) {
        Some(problem) => Err(AlienError::new(ErrorData::ExtensionBuildFailed {
            message: format!("the extension name `{name}` in `export!` {problem}"),
        })),
        None => Ok(()),
    }
}

/// The targets this machine can build without installing anything: the Rust target is installed
/// for the workspace's toolchain, and so is the compiler the target needs from this host.
///
/// Fails when rustup can't list the installed targets: a bundle built then would have no
/// controller for any machine, though one may be buildable here.
fn buildable_targets(root: &Path) -> Result<Vec<BinaryTarget>> {
    let listed = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .current_dir(root)
        .output();
    let installed = match listed {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).into_owned()
        }
        Ok(output) => {
            return Err(cannot_list_targets(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ))
        }
        Err(error) => return Err(cannot_list_targets(error.to_string())),
    };
    let host = BuildHost::current();
    let runs = |program: &str, arg: &str| {
        Command::new(program)
            .arg(arg)
            .output()
            .is_ok_and(|output| output.status.success())
    };
    // The paths RustToolchain checks before it would install these subcommands.
    let cargo_bin = |name: &str| {
        dirs::home_dir().is_some_and(|home| home.join(".cargo/bin").join(name).exists())
    };
    Ok(BinaryTarget::ALL
        .iter()
        .copied()
        .filter(|target| {
            installed
                .lines()
                .any(|line| line.trim() == target.rust_target_triple())
        })
        .filter(
            |target| match (target, target.cargo_build_strategy_for(host)) {
                (BinaryTarget::DarwinArm64, _) => {
                    matches!(host, BuildHost::DarwinArm64 | BuildHost::DarwinX64)
                }
                (BinaryTarget::LinuxX64 | BinaryTarget::LinuxArm64, CargoBuildStrategy::Native) => {
                    let musl_gcc = match target {
                        BinaryTarget::LinuxX64 => "x86_64-linux-musl-gcc",
                        _ => "aarch64-linux-musl-gcc",
                    };
                    runs(musl_gcc, "--version") || runs("zig", "version")
                }
                (_, CargoBuildStrategy::Native) => true,
                (_, CargoBuildStrategy::Zigbuild) => {
                    runs("zig", "version") && cargo_bin("cargo-zigbuild")
                }
                (_, CargoBuildStrategy::Xwin) => cargo_bin("cargo-xwin"),
            },
        )
        .collect())
}

fn cannot_list_targets(reason: String) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ExtensionBuildFailed {
        message: format!(
            "cannot list the Rust targets installed here ({reason}); install rustup, or name the controller's targets with --target"
        ),
    })
}

async fn build_controller(
    workspace: &ExtensionWorkspace,
    controller: &Member,
    bin: &str,
    target: BinaryTarget,
) -> Result<Vec<u8>> {
    let context = ToolchainContext {
        src_dir: controller.dir.clone(),
        build_dir: workspace.target_dir.join("alien-extension-controller"),
        cache_store: None,
        cache_prefix: String::new(),
        build_target: target,
        runtime_platform_name: "extension".to_string(),
        debug_mode: false,
        pull_base_images: false,
        workload: WorkloadKind::Daemon,
    };
    RustToolchain {
        binary_name: bin.to_string(),
    }
    .build(&context)
    .await
    .context(ErrorData::ExtensionBuildFailed {
        message: format!("the controller {bin} did not build for {target}"),
    })?;
    let path = workspace
        .target_dir
        .join(target.rust_target_triple())
        .join("release")
        .join(format!("{bin}{}", target.binary_extension()));
    std::fs::read(&path)
        .into_alien_error()
        .context(file_error("read", &path))
}

/// Records which platforms each type has a controller on, and refuses a controller for a type
/// the hooks module doesn't declare: no stack could name it.
fn add_controller_platforms(
    resource_types: &mut IndexMap<ResourceType, ResourceTypeDescriptor>,
    listed: IndexMap<String, Vec<Platform>>,
) -> Result<()> {
    for (resource_type, platforms) in listed {
        let descriptor = resource_types
            .get_mut(&ResourceType::from(resource_type.as_str()))
            .ok_or_else(|| {
                AlienError::new(ErrorData::ExtensionBuildFailed {
                    message: format!(
                        "the controller registers `{resource_type}`, which the hooks crate doesn't declare with `ext.resource::<T>()`"
                    ),
                })
            })?;
        descriptor.controller_platforms = platforms;
    }
    Ok(())
}

/// Refuses a type that can be Live with no controller: Alien's controllers create a Live
/// resource, and there would be none for it.
fn check_lifecycles(resource_types: &IndexMap<ResourceType, ResourceTypeDescriptor>) -> Result<()> {
    for (resource_type, descriptor) in resource_types {
        if descriptor.lifecycles.contains(&ResourceLifecycle::Live)
            && descriptor.controller_platforms.is_empty()
        {
            return Err(AlienError::new(ErrorData::ExtensionBuildFailed {
                message: format!(
                    "`{resource_type}` can be Live, and no controller registers it; register one with `run_controllers`, or declare it `lifecycles = [Frozen]`"
                ),
            }));
        }
    }
    Ok(())
}

/// The core zip. Entries carry fixed timestamps and permissions, so the same inputs give the same
/// bytes and the same digest.
fn core_zip(manifest: &ExtensionBundleManifest, wasm: &[u8]) -> Result<Vec<u8>> {
    let manifest_json = serde_json::to_vec_pretty(manifest)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "serialize".to_string(),
            reason: "the bundle manifest did not serialize".to_string(),
        })?;
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644);
    let json = |value: &serde_json::Value| {
        serde_json::to_vec_pretty(value)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "serialize".to_string(),
                reason: "a bundle file did not serialize".to_string(),
            })
    };
    let mut entries = vec![
        (EXTENSION_BUNDLE_MANIFEST.to_string(), manifest_json),
        (EXTENSION_HOOKS_MODULE.to_string(), wasm.to_vec()),
    ];
    // Each permission set as a file, `permissions/<type>/<name>.json`, for a reviewer to read.
    for descriptor in manifest.resource_types.values() {
        for set in &descriptor.permission_sets {
            let path = set
                .id
                .strip_prefix(&format!("{}/", manifest.name))
                .unwrap_or(&set.id);
            let value =
                serde_json::to_value(set)
                    .into_alien_error()
                    .context(ErrorData::JsonError {
                        operation: "serialize".to_string(),
                        reason: format!("the permission set {} did not serialize", set.id),
                    })?;
            entries.push((format!("permissions/{path}.json"), json(&value)?));
        }
    }
    if !manifest.resource_types.is_empty() {
        let mut schemas = serde_json::Map::new();
        for (resource_type, descriptor) in &manifest.resource_types {
            for (schema, suffix) in [
                (&descriptor.schemas.config, ""),
                (&descriptor.schemas.outputs, "Outputs"),
            ] {
                let name = schema
                    .get("title")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("{resource_type}{suffix}"));
                schemas.insert(name, serde_json::Value::Object(schema.clone()));
            }
        }
        let openapi = serde_json::json!({
            "openapi": "3.1.0",
            "info": { "title": format!("The {} extension's resource types", manifest.name), "version": manifest.version },
            "components": { "schemas": schemas },
        });
        entries.push(("schemas/openapi.json".to_string(), json(&openapi)?));
    }
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (entry, bytes) in &entries {
        archive
            .start_file(entry.as_str(), options)
            .and_then(|()| archive.write_all(bytes).map_err(Into::into))
            .into_alien_error()
            .context(ErrorData::ExtensionBuildFailed {
                message: format!("could not add {entry} to the core zip"),
            })?;
    }
    let cursor = archive
        .finish()
        .into_alien_error()
        .context(ErrorData::ExtensionBuildFailed {
            message: "could not finish the core zip".to_string(),
        })?;
    Ok(cursor.into_inner())
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes)
        .into_alien_error()
        .context(file_error("write", path))
}

fn file_error(operation: &str, path: &Path) -> ErrorData {
    ErrorData::FileOperationFailed {
        operation: operation.to_string(),
        file_path: path.display().to_string(),
        reason: format!("could not {operation} it"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    /// With no rustup to ask, the build fails and says how to go on, rather than building a
    /// bundle with no controller for any machine.
    #[test]
    fn a_build_that_cannot_list_targets_says_so() {
        let empty = tempfile::tempdir().unwrap();
        // Each test runs in its own process under nextest.
        std::env::set_var("PATH", empty.path());
        let Err(error) = buildable_targets(empty.path()) else {
            panic!("no rustup, no targets")
        };
        assert!(error.message.contains("--target"), "{error}");
    }

    fn bundle_manifest(
        controllers: IndexMap<BinaryTarget, Option<ControllerBinary>>,
    ) -> ExtensionBundleManifest {
        ExtensionBundleManifest {
            name: "acme".to_string(),
            version: "0.1.0".to_string(),
            protocol: ExtensionProtocols {
                hooks: HOOKS_PROTOCOL,
                controller: Some(alien_infra::step_protocol::protocol::PROTOCOL_VERSION),
            },
            controller_url: None,
            hooks_sha256: "ab".repeat(32),
            controllers,
            resource_types: IndexMap::new(),
        }
    }

    #[test]
    fn the_core_zip_is_the_same_bytes_every_time() {
        let manifest = bundle_manifest(IndexMap::new());
        let first = core_zip(&manifest, b"\0asm module").unwrap();
        assert_eq!(first, core_zip(&manifest, b"\0asm module").unwrap());

        let mut archive = zip::ZipArchive::new(Cursor::new(first)).unwrap();
        let names: Vec<_> = archive.file_names().map(str::to_string).collect();
        assert_eq!(names, ["manifest.json", "hooks.wasm"]);
        let mut wasm = Vec::new();
        archive
            .by_name("hooks.wasm")
            .unwrap()
            .read_to_end(&mut wasm)
            .unwrap();
        assert_eq!(wasm, b"\0asm module");
    }

    #[test]
    fn the_manifest_lists_an_unbuilt_target_as_null() {
        let mut controllers = IndexMap::new();
        controllers.insert(BinaryTarget::WindowsX64, None);
        controllers.insert(
            BinaryTarget::LinuxX64,
            Some(ControllerBinary {
                sha256: "cd".repeat(32),
                size: 375136,
            }),
        );
        let json = serde_json::to_value(bundle_manifest(controllers)).unwrap();
        assert_eq!(json["controllers"]["windows-x64"], serde_json::Value::Null);
        assert_eq!(json["controllers"]["linux-x64"]["size"], 375136);
        assert_eq!(
            json["protocol"],
            serde_json::json!({ "hooks": 1, "controller": 1 })
        );
        assert_eq!(json["controllerUrl"], serde_json::Value::Null);
    }

    #[test]
    fn the_generated_module_exports_the_extension() {
        let manifest = ExtensionManifest {
            name: "backups".to_string(),
            version: "0.1.0".to_string(),
            digest: "sha256:4ff9".to_string(),
            resource_types: IndexMap::new(),
        };
        assert_eq!(
            typescript::index_ts(&manifest).unwrap(),
            r#"// Written by `alien extensions build`. The next build replaces it.
import type { ExtensionManifest } from "@alienplatform/core"

/** The backups extension. List it in `Stack.extensions()`. */
const extension: ExtensionManifest = {
  "name": "backups",
  "version": "0.1.0",
  "digest": "sha256:4ff9",
  "resourceTypes": {}
}

// The Alien CLI reads the extension's bundle from beside this module.
const modules = ((globalThis as Record<symbol, unknown>)[Symbol.for("alien.extensionModules")] ??= []) as { name: string; url: string }[]
modules.push({ name: extension.name, url: import.meta.url })

export default extension
"#
        );
    }

    fn descriptor(lifecycles: serde_json::Value) -> ResourceTypeDescriptor {
        serde_json::from_value(serde_json::json!({
            "lifecycles": lifecycles,
            "holdsData": false,
            "setup": { "cloudformation": [], "terraform": [], "helm": [], "importers": [] },
            "controllerPlatforms": [],
            "permissionSets": [],
            "link": { "permissionSets": [] },
            "schemas": { "config": {}, "outputs": {} }
        }))
        .unwrap()
    }

    #[test]
    fn records_the_platforms_the_controller_registers() {
        let mut types = IndexMap::new();
        types.insert(
            ResourceType::from("acme/dns-zone"),
            descriptor(serde_json::json!(["frozen", "live"])),
        );
        let mut listed = IndexMap::new();
        listed.insert(
            "acme/dns-zone".to_string(),
            vec![Platform::Aws, Platform::Local],
        );
        add_controller_platforms(&mut types, listed).unwrap();
        assert_eq!(
            types[&ResourceType::from("acme/dns-zone")].controller_platforms,
            vec![Platform::Aws, Platform::Local]
        );
        check_lifecycles(&types).unwrap();

        let mut unknown = IndexMap::new();
        unknown.insert("acme/other".to_string(), vec![Platform::Aws]);
        let error = add_controller_platforms(&mut types, unknown).unwrap_err();
        assert!(error.to_string().contains("`acme/other`"), "{error}");
    }

    #[test]
    fn a_live_type_needs_a_controller() {
        let mut types = IndexMap::new();
        types.insert(
            ResourceType::from("acme/dns-zone"),
            descriptor(serde_json::json!(["frozen", "live"])),
        );
        let error = check_lifecycles(&types).unwrap_err();
        assert!(error.to_string().contains("can be Live"), "{error}");
        types.insert(
            ResourceType::from("acme/dns-zone"),
            descriptor(serde_json::json!(["frozen"])),
        );
        check_lifecycles(&types).unwrap();
    }

    #[test]
    fn refuses_names_no_stack_could_use() {
        for name in ["", "acme/dns", "alien", "outputs", "my_ext", "Acme"] {
            assert!(check_name(name).is_err(), "{name:?}");
        }
        check_name("backups").unwrap();
        let error = check_name(&"a".repeat(25)).unwrap_err();
        assert!(
            error.to_string().contains("longer than 24 characters"),
            "{error}"
        );
    }
}
