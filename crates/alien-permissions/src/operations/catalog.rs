use alien_permission_types::{
    BindingConfiguration, GcpBindingSpec, PermissionSet, PermissionSetReference,
};

use super::{catalog, invalid, types::*, Result};

pub fn known(reference: &str) -> bool {
    matches!(reference, "deployments/get" | "pods/get" | "pods/delete")
        || catalog().aws.contains_key(reference)
}

pub fn resolve_aws(references: &[String]) -> Result<Vec<AwsDeclaration>> {
    let mut result = Vec::new();
    for reference in references
        .iter()
        .filter(|reference| reference.starts_with("operations/"))
    {
        let Some(ceiling) = catalog().aws.get(reference) else {
            return invalid(
                format!("Unknown operation permission reference '{reference}'"),
                reference,
                "",
            );
        };
        result.push(AwsDeclaration {
            effect: alien_permission_types::AwsPermissionEffect::Allow,
            actions: ceiling.actions.clone(),
            resources: ceiling.resources.clone(),
            condition: None,
            reason: reference.clone(),
        });
    }
    Ok(result)
}

pub fn gcp_scope(binding: &BindingConfiguration<GcpBindingSpec>) -> Result<String> {
    match (&binding.stack, &binding.resource) {
        (Some(stack), None) if stack.scope == GCP_PROJECT_SCOPE && stack.condition.is_none() => {
            Ok(GCP_PROJECT_SCOPE.into())
        }
        (None, Some(resource))
            if resource.scope == GCP_BUCKET_SCOPE && resource.condition.is_none() =>
        {
            Ok(GCP_BUCKET_SCOPE.into())
        }
        _ => invalid("GCP operation binding is not grantable", "gcp", ""),
    }
}

pub fn validate_sets(sets: &[PermissionSet]) -> Result<()> {
    for set in sets {
        for permission in set.platforms.aws.iter().flatten() {
            let Some(ceiling) = catalog().aws.get(&set.id) else {
                return invalid(
                    "Custom AWS operation permissions are not grantable",
                    &set.id,
                    "",
                );
            };
            if permission
                .grant
                .actions
                .iter()
                .flatten()
                .any(|action| !ceiling.actions.contains(action))
            {
                return invalid(
                    "AWS operation permission exceeds its reviewed action ceiling",
                    &set.id,
                    "",
                );
            }
            for binding in [
                permission.binding.stack.as_ref(),
                permission.binding.resource.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                if !binding.not_resources.is_empty()
                    || binding
                        .resources
                        .iter()
                        .any(|resource| !ceiling.resources.contains(resource))
                {
                    return invalid(
                        "AWS operation permission exceeds its reviewed resource ceiling",
                        &set.id,
                        "",
                    );
                }
            }
        }
        if set
            .platforms
            .azure
            .as_ref()
            .is_some_and(|entries| !entries.is_empty())
        {
            return invalid("Azure operation permissions are not supported yet: no Azure resource permission has been reviewed for operations", &set.id, "");
        }
        for permission in set.platforms.gcp.iter().flatten() {
            let grant = &permission.grant;
            let Some(permissions) = grant
                .permissions
                .as_ref()
                .filter(|permissions| !permissions.is_empty())
            else {
                return invalid(
                    "GCP operation grants must contain only supported metadata permissions",
                    &set.id,
                    "",
                );
            };
            if grant.actions.is_some()
                || grant.predefined_roles.is_some()
                || grant.residual_permissions.is_some()
                || grant.data_actions.is_some()
                || permissions
                    .iter()
                    .any(|permission| !catalog().gcp.contains_key(permission))
            {
                return invalid(
                    "GCP operation grants must contain only supported metadata permissions",
                    &set.id,
                    "",
                );
            }
            let scope = gcp_scope(&permission.binding)?;
            if permissions
                .iter()
                .any(|permission| catalog().gcp.get(permission) != Some(&scope))
            {
                return invalid("GCP operation permissions require an unconditional binding at their reviewed scope", &set.id, &scope);
            }
        }
    }
    Ok(())
}

pub fn validate_references(references: &[PermissionSetReference]) -> Result<()> {
    for reference in references {
        match reference {
            PermissionSetReference::Name(name) if !known(name) => {
                return invalid(
                    format!("Unknown operation permission reference '{name}'"),
                    name,
                    "",
                )
            }
            PermissionSetReference::Inline(set) => validate_sets(std::slice::from_ref(set))?,
            _ => {}
        }
    }
    Ok(())
}

pub fn unreviewed(references: &[PermissionSetReference]) -> Vec<String> {
    let mut result = Vec::new();
    for reference in references {
        match reference {
            PermissionSetReference::Name(name) if !known(name) => {
                result.push(format!("permission reference '{name}'"))
            }
            PermissionSetReference::Inline(set) => {
                for action in set
                    .platforms
                    .aws
                    .iter()
                    .flatten()
                    .flat_map(|permission| permission.grant.actions.iter().flatten())
                {
                    let alternatives: Vec<_> = catalog()
                        .aws
                        .iter()
                        .filter(|(_, ceiling)| ceiling.actions.contains(action))
                        .map(|(id, _)| format!("'{id}'"))
                        .collect();
                    result.push(if alternatives.is_empty() {
                        format!("AWS action {action}")
                    } else {
                        format!(
                            "AWS action {action} (declare {} instead)",
                            alternatives.join(" or ")
                        )
                    });
                }
                for permission in set
                    .platforms
                    .gcp
                    .iter()
                    .flatten()
                    .flat_map(|permission| permission.grant.permissions.iter().flatten())
                    .filter(|permission| !catalog().gcp.contains_key(*permission))
                {
                    result.push(format!("GCP permission {permission}"));
                }
                if set
                    .platforms
                    .azure
                    .as_ref()
                    .is_some_and(|entries| !entries.is_empty())
                {
                    result.push(format!(
                        "Azure permissions in '{}' (Azure operations are not supported yet)",
                        set.id
                    ));
                }
            }
            _ => {}
        }
    }
    result
}

pub fn resolve_plugin(plugin: DeclaredPlugin, origin: PluginOrigin) -> Result<CatalogPlugin> {
    let mut operations = Vec::new();
    for operation in plugin.operations {
        validate_references(&operation.permissions)?;
        if matches!(origin, PluginOrigin::Custom) {
            reject_inline_aws(&operation.permissions)?;
        }
        let named: Vec<_> = operation
            .permissions
            .iter()
            .filter_map(|reference| match reference {
                PermissionSetReference::Name(name) => Some(name.clone()),
                _ => None,
            })
            .collect();
        let sets: Vec<_> = operation
            .permissions
            .iter()
            .filter_map(|reference| match reference {
                PermissionSetReference::Inline(set) => Some(set.clone()),
                _ => None,
            })
            .collect();
        let mut aws = super::aws::for_catalog(&sets);
        aws.extend(resolve_aws(&named)?);
        let mut gcp = Vec::new();
        for set in &sets {
            for permission in set.platforms.gcp.iter().flatten() {
                gcp.push(GcpDeclaration {
                    permissions: permission.grant.permissions.clone().unwrap_or_default(),
                    scope: gcp_scope(&permission.binding)?,
                    reason: permission
                        .description
                        .clone()
                        .unwrap_or_else(|| set.description.clone()),
                });
            }
        }
        let attribution = Attribution {
            plugin: plugin.name.clone(),
            operation: operation.name.clone(),
        };
        let kubernetes_permissions = super::kubernetes::resolve(
            &named,
            operation.kubernetes_permissions,
            operation.tier.as_deref().unwrap_or(&plugin.tier),
            Some(&attribution),
        )?;
        operations.push(CatalogOperation {
            name: operation.name,
            permissions: CloudDeclarations { aws, gcp },
            kubernetes_permissions,
        });
    }
    Ok(CatalogPlugin {
        name: plugin.name,
        enabled: true,
        operations,
    })
}

pub fn reject_inline_aws(references: &[PermissionSetReference]) -> Result<()> {
    if let Some(set) = references.iter().find_map(|reference| match reference {
        PermissionSetReference::Inline(set)
            if set
                .platforms
                .aws
                .as_ref()
                .is_some_and(|entries| !entries.is_empty()) =>
        {
            Some(set)
        }
        _ => None,
    }) {
        let actions = unreviewed(references);
        return invalid(format!("Custom operation declares AWS actions directly: {}. Use reviewed named capabilities instead of inline AWS grants", actions.join(", ")), "custom-inline-aws-grant", &set.id);
    }
    Ok(())
}
