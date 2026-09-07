//! `alien operations test` — run a plugin's own test suite. Fully offline:
//! this shells out to `cargo test` in the plugin's directory, the same way a
//! plugin author would run their own tests directly, but as one command
//! that also confirms the manifest is valid first (a broken manifest is a
//! more useful failure than a confusing test-time error).

use std::path::Path;
use std::process::Command;

use alien_error::{AlienError, Context, IntoAlienError};

use crate::commands::operations::check_task;
use crate::error::{ErrorData, Result};

pub fn test_task(directory: Option<&str>, json: bool) -> Result<()> {
    // Fail fast on a broken manifest rather than letting `cargo test` run
    // against a plugin that can't be loaded at all.
    check_task(directory, false)?;

    let directory = Path::new(directory.unwrap_or("."));
    let status = Command::new("cargo")
        .arg("test")
        .current_dir(directory)
        .status()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("could not run 'cargo test' in '{}'", directory.display()),
        })?;

    if !status.success() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "plugin tests failed in '{}' (cargo test exit code {})",
                directory.display(),
                status.code().unwrap_or(-1)
            ),
        }));
    }

    if json {
        crate::output::print_json(&serde_json::json!({ "passed": true }))?;
    } else {
        println!("All tests passed in '{}'.", directory.display());
    }
    Ok(())
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
    fn fails_fast_on_an_invalid_manifest_without_running_cargo() {
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

        let err = test_task(Some(temp.path().to_str().expect("utf8 path")), false)
            .expect_err("invalid manifest must fail before cargo runs");
        assert_eq!(err.code, "CONFIGURATION_ERROR");
        // No Cargo.toml exists in this temp dir, so if `cargo test` had run
        // it would fail with a different, cargo-specific error — the
        // manifest-duplicate message proves check ran first and short-circuited.
        assert!(err.to_string().contains("more than once"));
    }
}
