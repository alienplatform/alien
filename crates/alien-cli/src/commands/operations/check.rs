//! `alien operations check` — validate a plugin's manifest offline, with no
//! platform account or network access.

use std::path::Path;

use alien_error::{Context, IntoAlienError};
use alien_operations_sdk::CanonicalPluginManifest;
use serde_json::Value;

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
pub fn validate_manifest(directory: Option<&str>) -> Result<CanonicalPluginManifest> {
    let manifest_path = manifest_path(directory);
    let bytes = std::fs::read(&manifest_path).into_alien_error().context(
        ErrorData::ConfigurationError {
            message: format!("could not read '{}'", manifest_path.display()),
        },
    )?;
    parse_manifest_for_cli(&bytes).context(ErrorData::ConfigurationError {
        message: format!(
            "'{}' is not a valid plugin manifest",
            manifest_path.display()
        ),
    })
}

/// Preserve released manifest behavior at CLI ingestion while making the
/// canonical contract strict. A canonical-only marker opts the entire
/// manifest into canonical validation; we never fall back after that fails.
pub(super) fn parse_manifest_for_cli(
    bytes: &[u8],
) -> alien_operations_sdk::Result<CanonicalPluginManifest> {
    let value: Value = match serde_json::from_slice(bytes) {
        Ok(value) => value,
        // Reuse the SDK's typed parse error for malformed JSON.
        Err(_) => return CanonicalPluginManifest::parse(bytes),
    };
    let uses_canonical_contract = value
        .get("operations")
        .and_then(Value::as_array)
        .is_some_and(|operations| operations.iter().any(operation_uses_canonical_contract));

    if uses_canonical_contract {
        CanonicalPluginManifest::parse_and_validate(bytes)
    } else {
        #[allow(deprecated)]
        alien_operations_sdk::PluginManifest::parse_and_validate(bytes)
            .map(alien_operations_sdk::PluginManifest::into_canonical)
    }
}

fn operation_uses_canonical_contract(operation: &Value) -> bool {
    let Some(operation) = operation.as_object() else {
        return false;
    };
    operation.contains_key("inputSchema")
        || operation.contains_key("outputSchema")
        || operation.contains_key("permissions")
        || operation
            .get("requiredPermissions")
            .and_then(Value::as_array)
            .is_some_and(|permissions| permissions.iter().any(|permission| !permission.is_string()))
}

fn manifest_path(directory: Option<&str>) -> std::path::PathBuf {
    Path::new(directory.unwrap_or(".")).join(alien_operations_sdk::manifest::MANIFEST_FILENAME)
}

fn print_summary(directory: Option<&str>, manifest: &CanonicalPluginManifest) {
    println!(
        "'{}' is valid: plugin '{}' v{} ({} tier)",
        manifest_path(directory).display(),
        manifest.name,
        manifest.version,
        manifest.tier.as_str()
    );
    for operation in &manifest.operations {
        let tier = operation.effective_tier(manifest.tier);
        let description = operation
            .description
            .as_deref()
            .unwrap_or("(no description)");
        println!("  {} [{}] — {description}", operation.name, tier.as_str());
    }
}

#[cfg(test)]
pub(super) fn legacy_verification_manifest() -> &'static str {
    r#"{
        "name": "legacy-operations",
        "version": "1.0.0",
        "tier": "mutating",
        "binaries": { "amd64": "legacy-operations-linux-amd64" },
        "operations": [
            {
                "name": "status",
                "tier": "read-only",
                "paramsSchema": {
                    "type": "object",
                    "properties": { "id": { "type": "string" } }
                }
            },
            {
                "name": "restart",
                "paramsSchema": {
                    "type": "object",
                    "properties": { "id": { "type": "string" } }
                },
                "verification": {
                    "changes": "restarts the service",
                    "pollOperation": "status",
                    "pollParamsFromResult": { "id": "id" },
                    "successField": "state",
                    "successValue": "ready",
                    "timeoutSeconds": 60
                },
                "sensitiveOutput": { "kind": "redact", "fields": ["token"] }
            }
        ]
    }"#
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
    fn accepts_a_released_legacy_verification_and_redaction_manifest() {
        let temp = tempfile::tempdir().expect("create temp dir");
        write_manifest(temp.path(), super::legacy_verification_manifest());

        let manifest = validate_manifest(Some(temp.path().to_str().expect("utf8 path")))
            .expect("released-valid legacy manifest must remain CLI-compatible");
        assert!(manifest.operations[0].input_schema.is_some());
        assert!(manifest.operations[0].output_schema.is_none());
    }

    #[test]
    fn never_falls_back_when_a_canonical_manifest_fails_strict_validation() {
        let temp = tempfile::tempdir().expect("create temp dir");
        write_manifest(
            temp.path(),
            r#"{
                "name": "postgres",
                "version": "1.0.0",
                "tier": "read-only",
                "binaries": { "amd64": "postgres-linux-amd64" },
                "operations": [{
                    "name": "health",
                    "inputSchema": { "type": "object" },
                    "timeoutSeconds": 0
                }]
            }"#,
        );

        let error = validate_manifest(Some(temp.path().to_str().expect("utf8 path")))
            .expect_err("a canonical marker must keep strict validation enabled");
        assert!(error.to_string().contains("timeoutSeconds"));
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
