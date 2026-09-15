//! Generate a Markdown reference page from a plugin's manifest.
//!
//! A plugin author declares parameters, risk, permissions, timeout,
//! retries, verification, and sensitive-output handling once in the
//! manifest; this renders that declaration as human-readable docs instead
//! of requiring it to be hand-written and kept in sync separately.

use std::fmt::Write as _;

#[allow(deprecated)]
use crate::manifest::{
    CanonicalOperationManifest, CanonicalPluginManifest, PluginManifest, RiskTier,
    SensitiveOutputPolicy,
};

/// Render a Markdown reference page documenting every operation `manifest`
/// declares.
///
/// This keeps the original public signature source compatible. Canonical
/// manifest consumers should use [`generate_docs_canonical`].
#[allow(deprecated)]
pub fn generate_docs(manifest: &PluginManifest) -> String {
    generate_docs_canonical(&manifest.clone().into_canonical())
}

/// Render documentation from the canonical operation contract.
pub fn generate_docs_canonical(manifest: &CanonicalPluginManifest) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# {}", manifest.name);
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Version `{}` · default risk tier: `{}`",
        manifest.version,
        manifest.tier.as_str()
    );
    let _ = writeln!(out);

    if manifest.operations.is_empty() {
        let _ = writeln!(out, "This plugin declares no operations.");
        return out;
    }

    let _ = writeln!(out, "## Operations");
    for operation in &manifest.operations {
        render_operation(&mut out, manifest.tier, operation);
    }
    out
}

fn render_operation(
    out: &mut String,
    plugin_tier: RiskTier,
    operation: &CanonicalOperationManifest,
) {
    let tier = operation.effective_tier(plugin_tier);
    let _ = writeln!(out);
    let _ = writeln!(out, "### `{}` — {}", operation.name, tier.as_str());
    let _ = writeln!(out);
    if let Some(description) = &operation.description {
        let _ = writeln!(out, "{description}");
        let _ = writeln!(out);
    }

    if !operation.required_permissions.is_empty() {
        let _ = writeln!(out, "**Required permissions:**");
        for permission in &operation.required_permissions {
            let _ = writeln!(out, "- `{}`", permission.id());
        }
        let _ = writeln!(out);
    }

    if let Some(timeout) = operation.timeout_seconds {
        let _ = writeln!(out, "**Timeout:** {timeout}s");
        let _ = writeln!(out);
    }

    if let Some(retries) = &operation.retries {
        let _ = writeln!(
            out,
            "**Retries:** up to {} attempts, {}s apart",
            retries.max_attempts, retries.interval_seconds
        );
        let _ = writeln!(out);
    }

    if let Some(verification) = &operation.verification {
        let _ = writeln!(out, "**Verification:** {}", verification.changes);
        let _ = writeln!(
            out,
            "Confirmed by polling `{}` (up to {}s) until `{}` equals `{}`.",
            verification.poll_operation,
            verification.timeout_seconds,
            verification.success_field,
            verification.success_value
        );
        let _ = writeln!(out);
    }

    match &operation.sensitive_output {
        SensitiveOutputPolicy::None => {}
        SensitiveOutputPolicy::Redact { fields } => {
            let _ = writeln!(
                out,
                "**Sensitive output:** redacts `{}` before display or logging.",
                fields.join("`, `")
            );
            let _ = writeln!(out);
        }
        SensitiveOutputPolicy::RequireConfirmation => {
            let _ = writeln!(
                out,
                "**Sensitive output:** result requires explicit confirmation before display."
            );
            let _ = writeln!(out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_json(operations: &str) -> String {
        format!(
            r#"{{
                "name": "postgres",
                "version": "1.0.0",
                "tier": "read-only",
                "binaries": {{ "amd64": "postgres-linux-amd64" }},
                "operations": [{operations}]
            }}"#
        )
    }

    #[test]
    fn renders_plugin_name_and_version() {
        let manifest = CanonicalPluginManifest::parse_and_validate(manifest_json("").as_bytes())
            .expect("valid manifest");
        let docs = generate_docs_canonical(&manifest);
        assert!(docs.contains("# postgres"));
        assert!(docs.contains("`1.0.0`"));
    }

    #[test]
    fn renders_operation_description_and_tier() {
        let manifest = CanonicalPluginManifest::parse_and_validate(
            manifest_json(
                r#"{"name": "vacuum", "tier": "mutating", "description": "Run VACUUM."}"#,
            )
            .as_bytes(),
        )
        .expect("valid manifest");
        let docs = generate_docs_canonical(&manifest);
        assert!(docs.contains("### `vacuum` — mutating"));
        assert!(docs.contains("Run VACUUM."));
    }

    #[test]
    fn renders_required_permissions_and_timeout() {
        let manifest = CanonicalPluginManifest::parse_and_validate(
            manifest_json(
                r#"{
                    "name": "vacuum",
                    "tier": "mutating",
                    "requiredPermissions": ["postgres/vacuum"],
                    "timeoutSeconds": 30
                }"#,
            )
            .as_bytes(),
        )
        .expect("valid manifest");
        let docs = generate_docs_canonical(&manifest);
        assert!(docs.contains("`postgres/vacuum`"));
        assert!(docs.contains("**Timeout:** 30s"));
    }

    #[test]
    fn renders_verification_details() {
        let manifest = CanonicalPluginManifest::parse_and_validate(
            manifest_json(
                r#"
                {"name": "get-pod-status", "tier": "read-only", "outputSchema": {
                    "type": "object",
                    "properties": {"status": {"type": "string"}}
                }},
                {"name": "restart-pod", "tier": "mutating", "verification": {
                    "changes": "the pod restarts",
                    "pollOperation": "get-pod-status",
                    "successField": "status",
                    "successValue": "Running",
                    "timeoutSeconds": 60
                }}
                "#,
            )
            .as_bytes(),
        )
        .expect("valid manifest");
        let docs = generate_docs_canonical(&manifest);
        assert!(docs.contains("the pod restarts"));
        assert!(docs.contains("`get-pod-status`"));
        assert!(docs.contains("`status`"));
        assert!(docs.contains("`Running`"));
    }

    #[test]
    fn renders_a_plugin_with_no_operations() {
        let manifest = CanonicalPluginManifest::parse_and_validate(manifest_json("").as_bytes())
            .expect("valid manifest");
        let docs = generate_docs_canonical(&manifest);
        assert!(docs.contains("declares no operations"));
    }

    #[allow(deprecated)]
    #[test]
    fn legacy_generate_docs_signature_remains_source_compatible() {
        let generate: fn(&PluginManifest) -> String = generate_docs;
        let manifest = PluginManifest::parse_and_validate(manifest_json("").as_bytes())
            .expect("valid legacy manifest");

        assert!(generate(&manifest).contains("# postgres"));
    }
}
