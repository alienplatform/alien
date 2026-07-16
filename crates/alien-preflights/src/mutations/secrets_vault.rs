//! Adds the deployment secrets vault and grants runtime access only to Worker
//! wrappers that need vault-backed app or runtime secrets.

use crate::error::Result;
use crate::StackMutation;
use alien_core::permissions::{PermissionProfile, PermissionSetReference};
use alien_core::{
    ComputeCluster, ComputeKind, DeploymentConfig, Platform, RemoteStackManagement, ResourceEntry,
    ResourceLifecycle, ResourceRef, SecretDelivery, Stack, StackState, Vault, Worker,
};
use async_trait::async_trait;
use tracing::{debug, info};

/// Adds secrets vault for environment variable storage.
///
/// Creates the "secrets" vault (if missing). Worker wrappers receive the vault
/// link/read permission only when needed for app or runtime-owned secrets.
/// Runtime-less Containers and Daemons receive secrets from their hosting
/// layer and must not get vault data-plane access.
///
/// Steps:
/// 1. Add "secrets" vault resource (if not present)
/// 2. Link the vault to Worker runtimes that consume vault-backed secrets
/// 3. Add vault/data-read to those Worker profiles
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

        // Always run to ensure required vault permissions are present
        // even if the vault resource already exists (idempotent)
        true
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<Stack> {
        info!("Adding deployment secrets vault and scoped runtime access");

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

        // Native-projected Workers need the vault only when their wrapper has
        // runtime-owned monitoring credentials. Other Worker hosts consume the
        // vault-backed app-secret pointer regardless of monitoring.
        let worker_vault_access_required =
            !SecretDelivery::resolve(stack_state.platform, ComputeKind::Worker)
                .is_native_projection()
                || config.monitoring.is_some();
        if worker_vault_access_required {
            link_vault_to_worker_runtimes(&mut stack, secrets_vault_id)?;
            add_vault_read_permissions_to_worker_profiles(&mut stack, secrets_vault_id)?;
        }
        add_vault_dependency_to_compute_clusters(&mut stack, secrets_vault_id);

        // Step 4: Add vault data permissions to management profile for the secrets vault.
        // This allows the control plane to sync secret environment variables without
        // granting access to user-declared vaults.
        add_vault_permissions_to_management(&mut stack, secrets_vault_id)?;

        Ok(stack)
    }
}

/// Compute-cluster machine identities receive narrowly scoped read access to the
/// deployment secrets vault. Make the Frozen vault an explicit dependency so
/// provider-specific ComputeCluster controllers never assign that access
/// against a vault whose outputs or cloud resource do not exist yet.
fn add_vault_dependency_to_compute_clusters(stack: &mut Stack, vault_id: &str) {
    let vault_ref = ResourceRef::new(Vault::RESOURCE_TYPE, vault_id);

    for (resource_id, entry) in &mut stack.resources {
        if entry.config.resource_type() != ComputeCluster::RESOURCE_TYPE {
            continue;
        }
        if entry
            .dependencies
            .iter()
            .any(|dependency| dependency == &vault_ref)
        {
            continue;
        }

        entry.dependencies.push(vault_ref.clone());
        debug!(
            compute_cluster = %resource_id,
            vault = %vault_id,
            "Made the compute cluster depend on its deployment secrets vault"
        );
    }
}

/// Link the secrets vault only to Worker wrappers selected by the caller.
fn link_vault_to_worker_runtimes(stack: &mut Stack, vault_id: &str) -> Result<()> {
    let vault_ref = ResourceRef::new(Vault::RESOURCE_TYPE, vault_id);
    let mut linked_count = 0;

    for (resource_id, entry) in &mut stack.resources {
        let resource_type = entry.config.resource_type();

        if resource_type != Worker::RESOURCE_TYPE {
            continue;
        }

        let Some(worker) = entry.config.downcast_mut::<Worker>() else {
            continue;
        };

        // Add vault link if not already present
        if !worker.links.iter().any(|link| link.id() == vault_id) {
            worker.links.push(vault_ref.clone());
            linked_count += 1;
            debug!("Linked secrets vault to compute resource '{}'", resource_id);
        }
    }

    if linked_count > 0 {
        info!("Linked secrets vault to {linked_count} Worker runtimes");
    }

    Ok(())
}

/// Add vault/data-read only to Worker profiles selected by the caller.
fn add_vault_read_permissions_to_worker_profiles(
    stack: &mut Stack,
    vault_name: &str,
) -> Result<()> {
    // Get Worker permission profile names.
    let profile_names: Vec<String> = stack
        .resources
        .iter()
        .filter_map(|(_, entry)| {
            let resource_type = entry.config.resource_type();

            if resource_type != Worker::RESOURCE_TYPE {
                return None;
            }

            entry
                .config
                .downcast_ref::<Worker>()
                .map(|worker| worker.permissions.clone())
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

    #[test]
    fn compute_cluster_depends_on_secrets_vault_exactly_once() {
        let mut stack = Stack {
            id: "test-stack".to_string(),
            resources: IndexMap::from([(
                "compute".to_string(),
                ResourceEntry {
                    config: alien_core::Resource::new(
                        ComputeCluster::new("compute".to_string()).build(),
                    ),
                    lifecycle: ResourceLifecycle::Frozen,
                    dependencies: Vec::new(),
                    remote_access: false,
                },
            )]),
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        add_vault_dependency_to_compute_clusters(&mut stack, "secrets");
        add_vault_dependency_to_compute_clusters(&mut stack, "secrets");

        let dependencies = &stack
            .resources
            .get("compute")
            .expect("compute cluster")
            .dependencies;
        assert_eq!(
            dependencies,
            &[ResourceRef::new(Vault::RESOURCE_TYPE, "secrets")]
        );
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
        let result_stack = mutation
            .mutate(stack.clone(), &stack_state, &config)
            .await
            .unwrap();

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

        // Kubernetes projects app secrets natively. Without runtime monitoring,
        // its Worker wrapper has no reason to access the vault.
        let kubernetes_stack = mutation
            .mutate(
                stack.clone(),
                &StackState::new(Platform::Kubernetes),
                &config,
            )
            .await
            .unwrap();
        let worker = kubernetes_stack
            .resources
            .get("test-worker")
            .unwrap()
            .config
            .downcast_ref::<Worker>()
            .unwrap();
        assert!(worker.links.iter().all(|link| link.id() != "secrets"));
        assert!(kubernetes_stack
            .permissions
            .profiles
            .get("test-profile")
            .unwrap()
            .0
            .get("secrets")
            .is_none());

        // Runtime monitoring adds a vault-backed ALIEN_RUNTIME_SECRETS pointer
        // for the Worker wrapper, so the link and read permission are required.
        let mut monitoring_config = config;
        monitoring_config.monitoring = Some(alien_core::OtlpConfig {
            logs_endpoint: "https://example.com/v1/logs".to_string(),
            logs_auth_header: "authorization=Bearer test".to_string(),
            metrics_endpoint: None,
            metrics_auth_header: None,
            resource_attributes: Default::default(),
        });
        let kubernetes_stack = mutation
            .mutate(
                stack,
                &StackState::new(Platform::Kubernetes),
                &monitoring_config,
            )
            .await
            .unwrap();
        let worker = kubernetes_stack
            .resources
            .get("test-worker")
            .unwrap()
            .config
            .downcast_ref::<Worker>()
            .unwrap();
        assert!(worker.links.iter().any(|link| link.id() == "secrets"));
        assert!(kubernetes_stack
            .permissions
            .profiles
            .get("test-profile")
            .unwrap()
            .0
            .get("secrets")
            .unwrap()
            .iter()
            .any(|permission| permission.id() == "vault/data-read"));
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
    async fn runtime_less_container_gets_no_vault_link_or_read_permission() {
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

        // The hosting layer projects Container secrets before process start.
        let container_entry = result_stack.resources.get("test-container").unwrap();
        let container = container_entry.config.downcast_ref::<Container>().unwrap();
        assert!(
            container.links.iter().all(|link| link.id() != "secrets"),
            "runtime-less Container must not receive the secrets vault binding"
        );

        let container_profile = result_stack
            .permissions
            .profiles
            .get("test-profile")
            .unwrap();
        assert!(
            !container_profile.0.contains_key("secrets"),
            "runtime-less Container profile must not get vault data-plane access"
        );
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

        // Only the Worker wrapper consumes vault pointers.
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
        assert!(container.links.iter().all(|link| link.id() != "secrets"));

        let api_profile = result_stack
            .permissions
            .profiles
            .get("api-profile")
            .unwrap();
        assert!(api_profile
            .0
            .get("secrets")
            .expect("Worker vault grant")
            .iter()
            .any(|permission| permission.id() == "vault/data-read"));
        let container_profile = result_stack
            .permissions
            .profiles
            .get("worker-profile")
            .unwrap();
        assert!(!container_profile.0.contains_key("secrets"));
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
