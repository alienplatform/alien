//! ServiceAccount dependencies mutation that adds ServiceAccount dependencies
//! to resources with resource-scoped permissions.

use crate::error::Result;
use crate::StackMutation;
use alien_core::{DeploymentConfig, Platform, ResourceRef, ServiceAccount, Stack, StackState};
use async_trait::async_trait;
use tracing::{debug, info};

/// Resource types that carry a `permissions` profile and therefore need a dependency
/// on the corresponding `{permissions}-sa` service account.
const PERMISSION_BEARING_RESOURCE_TYPES: &[&str] = &["container", "worker"];

/// Mutation that adds ServiceAccount dependencies to resources that have resource-scoped permissions.
///
/// This ensures that resources with resource-scoped permissions depend on the ServiceAccounts
/// that will have those permissions applied to them.
pub struct ServiceAccountDependenciesMutation;

#[async_trait]
impl StackMutation for ServiceAccountDependenciesMutation {
    fn description(&self) -> &'static str {
        "Add ServiceAccount dependencies to resources with resource-scoped permissions"
    }

    fn should_run(
        &self,
        stack: &Stack,
        _stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        // Run if stack has permission profiles with resource-scoped permissions
        for (_profile_name, profile) in &stack.permissions.profiles {
            for (resource_id, _permission_set_ids) in &profile.0 {
                if resource_id != "*" {
                    return true; // Found resource-scoped permissions
                }
            }
        }

        // Also check management permissions for resource-scoped permissions
        match &stack.permissions.management {
            alien_core::ManagementPermissions::Extend(profile)
            | alien_core::ManagementPermissions::Override(profile) => {
                for (resource_id, _permission_set_ids) in &profile.0 {
                    if resource_id != "*" {
                        return true; // Found resource-scoped management permissions
                    }
                }
            }
            alien_core::ManagementPermissions::Auto => {}
        }

        // Also run if any container or worker uses a named permission profile, so we can
        // wire it as a declared dependency on the corresponding SA.
        for (_resource_id, entry) in &stack.resources {
            let rtype = entry.config.resource_type();
            if PERMISSION_BEARING_RESOURCE_TYPES.contains(&rtype.as_ref()) {
                if entry.config.get_permissions().is_some() {
                    return true;
                }
            }
        }

        false
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<Stack> {
        info!("Adding ServiceAccount dependencies to resources with resource-scoped permissions");

        // Collect all resource IDs that need ServiceAccount dependencies
        let mut resource_dependencies: Vec<(String, Vec<ResourceRef>)> = Vec::new();

        // For each permission profile, find which resources have resource-scoped permissions
        for (profile_name, profile) in &stack.permissions.profiles {
            let service_account_id = format!("{}-sa", profile_name);

            // Create a ResourceRef for this ServiceAccount
            let service_account_ref =
                ResourceRef::new(ServiceAccount::RESOURCE_TYPE, service_account_id.clone());

            // Find all resources that have permissions specifically for this profile
            for (resource_id, _permission_set_ids) in &profile.0 {
                // Skip global permissions (*)
                if resource_id == "*" {
                    continue;
                }

                // This resource has resource-scoped permissions from this profile
                // Add the ServiceAccount as a dependency
                resource_dependencies
                    .push((resource_id.clone(), vec![service_account_ref.clone()]));
            }
        }

        // Handle management permissions dependencies only when management is
        // performed by a stack-local service account. Cloud platforms, and
        // Kubernetes setup backed by a cloud base platform, use
        // RemoteStackManagement instead; wiring `management-sa` there creates a
        // dependency on a resource that is intentionally not in the stack.
        if !management_permissions_use_remote_stack_management(stack_state.platform, config) {
            match &stack.permissions.management {
                alien_core::ManagementPermissions::Extend(management_profile)
                | alien_core::ManagementPermissions::Override(management_profile) => {
                    let service_account_id = "management-sa".to_string();
                    let service_account_ref =
                        ResourceRef::new(ServiceAccount::RESOURCE_TYPE, service_account_id.clone());

                    // Find all resources that have management permissions
                    for (resource_id, _permission_set_ids) in &management_profile.0 {
                        // Skip global permissions (*)
                        if resource_id == "*" {
                            continue;
                        }

                        // This resource has management permissions
                        // Add the management ServiceAccount as a dependency
                        resource_dependencies
                            .push((resource_id.clone(), vec![service_account_ref.clone()]));
                    }
                }
                alien_core::ManagementPermissions::Auto => {}
            }
        }

        // Second pass: for every container/worker with a named permission profile, add
        // the corresponding {profile}-sa as a declared dependency.  This ensures the
        // executor waits for the SA before creating the resource and propagates SA changes
        // to the resource automatically — the consumer side of the SA dependency, not just
        // the target-resource side wired by the first pass above.
        let resource_ids: Vec<String> = stack.resources.keys().cloned().collect();
        for resource_id in resource_ids {
            let (rtype, permissions_profile) = {
                let entry = &stack.resources[&resource_id];
                let rtype = entry.config.resource_type();
                let perm = entry.config.get_permissions().map(|s| s.to_owned());
                (rtype, perm)
            };

            if !PERMISSION_BEARING_RESOURCE_TYPES.contains(&rtype.as_ref()) {
                continue;
            }

            let profile_name = match permissions_profile {
                Some(p) => p,
                None => continue,
            };

            let sa_id = format!("{}-sa", profile_name);

            // Only add the dep if the SA resource actually exists in the stack (it is
            // created by ServiceAccountMutation which runs before this mutation).
            if !stack.resources.contains_key(&sa_id) {
                continue;
            }

            let sa_ref = ResourceRef::new(ServiceAccount::RESOURCE_TYPE, sa_id.clone());
            resource_dependencies.push((resource_id.clone(), vec![sa_ref]));
            debug!(
                "Queued SA dependency '{}' → '{}' (permission profile consumer)",
                resource_id, sa_id
            );
        }

        // Apply the dependencies to the actual resources
        let mut dependencies_added = 0;
        for (resource_id, service_account_deps) in resource_dependencies {
            if let Some(resource_entry) = stack.resources.get_mut(&resource_id) {
                // Add ServiceAccount dependencies, avoiding duplicates
                for service_account_dep in service_account_deps {
                    if !resource_entry
                        .dependencies
                        .iter()
                        .any(|dep| dep.id() == service_account_dep.id())
                    {
                        resource_entry.dependencies.push(service_account_dep);
                        dependencies_added += 1;
                        debug!(
                            "Added ServiceAccount dependency to resource '{}'",
                            resource_id
                        );
                    }
                }
            }
        }

        info!(
            "Added {} ServiceAccount dependencies to resources",
            dependencies_added
        );

        Ok(stack)
    }
}

fn management_permissions_use_remote_stack_management(
    platform: Platform,
    config: &DeploymentConfig,
) -> bool {
    if matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure) {
        return true;
    }

    platform == Platform::Kubernetes
        && matches!(
            config.base_platform,
            Some(Platform::Aws | Platform::Gcp | Platform::Azure)
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::permissions::{ManagementPermissions, PermissionProfile, PermissionsConfig};
    use alien_core::{
        EnvironmentVariablesSnapshot, ExternalBindings, ResourceEntry, ResourceLifecycle,
        StackSettings, Vault,
    };
    use indexmap::IndexMap;

    fn empty_config(base_platform: Option<Platform>) -> DeploymentConfig {
        let builder = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: String::new(),
                created_at: "2024-01-01T00:00:00Z".to_string(),
            })
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default());
        match base_platform {
            Some(base_platform) => builder.base_platform(base_platform).build(),
            None => builder.build(),
        }
    }

    fn stack_with_management_scoped_vault() -> Stack {
        let mut resources = IndexMap::new();
        resources.insert(
            "secrets".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(Vault::new("secrets".to_string()).build()),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let management = ManagementPermissions::Extend(PermissionProfile(
            [(
                "secrets".to_string(),
                vec![alien_core::permissions::PermissionSetReference::from_name(
                    "vault/data-write",
                )],
            )]
            .into_iter()
            .collect(),
        ));

        Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management,
            },
            supported_platforms: None,
        }
    }

    #[tokio::test]
    async fn cloud_backed_kubernetes_skips_management_sa_dependencies() {
        let stack = stack_with_management_scoped_vault();
        let result = ServiceAccountDependenciesMutation
            .mutate(
                stack,
                &StackState::new(Platform::Kubernetes),
                &empty_config(Some(Platform::Aws)),
            )
            .await
            .unwrap();

        let secrets = result.resources.get("secrets").unwrap();
        assert!(
            secrets
                .dependencies
                .iter()
                .all(|dependency| dependency.id() != "management-sa"),
            "cloud-backed Kubernetes setup uses RemoteStackManagement, not a stack-local management-sa"
        );
    }

    #[tokio::test]
    async fn plain_kubernetes_keeps_management_sa_dependencies() {
        let stack = stack_with_management_scoped_vault();
        let result = ServiceAccountDependenciesMutation
            .mutate(
                stack,
                &StackState::new(Platform::Kubernetes),
                &empty_config(None),
            )
            .await
            .unwrap();

        let secrets = result.resources.get("secrets").unwrap();
        assert!(
            secrets
                .dependencies
                .iter()
                .any(|dependency| dependency.id() == "management-sa"),
            "plain Kubernetes management still depends on a stack-local management-sa"
        );
    }
}
