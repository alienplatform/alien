use std::collections::BTreeMap;

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

/// Rules the Remote Operator uses for its own work, whatever operations are
/// enabled. Each sync lists the Deployments, StatefulSets and DaemonSets in
/// scope, then lists each workload's pods, events and pod metrics. Pod log
/// collection in `podApi` mode lists DaemonSets and pods. Operations add their
/// own declared rules on top of these.
pub fn operator_runtime_rules() -> Vec<KubernetesRule> {
    const INVENTORY: &str = "Workload inventory reported on every sync";
    [
        ("apps", "deployments", INVENTORY),
        ("apps", "statefulsets", INVENTORY),
        (
            "apps",
            "daemonsets",
            "Workload inventory reported on every sync; log collector discovery in podApi mode",
        ),
        (
            "",
            "pods",
            "Pod status of each observed workload; pod log discovery in podApi mode",
        ),
        ("", "events", "Recent events of each observed workload"),
        (
            "metrics.k8s.io",
            "pods",
            "CPU and memory of each observed workload",
        ),
    ]
    .into_iter()
    .map(|(api_group, resource, reason)| KubernetesRule {
        api_group: api_group.to_owned(),
        resource: resource.to_owned(),
        verbs: vec!["list".to_owned()],
        resource_names: vec![],
        reason: reason.to_owned(),
    })
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
