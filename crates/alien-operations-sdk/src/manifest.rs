//! The plugin manifest — the declared contract between a plugin and the
//! runtime that hosts it.
//!
//! A plugin ships a manifest (conventionally `metadata.json`) alongside one
//! native binary per supported architecture. The manifest is the single
//! source of truth the runtime uses to generate permissions, access-request
//! prompts, CLI help, MCP tool schemas, and docs — a plugin author declares
//! it once and gets all of those for free.
//!
//! ```json
//! {
//!   "name": "postgres",
//!   "version": "1.0.0",
//!   "tier": "read-only",
//!   "binaries": {
//!     "amd64": "postgres-linux-amd64",
//!     "arm64": "postgres-linux-arm64"
//!   },
//!   "operations": [
//!     {
//!       "name": "vacuum",
//!       "tier": "mutating",
//!       "description": "Run VACUUM on a table.",
//!       "requiredPermissions": ["postgres/vacuum"],
//!       "timeoutSeconds": 30,
//!       "retries": { "maxAttempts": 2, "intervalSeconds": 5 },
//!       "sensitiveOutput": "none"
//!     }
//!   ]
//! }
//! ```

use std::collections::{BTreeMap, BTreeSet};

use alien_error::AlienError;
use schemars::schema::RootSchema;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};
use crate::verification::Verification;

/// The `metadata.json` filename conventionally used inside a plugin bundle.
pub const MANIFEST_FILENAME: &str = "metadata.json";

/// A CPU architecture a plugin binary targets. Only `amd64` and `arm64` are
/// supported, matching the operator/worker deploy targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Arch {
    /// x86-64 (`x86_64`).
    Amd64,
    /// AArch64 (`aarch64`).
    Arm64,
}

impl Arch {
    /// The architecture of the host this process is running on, if supported.
    pub fn host() -> Option<Self> {
        match std::env::consts::ARCH {
            "x86_64" => Some(Arch::Amd64),
            "aarch64" => Some(Arch::Arm64),
            _ => None,
        }
    }

    /// The canonical lowercase token used in manifests and error messages.
    pub fn as_str(&self) -> &'static str {
        match self {
            Arch::Amd64 => "amd64",
            Arch::Arm64 => "arm64",
        }
    }
}

/// How risky an operation is, and therefore whether a workspace policy
/// should let it run unattended or require explicit approval first.
///
/// The default for an unspecified tier is the most restrictive
/// ([`RiskTier::Destructive`]) so that an under-annotated plugin fails safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RiskTier {
    /// Reads state only; never mutates the target environment.
    ReadOnly,
    /// Mutates non-destructively (config changes, restarts).
    Mutating,
    /// Potentially destructive or irreversible (deletes, data loss risk).
    Destructive,
}

impl RiskTier {
    /// The canonical kebab-case token.
    pub fn as_str(&self) -> &'static str {
        match self {
            RiskTier::ReadOnly => "read-only",
            RiskTier::Mutating => "mutating",
            RiskTier::Destructive => "destructive",
        }
    }
}

impl Default for RiskTier {
    fn default() -> Self {
        // Fail safe: an operation with no declared tier is treated as the
        // highest risk, so it always routes through approval.
        RiskTier::Destructive
    }
}

/// How a plugin's output should be treated before it is shown to a human or
/// handed to an AI agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SensitiveOutputPolicy {
    /// No special handling; the result is safe to display and log as-is.
    None,
    /// The named top-level result fields (dot-paths into the JSON result)
    /// must be redacted before display or logging.
    Redact {
        /// Dot-paths into the result JSON that must be redacted, e.g.
        /// `"connectionString"` or `"rows.password"`.
        fields: Vec<String>,
    },
    /// The result may contain sensitive data the caller must explicitly
    /// acknowledge before it is displayed (e.g. a one-time secret value).
    RequireConfirmation,
}

impl Default for SensitiveOutputPolicy {
    fn default() -> Self {
        SensitiveOutputPolicy::None
    }
}

/// A bounded retry policy for the operation's own invocation (distinct from
/// [`Verification::retry`], which retries the post-write confirmation poll).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryPolicy {
    /// Maximum number of attempts, including the first (so `1` means no
    /// retry).
    pub max_attempts: u32,
    /// Delay between attempts, in seconds.
    pub interval_seconds: u32,
}

/// One named operation a plugin exposes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationManifest {
    /// Operation name, unique within the plugin (e.g. `vacuum`).
    pub name: String,
    /// Risk tier for this specific operation. Falls back to the plugin's
    /// tier when omitted (resolved during [`PluginManifest::validate`],
    /// never left unset in memory after loading through this crate).
    #[serde(default)]
    pub tier: Option<RiskTier>,
    /// Human-readable description surfaced in the catalog, CLI help, and
    /// generated MCP tool schema.
    #[serde(default)]
    pub description: Option<String>,
    /// JSON Schema for this operation's invocation params. `None` means the
    /// operation takes no params.
    #[serde(default)]
    pub params_schema: Option<RootSchema>,
    /// IDs of permission sets (see `alien-permissions`) this operation
    /// requires, e.g. `"postgres/vacuum"`. Each ID must resolve to a
    /// permission set already defined in `alien-permissions`; a plugin
    /// author needing a new permission contributes that permission set
    /// first.
    #[serde(default)]
    pub required_permissions: Vec<String>,
    /// How long the runtime should wait for this operation to complete
    /// before treating it as failed.
    #[serde(default)]
    pub timeout_seconds: Option<u32>,
    /// How the runtime should retry this operation's own invocation if it
    /// fails transiently. Absent means no retry.
    #[serde(default)]
    pub retries: Option<RetryPolicy>,
    /// How to confirm a mutating/destructive operation actually took
    /// effect. Absent means the runtime treats a successful exit as proof
    /// enough.
    #[serde(default)]
    pub verification: Option<Verification>,
    /// How this operation's result should be treated before display or
    /// logging.
    #[serde(default)]
    pub sensitive_output: SensitiveOutputPolicy,
}

impl OperationManifest {
    /// The effective risk tier: this operation's own tier if declared,
    /// otherwise the plugin's default.
    pub fn effective_tier(&self, plugin_default: RiskTier) -> RiskTier {
        self.tier.unwrap_or(plugin_default)
    }
}

/// The parsed, validated contents of a plugin manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    /// Plugin name, unique within a workspace (e.g. `postgres`).
    pub name: String,
    /// Semantic version string (e.g. `1.0.0`). Compared as an opaque
    /// string; this crate does not impose semver ordering.
    pub version: String,
    /// The plugin-level default risk tier. Individual operations may
    /// override it.
    #[serde(default)]
    pub tier: RiskTier,
    /// Map of architecture to the bundle entry name of that arch's binary.
    pub binaries: BTreeMap<Arch, String>,
    /// The operations this plugin exposes.
    #[serde(default)]
    pub operations: Vec<OperationManifest>,
}

impl PluginManifest {
    /// Parse a manifest from `metadata.json` bytes without validating it.
    /// Prefer [`PluginManifest::parse_and_validate`] unless you specifically
    /// need the unvalidated form.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).map_err(|err| {
            AlienError::new(ErrorData::ManifestInvalid {
                reason: err.to_string(),
            })
        })
    }

    /// Parse and validate `metadata.json` bytes.
    pub fn parse_and_validate(bytes: &[u8]) -> Result<Self> {
        let manifest = Self::parse(bytes)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Look up a declared operation by name.
    pub fn operation(&self, name: &str) -> Option<&OperationManifest> {
        self.operations.iter().find(|op| op.name == name)
    }

    /// Validate internal consistency of the manifest:
    /// - operation names are unique
    /// - every `verification.pollOperation` names a read-only operation
    ///   declared by this same plugin
    pub fn validate(&self) -> Result<()> {
        let mut seen = BTreeSet::new();
        for operation in &self.operations {
            if !seen.insert(operation.name.as_str()) {
                return Err(AlienError::new(ErrorData::OperationDuplicate {
                    plugin: self.name.clone(),
                    operation: operation.name.clone(),
                }));
            }
        }

        for operation in &self.operations {
            let Some(verification) = &operation.verification else {
                continue;
            };
            let poll = self.operation(&verification.poll_operation);
            let is_read_only_poll = poll
                .map(|poll_op| poll_op.effective_tier(self.tier) == RiskTier::ReadOnly)
                .unwrap_or(false);
            if !is_read_only_poll {
                return Err(AlienError::new(ErrorData::VerificationPollInvalid {
                    plugin: self.name.clone(),
                    operation: operation.name.clone(),
                    poll_operation: verification.poll_operation.clone(),
                }));
            }
        }

        Ok(())
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
                "binaries": {{ "amd64": "postgres-linux-amd64", "arm64": "postgres-linux-arm64" }},
                "operations": [{operations}]
            }}"#
        )
    }

    #[test]
    fn parses_minimal_manifest() {
        let manifest = PluginManifest::parse_and_validate(manifest_json("").as_bytes())
            .expect("minimal manifest should parse and validate");
        assert_eq!(manifest.name, "postgres");
        assert_eq!(manifest.tier, RiskTier::ReadOnly);
        assert!(manifest.operations.is_empty());
    }

    #[test]
    fn rejects_duplicate_operation_names() {
        let json = manifest_json(r#"{"name": "vacuum"}, {"name": "vacuum", "tier": "mutating"}"#);
        let err = PluginManifest::parse_and_validate(json.as_bytes())
            .expect_err("duplicate operation names must fail validation");
        assert!(err.to_string().contains("more than once"));
    }

    #[test]
    fn rejects_verification_poll_operation_that_is_not_read_only() {
        let json = manifest_json(
            r#"
            {"name": "restart", "tier": "mutating", "verification": {
                "changes": "the pod restarts",
                "pollOperation": "restart",
                "successField": "status",
                "successValue": "Running",
                "timeoutSeconds": 60
            }}
            "#,
        );
        let err = PluginManifest::parse_and_validate(json.as_bytes())
            .expect_err("poll operation must be read-only");
        assert!(err.to_string().contains("pollOperation"));
    }

    #[test]
    fn accepts_verification_polling_a_read_only_sibling() {
        let json = manifest_json(
            r#"
            {"name": "get-pod-status", "tier": "read-only"},
            {"name": "restart-pod", "tier": "mutating", "verification": {
                "changes": "the pod restarts",
                "pollOperation": "get-pod-status",
                "successField": "status",
                "successValue": "Running",
                "timeoutSeconds": 60
            }}
            "#,
        );
        PluginManifest::parse_and_validate(json.as_bytes())
            .expect("verification polling a read-only sibling operation should validate");
    }

    #[test]
    fn unspecified_tier_defaults_to_destructive() {
        assert_eq!(RiskTier::default(), RiskTier::Destructive);
    }

    #[test]
    fn operation_tier_falls_back_to_plugin_default() {
        let op = OperationManifest {
            name: "vacuum".into(),
            tier: None,
            description: None,
            params_schema: None,
            required_permissions: vec![],
            timeout_seconds: None,
            retries: None,
            verification: None,
            sensitive_output: SensitiveOutputPolicy::None,
        };
        assert_eq!(op.effective_tier(RiskTier::Mutating), RiskTier::Mutating);
    }
}
