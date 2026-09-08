//! Explicit Kubernetes requirements for an operation. Installers apply these
//! rules only for enabled plugins and within the selected installation scope.
//!
//! Version 1 supports reads, pod deletion, and workload scale patches. It does
//! not support Secrets, impersonation, RBAC changes, remote execution, or
//! workload creation (which could mount Secrets or assume a service account).
//!
//! Add this field to an operation in `metadata.json`:
//! ```json
//! "kubernetesPermissions": {
//!   "schemaVersion": 1,
//!   "rules": [{
//!     "apiGroup": "networking.k8s.io",
//!     "resource": "ingresses",
//!     "verbs": ["get"],
//!     "resourceNames": ["customer-ingress"],
//!     "reason": "Inspect the selected ingress routing configuration"
//!   }]
//! }
//! ```
//! No namespace, label selector, non-resource URL, wildcard, or role binding
//! can be declared here. The installation's chosen namespace/cluster scope
//! and diagnostics/remediation ceiling remain authoritative. Re-render and
//! reapply the setup-owned Role/ClusterRole when enabled plugins change.

use alien_error::AlienError;
use serde::{Deserialize, Serialize};

use crate::{ErrorData, Result, RiskTier};

/// Versioned requirements declared in an operation's `kubernetesPermissions`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KubernetesPermissions {
    /// Must be 1. Unknown versions are rejected, never silently ignored.
    pub schema_version: u32,
    /// Explicit rules; each rule addresses one API group and resource.
    pub rules: Vec<KubernetesPermissionRule>,
}

/// One Kubernetes API requirement, inherited by the installation's Role or
/// ClusterRole. An empty API group denotes the Kubernetes core API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KubernetesPermissionRule {
    pub api_group: String,
    /// Plural resource, optionally including a supported subresource.
    pub resource: String,
    /// Concrete verbs; no wildcards or privilege-management verbs.
    pub verbs: Vec<String>,
    /// Empty means all names within the installation scope. Kubernetes requires
    /// a matching metadata.name field selector for list/watch with named access.
    #[serde(default)]
    pub resource_names: Vec<String>,
    /// Human-readable explanation emitted alongside the generated RBAC rule.
    pub reason: String,
}

/// An enabled operation's requirements and their attribution, supplied by the
/// installer after resolving the operation's effective risk tier.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KubernetesOperationPermissions {
    pub plugin: String,
    pub operation: String,
    pub tier: RiskTier,
    pub permissions: KubernetesPermissions,
}

impl KubernetesOperationPermissions {
    pub fn validate(&self) -> Result<()> {
        validate_label(&self.plugin, "plugin")?;
        validate_label(&self.operation, "operation")?;
        self.permissions.validate(self.tier)
    }
}

impl KubernetesPermissions {
    /// Validate before emitting permissions, even when constructed directly in
    /// Rust rather than loaded through `PluginManifest::parse_and_validate`.
    pub fn validate(&self, tier: RiskTier) -> Result<()> {
        if self.schema_version != 1 {
            return invalid("unsupported Kubernetes permission schemaVersion; expected 1");
        }
        if self.rules.is_empty() {
            return invalid("Kubernetes permission rules must not be empty");
        }
        for rule in &self.rules {
            if (!rule.api_group.is_empty() && !valid_token(&rule.api_group, false))
                || !valid_token(&rule.resource, true)
            {
                return invalid("Kubernetes API groups and resources must be concrete tokens");
            }
            let mut parts = rule.resource.split('/');
            let resource = parts.next().unwrap_or_default();
            let subresource = parts.next();
            if resource == "secrets"
                || parts.next().is_some()
                || subresource.is_some_and(|sub| !matches!(sub, "log" | "scale" | "status"))
            {
                return invalid("Kubernetes Secrets and privileged subresources are not supported");
            }
            if rule.verbs.is_empty() {
                return invalid("Kubernetes permission verbs must not be empty");
            }
            for verb in &rule.verbs {
                let read = matches!(verb.as_str(), "get" | "list" | "watch");
                let remediation =
                    (rule.api_group.is_empty() && rule.resource == "pods" && verb == "delete")
                        || (rule.api_group == "apps"
                            && matches!(
                                rule.resource.as_str(),
                                "deployments/scale" | "statefulsets/scale" | "replicasets/scale"
                            )
                            && verb == "patch");
                if !read && (!remediation || tier == RiskTier::ReadOnly) {
                    return invalid(
                        "unsupported Kubernetes verb/resource or write in read-only operation",
                    );
                }
            }
            for name in &rule.resource_names {
                // Concrete path-segment names only; reject Helm expressions too.
                if !valid_token(name, false) {
                    return invalid("Kubernetes resourceNames must be concrete names");
                }
            }
            validate_label(&rule.reason, "reason")?;
        }
        Ok(())
    }
}

fn valid_token(value: &str, allow_slash: bool) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'.')
                || (allow_slash && byte == b'/')
        })
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value
            .bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
}

fn validate_label(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty()
        || value.len() > 1024
        || value.chars().any(char::is_control)
        || value.contains("{{")
        || value.contains("}}")
    {
        return invalid(&format!("Kubernetes permission {field} must be nonempty single-line text without template expressions"));
    }
    Ok(())
}

fn invalid<T>(reason: &str) -> Result<T> {
    Err(AlienError::new(ErrorData::ManifestInvalid {
        reason: reason.to_owned(),
    }))
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use crate::PluginManifest;

    fn manifest() -> Value {
        json!({
            "name": "inspector", "version": "1", "tier": "read-only",
            "binaries": {"amd64": "inspector"},
            "operations": [{"name": "inspect", "kubernetesPermissions": {
                "schemaVersion": 1,
                "rules": [{"apiGroup": "example.com", "resource": "widgets",
                    "verbs": ["get", "list", "watch"], "resourceNames": ["sample"],
                    "reason": "Inspect the selected widget"}]
            }}]
        })
    }

    #[test]
    fn declaration_round_trips_through_the_public_manifest() {
        let value = manifest();
        let parsed = PluginManifest::parse_and_validate(value.to_string().as_bytes()).unwrap();
        let actual = serde_json::to_value(parsed).unwrap();
        assert_eq!(
            actual["operations"][0]["kubernetesPermissions"],
            value["operations"][0]["kubernetesPermissions"]
        );
    }

    #[test]
    fn unsafe_or_unknown_declarations_fail_closed() {
        for (field, value) in [
            ("verbs", json!(["*"])),
            ("verbs", json!(["escalate"])),
            ("verbs", json!(["impersonate"])),
            ("verbs", json!(["create"])),
            ("verbs", json!(["deletecollection"])),
            ("verbs", json!([])),
            ("resource", json!("secrets")),
            ("resource", json!("secrets/status")),
            ("resource", json!("pods/exec")),
            ("resource", json!("pods/attach")),
            ("resource", json!("serviceaccounts/token")),
            ("resource", json!("*")),
            ("apiGroup", json!("*")),
            ("resourceNames", json!(["*"])),
            ("reason", json!("{{ lookup }}")),
            ("reason", json!("line\n- injected")),
            ("namespace", json!("another-namespace")),
        ] {
            let mut input = manifest();
            input["operations"][0]["kubernetesPermissions"]["rules"][0][field] = value;
            assert!(
                PluginManifest::parse_and_validate(input.to_string().as_bytes()).is_err(),
                "accepted invalid {field}: {input}"
            );
        }
        let mut input = manifest();
        input["operations"][0]["kubernetesPermissions"]["schemaVersion"] = json!(2);
        assert!(PluginManifest::parse_and_validate(input.to_string().as_bytes()).is_err());
        input["operations"][0]["kubernetesPermissions"] = Value::Null;
        assert!(PluginManifest::parse_and_validate(input.to_string().as_bytes()).is_err());
    }

    #[test]
    fn write_requirements_cannot_hide_in_a_read_only_operation() {
        let mut input = manifest();
        input["operations"][0]["kubernetesPermissions"]["rules"][0] = json!({
            "apiGroup": "", "resource": "pods", "verbs": ["delete"], "reason": "Restart a pod"
        });
        assert!(PluginManifest::parse_and_validate(input.to_string().as_bytes()).is_err());
        input["operations"][0]["tier"] = json!("mutating");
        assert!(PluginManifest::parse_and_validate(input.to_string().as_bytes()).is_ok());
        input["operations"][0]["kubernetesPermissions"]["rules"][0]["verbs"] = json!(["create"]);
        assert!(PluginManifest::parse_and_validate(input.to_string().as_bytes()).is_err());
    }
}
