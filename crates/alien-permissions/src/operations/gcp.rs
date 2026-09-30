use std::collections::BTreeMap;

use super::{invalid, sorted, types::*, Result};

pub fn collect(plugins: &[CatalogPlugin]) -> Vec<GcpGrant> {
    let mut grants = BTreeMap::<String, GcpGrant>::new();
    for plugin in plugins.iter().filter(|plugin| plugin.enabled) {
        for operation in &plugin.operations {
            for declaration in &operation.permissions.gcp {
                let source = Source {
                    plugin: plugin.name.clone(),
                    operation: operation.name.clone(),
                    reason: declaration.reason.clone(),
                };
                for permission in &declaration.permissions {
                    let grant = grants
                        .entry(format!("{}:{permission}", declaration.scope))
                        .or_insert_with(|| GcpGrant {
                            permission: permission.clone(),
                            scope: declaration.scope.clone(),
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
    grants.into_values().collect()
}

fn valid_bucket(name: &str) -> bool {
    let max = if name.contains('.') { 222 } else { 63 };
    name.len() >= 3
        && name.len() <= max
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
        && name
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && name
            .bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && name
            .split('.')
            .all(|part| !part.is_empty() && part.len() <= 63)
        && !name.starts_with("goog")
        && !name.replace('0', "o").contains("google")
        && !(name.split('.').count() == 4
            && name
                .split('.')
                .all(|part| part.len() <= 3 && part.bytes().all(|byte| byte.is_ascii_digit())))
}

pub fn compile(
    grants: &[GcpGrant],
    ceilings: &GcpResourceCeilings,
) -> Result<GcpWorkloadIdentityGrants> {
    if let Some(name) = ceilings
        .gcs_bucket_names
        .iter()
        .find(|name| !valid_bucket(name))
    {
        return invalid(format!("Installer-provided Cloud Storage bucket '{name}' must be an exact bucket name, without gs:// or wildcards"), "installer-resource-ceiling", name);
    }
    if let Some(grant) = grants
        .iter()
        .find(|grant| grant.scope != GCP_PROJECT_SCOPE && grant.scope != GCP_BUCKET_SCOPE)
    {
        return invalid(
            format!("Unsupported Google Cloud scope '{}'", grant.scope),
            &grant.permission,
            &grant.scope,
        );
    }
    let bucket_grants: Vec<_> = grants
        .iter()
        .filter(|grant| grant.scope == GCP_BUCKET_SCOPE)
        .cloned()
        .collect();
    if let Some(grant) = bucket_grants
        .first()
        .filter(|_| ceilings.gcs_bucket_names.is_empty())
    {
        return invalid(format!("Google Cloud permission '{}' requires at least one installer-provided Cloud Storage bucket name", grant.permission), &grant.permission, GCP_BUCKET_SCOPE);
    }
    Ok(GcpWorkloadIdentityGrants {
        project: grants
            .iter()
            .filter(|grant| grant.scope == GCP_PROJECT_SCOPE)
            .cloned()
            .collect(),
        buckets: GcpBucketGrants {
            names: sorted(&ceilings.gcs_bucket_names),
            grants: bucket_grants,
        },
    })
}
