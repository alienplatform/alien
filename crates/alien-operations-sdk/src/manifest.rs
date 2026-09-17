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
//!       "permissions": ["postgres/vacuum"],
//!       "inputSchema": { "type": "object" },
//!       "outputSchema": { "type": "object" },
//!       "timeoutSeconds": 30,
//!       "retries": { "maxAttempts": 2, "intervalSeconds": 5 },
//!       "sensitiveOutput": { "kind": "none" }
//!     }
//!   ]
//! }
//! ```

use std::collections::{BTreeMap, BTreeSet};

use alien_core::permissions::{PermissionSet, PermissionSetReference};
use alien_error::AlienError;
use schemars::schema::{RootSchema, Schema, SchemaObject, SingleOrVec};
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};
use crate::kubernetes::KubernetesPermissions;
use crate::verification::Verification;

/// The `metadata.json` filename conventionally used inside a plugin bundle.
pub const MANIFEST_FILENAME: &str = "metadata.json";

/// Maximum uncompressed size of one executable bundle entry.
pub const MAX_BUNDLE_EXECUTABLE_BYTES: u64 = 64 * 1024 * 1024;

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", deny_unknown_fields)]
enum SensitiveOutputPolicyWire {
    None {},
    Redact { fields: Vec<String> },
    RequireConfirmation {},
}

impl<'de> Deserialize<'de> for SensitiveOutputPolicy {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(
            match SensitiveOutputPolicyWire::deserialize(deserializer)? {
                SensitiveOutputPolicyWire::None {} => Self::None,
                SensitiveOutputPolicyWire::Redact { fields } => Self::Redact { fields },
                SensitiveOutputPolicyWire::RequireConfirmation {} => Self::RequireConfirmation,
            },
        )
    }
}

impl Default for SensitiveOutputPolicy {
    fn default() -> Self {
        SensitiveOutputPolicy::None
    }
}

impl SensitiveOutputPolicy {
    fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

/// A bounded retry policy for the operation's own invocation (distinct from
/// [`Verification::retry`], which retries the post-write confirmation poll).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetryPolicy {
    /// Maximum number of attempts, including the first (so `1` means no
    /// retry).
    pub max_attempts: u32,
    /// Delay between attempts, in seconds.
    pub interval_seconds: u32,
}

/// The original operation manifest published by this crate.
///
/// This type is intentionally kept source compatible for downstream crates
/// that construct it with struct literals. New code should use
/// [`CanonicalOperationManifest`].
#[deprecated(note = "use CanonicalOperationManifest for the canonical contract")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationManifest {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_kubernetes_permissions"
    )]
    pub kubernetes_permissions: Option<KubernetesPermissions>,
    pub name: String,
    #[serde(default)]
    pub tier: Option<RiskTier>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub params_schema: Option<RootSchema>,
    #[serde(default)]
    pub required_permissions: Vec<String>,
    #[serde(default)]
    pub timeout_seconds: Option<u32>,
    #[serde(default)]
    pub retries: Option<RetryPolicy>,
    #[serde(default)]
    pub verification: Option<Verification>,
    #[serde(default)]
    pub sensitive_output: SensitiveOutputPolicy,
}

#[allow(deprecated)]
impl OperationManifest {
    pub fn effective_tier(&self, plugin_default: RiskTier) -> RiskTier {
        self.tier.unwrap_or(plugin_default)
    }

    /// Convert the legacy contract into the canonical contract without
    /// changing the meaning of any existing field.
    pub fn into_canonical(self) -> CanonicalOperationManifest {
        self.into()
    }
}

/// The original plugin manifest published by this crate.
///
/// This remains available for source compatibility. Canonical metadata
/// readers and writers should use [`CanonicalPluginManifest`].
#[deprecated(note = "use CanonicalPluginManifest for the canonical contract")]
#[allow(deprecated)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub tier: RiskTier,
    pub binaries: BTreeMap<Arch, String>,
    #[serde(default)]
    pub operations: Vec<OperationManifest>,
}

#[allow(deprecated)]
impl PluginManifest {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).map_err(|err| {
            AlienError::new(ErrorData::ManifestInvalid {
                reason: err.to_string(),
            })
        })
    }

    pub fn parse_and_validate(bytes: &[u8]) -> Result<Self> {
        let manifest = Self::parse(bytes)?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn operation(&self, name: &str) -> Option<&OperationManifest> {
        self.operations
            .iter()
            .find(|operation| operation.name == name)
    }

    pub fn binary_for(&self, arch: Arch) -> Result<&str> {
        binary_for(&self.name, &self.binaries, arch)
    }

    pub fn tier_for(&self, operation: &str) -> Result<RiskTier> {
        self.operation(operation)
            .map(|operation| operation.effective_tier(self.tier))
            .ok_or_else(|| unknown_operation(&self.name, operation))
    }

    pub fn content_key(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }

    pub fn into_canonical(self) -> CanonicalPluginManifest {
        self.into()
    }

    pub fn validate(&self) -> Result<()> {
        require_non_empty(&self.name, "name")?;
        require_non_empty(&self.version, "version")?;
        if self.binaries.is_empty() {
            return Err(AlienError::new(ErrorData::FieldEmpty {
                field: "binaries".to_string(),
            }));
        }
        for (arch, entry) in &self.binaries {
            require_non_empty(entry, &format!("binaries.{}", arch.as_str()))?;
            reject_reserved_binary_entry(entry, &format!("binaries.{}", arch.as_str()))?;
        }

        let mut seen = BTreeSet::new();
        for (index, operation) in self.operations.iter().enumerate() {
            require_non_empty(&operation.name, &format!("operations[{index}].name"))?;
            if let Some(permissions) = &operation.kubernetes_permissions {
                permissions.validate(operation.effective_tier(self.tier))?;
            }
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

/// One named operation in the canonical plugin contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CanonicalOperationManifest {
    /// Explicit Kubernetes API requirements, compiled by the installer for
    /// enabled plugins within the chosen scope and permission ceiling.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_kubernetes_permissions"
    )]
    pub kubernetes_permissions: Option<KubernetesPermissions>,
    /// Operation name, unique within the plugin (e.g. `vacuum`).
    pub name: String,
    /// Risk tier for this specific operation. Falls back to the plugin's
    /// tier when omitted (resolved through [`CanonicalOperationManifest::effective_tier`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<RiskTier>,
    /// Human-readable description surfaced in the catalog, CLI help, and
    /// generated MCP tool schema.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema for this operation's invocation params. `None` means the
    /// operation takes no params. Legacy `paramsSchema` manifests remain
    /// readable; serialization always emits the canonical `inputSchema` key.
    #[serde(
        default,
        rename = "inputSchema",
        alias = "paramsSchema",
        skip_serializing_if = "Option::is_none"
    )]
    pub input_schema: Option<RootSchema>,
    /// JSON Schema for a successful operation result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<RootSchema>,
    /// Permission-set references required by this operation. Prefer stable
    /// identifiers from `alien-permissions`; inline definitions remain
    /// supported for existing bundles. Legacy `requiredPermissions` arrays
    /// are accepted and normalized to the canonical `permissions` key.
    #[serde(
        default,
        rename = "permissions",
        alias = "requiredPermissions",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub required_permissions: Vec<PermissionSetReference>,
    /// How long the runtime should wait for this operation to complete
    /// before treating it as failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,
    /// How the runtime should retry this operation's own invocation if it
    /// fails transiently. Absent means no retry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retries: Option<RetryPolicy>,
    /// How to confirm a mutating/destructive operation actually took
    /// effect. Absent means the runtime treats a successful exit as proof
    /// enough.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<Verification>,
    /// How this operation's result should be treated before display or
    /// logging.
    #[serde(default, skip_serializing_if = "SensitiveOutputPolicy::is_none")]
    pub sensitive_output: SensitiveOutputPolicy,
}

fn deserialize_kubernetes_permissions<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<KubernetesPermissions>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // Explicit null is not a versioned permission declaration. Parsing the
    // concrete type here preserves that fail-closed distinction from absence.
    KubernetesPermissions::deserialize(deserializer).map(Some)
}

impl CanonicalOperationManifest {
    /// The effective risk tier: this operation's own tier if declared,
    /// otherwise the plugin's default.
    pub fn effective_tier(&self, plugin_default: RiskTier) -> RiskTier {
        self.tier.unwrap_or(plugin_default)
    }

    /// Stable identifiers for every named or inline permission reference.
    pub fn permission_ids(&self) -> impl Iterator<Item = &str> {
        self.required_permissions
            .iter()
            .map(PermissionSetReference::id)
    }
}

/// The parsed, validated contents of a canonical plugin manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CanonicalPluginManifest {
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
    pub operations: Vec<CanonicalOperationManifest>,
}

#[allow(deprecated)]
impl From<OperationManifest> for CanonicalOperationManifest {
    fn from(operation: OperationManifest) -> Self {
        Self {
            kubernetes_permissions: operation.kubernetes_permissions,
            name: operation.name,
            tier: operation.tier,
            description: operation.description,
            input_schema: operation.params_schema,
            output_schema: None,
            required_permissions: operation
                .required_permissions
                .into_iter()
                .map(PermissionSetReference::from_name)
                .collect(),
            timeout_seconds: operation.timeout_seconds,
            retries: operation.retries,
            verification: operation.verification,
            sensitive_output: operation.sensitive_output,
        }
    }
}

#[allow(deprecated)]
impl From<PluginManifest> for CanonicalPluginManifest {
    fn from(manifest: PluginManifest) -> Self {
        Self {
            name: manifest.name,
            version: manifest.version,
            tier: manifest.tier,
            binaries: manifest.binaries,
            operations: manifest
                .operations
                .into_iter()
                .map(CanonicalOperationManifest::from)
                .collect(),
        }
    }
}

impl CanonicalPluginManifest {
    /// Parse a manifest from `metadata.json` bytes without validating it.
    /// Prefer [`CanonicalPluginManifest::parse_and_validate`] unless you specifically
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
    pub fn operation(&self, name: &str) -> Option<&CanonicalOperationManifest> {
        self.operations.iter().find(|op| op.name == name)
    }

    /// The bundle entry for `arch`.
    pub fn binary_for(&self, arch: Arch) -> Result<&str> {
        binary_for(&self.name, &self.binaries, arch)
    }

    /// The effective risk tier for a named operation.
    pub fn tier_for(&self, operation: &str) -> Result<RiskTier> {
        self.operation(operation)
            .map(|operation| operation.effective_tier(self.tier))
            .ok_or_else(|| unknown_operation(&self.name, operation))
    }

    /// Stable extraction-directory key for this manifest.
    pub fn content_key(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }

    /// Validate internal consistency of the manifest:
    /// - the plugin name, version, every declared binary entry, and every
    ///   operation name are non-empty
    /// - operation names are unique
    /// - every `verification.pollOperation` names a read-only operation
    ///   declared by this same plugin
    pub fn validate(&self) -> Result<()> {
        require_non_empty(&self.name, "name")?;
        require_non_empty(&self.version, "version")?;
        if self.binaries.is_empty() {
            return Err(AlienError::new(ErrorData::FieldEmpty {
                field: "binaries".to_string(),
            }));
        }
        let mut binary_entries = BTreeSet::new();
        for (arch, entry) in &self.binaries {
            let field = format!("binaries.{}", arch.as_str());
            require_non_empty(entry, &field)?;
            reject_reserved_binary_entry(entry, &field)?;
            if !valid_binary_entry(entry) {
                return Err(AlienError::new(ErrorData::ManifestInvalid {
                    reason: format!(
                        "{field} must be a root filename containing only ASCII letters, digits, '.', '_', or '-'"
                    ),
                }));
            }
            if !binary_entries.insert(entry) {
                return Err(AlienError::new(ErrorData::ManifestInvalid {
                    reason: format!(
                        "binary entry '{entry}' is declared for more than one architecture"
                    ),
                }));
            }
        }

        let mut seen = BTreeSet::new();
        let mut permission_definitions = BTreeMap::new();
        for (index, operation) in self.operations.iter().enumerate() {
            require_non_empty(&operation.name, &format!("operations[{index}].name"))?;
            validate_operation_contract(&self.name, self.tier, operation)?;
            for permission in &operation.required_permissions {
                let permission_id = permission.id();
                if let Some(existing) = permission_definitions.get(permission_id) {
                    if existing != permission {
                        return invalid_operation(
                            &self.name,
                            operation,
                            "permissions",
                            &format!(
                                "permission '{permission_id}' has conflicting definitions across operations"
                            ),
                        );
                    }
                } else {
                    permission_definitions.insert(permission_id.to_string(), permission.clone());
                }
            }
            if let Some(permissions) = &operation.kubernetes_permissions {
                permissions.validate(operation.effective_tier(self.tier))?;
            }
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
            let poll = poll.expect("a valid read-only poll operation exists");
            let Some(poll_output_schema) = poll.output_schema.as_ref() else {
                return invalid_operation(
                    &self.name,
                    operation,
                    "verification.successField",
                    "cannot be verified because the poll operation has no outputSchema",
                );
            };
            if !root_schema_contains_path(poll_output_schema, &verification.success_field) {
                return invalid_operation(
                    &self.name,
                    operation,
                    "verification.successField",
                    &format!(
                        "path '{}' is not declared by poll operation '{}' outputSchema",
                        verification.success_field, verification.poll_operation
                    ),
                );
            }

            if !verification.poll_params_from_result.is_empty() {
                let Some(operation_output_schema) = operation.output_schema.as_ref() else {
                    return invalid_operation(
                        &self.name,
                        operation,
                        "verification.pollParamsFromResult",
                        "cannot extract poll parameters without an outputSchema",
                    );
                };
                let Some(poll_input_schema) = poll.input_schema.as_ref() else {
                    return invalid_operation(
                        &self.name,
                        operation,
                        "verification.pollParamsFromResult",
                        "cannot pass poll parameters because the poll operation has no inputSchema",
                    );
                };
                for (poll_param, result_path) in &verification.poll_params_from_result {
                    if poll_param.contains('.')
                        || !root_schema_contains_path(poll_input_schema, poll_param)
                    {
                        return invalid_operation(
                            &self.name,
                            operation,
                            "verification.pollParamsFromResult",
                            &format!(
                                "poll parameter '{poll_param}' is not a top-level property declared by '{}' inputSchema",
                                verification.poll_operation
                            ),
                        );
                    }
                    if !root_schema_contains_path(operation_output_schema, result_path) {
                        return invalid_operation(
                            &self.name,
                            operation,
                            "verification.pollParamsFromResult",
                            &format!("result path '{result_path}' is not declared by outputSchema"),
                        );
                    }
                }
            }
        }

        Ok(())
    }
}

fn binary_for<'a>(
    plugin: &str,
    binaries: &'a BTreeMap<Arch, String>,
    arch: Arch,
) -> Result<&'a str> {
    binaries.get(&arch).map(String::as_str).ok_or_else(|| {
        let available = binaries
            .keys()
            .map(Arch::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        AlienError::new(ErrorData::ArchUnsupported {
            plugin: plugin.to_string(),
            arch: arch.as_str().to_string(),
            available,
        })
    })
}

fn unknown_operation(plugin: &str, operation: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::OperationUnknown {
        plugin: plugin.to_string(),
        operation: operation.to_string(),
    })
}

fn validate_operation_contract(
    plugin: &str,
    plugin_tier: RiskTier,
    operation: &CanonicalOperationManifest,
) -> Result<()> {
    if operation
        .description
        .as_ref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return invalid_operation(plugin, operation, "description", "must not be empty");
    }
    if operation.timeout_seconds == Some(0) {
        return invalid_operation(
            plugin,
            operation,
            "timeoutSeconds",
            "must be greater than zero",
        );
    }
    if let Some(retries) = operation.retries {
        validate_retry_policy(plugin, operation, "retries", retries)?;
        if operation.timeout_seconds.is_none() {
            return invalid_operation(
                plugin,
                operation,
                "retries",
                "requires timeoutSeconds so execution remains bounded",
            );
        }
    }

    let mut permission_ids = BTreeSet::new();
    for permission in &operation.required_permissions {
        let permission_id = permission.id();
        if permission_id.trim().is_empty() {
            return invalid_operation(
                plugin,
                operation,
                "permissions",
                "identifiers must not be empty",
            );
        }
        if !permission_ids.insert(permission_id) {
            return invalid_operation(
                plugin,
                operation,
                "permissions",
                &format!("permission '{permission_id}' is declared more than once"),
            );
        }
        if let PermissionSetReference::Inline(permission_set) = permission {
            validate_inline_permission_set(plugin, operation, permission_set)?;
        }
    }

    if let Some(verification) = &operation.verification {
        if operation.effective_tier(plugin_tier) == RiskTier::ReadOnly {
            return invalid_operation(
                plugin,
                operation,
                "verification",
                "read-only operations cannot declare post-write verification",
            );
        }
        for (field, value) in [
            ("verification.changes", verification.changes.as_str()),
            (
                "verification.pollOperation",
                verification.poll_operation.as_str(),
            ),
            (
                "verification.successField",
                verification.success_field.as_str(),
            ),
            (
                "verification.successValue",
                verification.success_value.as_str(),
            ),
        ] {
            if value.trim().is_empty() {
                return invalid_operation(plugin, operation, field, "must not be empty");
            }
        }
        if verification.timeout_seconds == 0 {
            return invalid_operation(
                plugin,
                operation,
                "verification.timeoutSeconds",
                "must be greater than zero",
            );
        }
        if let Some(retries) = verification.retry {
            validate_retry_policy(plugin, operation, "verification.retry", retries)?;
        }
        for (poll_param, result_path) in &verification.poll_params_from_result {
            if poll_param.trim().is_empty() || result_path.trim().is_empty() {
                return invalid_operation(
                    plugin,
                    operation,
                    "verification.pollParamsFromResult",
                    "parameter names and result paths must not be empty",
                );
            }
        }
    }

    if let SensitiveOutputPolicy::Redact { fields } = &operation.sensitive_output {
        if fields.is_empty() || fields.iter().any(|field| field.trim().is_empty()) {
            return invalid_operation(
                plugin,
                operation,
                "sensitiveOutput.fields",
                "must contain non-empty output paths",
            );
        }
        let Some(output_schema) = operation.output_schema.as_ref() else {
            return invalid_operation(
                plugin,
                operation,
                "outputSchema",
                "is required when sensitive output fields are redacted",
            );
        };
        if let Some(field) = fields
            .iter()
            .find(|field| !root_schema_contains_redaction_path(output_schema, field))
        {
            return invalid_operation(
                plugin,
                operation,
                "sensitiveOutput.fields",
                &format!("path '{field}' is not declared by outputSchema"),
            );
        }
    }
    Ok(())
}

fn root_schema_contains_path(root: &RootSchema, path: &str) -> bool {
    root_schema_contains_path_with_arrays(root, path, false)
}

/// Redaction applies the same path to every element it encounters in an
/// array, so a schema path through array items is valid for that policy. The
/// verification reader does not have that behavior and intentionally uses
/// [`root_schema_contains_path`] instead.
fn root_schema_contains_redaction_path(root: &RootSchema, path: &str) -> bool {
    root_schema_contains_path_with_arrays(root, path, true)
}

fn root_schema_contains_path_with_arrays(
    root: &RootSchema,
    path: &str,
    transparent_arrays: bool,
) -> bool {
    let segments = path.split('.').collect::<Vec<_>>();
    let mut visited_references = BTreeSet::new();
    schema_object_contains_path(
        root,
        &root.schema,
        &segments,
        transparent_arrays,
        &mut visited_references,
    )
}

fn schema_contains_path(
    root: &RootSchema,
    schema: &Schema,
    segments: &[&str],
    transparent_arrays: bool,
    visited_references: &mut BTreeSet<(String, usize)>,
) -> bool {
    match schema {
        Schema::Bool(_) => false,
        Schema::Object(schema) => schema_object_contains_path(
            root,
            schema,
            segments,
            transparent_arrays,
            visited_references,
        ),
    }
}

fn schema_object_contains_path(
    root: &RootSchema,
    schema: &SchemaObject,
    segments: &[&str],
    transparent_arrays: bool,
    visited_references: &mut BTreeSet<(String, usize)>,
) -> bool {
    if segments.is_empty() {
        return true;
    }

    if let Some(reference) = &schema.reference {
        let visit = (reference.clone(), segments.len());
        if !visited_references.insert(visit.clone()) {
            return false;
        }
        let found = local_definition(root, reference).is_some_and(|definition| {
            schema_contains_path(
                root,
                &definition,
                segments,
                transparent_arrays,
                visited_references,
            )
        });
        visited_references.remove(&visit);
        if found {
            return true;
        }
    }

    if let Some(items) = transparent_arrays
        .then(|| schema.array.as_ref().and_then(|array| array.items.as_ref()))
        .flatten()
    {
        let found = match items {
            SingleOrVec::Single(item) => {
                schema_contains_path(root, item, segments, transparent_arrays, visited_references)
            }
            SingleOrVec::Vec(items) => items.iter().any(|item| {
                schema_contains_path(root, item, segments, transparent_arrays, visited_references)
            }),
        };
        if found {
            return true;
        }
    }

    let (head, tail) = segments.split_first().expect("checked non-empty");
    if let Some(property) = schema
        .object
        .as_ref()
        .and_then(|object| object.properties.get(*head))
    {
        if tail.is_empty()
            || schema_contains_path(root, property, tail, transparent_arrays, visited_references)
        {
            return true;
        }
    }

    schema.subschemas.as_ref().is_some_and(|subschemas| {
        [
            subschemas.all_of.as_ref(),
            subschemas.any_of.as_ref(),
            subschemas.one_of.as_ref(),
        ]
        .into_iter()
        .flatten()
        .flatten()
        .any(|branch| {
            schema_contains_path(
                root,
                branch,
                segments,
                transparent_arrays,
                visited_references,
            )
        }) || [
            subschemas.if_schema.as_deref(),
            subschemas.then_schema.as_deref(),
            subschemas.else_schema.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|branch| {
            schema_contains_path(
                root,
                branch,
                segments,
                transparent_arrays,
                visited_references,
            )
        })
    })
}

fn local_definition(root: &RootSchema, reference: &str) -> Option<Schema> {
    let pointer = reference.strip_prefix('#')?;
    if !pointer.starts_with('/') {
        return None;
    }
    let document = serde_json::to_value(root).ok()?;
    let resolved = document.pointer(pointer).or_else(|| {
        pointer
            .strip_prefix("/$defs/")
            .and_then(|tail| document.pointer(&format!("/definitions/{tail}")))
    })?;
    serde_json::from_value(resolved.clone()).ok()
}

fn validate_retry_policy(
    plugin: &str,
    operation: &CanonicalOperationManifest,
    field: &str,
    retries: RetryPolicy,
) -> Result<()> {
    if retries.max_attempts == 0 {
        return invalid_operation(
            plugin,
            operation,
            field,
            "maxAttempts must be greater than zero",
        );
    }
    if retries.max_attempts > 1 && retries.interval_seconds == 0 {
        return invalid_operation(
            plugin,
            operation,
            field,
            "intervalSeconds must be greater than zero when retrying",
        );
    }
    Ok(())
}

fn validate_inline_permission_set(
    plugin: &str,
    operation: &CanonicalOperationManifest,
    permission_set: &PermissionSet,
) -> Result<()> {
    if permission_set.description.trim().is_empty() {
        return invalid_operation(
            plugin,
            operation,
            "permissions",
            &format!("permission '{}' must have a description", permission_set.id),
        );
    }
    let platforms = &permission_set.platforms;
    if platforms.aws.as_ref().is_none_or(Vec::is_empty)
        && platforms.gcp.as_ref().is_none_or(Vec::is_empty)
        && platforms.azure.as_ref().is_none_or(Vec::is_empty)
    {
        return invalid_operation(
            plugin,
            operation,
            "permissions",
            &format!(
                "permission '{}' must declare a platform grant",
                permission_set.id
            ),
        );
    }

    for permission in platforms.aws.iter().flatten() {
        validate_grant_values(
            plugin,
            operation,
            &permission_set.id,
            "AWS actions",
            &[permission.grant.actions.as_deref()],
        )?;
        if permission.binding.is_empty() {
            return invalid_permission_binding(plugin, operation, &permission_set.id, "AWS");
        }
        for binding in [
            permission.binding.stack.as_ref(),
            permission.binding.resource.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if binding.resources.is_empty()
                || binding
                    .resources
                    .iter()
                    .any(|resource| resource.trim().is_empty())
            {
                return invalid_operation(
                    plugin,
                    operation,
                    "permissions",
                    &format!(
                        "permission '{}' has an empty AWS resource binding",
                        permission_set.id
                    ),
                );
            }
        }
    }
    for permission in platforms.gcp.iter().flatten() {
        validate_grant_values(
            plugin,
            operation,
            &permission_set.id,
            "GCP grants",
            &[
                permission.grant.permissions.as_deref(),
                permission.grant.residual_permissions.as_deref(),
                permission.grant.predefined_roles.as_deref(),
            ],
        )?;
        if permission.binding.is_empty() {
            return invalid_permission_binding(plugin, operation, &permission_set.id, "GCP");
        }
        for binding in [
            permission.binding.stack.as_ref(),
            permission.binding.resource.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if binding.scope.trim().is_empty() {
                return invalid_operation(
                    plugin,
                    operation,
                    "permissions",
                    &format!("permission '{}' has an empty GCP scope", permission_set.id),
                );
            }
        }
    }
    for permission in platforms.azure.iter().flatten() {
        validate_grant_values(
            plugin,
            operation,
            &permission_set.id,
            "Azure grants",
            &[
                permission.grant.actions.as_deref(),
                permission.grant.data_actions.as_deref(),
                permission.grant.predefined_roles.as_deref(),
            ],
        )?;
        if permission.binding.is_empty() {
            return invalid_permission_binding(plugin, operation, &permission_set.id, "Azure");
        }
        for binding in [
            permission.binding.stack.as_ref(),
            permission.binding.resource.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if binding.scope.trim().is_empty() {
                return invalid_operation(
                    plugin,
                    operation,
                    "permissions",
                    &format!(
                        "permission '{}' has an empty Azure scope",
                        permission_set.id
                    ),
                );
            }
        }
    }
    Ok(())
}

fn validate_grant_values(
    plugin: &str,
    operation: &CanonicalOperationManifest,
    permission_id: &str,
    label: &str,
    groups: &[Option<&[String]>],
) -> Result<()> {
    let mut has_value = false;
    for values in groups.iter().filter_map(|values| *values) {
        if values.is_empty() || values.iter().any(|value| value.trim().is_empty()) {
            return invalid_operation(
                plugin,
                operation,
                "permissions",
                &format!("permission '{permission_id}' must declare non-empty {label}"),
            );
        }
        has_value = true;
    }
    if !has_value {
        return invalid_operation(
            plugin,
            operation,
            "permissions",
            &format!("permission '{permission_id}' must declare non-empty {label}"),
        );
    }
    Ok(())
}

fn invalid_permission_binding<T>(
    plugin: &str,
    operation: &CanonicalOperationManifest,
    permission_id: &str,
    platform: &str,
) -> Result<T> {
    invalid_operation(
        plugin,
        operation,
        "permissions",
        &format!("permission '{permission_id}' must declare a {platform} binding"),
    )
}

fn invalid_operation<T>(
    plugin: &str,
    operation: &CanonicalOperationManifest,
    field: &str,
    reason: &str,
) -> Result<T> {
    Err(AlienError::new(ErrorData::OperationContractInvalid {
        plugin: plugin.to_string(),
        operation: operation.name.clone(),
        field: field.to_string(),
        reason: reason.to_string(),
    }))
}

fn require_non_empty(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(AlienError::new(ErrorData::FieldEmpty {
            field: field.to_string(),
        }));
    }
    Ok(())
}

fn valid_binary_entry(value: &str) -> bool {
    value.len() <= 255
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn reject_reserved_binary_entry(value: &str, field: &str) -> Result<()> {
    if value == MANIFEST_FILENAME {
        return Err(AlienError::new(ErrorData::ManifestInvalid {
            reason: format!(
                "{field} must not use the reserved manifest filename '{MANIFEST_FILENAME}'"
            ),
        }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_manifest_matches_shared_cross_language_acceptance_vectors() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/canonical-manifest-parity.json"
        ))
        .expect("shared parity vectors must be JSON");

        for case in cases.as_array().expect("parity vectors must be an array") {
            let name = case["name"].as_str().expect("case must have a name");
            let accepted = case["accepted"]
                .as_bool()
                .expect("case must declare acceptance");
            let bytes = serde_json::to_vec(&case["manifest"]).expect("serialize case manifest");
            assert_eq!(
                CanonicalPluginManifest::parse_and_validate(&bytes).is_ok(),
                accepted,
                "Rust canonical manifest parser disagrees with shared case '{name}'",
            );
        }
    }

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
        let manifest = CanonicalPluginManifest::parse_and_validate(manifest_json("").as_bytes())
            .expect("minimal manifest should parse and validate");
        assert_eq!(manifest.name, "postgres");
        assert_eq!(manifest.tier, RiskTier::ReadOnly);
        assert!(manifest.operations.is_empty());
    }

    #[test]
    #[allow(deprecated)]
    fn rejects_the_reserved_manifest_filename_as_a_binary_entry() {
        let json = manifest_json("").replace("postgres-linux-amd64", MANIFEST_FILENAME);

        for result in [
            PluginManifest::parse_and_validate(json.as_bytes()).map(|_| ()),
            CanonicalPluginManifest::parse_and_validate(json.as_bytes()).map(|_| ()),
        ] {
            let error = result.expect_err("metadata.json must remain reserved for the manifest");
            assert!(error.to_string().contains("reserved manifest filename"));
        }
    }

    #[test]
    fn rejects_duplicate_operation_names() {
        let json = manifest_json(r#"{"name": "vacuum"}, {"name": "vacuum", "tier": "mutating"}"#);
        let err = CanonicalPluginManifest::parse_and_validate(json.as_bytes())
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
        let err = CanonicalPluginManifest::parse_and_validate(json.as_bytes())
            .expect_err("poll operation must be read-only");
        assert!(err.to_string().contains("pollOperation"));
    }

    #[test]
    fn accepts_verification_polling_a_read_only_sibling() {
        let json = manifest_json(
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
        );
        CanonicalPluginManifest::parse_and_validate(json.as_bytes())
            .expect("verification polling a read-only sibling operation should validate");
    }

    #[test]
    fn unspecified_tier_defaults_to_destructive() {
        assert_eq!(RiskTier::default(), RiskTier::Destructive);
    }

    #[test]
    fn operation_tier_falls_back_to_plugin_default() {
        let op = CanonicalOperationManifest {
            kubernetes_permissions: None,
            name: "vacuum".into(),
            tier: None,
            description: None,
            input_schema: None,
            output_schema: None,
            required_permissions: vec![],
            timeout_seconds: None,
            retries: None,
            verification: None,
            sensitive_output: SensitiveOutputPolicy::None,
        };
        assert_eq!(op.effective_tier(RiskTier::Mutating), RiskTier::Mutating);
    }

    #[test]
    fn rejects_an_empty_plugin_name() {
        let json = r#"{
            "name": "",
            "version": "1.0.0",
            "binaries": { "amd64": "postgres-linux-amd64" },
            "operations": []
        }"#;
        let err = CanonicalPluginManifest::parse_and_validate(json.as_bytes())
            .expect_err("empty plugin name must fail validation");
        assert!(err.to_string().contains("name"));
    }

    #[test]
    fn rejects_a_whitespace_only_version() {
        let json = r#"{
            "name": "postgres",
            "version": "   ",
            "binaries": { "amd64": "postgres-linux-amd64" },
            "operations": []
        }"#;
        let err = CanonicalPluginManifest::parse_and_validate(json.as_bytes())
            .expect_err("whitespace-only version must fail validation");
        assert!(err.to_string().contains("version"));
    }

    #[test]
    fn rejects_a_manifest_with_no_binaries() {
        let json = r#"{
            "name": "postgres",
            "version": "1.0.0",
            "binaries": {},
            "operations": []
        }"#;
        let err = CanonicalPluginManifest::parse_and_validate(json.as_bytes())
            .expect_err("empty binaries map must fail validation");
        assert!(err.to_string().contains("binaries"));
    }

    #[test]
    fn rejects_an_empty_binary_entry() {
        let json = r#"{
            "name": "postgres",
            "version": "1.0.0",
            "binaries": { "amd64": "" },
            "operations": []
        }"#;
        let err = CanonicalPluginManifest::parse_and_validate(json.as_bytes())
            .expect_err("empty binary entry must fail validation");
        assert!(err.to_string().contains("binaries.amd64"));
    }

    #[test]
    fn rejects_an_empty_operation_name() {
        let json = manifest_json(r#"{"name": ""}"#);
        let err = CanonicalPluginManifest::parse_and_validate(json.as_bytes())
            .expect_err("empty operation name must fail validation");
        assert!(err.to_string().contains("operations[0].name"));
    }

    #[test]
    fn legacy_schema_and_permission_keys_normalize_to_the_canonical_contract() {
        let json = manifest_json(
            r#"{
                "name": "vacuum",
                "tier": "mutating",
                "paramsSchema": {"type": "object"},
                "outputSchema": {"type": "object"},
                "requiredPermissions": ["postgres/management"],
                "timeoutSeconds": 30,
                "retries": {"maxAttempts": 2, "intervalSeconds": 1}
            }"#,
        );
        let manifest = CanonicalPluginManifest::parse_and_validate(json.as_bytes())
            .expect("legacy keys must remain readable");
        let encoded = serde_json::to_value(manifest).expect("canonical manifest serializes");
        let operation = &encoded["operations"][0];

        assert_eq!(operation["inputSchema"]["type"], "object");
        assert_eq!(operation["outputSchema"]["type"], "object");
        assert_eq!(
            operation["permissions"],
            serde_json::json!(["postgres/management"])
        );
        assert!(operation.get("paramsSchema").is_none());
        assert!(operation.get("requiredPermissions").is_none());
    }

    #[test]
    fn canonical_contract_accepts_a_valid_inline_permission_set() {
        let json = manifest_json(
            r#"{
                "name": "inspect",
                "permissions": [{
                    "id": "operations/example/inspect",
                    "description": "Inspect the selected example.",
                    "platforms": {"aws": [{
                        "grant": {"actions": ["example:Get"]},
                        "binding": {"resource": {"resources": ["arn:aws:example:*:*:item/*"]}}
                    }]}
                }]
            }"#,
        );
        let manifest = CanonicalPluginManifest::parse_and_validate(json.as_bytes())
            .expect("valid inline permissions must remain compatible");

        assert_eq!(
            manifest.operations[0].permission_ids().collect::<Vec<_>>(),
            vec!["operations/example/inspect"]
        );
    }

    #[test]
    fn canonical_and_legacy_schema_keys_cannot_conflict() {
        let json = manifest_json(
            r#"{
                "name": "inspect",
                "inputSchema": {"type": "object"},
                "paramsSchema": {"type": "string"}
            }"#,
        );
        let error = CanonicalPluginManifest::parse_and_validate(json.as_bytes())
            .expect_err("duplicate schema keys must fail instead of picking one");

        assert!(error.to_string().contains("duplicate field"));
    }

    #[test]
    fn canonical_and_legacy_permission_keys_cannot_conflict() {
        let json = manifest_json(
            r#"{
                "name": "inspect",
                "permissions": ["example/read"],
                "requiredPermissions": ["example/legacy-read"]
            }"#,
        );
        let error = CanonicalPluginManifest::parse_and_validate(json.as_bytes())
            .expect_err("duplicate permission keys must fail instead of merging grants");

        assert!(error.to_string().contains("duplicate field"));
    }

    #[test]
    fn execution_retries_require_a_positive_timeout() {
        let json = manifest_json(
            r#"{
                "name": "inspect",
                "retries": {"maxAttempts": 2, "intervalSeconds": 1}
            }"#,
        );
        let error = CanonicalPluginManifest::parse_and_validate(json.as_bytes())
            .expect_err("retrying without a timeout must fail");

        assert!(error.to_string().contains("requires timeoutSeconds"));
    }

    #[test]
    fn retry_attempts_must_be_positive() {
        let json = manifest_json(
            r#"{
                "name": "inspect",
                "timeoutSeconds": 10,
                "retries": {"maxAttempts": 0, "intervalSeconds": 1}
            }"#,
        );
        let error = CanonicalPluginManifest::parse_and_validate(json.as_bytes())
            .expect_err("zero retry attempts must fail");

        assert!(error.to_string().contains("maxAttempts"));
    }

    #[test]
    fn timeout_must_be_positive() {
        let json = manifest_json(r#"{"name": "inspect", "timeoutSeconds": 0}"#);
        let error = CanonicalPluginManifest::parse_and_validate(json.as_bytes())
            .expect_err("zero timeout must fail");

        assert!(error.to_string().contains("timeoutSeconds"));
        assert!(error.to_string().contains("greater than zero"));
    }

    #[test]
    fn unknown_risk_tiers_fail_during_contract_decoding() {
        let json = manifest_json(r#"{"name": "inspect", "tier": "unsafe"}"#);
        let error = CanonicalPluginManifest::parse_and_validate(json.as_bytes())
            .expect_err("unknown risk tiers must fail closed");

        assert!(error.to_string().contains("unknown variant"));
        assert!(error.to_string().contains("unsafe"));
    }

    #[test]
    fn read_only_operations_cannot_declare_post_write_verification() {
        let json = manifest_json(
            r#"
            {"name": "status", "tier": "read-only"},
            {"name": "inspect", "tier": "read-only", "verification": {
                "changes": "nothing",
                "pollOperation": "status",
                "successField": "status",
                "successValue": "ready",
                "timeoutSeconds": 10
            }}
            "#,
        );
        let error = CanonicalPluginManifest::parse_and_validate(json.as_bytes())
            .expect_err("read-only verification is an invalid policy combination");

        assert!(error.to_string().contains("read-only operations"));
    }

    #[test]
    fn redacted_output_fields_require_an_output_schema() {
        let json = manifest_json(
            r#"{
                "name": "credentials",
                "sensitiveOutput": {"kind": "redact", "fields": ["password"]}
            }"#,
        );
        let error = CanonicalPluginManifest::parse_and_validate(json.as_bytes())
            .expect_err("redaction paths without an output contract must fail");

        assert!(error.to_string().contains("outputSchema"));
    }

    #[test]
    fn redacted_output_fields_must_resolve_against_the_output_schema() {
        let json = manifest_json(
            r##"{
                "name": "credentials",
                "outputSchema": {
                    "definitions": {
                        "Credential": {
                            "type": "object",
                            "properties": {"password": {"type": "string"}}
                        }
                    },
                    "type": "object",
                    "properties": {
                        "rows": {
                            "type": "array",
                            "items": {"$ref": "#/definitions/Credential"}
                        }
                    }
                },
                "sensitiveOutput": {"kind": "redact", "fields": ["rows.password"]}
            }"##,
        );
        CanonicalPluginManifest::parse_and_validate(json.as_bytes())
            .expect("a nested array/ref redaction path declared by outputSchema should validate");

        let nested_pointer = manifest_json(
            r##"{
                "name": "credentials",
                "outputSchema": {
                    "definitions": {
                        "Envelope": {
                            "type": "object",
                            "properties": {
                                "credential/value~raw": {
                                    "type": "object",
                                    "properties": {"password": {"type": "string"}}
                                }
                            }
                        }
                    },
                    "type": "object",
                    "properties": {
                        "result": {
                            "$ref": "#/definitions/Envelope/properties/credential~1value~0raw"
                        }
                    }
                },
                "sensitiveOutput": {"kind": "redact", "fields": ["result.password"]}
            }"##,
        );
        CanonicalPluginManifest::parse_and_validate(nested_pointer.as_bytes())
            .expect("nested RFC 6901 local refs should resolve escaped path segments");
        let invalid_pointer =
            nested_pointer.replacen("credential~1value~0raw", "credential~1value~0missing", 1);
        let error = CanonicalPluginManifest::parse_and_validate(invalid_pointer.as_bytes())
            .expect_err("a nested RFC 6901 pointer must resolve to an existing schema");
        assert!(error.to_string().contains("result.password"), "{error}");

        let json = manifest_json(
            r#"{
                "name": "credentials",
                "outputSchema": {
                    "type": "object",
                    "properties": {"username": {"type": "string"}}
                },
                "sensitiveOutput": {"kind": "redact", "fields": ["password"]}
            }"#,
        );
        let error = CanonicalPluginManifest::parse_and_validate(json.as_bytes())
            .expect_err("a nonexistent redaction path must fail closed");
        assert!(error.to_string().contains("path 'password'"));
    }

    #[test]
    fn verification_paths_must_resolve_against_the_relevant_schemas() {
        let valid = manifest_json(
            r#"
            {"name": "get-pod", "tier": "read-only",
             "inputSchema": {"type": "object", "properties": {"podName": {"type": "string"}}},
             "outputSchema": {"type": "object", "properties": {
                 "state": {"type": "object", "properties": {"status": {"type": "string"}}}
             }}},
            {"name": "restart-pod", "tier": "mutating",
             "outputSchema": {"type": "object", "properties": {
                 "pod": {"type": "object", "properties": {"name": {"type": "string"}}}
             }},
             "verification": {
                 "changes": "the pod restarts",
                 "pollOperation": "get-pod",
                 "pollParamsFromResult": {"podName": "pod.name"},
                 "successField": "state.status",
                 "successValue": "Running",
                 "timeoutSeconds": 60
             }}
            "#,
        );
        CanonicalPluginManifest::parse_and_validate(valid.as_bytes())
            .expect("schema-resolved verification paths should validate");

        for (needle, invalid) in [
            (
                "missingSource",
                valid.replace("\"pod.name\"", "\"pod.missingSource\""),
            ),
            (
                "missingParam",
                valid.replace(
                    "\"podName\": \"pod.name\"",
                    "\"missingParam\": \"pod.name\"",
                ),
            ),
            (
                "missingStatus",
                valid.replace("\"state.status\"", "\"state.missingStatus\""),
            ),
        ] {
            let error = CanonicalPluginManifest::parse_and_validate(invalid.as_bytes())
                .expect_err("an unverifiable schema path must fail closed");
            assert!(error.to_string().contains(needle), "{error}");
        }

        let array_result = valid.replace(
            r#""outputSchema": {"type": "object", "properties": {
                 "state": {"type": "object", "properties": {"status": {"type": "string"}}}
             }}"#,
            r#""outputSchema": {"type": "array", "items": {"type": "object", "properties": {
                 "state": {"type": "object", "properties": {"status": {"type": "string"}}}
             }}}"#,
        );
        let error = CanonicalPluginManifest::parse_and_validate(array_result.as_bytes())
            .expect_err("verification must not treat array items as the result object");
        assert!(error.to_string().contains("state.status"), "{error}");
    }

    #[test]
    fn permission_ids_are_plugin_wide_and_conflicting_definitions_are_rejected() {
        let permission = r#"{
            "id": "operations/example/read",
            "description": "Read the selected example.",
            "platforms": {"aws": [{
                "grant": {"actions": ["example:Get"]},
                "binding": {"resource": {"resources": ["arn:aws:example:*:*:item/*"]}}
            }]}
        }"#;
        let identical = manifest_json(&format!(
            r#"{{"name": "one", "permissions": [{permission}]}},
                {{"name": "two", "permissions": [{permission}]}}"#
        ));
        CanonicalPluginManifest::parse_and_validate(identical.as_bytes())
            .expect("identical permission definitions may be shared across operations");

        let conflicting = identical.replacen("example:Get\"]", "example:List\"]", 1);
        let error = CanonicalPluginManifest::parse_and_validate(conflicting.as_bytes())
            .expect_err("different definitions with one permission ID must fail closed");
        assert!(error.to_string().contains("conflicting definitions"));

        let named_and_inline = manifest_json(&format!(
            r#"{{"name": "one", "permissions": ["operations/example/read"]}},
                {{"name": "two", "permissions": [{permission}]}}"#
        ));
        let error = CanonicalPluginManifest::parse_and_validate(named_and_inline.as_bytes())
            .expect_err("named and inline uses of one permission ID conflict");
        assert!(error.to_string().contains("conflicting definitions"));
    }

    #[test]
    fn inline_aws_permissions_require_a_binding() {
        let json = manifest_json(
            r#"{
                "name": "inspect",
                "permissions": [{
                    "id": "operations/example/inspect",
                    "description": "Inspect the selected example.",
                    "platforms": {"aws": [{
                        "grant": {"actions": ["example:Get"]},
                        "binding": {}
                    }]}
                }]
            }"#,
        );
        let error = CanonicalPluginManifest::parse_and_validate(json.as_bytes())
            .expect_err("unbound inline grants must fail");

        assert!(error.to_string().contains("AWS binding"));
    }

    #[allow(deprecated)]
    #[test]
    fn legacy_struct_literals_remain_source_compatible_and_convert_explicitly() {
        let legacy = super::OperationManifest {
            kubernetes_permissions: None,
            name: "inspect".to_string(),
            tier: Some(RiskTier::ReadOnly),
            description: None,
            params_schema: None,
            required_permissions: vec!["example/read".to_string()],
            timeout_seconds: None,
            retries: None,
            verification: None,
            sensitive_output: SensitiveOutputPolicy::None,
        };

        let canonical = legacy.into_canonical();
        assert_eq!(
            canonical.permission_ids().collect::<Vec<_>>(),
            vec!["example/read"]
        );
        assert!(canonical.output_schema.is_none());
    }

    #[allow(deprecated)]
    #[test]
    fn legacy_validation_keeps_pre_canonical_verification_and_redaction_semantics() {
        let json = manifest_json(
            r#"
            {"name": "status", "tier": "read-only"},
            {"name": "restart", "tier": "mutating",
             "paramsSchema": {"type": "object"},
             "verification": {
                 "changes": "the resource restarts",
                 "pollOperation": "status",
                 "pollParamsFromResult": {},
                 "successField": "state",
                 "successValue": "ready",
                 "timeoutSeconds": 30
             },
             "sensitiveOutput": {"kind": "redact", "fields": ["token"]}}
            "#,
        );

        let legacy = super::PluginManifest::parse_and_validate(json.as_bytes())
            .expect("the exact legacy validator accepted schemas without outputSchema");
        assert!(
            legacy.into_canonical().validate().is_err(),
            "strict canonical validation remains opt-in through explicit conversion"
        );
    }
}
