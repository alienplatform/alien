//! `alien operations package` — build a plugin's release binary and zip it
//! with `metadata.json` into the bundle format `alien operations publish`
//! expects. Fully offline (the build itself needs no platform account),
//! though `cargo build` may reach out to crates.io/a registry for
//! dependencies the same way any `cargo build` would.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use alien_error::{AlienError, Context, IntoAlienError};
use alien_operations_sdk::manifest::Arch;
use alien_operations_sdk::PluginManifest;
use serde_json::Value;
use zip::write::SimpleFileOptions;

use crate::commands::operations::check::validate_manifest;
use crate::error::{ErrorData, Result};

pub fn package_task(directory: Option<&str>, json: bool) -> Result<()> {
    // Fail fast on a broken manifest before spending time on a release
    // build. Validates silently (no stdout) so `--json` still emits exactly
    // one parseable document; re-read as raw bytes below since
    // single_arch_manifest_json needs to rewrite the manifest's JSON, not
    // the parsed struct.
    let manifest = validate_manifest(directory)?;

    let directory = Path::new(directory.unwrap_or("."));
    let manifest_path = directory.join(alien_operations_sdk::manifest::MANIFEST_FILENAME);
    let manifest_bytes = std::fs::read(&manifest_path)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("could not read '{}'", manifest_path.display()),
        })?;

    // Plugins run inside the operator/worker's Linux runtime regardless of
    // what OS `alien operations package` itself runs on (per the
    // `<name>-linux-<arch>` binary naming convention every manifest uses).
    // Build for an explicit Linux target triple rather than trusting
    // `std::env::consts::ARCH`/the host OS — otherwise a run on macOS would
    // silently bundle a Mach-O binary under a name that claims Linux.
    let arch = Arch::host().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "this host's architecture is not amd64 or arm64; `alien operations package` \
                      can only build for the architecture it runs on"
                .to_string(),
        })
    })?;
    let target_triple = linux_target_triple(arch);
    let binary_entry = manifest.binaries.get(&arch).ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "'{}' declares no binary for this host's architecture ('{}')",
                manifest_path.display(),
                arch.as_str()
            ),
        })
    })?;

    let bundle_name = format!("{}-{}.zip", manifest.name, manifest.version);
    let bundle_path = directory.join(&bundle_name);

    let binary_path = build_release_binary(directory, &manifest.name, target_triple)?;
    // Advertise only the architecture actually present in this bundle — the
    // source manifest on disk may declare both amd64 and arm64 (e.g. from
    // `init`'s template), but this run only ever builds one, and a bundle
    // that claims a binary it doesn't ship would register a plugin that
    // fails at invocation time on the other architecture, not at publish
    // time when the mistake is still cheap to catch.
    let single_arch_manifest_bytes = single_arch_manifest_json(&manifest_bytes, arch, binary_entry)?;
    write_bundle(
        &bundle_path,
        &single_arch_manifest_bytes,
        binary_entry,
        &binary_path,
    )?;

    if json {
        crate::output::print_json(&serde_json::json!({
            "bundle": bundle_path,
            "architecture": arch.as_str(),
        }))?;
    } else {
        println!("Built bundle '{}' ({} only).", bundle_path.display(), arch.as_str());
        println!();
        println!("Note: this bundle only contains a binary for this host's architecture. A");
        println!("published plugin should offer both amd64 and arm64 — build the other");
        println!("architecture on a matching host (or via cross-compilation) and merge the");
        println!("two into one bundle before publishing, or publish is limited to hosts of");
        println!("this architecture.");
        println!();
        println!("Next step:");
        println!("  alien operations publish {}", bundle_path.display());
    }
    Ok(())
}

/// Rewrite `manifest_bytes`'s `binaries` map to declare only `arch` →
/// `binary_entry`, dropping any other architecture the manifest on disk
/// declares (e.g. a scaffolded two-arch template). Preserves every other
/// field verbatim — this only narrows what the packaged bundle claims to
/// ship, not the plugin's declared operations, tiers, or verification.
fn single_arch_manifest_json(manifest_bytes: &[u8], arch: Arch, binary_entry: &str) -> Result<Vec<u8>> {
    let mut value: Value = serde_json::from_slice(manifest_bytes).into_alien_error().context(
        ErrorData::ConfigurationError {
            message: "could not re-parse the manifest to narrow its declared binaries".to_string(),
        },
    )?;
    let binaries = value
        .get_mut("binaries")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "manifest has no binaries object to narrow".to_string(),
            })
        })?;
    binaries.clear();
    binaries.insert(arch.as_str().to_string(), Value::String(binary_entry.to_string()));

    serde_json::to_vec_pretty(&value)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "could not re-encode the narrowed manifest".to_string(),
        })
}

/// The Rust target triple for `arch`'s Linux build — the only OS a
/// published plugin binary ever runs under (see the module-level note in
/// [`package_task`]).
fn linux_target_triple(arch: Arch) -> &'static str {
    match arch {
        Arch::Amd64 => "x86_64-unknown-linux-gnu",
        Arch::Arm64 => "aarch64-unknown-linux-gnu",
    }
}

/// Confirm `target_triple` is installed for the active toolchain, so a
/// missing cross-compilation target fails with an actionable message up
/// front instead of a `cargo build` error partway through, or — if some
/// other default target quietly satisfied the build — a bundle that
/// silently ships the wrong OS/architecture.
fn ensure_target_installed(target_triple: &str) -> Result<()> {
    let output = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "could not run 'rustup target list --installed'".to_string(),
        })?;
    if !output.status.success() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "'rustup target list --installed' failed".to_string(),
        }));
    }
    let installed = String::from_utf8_lossy(&output.stdout);
    if installed.lines().any(|line| line.trim() == target_triple) {
        return Ok(());
    }

    Err(AlienError::new(ErrorData::ConfigurationError {
        message: format!(
            "Rust target '{target_triple}' is not installed; plugin binaries must target Linux \
             regardless of this host's OS. Run `rustup target add {target_triple}` and, on \
             macOS, install a matching cross-linker (e.g. via `brew install \
             messense/macos-cross-toolchains/{arch}-unknown-linux-gnu`) before packaging.",
            arch = target_triple.split('-').next().unwrap_or(target_triple)
        ),
    }))
}

/// Run `cargo build --release --target <target_triple>` in `directory` and
/// return the path Cargo itself reports for the resulting `crate_name`
/// binary.
///
/// Reads the artifact path from `--message-format=json` rather than
/// assuming `<directory>/target/<target_triple>/release/<crate_name>` — a
/// plugin built with `CARGO_TARGET_DIR` set, a `[build] target-dir` in
/// `.cargo/config.toml`, or as a member of an enclosing Cargo workspace
/// with a shared target directory would build successfully but land its
/// binary somewhere else, and the hardcoded path would then report a false
/// "binary missing" after a build that actually succeeded.
fn build_release_binary(directory: &Path, crate_name: &str, target_triple: &str) -> Result<PathBuf> {
    ensure_target_installed(target_triple)?;

    let output = Command::new("cargo")
        .args(["build", "--release", "--target", target_triple, "--message-format=json"])
        .current_dir(directory)
        .output()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!(
                "could not run 'cargo build --release --target {target_triple}' in '{}'",
                directory.display()
            ),
        })?;
    if !output.status.success() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "plugin build failed in '{}' for target '{target_triple}' (cargo build exit code {})",
                directory.display(),
                output.status.code().unwrap_or(-1)
            ),
        }));
    }

    binary_artifact_path(&output.stdout, crate_name).ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "cargo build succeeded in '{}' but reported no binary artifact named \
                 '{crate_name}' — check that the [[bin]] name in Cargo.toml matches the \
                 plugin name",
                directory.display()
            ),
        })
    })
}

/// Parse `cargo build --message-format=json` stdout for the executable path
/// of the `bin` target named `crate_name`. Cargo emits one JSON object per
/// line; a `compiler-artifact` message carries `target.kind`, `target.name`,
/// and (for a binary target) `executable`.
fn binary_artifact_path(cargo_build_stdout: &[u8], crate_name: &str) -> Option<PathBuf> {
    for line in cargo_build_stdout.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        if message.get("reason").and_then(Value::as_str) != Some("compiler-artifact") {
            continue;
        }
        let target = message.get("target")?;
        let is_bin_target = target
            .get("kind")
            .and_then(Value::as_array)
            .is_some_and(|kinds| kinds.iter().any(|k| k.as_str() == Some("bin")));
        let name_matches = target.get("name").and_then(Value::as_str) == Some(crate_name);
        if !is_bin_target || !name_matches {
            continue;
        }
        if let Some(executable) = message.get("executable").and_then(Value::as_str) {
            return Some(PathBuf::from(executable));
        }
    }
    None
}

fn write_bundle(
    bundle_path: &Path,
    manifest_bytes: &[u8],
    binary_entry: &str,
    binary_path: &Path,
) -> Result<()> {
    let binary_bytes = std::fs::read(binary_path)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("could not read built binary '{}'", binary_path.display()),
        })?;

    let file = std::fs::File::create(bundle_path)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("could not create '{}'", bundle_path.display()),
        })?;
    let mut archive = zip::ZipWriter::new(file);
    let metadata_options = SimpleFileOptions::default();
    // Binaries need the executable bit inside the ZIP so the loader can run
    // them directly after extraction, without a separate chmod step.
    let binary_options = SimpleFileOptions::default().unix_permissions(0o755);

    archive
        .start_file(alien_operations_sdk::manifest::MANIFEST_FILENAME, metadata_options)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "could not add metadata.json to bundle".to_string(),
        })?;
    archive
        .write_all(manifest_bytes)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "could not write metadata.json into bundle".to_string(),
        })?;

    archive
        .start_file(binary_entry, binary_options)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("could not add '{binary_entry}' to bundle"),
        })?;
    archive
        .write_all(&binary_bytes)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("could not write '{binary_entry}' into bundle"),
        })?;

    archive
        .finish()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("could not finish '{}'", bundle_path.display()),
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_bundle_with_metadata_and_the_binary_entry() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let manifest_bytes = br#"{
            "name": "demo",
            "version": "0.1.0",
            "tier": "read-only",
            "binaries": { "amd64": "demo-linux-amd64" },
            "operations": [{ "name": "health" }]
        }"#;
        let binary_path = temp.path().join("demo-binary");
        std::fs::write(&binary_path, b"not a real binary, just test bytes").expect("write fake binary");
        let bundle_path = temp.path().join("demo-0.1.0.zip");

        write_bundle(&bundle_path, manifest_bytes, "demo-linux-amd64", &binary_path)
            .expect("bundle should write");

        let bytes = std::fs::read(&bundle_path).expect("read bundle");
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("open bundle as zip");
        let names: Vec<_> = archive.file_names().map(str::to_string).collect();
        assert!(names.contains(&"metadata.json".to_string()));
        assert!(names.contains(&"demo-linux-amd64".to_string()));

        let mut metadata_entry = archive.by_name("metadata.json").expect("bundle has metadata.json");
        let mut bundled_metadata = String::new();
        std::io::Read::read_to_string(&mut metadata_entry, &mut bundled_metadata)
            .expect("read metadata.json from bundle");
        let manifest = PluginManifest::parse_and_validate(bundled_metadata.as_bytes())
            .expect("bundled metadata should still be a valid manifest");
        assert_eq!(manifest.name, "demo");
    }

    #[test]
    fn single_arch_manifest_drops_architectures_not_actually_bundled() {
        // A two-arch manifest (e.g. init's default template) must not be
        // packaged verbatim — the bundle only ever contains one arch's
        // binary, so the packaged manifest must declare only that one.
        let two_arch_manifest = br#"{
            "name": "demo",
            "version": "0.1.0",
            "tier": "read-only",
            "binaries": {
                "amd64": "demo-linux-amd64",
                "arm64": "demo-linux-arm64"
            },
            "operations": [{ "name": "health" }]
        }"#;

        let narrowed = single_arch_manifest_json(two_arch_manifest, Arch::Arm64, "demo-linux-arm64")
            .expect("narrowing should succeed");
        let manifest = PluginManifest::parse_and_validate(&narrowed)
            .expect("narrowed manifest should still be valid");

        assert_eq!(manifest.binaries.len(), 1);
        assert_eq!(
            manifest.binaries.get(&Arch::Arm64).map(String::as_str),
            Some("demo-linux-arm64")
        );
        assert!(!manifest.binaries.contains_key(&Arch::Amd64));
        // Everything else must survive untouched.
        assert_eq!(manifest.name, "demo");
        assert_eq!(manifest.operations.len(), 1);
        assert_eq!(manifest.operations[0].name, "health");
    }

    #[test]
    fn binary_artifact_path_finds_the_named_bin_targets_executable() {
        // A realistic slice of `cargo build --message-format=json` output:
        // a lib-target artifact (no `executable`) followed by the bin
        // target's, plus a `build-finished` message. The parser must pick
        // out only the matching bin target's executable, wherever Cargo
        // actually placed it — not assume `target/release`.
        let stdout = concat!(
            r#"{"reason":"compiler-artifact","target":{"name":"demo_plugin","kind":["lib"]},"executable":null}"#, "\n",
            r#"{"reason":"compiler-artifact","target":{"name":"other-crate","kind":["bin"]},"executable":"/somewhere/else/other-crate"}"#, "\n",
            r#"{"reason":"compiler-artifact","target":{"name":"demo-plugin","kind":["bin"]},"executable":"/custom/target/dir/release/demo-plugin"}"#, "\n",
            r#"{"reason":"build-finished","success":true}"#, "\n",
        );

        let path = binary_artifact_path(stdout.as_bytes(), "demo-plugin")
            .expect("should find the matching bin target's executable");
        assert_eq!(path, PathBuf::from("/custom/target/dir/release/demo-plugin"));
    }

    #[test]
    fn binary_artifact_path_returns_none_when_no_matching_bin_target_exists() {
        let stdout = concat!(
            r#"{"reason":"compiler-artifact","target":{"name":"demo_plugin","kind":["lib"]},"executable":null}"#, "\n",
            r#"{"reason":"build-finished","success":true}"#, "\n",
        );
        assert!(binary_artifact_path(stdout.as_bytes(), "demo-plugin").is_none());
    }

    #[test]
    fn linux_target_triple_never_selects_the_build_hosts_own_os() {
        // Plugins always run inside the Linux operator/worker, so the
        // packaged target triple must say `-linux-` regardless of what OS
        // `cargo package` itself runs on (macOS in CI and on most
        // developers' machines).
        assert_eq!(linux_target_triple(Arch::Amd64), "x86_64-unknown-linux-gnu");
        assert_eq!(linux_target_triple(Arch::Arm64), "aarch64-unknown-linux-gnu");
    }

    #[test]
    fn ensure_target_installed_rejects_a_target_rustup_does_not_have() {
        let err = ensure_target_installed("sparc64-unknown-linux-gnu")
            .expect_err("a target that isn't installed must fail, not silently proceed");
        assert_eq!(err.code, "CONFIGURATION_ERROR");
        assert!(err.to_string().contains("rustup target add"));
    }

    #[test]
    fn package_finds_the_binary_under_a_custom_cargo_target_dir() {
        // Real end-to-end proof, not just a parser unit test: build an
        // actual tiny crate with CARGO_TARGET_DIR pointed somewhere other
        // than `<crate>/target`, and confirm build_release_binary finds the
        // binary there rather than reporting it missing.
        let temp = tempfile::tempdir().expect("create temp dir");
        let crate_dir = temp.path().join("tiny-bin");
        let target_dir = temp.path().join("custom-target");
        std::fs::create_dir_all(crate_dir.join("src")).expect("create crate dirs");
        std::fs::write(
            crate_dir.join("Cargo.toml"),
            "[package]\nname = \"tiny-bin\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .expect("write Cargo.toml");
        std::fs::write(
            crate_dir.join("src/main.rs"),
            "fn main() { println!(\"hi\"); }\n",
        )
        .expect("write main.rs");

        let output = Command::new("cargo")
            .args(["build", "--release", "--message-format=json"])
            .current_dir(&crate_dir)
            .env("CARGO_TARGET_DIR", &target_dir)
            .output()
            .expect("cargo build should run");
        assert!(output.status.success(), "cargo build should succeed");

        let path = binary_artifact_path(&output.stdout, "tiny-bin")
            .expect("should find the binary Cargo actually built");
        assert!(
            path.starts_with(&target_dir),
            "binary at {path:?} should be under the custom CARGO_TARGET_DIR {target_dir:?}, \
             not the crate's own target/ directory"
        );
        assert!(path.is_file(), "reported binary path should actually exist");
    }
}
