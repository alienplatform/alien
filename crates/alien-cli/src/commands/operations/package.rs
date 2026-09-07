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
use zip::write::SimpleFileOptions;

use crate::commands::operations::check_task;
use crate::error::{ErrorData, Result};

pub fn package_task(directory: Option<&str>, json: bool) -> Result<()> {
    // Fail fast on a broken manifest before spending time on a release build.
    check_task(directory, false)?;

    let directory = Path::new(directory.unwrap_or("."));
    let manifest_path = directory.join(alien_operations_sdk::manifest::MANIFEST_FILENAME);
    let manifest_bytes = std::fs::read(&manifest_path)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("could not read '{}'", manifest_path.display()),
        })?;
    let manifest = PluginManifest::parse_and_validate(&manifest_bytes).context(
        ErrorData::ConfigurationError {
            message: format!("'{}' is not a valid plugin manifest", manifest_path.display()),
        },
    )?;

    let arch = Arch::host().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "this host's architecture is not amd64 or arm64; `alien operations package` \
                      can only build for the architecture it runs on"
                .to_string(),
        })
    })?;
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

    let binary_path = build_release_binary(directory, &manifest.name)?;
    write_bundle(&bundle_path, &manifest_bytes, binary_entry, &binary_path)?;

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

/// Run `cargo build --release` in `directory` and return the path to the
/// resulting binary named `crate_name`.
fn build_release_binary(directory: &Path, crate_name: &str) -> Result<PathBuf> {
    let status = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(directory)
        .status()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("could not run 'cargo build --release' in '{}'", directory.display()),
        })?;
    if !status.success() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "plugin build failed in '{}' (cargo build exit code {})",
                directory.display(),
                status.code().unwrap_or(-1)
            ),
        }));
    }

    let binary_name = if cfg!(windows) {
        format!("{crate_name}.exe")
    } else {
        crate_name.to_string()
    };
    let binary_path = directory.join("target/release").join(&binary_name);
    if !binary_path.is_file() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "expected a release binary at '{}' after a successful build, but it was not \
                 there — check that the [[bin]] name in Cargo.toml matches the plugin name",
                binary_path.display()
            ),
        }));
    }
    Ok(binary_path)
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
}
