//! `alien operations check` — validate a plugin's manifest offline, with no
//! platform account or network access.

use std::path::Path;

use alien_error::{Context, IntoAlienError};
use alien_operations_sdk::PluginManifest;

use crate::error::{ErrorData, Result};

/// Read and validate `metadata.json` in `directory` (or the current
/// directory). Prints a summary of the declared operations on success.
pub fn check_task(directory: Option<&str>, json: bool) -> Result<()> {
    let manifest = validate_manifest(directory)?;

    if json {
        crate::output::print_json(&manifest)?;
    } else {
        print_summary(directory, &manifest);
    }
    Ok(())
}

/// Read and validate `metadata.json` in `directory` without printing
/// anything, success or failure. For a command that runs `check` as an
/// internal precondition (`test`, `package`) rather than as its own
/// user-facing result — those commands own stdout for their own
/// `--json` contract, and a validation summary printed on the way there
/// would corrupt it into more than one JSON document.
pub fn validate_manifest(directory: Option<&str>) -> Result<PluginManifest> {
    let manifest_path = manifest_path(directory);
    let bytes = std::fs::read(&manifest_path)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("could not read '{}'", manifest_path.display()),
        })?;
    PluginManifest::parse_and_validate(&bytes).context(ErrorData::ConfigurationError {
        message: format!("'{}' is not a valid plugin manifest", manifest_path.display()),
    })
}

fn manifest_path(directory: Option<&str>) -> std::path::PathBuf {
    Path::new(directory.unwrap_or(".")).join(alien_operations_sdk::manifest::MANIFEST_FILENAME)
}

fn print_summary(directory: Option<&str>, manifest: &PluginManifest) {
    println!(
        "'{}' is valid: plugin '{}' v{} ({} tier)",
        manifest_path(directory).display(),
        manifest.name,
        manifest.version,
        manifest.tier.as_str()
    );
    for operation in &manifest.operations {
        let tier = operation.effective_tier(manifest.tier);
        let description = operation.description.as_deref().unwrap_or("(no description)");
        println!("  {} [{}] — {description}", operation.name, tier.as_str());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_manifest(directory: &Path, contents: &str) {
        std::fs::write(
            directory.join(alien_operations_sdk::manifest::MANIFEST_FILENAME),
            contents,
        )
        .expect("write test manifest");
    }

    #[test]
    fn accepts_a_valid_manifest() {
        let temp = tempfile::tempdir().expect("create temp dir");
        write_manifest(
            temp.path(),
            r#"{
                "name": "postgres",
                "version": "1.0.0",
                "tier": "read-only",
                "binaries": { "amd64": "postgres-linux-amd64" },
                "operations": [{ "name": "health" }]
            }"#,
        );

        check_task(Some(temp.path().to_str().expect("utf8 path")), false)
            .expect("valid manifest should pass check");
    }

    #[test]
    fn rejects_a_manifest_with_duplicate_operations() {
        let temp = tempfile::tempdir().expect("create temp dir");
        write_manifest(
            temp.path(),
            r#"{
                "name": "postgres",
                "version": "1.0.0",
                "tier": "read-only",
                "binaries": { "amd64": "postgres-linux-amd64" },
                "operations": [{ "name": "health" }, { "name": "health" }]
            }"#,
        );

        let err = check_task(Some(temp.path().to_str().expect("utf8 path")), false)
            .expect_err("duplicate operations must fail check");
        assert_eq!(err.code, "CONFIGURATION_ERROR");
    }

    #[test]
    fn rejects_a_missing_manifest() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let err = check_task(Some(temp.path().to_str().expect("utf8 path")), false)
            .expect_err("missing manifest must fail check");
        assert_eq!(err.code, "CONFIGURATION_ERROR");
    }
}
