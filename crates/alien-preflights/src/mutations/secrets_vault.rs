//! Adds secrets vault and links it to all compute resources.

use crate::error::Result;
use crate::StackMutation;
use alien_core::permissions::{PermissionProfile, PermissionSetReference};
use alien_core::{
    Container, DeploymentConfig, Function, ResourceEntry, ResourceLifecycle, ResourceRef, Stack,
    StackState, Vault,
};
use async_trait::async_trait;
use tracing::{debug, info};

/// Adds secrets vault for environment variable storage.
///
/// Creates the "secrets" vault (if missing) and links it to all Functions and Containers.
/// Secrets are synced during deployment and loaded by alien-runtime at startup.
///
/// Steps:
/// 1. Add "secrets" vault resource (if not present)
/// 2. Link vault to all Functions and Containers
/// 3. Add vault/data-read to compute resource profiles
/// 4. Add vault/data-write to management profile
pub struct SecretsVaultMutation;

#[async_trait]
impl StackMutation for SecretsVaultMutation {
    fn description(&self) -> &'static str {
        "Add secrets vault for environment variable storage"
    }

    fn should_run(
        &self,
        _stack: &Stack,
        _stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        // Always run to ensure vault permissions are added to all profiles
        // even if the vault resource already exists (idempotent)
        true
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        _stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        info!("Adding secrets vault and linking to all compute resources");

        let secrets_vault_id = "secrets";

        // Step 1: Add vault resource if it doesn't already exist
        if !stack.resources.contains_key(secrets_vault_id) {
            let vault = Vault::new(secrets_vault_id.to_string()).build();
            let vault_entry = ResourceEntry {
                config: alien_core::Resource::new(vault),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            };
            stack
                .resources
                .insert(secrets_vault_id.to_string(), vault_entry);
            debug!("Added secrets vault resource");
        } else {
            debug!("Secrets vault already exists");
        }

        // Step 2: Link the vault to all compute resources (Functions and Containers)
        // This gives them the vault binding so alien-runtime can load secrets
        link_vault_to_compute_resources(&mut stack, secrets_vault_id)?;

        // Step 3: Add vault/data-read permission to all compute resource profiles
        // This allows Functions and Containers to read secrets from the vault
        add_vault_read_permissions_to_compute_profiles(&mut stack, secrets_vault_id)?;

        // Step 4: Add vault/data-write permission to management profile
        // This allows the control plane to sync secrets during deployment
        add_vault_write_permission_to_management(&mut stack, secrets_vault_id)?;

        Ok(stack)
    }
}

/// Link the secrets vault to all compute resources (Functions and Containers)
///
/// This ensures all source-based compute resources get the vault binding,
/// allowing alien-runtime to load secrets at startup via ALIEN_SECRETS.
fn link_vault_to_compute_resources(stack: &mut Stack, vault_id: &str) -> Result<()> {
    let vault_ref = ResourceRef::new(Vault::RESOURCE_TYPE, vault_id);
    let mut linked_count = 0;

    for (resource_id, entry) in &mut stack.resources {
        let resource_type = entry.config.resource_type();

        // Check if this is a compute resource (Function or Container)
        let is_compute =
            resource_type == Function::RESOURCE_TYPE || resource_type == Container::RESOURCE_TYPE;

        if !is_compute {
            continue;
        }

        // Get the links array (both Function and Container have .links field)
        let links = if resource_type == Function::RESOURCE_TYPE {
            if let Some(function) = entry.config.downcast_mut::<Function>() {
                &mut function.links
            } else {
                continue;
            }
        } else {
            if let Some(container) = entry.config.downcast_mut::<Container>() {
                &mut container.links
            } else {
                continue;
            }
        };

        // Add vault link if not already present
        if !links.iter().any(|link| link.id() == vault_id) {
            links.push(vault_ref.clone());
            linked_count += 1;
            debug!("Linked secrets vault to compute resource '{}'", resource_id);
        }
    }

    if linked_count > 0 {
        info!("Linked secrets vault to {} compute resources", linked_count);
    }

    Ok(())
}

/// Add vault/data-read permission to all compute resource permission profiles
///
/// Both Functions and Containers built from source need to read secrets from the vault.
/// This permission is added to their profiles, allowing alien-runtime to fetch secrets.
fn add_vault_read_permissions_to_compute_profiles(
    stack: &mut Stack,
    _vault_name: &str,
) -> Result<()> {
    // Get all compute resource permission profile names
    let profile_names: Vec<String> = stack
        .resources
        .iter()
        .filter_map(|(_, entry)| {
            let resource_type = entry.config.resource_type();

            // Check if this is a compute resource
            if resource_type == Function::RESOURCE_TYPE {
                entry
                    .config
                    .downcast_ref::<Function>()
                    .map(|f| f.permissions.clone())
            } else if resource_type == Container::RESOURCE_TYPE {
                entry
                    .config
                    .downcast_ref::<Container>()
                    .map(|c| c.permissions.clone())
            } else {
                None
            }
        })
        .collect();

    // Deduplicate profile names (multiple resources might use the same profile)
    let unique_profiles: std::collections::HashSet<String> = profile_names.into_iter().collect();

    // Add vault/data-read to each profile
    let vault_permission = PermissionSetReference::from_name("vault/data-read");

    for profile_name in unique_profiles {
        if let Some(profile) = stack.permissions.profiles.get_mut(&profile_name) {
            // Add to global scope (*) - simpler than resource-specific scoping
            if let Some(global_permissions) = profile.0.get_mut("*") {
                if !global_permissions
                    .iter()
                    .any(|p| p.id() == "vault/data-read")
                {
                    global_permissions.push(vault_permission.clone());
                    debug!("Added vault/data-read to profile '{}'", profile_name);
                }
            } else {
                // Create global scope if it doesn't exist
                profile
                    .0
                    .insert("*".to_string(), vec![vault_permission.clone()]);
                debug!(
                    "Added vault/data-read to new global scope in profile '{}'",
                    profile_name
                );
            }
        } else {
            // Profile doesn't exist - PermissionProfilesExistCheck should have caught this
            // Don't create the profile (fail fast on configuration errors)
            debug!(
                "Skipping vault permission for nonexistent profile '{}' (validation issue)",
                profile_name
            );
        }
    }

    Ok(())
}

/// Add vault/data-write permission to management profile
/// This allows the management service to sync secrets to the vault during deployment
fn add_vault_write_permission_to_management(stack: &mut Stack, _vault_name: &str) -> Result<()> {
    use alien_core::permissions::ManagementPermissions;

    // Get current management permissions
    let current_management = stack.permissions.management.clone();

    match current_management {
        ManagementPermissions::Auto | ManagementPermissions::Extend(_) => {
            // For Auto or Extend, we need to add vault/data-write to the management profile
            // This will be merged with auto-generated permissions by ManagementPermissionProfileMutation
            let vault_write_permission = PermissionSetReference::from_name("vault/data-write");

            let mut management_profile = match current_management {
                ManagementPermissions::Extend(profile) => profile,
                _ => PermissionProfile::new(),
            };

            // Add vault/data-write to global scope
            if let Some(global_permissions) = management_profile.0.get_mut("*") {
                if !global_permissions
                    .iter()
                    .any(|p| p.id() == "vault/data-write")
                {
                    global_permissions.push(vault_write_permission);
                    debug!("Added vault/data-write to management profile");
                }
            } else {
                // Create global scope if it doesn't exist
                management_profile
                    .0
                    .insert("*".to_string(), vec![vault_write_permission]);
                debug!("Added vault/data-write to new global scope in management profile");
            }

            stack.permissions.management = ManagementPermissions::Extend(management_profile);
        }
        ManagementPermissions::Override(_) => {
            // Don't modify override - user has full control
            debug!("Skipping vault/data-write addition - management permissions are overridden");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::permissions::{ManagementPermissions, PermissionsConfig};
    use alien_core::{
        Container, ContainerCode, EnvironmentVariablesSnapshot, ExternalBindings, Function,
        FunctionCode, Platform, ResourceEntry, ResourceLifecycle, ResourceSpec, StackSettings,
        StackState,
    };
    use indexmap::IndexMap;

    fn empty_env_snapshot() -> EnvironmentVariablesSnapshot {
        EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    #[tokio::test]
    async fn test_adds_secrets_vault_and_links_to_function() {
        let function = Function::new("test-function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test-profile".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-function".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(function),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let mut profiles = IndexMap::new();
        profiles.insert("test-profile".to_string(), PermissionProfile::new());

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles,
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = SecretsVaultMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Check that secrets vault was added
        assert!(result_stack.resources.contains_key("secrets"));
        let vault_entry = result_stack.resources.get("secrets").unwrap();
        assert_eq!(vault_entry.lifecycle, ResourceLifecycle::Frozen);

        // Check that vault was linked to the function
        let function_entry = result_stack.resources.get("test-function").unwrap();
        let function = function_entry.config.downcast_ref::<Function>().unwrap();
        assert!(
            function.links.iter().any(|link| link.id() == "secrets"),
            "Function should be linked to secrets vault"
        );

        // Check that vault/data-read was added to function profile
        let function_profile = result_stack
            .permissions
            .profiles
            .get("test-profile")
            .unwrap();
        let global_permissions = function_profile.0.get("*").unwrap();
        assert!(global_permissions
            .iter()
            .any(|p| p.id() == "vault/data-read"));
    }

    #[tokio::test]
    async fn test_links_vault_to_containers() {
        let container = Container::new("test-container".to_string())
            .code(ContainerCode::Image {
                image: "test:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("test-profile".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-container".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(container),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let mut profiles = IndexMap::new();
        profiles.insert("test-profile".to_string(), PermissionProfile::new());

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles,
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = SecretsVaultMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Check that vault was linked to the container
        let container_entry = result_stack.resources.get("test-container").unwrap();
        let container = container_entry.config.downcast_ref::<Container>().unwrap();
        assert!(
            container.links.iter().any(|link| link.id() == "secrets"),
            "Container should be linked to secrets vault"
        );

        // Check that vault/data-read was added to container profile
        let container_profile = result_stack
            .permissions
            .profiles
            .get("test-profile")
            .unwrap();
        let global_permissions = container_profile.0.get("*").unwrap();
        assert!(global_permissions
            .iter()
            .any(|p| p.id() == "vault/data-read"));
    }

    #[tokio::test]
    async fn test_does_not_add_duplicate_vault() {
        // Create a stack that already has a secrets vault
        let vault = Vault::new("secrets".to_string()).build();
        let mut resources = IndexMap::new();
        resources.insert(
            "secrets".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(vault),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = SecretsVaultMutation;

        // Mutation should always run (returns true)
        assert!(mutation.should_run(&stack, &stack_state, &config));

        // But when it runs, it should not add a duplicate vault
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Should still have exactly one vault
        assert_eq!(result_stack.resources.len(), 1);
        assert!(result_stack.resources.contains_key("secrets"));
    }

    #[tokio::test]
    async fn test_supports_mixed_functions_and_containers() {
        let function = Function::new("api".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("api-profile".to_string())
            .build();

        let container = Container::new("worker".to_string())
            .code(ContainerCode::Image {
                image: "test:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("worker-profile".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "api".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(function),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        resources.insert(
            "worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(container),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let mut profiles = IndexMap::new();
        profiles.insert("api-profile".to_string(), PermissionProfile::new());
        profiles.insert("worker-profile".to_string(), PermissionProfile::new());

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles,
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = SecretsVaultMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Both resources should be linked to secrets vault
        let function = result_stack
            .resources
            .get("api")
            .unwrap()
            .config
            .downcast_ref::<Function>()
            .unwrap();
        assert!(function.links.iter().any(|link| link.id() == "secrets"));

        let container = result_stack
            .resources
            .get("worker")
            .unwrap()
            .config
            .downcast_ref::<Container>()
            .unwrap();
        assert!(container.links.iter().any(|link| link.id() == "secrets"));

        // Both profiles should have vault/data-read
        for profile_name in ["api-profile", "worker-profile"] {
            let profile = result_stack.permissions.profiles.get(profile_name).unwrap();
            let global_permissions = profile.0.get("*").unwrap();
            assert!(
                global_permissions
                    .iter()
                    .any(|p| p.id() == "vault/data-read"),
                "Profile {} should have vault/data-read",
                profile_name
            );
        }
    }

    #[tokio::test]
    async fn test_adds_vault_data_write_to_management() {
        let function = Function::new("test-function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test-profile".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-function".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(function),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let mut profiles = IndexMap::new();
        profiles.insert("test-profile".to_string(), PermissionProfile::new());

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles,
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = SecretsVaultMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Check that management profile has vault/data-write
        match result_stack.permissions.management {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                assert!(
                    global_permissions
                        .iter()
                        .any(|p| p.id() == "vault/data-write"),
                    "Management profile should have vault/data-write"
                );
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_respects_override_management_permissions() {
        let function = Function::new("test-function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test-profile".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-function".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(function),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let mut profiles = IndexMap::new();
        profiles.insert("test-profile".to_string(), PermissionProfile::new());

        // Create an override management profile without vault/data-write
        let override_profile = PermissionProfile::new().global(["storage/management"]);

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles,
                management: ManagementPermissions::Override(override_profile.clone()),
            },
            supported_platforms: None,
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = SecretsVaultMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Check that override profile is unchanged (no vault/data-write added)
        match result_stack.permissions.management {
            ManagementPermissions::Override(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                assert_eq!(global_permissions.len(), 1);
                assert!(
                    global_permissions
                        .iter()
                        .any(|p| p.id() == "storage/management"),
                    "Override profile should keep original permissions"
                );
                assert!(
                    !global_permissions
                        .iter()
                        .any(|p| p.id() == "vault/data-write"),
                    "Override profile should not have vault/data-write added"
                );
            }
            _ => panic!("Expected Override management permissions"),
        }
    }
}
