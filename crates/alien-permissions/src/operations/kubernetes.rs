use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{catalog, invalid, sorted, types::*, Result};

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

fn valid_label(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 1024
        && !value
            .chars()
            .any(|character| character.is_control() || matches!(character, '\u{2028}' | '\u{2029}'))
        && !value.contains("{{")
        && !value.contains("}}")
}

pub fn validate_attribution(attribution: Option<&Attribution>) -> Result<()> {
    if let Some(attribution) = attribution {
        for (field, value) in [
            ("plugin", &attribution.plugin),
            ("operation", &attribution.operation),
        ] {
            if !valid_label(value) {
                return invalid(format!("Kubernetes permission {field} must be nonempty single-line text without template expressions"), "kubernetes", value);
            }
        }
    }
    Ok(())
}

pub fn validate(
    permissions: &KubernetesPermissions,
    tier: &str,
    attribution: Option<&Attribution>,
) -> Result<()> {
    validate_attribution(attribution)?;
    if !matches!(tier, "read-only" | "mutating" | "destructive") {
        return invalid("unknown operation risk tier", "kubernetes", tier);
    }
    if permissions.schema_version != 1 || permissions.rules.is_empty() {
        return invalid(
            "unsupported Kubernetes permission schemaVersion or empty rules",
            "kubernetes",
            "",
        );
    }
    for rule in &permissions.rules {
        validate_grant(
            &KubernetesGrant {
                api_group: rule.api_group.clone(),
                resource: rule.resource.clone(),
                verbs: rule.verbs.clone(),
                resource_names: rule.resource_names.clone(),
                sources: vec![],
            },
            tier,
        )?;
        if !valid_label(&rule.reason) {
            return invalid("Kubernetes permission reason must be nonempty single-line text without template expressions", "kubernetes", &rule.resource);
        }
    }
    Ok(())
}

fn validate_grant(grant: &KubernetesGrant, tier: &str) -> Result<()> {
    let fail = |message| invalid(message, "kubernetes", &grant.resource);
    if !grant.api_group.is_empty()
        && (grant.api_group.len() > 253 || !grant.api_group.split('.').all(valid_token))
    {
        return fail("Kubernetes API groups must be DNS subdomains");
    }
    let mut parts = grant.resource.split('/');
    let resource = parts.next().unwrap_or_default();
    let subresource = parts.next();
    if resource.len() > 63
        || !valid_token(resource)
        || resource.contains('.')
        || !resource
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
    {
        return fail("Kubernetes resources must be DNS-1035 labels");
    }
    if resource == "secrets"
        || parts.next().is_some()
        || subresource.is_some_and(|sub| !matches!(sub, "log" | "scale" | "status"))
    {
        return fail("Kubernetes Secrets and privileged subresources are not supported");
    }
    if grant.verbs.is_empty() {
        return fail("Kubernetes permission verbs must not be empty");
    }
    for verb in &grant.verbs {
        let read = matches!(verb.as_str(), "get" | "list" | "watch");
        let remediation =
            (grant.api_group.is_empty() && grant.resource == "pods" && verb == "delete")
                || (grant.api_group == "apps"
                    && matches!(
                        grant.resource.as_str(),
                        "deployments/scale" | "statefulsets/scale" | "replicasets/scale"
                    )
                    && verb == "patch");
        if !read && (!remediation || tier == "read-only") {
            return fail("unsupported Kubernetes verb/resource or write in read-only operation");
        }
    }
    if grant.resource_names.iter().any(|name| !valid_token(name)) {
        return fail("Kubernetes resourceNames must be concrete names");
    }
    Ok(())
}

pub fn resolve(
    references: &[String],
    declared: Option<KubernetesPermissions>,
    tier: &str,
    attribution: Option<&Attribution>,
) -> Result<Option<KubernetesPermissions>> {
    let mut rules = Vec::new();
    for reference in references {
        let (api_group, resource, verb, reason) = match reference.as_str() {
            "deployments/get" => (
                "apps",
                "deployments",
                "get",
                "Read Deployments for the reviewed deployments/get capability",
            ),
            "pods/get" => (
                "",
                "pods",
                "get",
                "Read pods for the reviewed pods/get capability",
            ),
            "pods/delete" => (
                "",
                "pods",
                "delete",
                "Delete pods for the reviewed pods/delete capability",
            ),
            _ if catalog().aws.contains_key(reference) => continue,
            _ => {
                return invalid(
                    format!("Unknown operation permission reference '{reference}'"),
                    reference,
                    "",
                )
            }
        };
        rules.push(KubernetesRule {
            api_group: api_group.into(),
            resource: resource.into(),
            verbs: vec![verb.into()],
            resource_names: vec![],
            reason: reason.into(),
        });
    }
    if let Some(declared) = declared {
        validate(&declared, tier, attribution)?;
        rules.extend(declared.rules);
    }
    if rules.is_empty() {
        return Ok(None);
    }
    let permissions = KubernetesPermissions {
        schema_version: 1,
        rules,
    };
    validate(&permissions, tier, attribution)?;
    Ok(Some(permissions))
}

pub fn collect(plugins: &[CatalogPlugin]) -> Vec<KubernetesGrant> {
    let mut grants = BTreeMap::<String, KubernetesGrant>::new();
    for plugin in plugins.iter().filter(|plugin| plugin.enabled) {
        for operation in &plugin.operations {
            if let Some(permissions) = &operation.kubernetes_permissions {
                for rule in &permissions.rules {
                    let source = Source {
                        plugin: plugin.name.clone(),
                        operation: operation.name.clone(),
                        reason: rule.reason.clone(),
                    };
                    let names = sorted(&rule.resource_names);
                    let names: Vec<_> = if names.is_empty() {
                        vec![vec![]]
                    } else {
                        names.into_iter().map(|name| vec![name]).collect()
                    };
                    for verb in sorted(&rule.verbs) {
                        for names in &names {
                            let key = serde_json::json!([
                                rule.api_group,
                                rule.resource,
                                [verb],
                                if names.is_empty() { None } else { Some(names) }
                            ])
                            .to_string();
                            let grant = grants.entry(key).or_insert_with(|| KubernetesGrant {
                                api_group: rule.api_group.clone(),
                                resource: rule.resource.clone(),
                                verbs: vec![verb.clone()],
                                resource_names: names.clone(),
                                sources: vec![],
                            });
                            if !grant.sources.contains(&source) {
                                grant.sources.push(source.clone());
                                grant.sources.sort();
                            }
                        }
                    }
                }
            }
        }
    }
    grants.into_values().collect()
}

/// An installation option that needs Kubernetes access beyond the Operator's
/// own sync work. The chart binds each feature's rules only when the feature
/// is on, and the permission review shows them under that condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OperatorFeature {
    /// Release-independent workloads the deployment owner requests through the
    /// API. Product charts retain this access to suspend or delete existing
    /// workloads after an installed release withdraws image approvals.
    DynamicContainers,
    /// Pod log collection through the Kubernetes API (`podApi` mode).
    PodLogs,
}

/// One rule the chart grants the Operator ServiceAccount for its own work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRuntimeRule {
    pub api_group: String,
    pub resource: String,
    pub verbs: Vec<String>,
    pub reason: String,
    /// `None` on every install; `Some` only on installs with that feature.
    pub feature: Option<OperatorFeature>,
}

/// Rules the Remote Operator uses for its own work, whatever operations are
/// enabled. Each sync lists the Deployments, StatefulSets and DaemonSets in
/// scope, then lists each workload's pods, events and pod metrics. Pod log
/// collection in `podApi` mode lists DaemonSets and pods. Operations add their
/// own declared rules on top of these.
///
/// Rules with a `feature` are bound only when the installation enables it;
/// the Helm generator and the permission review read the same list.
pub fn operator_runtime_rules() -> Vec<OperatorRuntimeRule> {
    const INVENTORY: &str = "Workload inventory reported on every sync";
    const DYNAMIC: &str = "Dynamic containers requested through the API, including suspension and cleanup after image approvals change";
    let list = ["list"];
    let manage = ["get", "list", "create", "update", "delete"];
    [
        ("apps", "deployments", &list[..], INVENTORY, None),
        ("apps", "statefulsets", &list[..], INVENTORY, None),
        (
            "apps",
            "daemonsets",
            &list[..],
            "Workload inventory reported on every sync; log collector discovery in podApi mode",
            None,
        ),
        (
            "",
            "pods",
            &list[..],
            "Pod status of each observed workload; pod log discovery in podApi mode",
            None,
        ),
        (
            "",
            "events",
            &list[..],
            "Recent events of each observed workload",
            None,
        ),
        (
            "metrics.k8s.io",
            "pods",
            &list[..],
            "CPU and memory of each observed workload",
            None,
        ),
        (
            "apps",
            "deployments",
            &manage[..],
            DYNAMIC,
            Some(OperatorFeature::DynamicContainers),
        ),
        (
            "",
            "services",
            &manage[..],
            DYNAMIC,
            Some(OperatorFeature::DynamicContainers),
        ),
        (
            "",
            "secrets",
            &manage[..],
            "Environment and registry credentials of dynamic containers",
            Some(OperatorFeature::DynamicContainers),
        ),
        (
            "",
            "pods/log",
            &["get"][..],
            "Pod log collection through the Kubernetes API in podApi mode",
            Some(OperatorFeature::PodLogs),
        ),
    ]
    .into_iter()
    .map(
        |(api_group, resource, verbs, reason, feature)| OperatorRuntimeRule {
            api_group: api_group.to_owned(),
            resource: resource.to_owned(),
            verbs: verbs.iter().map(|verb| (*verb).to_owned()).collect(),
            reason: reason.to_owned(),
            feature,
        },
    )
    .collect()
}

pub fn grants_verb(mode: KubernetesMode, verb: &str) -> bool {
    matches!(mode, KubernetesMode::Remediation) || matches!(verb, "get" | "list" | "watch")
}

pub fn compile(grants: &[KubernetesGrant], mode: KubernetesMode) -> Result<Vec<KubernetesGrant>> {
    for grant in grants {
        validate_grant(grant, "mutating")?;
    }
    Ok(grants
        .iter()
        .filter_map(|grant| {
            let mut grant = grant.clone();
            grant.verbs.retain(|verb| grants_verb(mode, verb));
            (!grant.verbs.is_empty()).then_some(grant)
        })
        .collect())
}
