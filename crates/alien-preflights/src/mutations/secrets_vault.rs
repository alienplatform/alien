//! Adds secrets vault and links it to all compute resources.

use crate::error::Result;
use crate::StackMutation;
use alien_core::permissions::{PermissionProfile, PermissionSetReference};
use alien_core::{
    Container, Daemon, DeploymentConfig, Platform, RemoteStackManagement, ResourceEntry,
    ResourceLifecycle, ResourceRef, Stack, StackState, Vault, Worker,
};
use async_trait::async_trait;
use tracing::{debug, info};

/// Adds secrets vault for environment variable storage.
///
/// Creates the "secrets" vault (if missing) and links it to all Workers, Daemons, and Containers.
/// Secrets are synced during deployment and loaded by alien-worker-runtime at startup.
///
/// Steps:
/// 1. Add "secrets" vault resource (if not present)
/// 2. Link vault to all Workers, Daemons, and Containers
/// 3. Add vault/data-read to compute resource profiles
/// 4. Add vault/data-read and vault/data-write to management profile for the secrets vault
pub struct SecretsVaultMutation;

#[async_trait]
impl StackMutation for SecretsVaultMutation {
    fn description(&self) -> &'static str {
        "Add secrets vault for environment variable storage"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> bool {
        if stack_state.platform == Platform::Machines {
            return stack.resources.contains_key("secrets")
                || config.external_bindings.has("secrets");
        }

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
                // The deployment loop syncs secrets into this vault from control-plane state
                // (`sync_secrets_to_vault` resolves it via `from_stack_state`), so its binding must
                // be synced. Safe to sync: the binding is only a reference (Parameter Store / Secret
                // Manager / Key Vault locator), never the secret values themselves.
                remote_access: true,
            };
            stack
                .resources
                .insert(secrets_vault_id.to_string(), vault_entry);
            debug!("Added secrets vault resource");
        } else {
            debug!("Secrets vault already exists");
        }

        // Step 2: Link the vault to all compute resources (Workers, Daemons, and Containers)
        // This gives them the vault binding so alien-worker-runtime can load secrets
        link_vault_to_compute_resources(&mut stack, secrets_vault_id)?;

        // Step 3: Add vault/data-read permission to all compute resource profiles
        // This allows Workers, Daemons, and Containers to read secrets from the vault
        add_vault_read_permissions_to_compute_profiles(&mut stack, secrets_vault_id)?;

        // Step 4: Add vault data permissions to management profile for the secrets vault.
        // This allows the control plane to sync secret environment variables without
        // granting access to user-declared vaults.
        add_vault_permissions_to_management(&mut stack, secrets_vault_id)?;

        Ok(stack)
    }
}

/// Link the secrets vault to all compute resources (Workers, Daemons, and Containers)
///
/// This ensures all source-based compute resources get the vault binding,
/// allowing alien-worker-runtime to load secrets at startup via ALIEN_SECRETS.
fn link_vault_to_compute_resources(stack: &mut Stack, vault_id: &str) -> Result<()> {
    let vault_ref = ResourceRef::new(Vault::RESOURCE_TYPE, vault_id);
    let mut linked_count = 0;

    for (resource_id, entry) in &mut stack.resources {
        let resource_type = entry.config.resource_type();

        // Check if this is a compute resource (Worker, Daemon, or Container)
        let is_compute = resource_type == Worker::RESOURCE_TYPE
            || resource_type == Daemon::RESOURCE_TYPE
            || resource_type == Container::RESOURCE_TYPE;

        if !is_compute {
            continue;
        }

        // Get the links array (Worker, Daemon, and Container have .links field)
        let links = if resource_type == Worker::RESOURCE_TYPE {
            if let Some(worker) = entry.config.downcast_mut::<Worker>() {
                &mut worker.links
            } else {
                continue;
            }
        } else if resource_type == Daemon::RESOURCE_TYPE {
            if let Some(daemon) = entry.config.downcast_mut::<Daemon>() {
                &mut daemon.links
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
/// Workers, Daemons, and Containers need to read secrets from the vault.
/// This permission is added to their profiles, allowing alien-worker-runtime to fetch secrets.
fn add_vault_read_permissions_to_compute_profiles(
    stack: &mut Stack,
    vault_name: &str,
) -> Result<()> {
    // Get all compute resource permission profile names
    let profile_names: Vec<String> = stack
        .resources
        .iter()
        .filter_map(|(_, entry)| {
            let resource_type = entry.config.resource_type();

            // Check if this is a compute resource
            if resource_type == Worker::RESOURCE_TYPE {
                entry
                    .config
                    .downcast_ref::<Worker>()
                    .map(|worker| worker.permissions.clone())
            } else if resource_type == Daemon::RESOURCE_TYPE {
                entry
                    .config
                    .downcast_ref::<Daemon>()
                    .map(|daemon| daemon.permissions.clone())
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
            let vault_permissions = profile.0.entry(vault_name.to_string()).or_default();
            if !vault_permissions
                .iter()
                .any(|p| p.id() == "vault/data-read")
            {
                vault_permissions.push(vault_permission.clone());
                debug!(
                    "Added vault/data-read to profile '{}' for vault '{}'",
                    profile_name, vault_name
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

/// Author explicit vault data permissions into the management profile for this vault.
/// The generator treats these like any other management grant; the preflight is
/// the permission author.
fn add_vault_permissions_to_management(stack: &mut Stack, vault_name: &str) -> Result<()> {
    use alien_core::permissions::ManagementPermissions;

    if !stack
        .resources
        .values()
        .any(|entry| entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE)
    {
        debug!(
            vault_name = %vault_name,
            "Skipping concrete vault management permissions because remote stack management is not present"
        );
        return Ok(());
    }

    // Get current management permissions
    let current_management = stack.permissions.management.clone();

    match current_management {
        ManagementPermissions::Auto | ManagementPermissions::Extend(_) => {
            // For Auto or Extend, author data access on this concrete vault resource.
            // This will be merged with auto-generated permissions by ManagementPermissionProfileMutation
            let vault_read_permission = PermissionSetReference::from_name("vault/data-read");
            let vault_write_permission = PermissionSetReference::from_name("vault/data-write");

            let mut management_profile = match current_management {
                ManagementPermissions::Extend(profile) => profile,
                _ => PermissionProfile::new(),
            };

            let vault_permissions = management_profile
                .0
                .entry(vault_name.to_string())
                .or_default();
            if !vault_permissions
                .iter()
                .any(|p| p.id() == "vault/data-read")
            {
                vault_permissions.push(vault_read_permission);
                debug!(
                    vault_name = %vault_name,
                    "Added vault/data-read to management profile for concrete vault resource"
                );
            }
            if !vault_permissions
                .iter()
                .any(|p| p.id() == "vault/data-write")
            {
                vault_permissions.push(vault_write_permission);
                debug!(
                    vault_name = %vault_name,
                    "Added vault/data-write to management profile for concrete vault resource"
                );
            }
            stack.permissions.management = ManagementPermissions::Extend(management_profile);
        }
        ManagementPermissions::Override(_) => {
            // Don't modify override - user has full control.
            debug!("Skipping concrete vault management permissions - management permissions are overridden");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::permissions::{ManagementPermissions, PermissionsConfig};
    use alien_core::{
        Container, ContainerCode, EnvironmentVariablesSnapshot, ExternalBindings, Platform,
        ResourceEntry, ResourceLifecycle, ResourceSpec, StackSettings, StackState, Worker,
        WorkerCode,
    };
    use indexmap::IndexMap;

    fn empty_env_snapshot() -> EnvironmentVariablesSnapshot {
        EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    fn remote_stack_management_entry() -> ResourceEntry {
        ResourceEntry {
            config: alien_core::Resource::new(
                RemoteStackManagement::new("management".to_string()).build(),
            ),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
        }
    }

    #[tokio::test]
    async fn test_adds_secrets_vault_and_links_to_function() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test-profile".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        resources.insert("management".to_string(), remote_stack_management_entry());

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
            inputs: vec![],
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

        // Check that vault was linked to the worker
        let function_entry = result_stack.resources.get("test-worker").unwrap();
        let worker = function_entry.config.downcast_ref::<Worker>().unwrap();
        assert!(
            worker.links.iter().any(|link| link.id() == "secrets"),
            "Worker should be linked to secrets vault"
        );

        // Check that vault/data-read was added to worker profile
        let function_profile = result_stack
            .permissions
            .profiles
            .get("test-profile")
            .unwrap();
        let vault_permissions = function_profile.0.get("secrets").unwrap();
        assert!(vault_permissions
            .iter()
            .any(|p| p.id() == "vault/data-read"));
    }

    #[tokio::test]
    async fn test_skips_machines_without_explicit_secrets_vault() {
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
            inputs: vec![],
        };

        let stack_state = StackState::new(Platform::Machines);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = SecretsVaultMutation;

        assert!(!mutation.should_run(&stack, &stack_state, &config));
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
            inputs: vec![],
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
        let vault_permissions = container_profile.0.get("secrets").unwrap();
        assert!(vault_permissions
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
            inputs: vec![],
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
        let worker = Worker::new("api".to_string())
            .code(WorkerCode::Image {
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
                config: alien_core::Resource::new(worker),
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
            inputs: vec![],
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
        let worker = result_stack
            .resources
            .get("api")
            .unwrap()
            .config
            .downcast_ref::<Worker>()
            .unwrap();
        assert!(worker.links.iter().any(|link| link.id() == "secrets"));

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
            let vault_permissions = profile.0.get("secrets").unwrap();
            assert!(
                vault_permissions
                    .iter()
                    .any(|p| p.id() == "vault/data-read"),
                "Profile {} should have vault/data-read",
                profile_name
            );
        }
    }

    #[tokio::test]
    async fn test_adds_vault_data_permissions_to_management() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test-profile".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        resources.insert("management".to_string(), remote_stack_management_entry());

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
            inputs: vec![],
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

        // Check that management profile has vault data permissions scoped to the secrets vault.
        match result_stack.permissions.management {
            ManagementPermissions::Extend(profile) => {
                assert!(
                    profile.0.get("*").map_or(true, |permissions| !permissions
                        .iter()
                        .any(|p| p.id() == "vault/data-write")),
                    "Management profile should not have global vault/data-write"
                );
                let vault_permissions = profile.0.get("secrets").unwrap();
                assert!(
                    vault_permissions
                        .iter()
                        .any(|p| p.id() == "vault/data-read"),
                    "Management profile should have vault/data-read for secrets vault"
                );
                assert!(
                    vault_permissions
                        .iter()
                        .any(|p| p.id() == "vault/data-write"),
                    "Management profile should have vault/data-write for secrets vault"
                );
                assert!(
                    profile.0.get("alien-vault").is_none(),
                    "Management profile should not get vault/data-write for user vaults by default"
                );
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_preserves_explicit_global_vault_data_write_and_adds_secrets_scope() {
        let user_vault = Vault::new("alien-vault".to_string()).build();
        let mut resources = IndexMap::new();
        resources.insert(
            "alien-vault".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(user_vault),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        resources.insert("management".to_string(), remote_stack_management_entry());

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Extend(
                    PermissionProfile::new().global(["vault/data-write", "storage/heartbeat"]),
                ),
            },
            supported_platforms: None,
            inputs: vec![],
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

        match result_stack.permissions.management {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                assert!(
                    global_permissions
                        .iter()
                        .any(|p| p.id() == "storage/heartbeat"),
                    "Unrelated global permissions should be preserved"
                );
                assert!(
                    global_permissions
                        .iter()
                        .any(|p| p.id() == "vault/data-write"),
                    "Explicit global vault/data-write should be preserved"
                );
                assert!(
                    profile
                        .0
                        .get("secrets")
                        .unwrap()
                        .iter()
                        .any(|p| p.id() == "vault/data-read"),
                    "vault/data-read should be added to the secrets vault resource"
                );
                assert!(
                    profile
                        .0
                        .get("secrets")
                        .unwrap()
                        .iter()
                        .any(|p| p.id() == "vault/data-write"),
                    "vault/data-write should be added to the secrets vault resource"
                );
                assert!(
                    profile.0.get("alien-vault").is_none(),
                    "user vaults should not receive resource-specific management vault/data-write by default"
                );
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_respects_override_management_permissions() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test-profile".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
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
            inputs: vec![],
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

    #[tokio::test]
    async fn test_skips_management_permissions_without_remote_stack_management() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test-profile".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
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
            inputs: vec![],
        };

        let stack_state = StackState::new(Platform::Local);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = SecretsVaultMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        assert!(result_stack.resources.contains_key("secrets"));
        assert!(
            matches!(
                result_stack.permissions.management,
                ManagementPermissions::Auto
            ),
            "Local stacks without remote stack management should not get resource-scoped management permissions"
        );
    }
}
