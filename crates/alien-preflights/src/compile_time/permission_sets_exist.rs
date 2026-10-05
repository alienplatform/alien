use alien_core::{ComputeCluster, PermissionSetReference, Platform, Stack};

use crate::{error::Result, CheckResult, CompileTimeCheck};

/// Validate the declaration selector before deciding whether node grants apply.
pub(crate) fn node_permissions_apply(
    cluster: &ComputeCluster,
    platform: Platform,
) -> std::result::Result<bool, String> {
    let Some(platforms) = &cluster.node_permissions_platforms else {
        return Ok(cluster.node_permissions.is_some());
    };
    if cluster.node_permissions.is_none()
        || platforms.is_empty()
        || platforms.iter().enumerate().any(|(index, selected)| {
            !matches!(selected, Platform::Aws | Platform::Gcp | Platform::Azure)
                || platforms[..index].contains(selected)
        })
    {
        return Err(format!("ComputeCluster '{}' nodePermissionsPlatforms requires a nodePermissions profile and a nonempty, distinct list containing only aws, gcp, or azure.", cluster.id));
    }
    Ok(platforms.contains(&platform))
}

/// Reject typos before permission generation can silently omit an access grant.
pub struct PermissionSetsExistCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for PermissionSetsExistCheck {
    fn description(&self) -> &'static str {
        "Named permission sets must exist"
    }

    fn should_run(&self, _stack: &Stack, _platform: Platform) -> bool {
        true
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let profiles = stack
            .permission_profiles()
            .iter()
            .map(|(name, profile)| (name.as_str(), profile))
            .chain(
                stack
                    .management()
                    .profile()
                    .map(|profile| ("management", profile)),
            );
        let mut errors = Vec::new();
        for (profile_name, profile) in profiles {
            for (resource_id, references) in &profile.0 {
                for reference in references {
                    if let PermissionSetReference::Name(name) = reference {
                        if alien_permissions::get_permission_set(name).is_none() {
                            errors.push(format!(
                                "Permission profile '{profile_name}' references unknown permission set '{name}' for resource '{resource_id}'. Use a registered permission name or an inline permission definition."
                            ));
                        }
                    }
                }
            }
        }
        // Node grants are inline and never become named workload profiles.
        for (cluster_id, entry) in stack.resources() {
            let Some(cluster) = entry.config.downcast_ref::<alien_core::ComputeCluster>() else {
                continue;
            };
            match node_permissions_apply(cluster, platform) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(message) => {
                    errors.push(message);
                    continue;
                }
            }
            let Some(profile) = &cluster.node_permissions else {
                continue;
            };
            for (resource_id, references) in &profile.0 {
                if resource_id == "*" || !stack.resources.contains_key(resource_id) {
                    errors.push(format!("ComputeCluster '{cluster_id}' node permissions must name an existing concrete resource, got '{resource_id}'."));
                    continue;
                }
                for reference in references {
                    let Some(set) = reference
                        .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                    else {
                        errors.push(format!("ComputeCluster '{cluster_id}' references unknown node permission set '{}'.", reference.id()));
                        continue;
                    };
                    let supported = match platform {
                        Platform::Aws => set.platforms.aws.as_ref().is_some_and(|entries| {
                            !entries.is_empty()
                                && entries.iter().all(|entry| entry.binding.resource.is_some())
                        }),
                        Platform::Gcp => set.platforms.gcp.as_ref().is_some_and(|entries| {
                            !entries.is_empty()
                                && entries.iter().all(|entry| entry.binding.resource.is_some())
                        }),
                        Platform::Azure => set.platforms.azure.as_ref().is_some_and(|entries| {
                            !entries.is_empty()
                                && entries.iter().all(|entry| entry.binding.resource.is_some())
                        }),
                        _ => false,
                    };
                    if !supported {
                        errors.push(format!("ComputeCluster '{cluster_id}' node permission set '{}' has no concrete resource binding on '{platform}'.", set.id));
                    }
                }
            }
        }
        Ok(if errors.is_empty() {
            CheckResult::success()
        } else {
            CheckResult::failed(errors)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{ManagementPermissions, PermissionProfile};

    #[tokio::test]
    async fn node_permissions_require_known_concrete_targets_and_supported_sets() {
        for (target, permission, platform, valid) in [
            ("objects", "storage/data-read", Platform::Aws, true),
            ("objects", "storage/data-read", Platform::Gcp, true),
            ("objects", "storage/data-write", Platform::Azure, true),
            ("*", "storage/data-read", Platform::Gcp, false),
            ("missing", "storage/data-read", Platform::Gcp, false),
            ("objects", "storage/typo", Platform::Gcp, false),
            ("objects", "storage/data-read", Platform::Kubernetes, false),
            ("objects", "storage/data-read", Platform::Machines, false),
        ] {
            let stack = Stack::new("example".to_string())
                .add(
                    alien_core::Storage::new("objects".to_string()).build(),
                    alien_core::ResourceLifecycle::Frozen,
                )
                .add(
                    alien_core::ComputeCluster::new("compute".to_string())
                        .node_permissions(PermissionProfile::new().resource(target, [permission]))
                        .build(),
                    alien_core::ResourceLifecycle::Frozen,
                )
                .build();
            let result = PermissionSetsExistCheck
                .check(&stack, platform)
                .await
                .unwrap();
            assert_eq!(
                result.success, valid,
                "{target} {permission} {platform}: {:?}",
                result.errors
            );
            assert_eq!(result.errors.is_empty(), valid);
            assert!(stack.permissions.profiles.is_empty());
        }
    }

    #[tokio::test]
    async fn rejects_misspelled_application_permissions_with_context() {
        let stack = Stack::new("example".to_string())
            .permission(
                "api",
                PermissionProfile::new().resource("metadata", ["postgres/connect"]),
            )
            .build();
        let result = PermissionSetsExistCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(!result.success);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("'api'"));
        assert!(result.errors[0].contains("'metadata'"));
        assert!(result.errors[0].contains("'postgres/connect'"));
    }

    #[tokio::test]
    async fn accepts_registered_and_inline_permissions() {
        let custom = alien_permissions::get_permission_set("postgres/data-access")
            .unwrap()
            .clone();
        let stack = Stack::new("example".to_string())
            .permission(
                "api",
                PermissionProfile::new().resource(
                    "metadata",
                    [
                        PermissionSetReference::from_name("postgres/data-access"),
                        PermissionSetReference::from_inline(custom),
                    ],
                ),
            )
            .build();
        let result = PermissionSetsExistCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(result.success, "{:?}", result.errors);
    }

    #[tokio::test]
    async fn rejects_unknown_explicit_management_permissions() {
        for management in [
            ManagementPermissions::Extend(PermissionProfile::new().global(["storage/typo"])),
            ManagementPermissions::Override(PermissionProfile::new().global(["storage/typo"])),
        ] {
            let stack = Stack::new("example".to_string())
                .management(management)
                .build();
            let result = PermissionSetsExistCheck
                .check(&stack, Platform::Aws)
                .await
                .unwrap();
            assert!(!result.success);
            assert_eq!(result.errors.len(), 1);
            assert!(result.errors[0].contains("'management'"));
            assert!(result.errors[0].contains("'storage/typo'"));
        }
    }
}
