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
use schemars::schema::RootSchema;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};
use crate::kubernetes::KubernetesPermissions;
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

impl SensitiveOutputPolicy {
    fn is_none(&self) -> bool {
        matches!(self, Self::None)
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
    /// tier when omitted (resolved through [`OperationManifest::effective_tier`]).
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
    pub params_schema: Option<RootSchema>,
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

impl OperationManifest {
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

    /// The bundle entry for `arch`.
    pub fn binary_for(&self, arch: Arch) -> Result<&str> {
        self.binaries.get(&arch).map(String::as_str).ok_or_else(|| {
            let available = self
                .binaries
                .keys()
                .map(Arch::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            AlienError::new(ErrorData::ArchUnsupported {
                plugin: self.name.clone(),
                arch: arch.as_str().to_string(),
                available,
            })
        })
    }

    /// The effective risk tier for a named operation.
    pub fn tier_for(&self, operation: &str) -> Result<RiskTier> {
        self.operation(operation)
            .map(|operation| operation.effective_tier(self.tier))
            .ok_or_else(|| {
                AlienError::new(ErrorData::OperationUnknown {
                    plugin: self.name.clone(),
                    operation: operation.to_string(),
                })
            })
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
        for (arch, entry) in &self.binaries {
            require_non_empty(entry, &format!("binaries.{}", arch.as_str()))?;
        }

        let mut seen = BTreeSet::new();
        for (index, operation) in self.operations.iter().enumerate() {
            require_non_empty(&operation.name, &format!("operations[{index}].name"))?;
            validate_operation_contract(&self.name, self.tier, operation)?;
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

fn validate_operation_contract(
    plugin: &str,
    plugin_tier: RiskTier,
    operation: &OperationManifest,
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
        if operation.output_schema.is_none() {
            return invalid_operation(
                plugin,
                operation,
                "outputSchema",
                "is required when sensitive output fields are redacted",
            );
        }
    }
    Ok(())
}

fn validate_retry_policy(
    plugin: &str,
    operation: &OperationManifest,
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
    operation: &OperationManifest,
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
    operation: &OperationManifest,
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
    operation: &OperationManifest,
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
    operation: &OperationManifest,
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
            kubernetes_permissions: None,
            name: "vacuum".into(),
            tier: None,
            description: None,
            params_schema: None,
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
        let err = PluginManifest::parse_and_validate(json.as_bytes())
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
        let err = PluginManifest::parse_and_validate(json.as_bytes())
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
        let err = PluginManifest::parse_and_validate(json.as_bytes())
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
        let err = PluginManifest::parse_and_validate(json.as_bytes())
            .expect_err("empty binary entry must fail validation");
        assert!(err.to_string().contains("binaries.amd64"));
    }

    #[test]
    fn rejects_an_empty_operation_name() {
        let json = manifest_json(r#"{"name": ""}"#);
        let err = PluginManifest::parse_and_validate(json.as_bytes())
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
        let manifest = PluginManifest::parse_and_validate(json.as_bytes())
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
        let manifest = PluginManifest::parse_and_validate(json.as_bytes())
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
        let error = PluginManifest::parse_and_validate(json.as_bytes())
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
        let error = PluginManifest::parse_and_validate(json.as_bytes())
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
        let error = PluginManifest::parse_and_validate(json.as_bytes())
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
        let error = PluginManifest::parse_and_validate(json.as_bytes())
            .expect_err("zero retry attempts must fail");

        assert!(error.to_string().contains("maxAttempts"));
    }

    #[test]
    fn timeout_must_be_positive() {
        let json = manifest_json(r#"{"name": "inspect", "timeoutSeconds": 0}"#);
        let error = PluginManifest::parse_and_validate(json.as_bytes())
            .expect_err("zero timeout must fail");

        assert!(error.to_string().contains("timeoutSeconds"));
        assert!(error.to_string().contains("greater than zero"));
    }

    #[test]
    fn unknown_risk_tiers_fail_during_contract_decoding() {
        let json = manifest_json(r#"{"name": "inspect", "tier": "unsafe"}"#);
        let error = PluginManifest::parse_and_validate(json.as_bytes())
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
        let error = PluginManifest::parse_and_validate(json.as_bytes())
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
        let error = PluginManifest::parse_and_validate(json.as_bytes())
            .expect_err("redaction paths without an output contract must fail");

        assert!(error.to_string().contains("outputSchema"));
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
        let error = PluginManifest::parse_and_validate(json.as_bytes())
            .expect_err("unbound inline grants must fail");

        assert!(error.to_string().contains("AWS binding"));
    }
}
