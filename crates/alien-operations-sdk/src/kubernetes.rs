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
            if !rule.api_group.is_empty() && !valid_api_group(&rule.api_group) {
                return invalid("Kubernetes API groups must be DNS subdomains");
            }
            let mut parts = rule.resource.split('/');
            let resource = parts.next().unwrap_or_default();
            let subresource = parts.next();
            if !valid_resource(resource) {
                return invalid("Kubernetes resources must be DNS-1035 labels");
            }
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
                if !valid_token(name) {
                    return invalid("Kubernetes resourceNames must be concrete names");
                }
            }
            validate_label(&rule.reason, "reason")?;
        }
        Ok(())
    }
}

fn valid_api_group(value: &str) -> bool {
    // Kubernetes DNS-1123 subdomain validation limits the whole name to 253
    // bytes and checks each label's syntax. Single-label builtins are valid.
    value.len() <= 253 && value.split('.').all(valid_token)
}

fn valid_resource(value: &str) -> bool {
    value.len() <= 63
        && valid_token(value)
        && !value.contains('.')
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
}

fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'.')
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
        // YAML parsers recognize Unicode line/paragraph separators as line
        // breaks too, although Rust does not classify them as control characters.
        || value.chars().any(|character| {
            character.is_control() || matches!(character, '\u{2028}' | '\u{2029}')
        })
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

    use crate::{KubernetesOperationPermissions, PluginManifest, RiskTier};

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
    fn api_groups_use_dns_subdomain_syntax() {
        for group in [
            "".to_owned(),
            "apps".to_owned(),
            "batch".to_owned(),
            "networking.k8s.io".to_owned(),
            "v2.custom-api.example.com".to_owned(),
            "1.example.com".to_owned(),
            "a".repeat(253),
        ] {
            let mut input = manifest();
            input["operations"][0]["kubernetesPermissions"]["rules"][0]["apiGroup"] = json!(group);
            assert!(
                PluginManifest::parse_and_validate(input.to_string().as_bytes()).is_ok(),
                "rejected valid API group: {group}"
            );
        }
        for group in [
            "example..com".to_owned(),
            "example.-com".to_owned(),
            "example-.com".to_owned(),
            ".example.com".to_owned(),
            "example.com.".to_owned(),
            "example_com".to_owned(),
            "Example.com".to_owned(),
            "example/com".to_owned(),
            "a".repeat(254),
        ] {
            let mut input = manifest();
            input["operations"][0]["kubernetesPermissions"]["rules"][0]["apiGroup"] = json!(group);
            assert!(
                PluginManifest::parse_and_validate(input.to_string().as_bytes()).is_err(),
                "accepted invalid API group: {group}"
            );
        }
    }

    #[test]
    fn resources_use_dns1035_labels_with_only_supported_subresources() {
        for resource in [
            "widgets".to_owned(),
            "widget-v2s".to_owned(),
            "pods/log".to_owned(),
            "deployments/scale".to_owned(),
            "widgets/status".to_owned(),
            "a".repeat(63),
        ] {
            let mut input = manifest();
            input["operations"][0]["kubernetesPermissions"]["rules"][0]["resource"] =
                json!(resource);
            assert!(
                PluginManifest::parse_and_validate(input.to_string().as_bytes()).is_ok(),
                "rejected valid resource: {resource}"
            );
        }
        for resource in [
            "".to_owned(),
            "1widgets".to_owned(),
            "custom.widgets".to_owned(),
            "Widgets".to_owned(),
            "-widgets".to_owned(),
            "widgets-".to_owned(),
            "widgets_v2".to_owned(),
            "pods/".to_owned(),
            "/status".to_owned(),
            "pods//log".to_owned(),
            "pods/status/scale".to_owned(),
            "a".repeat(64),
        ] {
            let mut input = manifest();
            input["operations"][0]["kubernetesPermissions"]["rules"][0]["resource"] =
                json!(resource);
            assert!(
                PluginManifest::parse_and_validate(input.to_string().as_bytes()).is_err(),
                "accepted invalid resource: {resource}"
            );
        }
    }

    #[test]
    fn resource_names_keep_the_existing_concrete_path_token_contract() {
        let mut input = manifest();
        input["operations"][0]["kubernetesPermissions"]["rules"][0]["resourceNames"] =
            json!(["1.widget-v2", "widget..name"]);
        assert!(PluginManifest::parse_and_validate(input.to_string().as_bytes()).is_ok());
    }

    #[test]
    fn attribution_rejects_all_yaml_line_breaks() {
        for line_break in ['\n', '\r', '\u{85}', '\u{2028}', '\u{2029}'] {
            let text = format!("Inspect{line_break}another line");
            for field in ["plugin", "operation", "reason"] {
                let parsed =
                    PluginManifest::parse_and_validate(manifest().to_string().as_bytes()).unwrap();
                let mut declaration = KubernetesOperationPermissions {
                    plugin: parsed.name,
                    operation: parsed.operations[0].name.clone(),
                    tier: RiskTier::ReadOnly,
                    permissions: parsed.operations[0].kubernetes_permissions.clone().unwrap(),
                };
                match field {
                    "plugin" => declaration.plugin = text.clone(),
                    "operation" => declaration.operation = text.clone(),
                    _ => declaration.permissions.rules[0].reason = text.clone(),
                }
                assert!(
                    declaration.validate().is_err(),
                    "accepted {line_break:?} in {field}"
                );
            }
            let mut input = manifest();
            input["operations"][0]["kubernetesPermissions"]["rules"][0]["reason"] = json!(text);
            assert!(PluginManifest::parse_and_validate(input.to_string().as_bytes()).is_err());
        }
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
