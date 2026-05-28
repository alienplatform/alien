use crate::error::Result;
use crate::StackMutation;
use alien_core::permissions::{ManagementPermissions, PermissionProfile, PermissionSetReference};
use alien_core::{
    ownership_policy_for_resource_type, DeploymentConfig, KubernetesCluster,
    KubernetesHeartbeatMode, Platform, ResourceLifecycle, Stack, StackState, Worker,
};
use alien_permissions::get_permission_set;
use indexmap::IndexMap;
use std::collections::BTreeSet;

/// Automatically adds management permission profile with necessary permissions for all resources in the stack.
///
/// This mutation generates management permissions based on resource lifecycles and feature policies:
/// - Live resources get `<resourceType>/provision`.
/// - Frozen resources get management only when their ownership policy allows
///   Alien to operate part of an existing setup-owned resource.
/// - Resources get `<resourceType>/heartbeat` permission sets (when heartbeat is not Disabled)
/// - Resources get `<resourceType>/telemetry` permission sets (when telemetry is not Disabled)
pub struct ManagementPermissionProfileMutation;

#[async_trait::async_trait]
impl StackMutation for ManagementPermissionProfileMutation {
    fn description(&self) -> &'static str {
        "Automatically add management permission profile with necessary permissions for all resources in the stack"
    }

    fn should_run(
        &self,
        stack: &Stack,
        _stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        // Only run if management is set to Auto or if we need to extend existing permissions
        match stack.management() {
            ManagementPermissions::Auto => true,
            ManagementPermissions::Extend(_) => true,
            ManagementPermissions::Override(_) => false, // Don't modify override settings
        }
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<Stack> {
        let current_management = stack.management().clone();

        match current_management {
            ManagementPermissions::Auto => {
                // Auto-generate management permissions based on resource lifecycles
                let management_profile =
                    generate_auto_management_profile(&stack, stack_state, config)?;
                if let Some(profile) = management_profile {
                    stack.permissions.management = ManagementPermissions::Extend(profile);
                }
            }
            ManagementPermissions::Extend(extend_profile) => {
                // Generate auto permissions first, then merge with extend profile
                let auto_profile = generate_auto_management_profile(&stack, stack_state, config)?;
                if let Some(mut auto_profile) = auto_profile {
                    // Merge the extend profile into the auto-generated profile
                    for (scope, permission_sets) in &extend_profile.0 {
                        if let Some(existing_permissions) = auto_profile.0.get_mut(scope) {
                            // Add new permission sets, avoiding duplicates
                            for permission_set in permission_sets {
                                if !existing_permissions.contains(permission_set) {
                                    existing_permissions.push(permission_set.clone());
                                }
                            }
                        } else {
                            // Add new scope if it doesn't exist
                            auto_profile
                                .0
                                .insert(scope.clone(), permission_sets.clone());
                        }
                    }
                    stack.permissions.management = ManagementPermissions::Extend(auto_profile);
                } else {
                    // No auto permissions generated, keep the extend profile as is
                    stack.permissions.management = ManagementPermissions::Extend(extend_profile);
                }
            }
            ManagementPermissions::Override(_) => {
                // Don't modify override settings
            }
        }

        Ok(stack)
    }
}

/// Generates the default management permission profile from resource ownership
/// and feature settings.
fn generate_auto_management_profile(
    stack: &Stack,
    stack_state: &StackState,
    config: &DeploymentConfig,
) -> Result<Option<PermissionProfile>> {
    // Generate management permission set IDs based on stack resources
    let mut permission_set_ids = BTreeSet::new();
    let mut resource_permission_set_ids: IndexMap<String, BTreeSet<String>> = IndexMap::new();
    let platform = stack_state.platform;

    // Iterate through all resources in the stack to determine required management permissions
    for (resource_id, resource_entry) in stack.resources() {
        let resource_type_value = resource_entry.config.resource_type();
        let resource_type = resource_type_value.0.as_ref();
        let policy = ownership_policy_for_resource_type(resource_type);

        match resource_entry.lifecycle {
            ResourceLifecycle::Live => {
                // Live resources are Alien-owned. Provision is required so
                // Alien can create, replace, and delete them after setup.
                permission_set_ids.insert(format!("{}/provision", resource_type));
            }
            ResourceLifecycle::Frozen if policy.frozen_requires_management() => {
                permission_set_ids.insert(format!("{}/management", resource_type));
            }
            ResourceLifecycle::Frozen => {
                // Frozen resources are setup-owned by default. Heartbeat,
                // telemetry, and explicit policy-granted management are added
                // independently.
            }
        }

        // Add heartbeat permissions if heartbeat is enabled (Auto or RequiresApproval)
        // Disabled means no infrastructure/IAM permissions at all
        if config.stack_settings.heartbeats.is_enabled()
            && resource_needs_cloud_heartbeat_permission(resource_type, resource_entry)
        {
            permission_set_ids.insert(format!("{}/heartbeat", resource_type));
        }

        // Add telemetry permissions if telemetry is enabled (Auto or RequiresApproval)
        // Disabled means no infrastructure/IAM permissions at all
        if config.stack_settings.telemetry.is_enabled() {
            permission_set_ids.insert(format!("{}/telemetry", resource_type));
        }

        // Add command dispatch permissions for workers with commands_enabled = true.
        if resource_type == "worker" {
            if let Some(worker) = resource_entry.config.downcast_ref::<Worker>() {
                if worker.commands_enabled {
                    match platform {
                        Platform::Aws | Platform::Gcp | Platform::Azure => {
                            // Preflights author the explicit command dispatch grant for
                            // this concrete worker. Each cloud maps it to that worker's
                            // platform command transport.
                            resource_permission_set_ids
                                .entry(resource_id.clone())
                                .or_default()
                                .insert("worker/dispatch-command".to_string());
                        }
                        _ => {
                            // Other platforms like Kubernetes and Local use HTTP polling.
                        }
                    }
                }
            }
        }
    }

    // Only create management permissions if there are resources that need management
    if permission_set_ids.is_empty() && resource_permission_set_ids.is_empty() {
        return Ok(None);
    }

    fn resolve_permission_refs(
        permission_set_ids: BTreeSet<String>,
    ) -> Vec<PermissionSetReference> {
        let mut valid_permission_refs = Vec::new();
        for permission_set_id in permission_set_ids {
            if get_permission_set(&permission_set_id).is_some() {
                valid_permission_refs.push(PermissionSetReference::from_name(permission_set_id));
            } else {
                // Log warning but continue - allows system to work even if some permission sets are missing
                tracing::debug!(
                    permission_set_id = %permission_set_id,
                    "Management permission set not found in registry, skipping"
                );
            }
        }
        valid_permission_refs
    }

    // Validate permission sets exist in registry and filter out missing ones
    let valid_permission_refs = resolve_permission_refs(permission_set_ids);

    // Create the management permission profile. Auto lifecycle permissions are
    // wildcard-scoped; command dispatch grants are scoped to the concrete worker
    // whose command transport they target.
    let mut management_permissions = IndexMap::new();
    if !valid_permission_refs.is_empty() {
        management_permissions.insert("*".to_string(), valid_permission_refs);
    }

    for (resource_id, permission_set_ids) in resource_permission_set_ids {
        let valid_resource_refs = resolve_permission_refs(permission_set_ids);
        if !valid_resource_refs.is_empty() {
            management_permissions.insert(resource_id, valid_resource_refs);
        }
    }

    if management_permissions.is_empty() {
        return Ok(None);
    }

    Ok(Some(PermissionProfile(management_permissions)))
}

fn resource_needs_cloud_heartbeat_permission(
    resource_type: &str,
    resource_entry: &alien_core::ResourceEntry,
) -> bool {
    if resource_type != KubernetesCluster::RESOURCE_TYPE.as_ref() {
        return true;
    }

    resource_entry
        .config
        .downcast_ref::<KubernetesCluster>()
        .is_some_and(|cluster| {
            cluster.heartbeat_mode == KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::permissions::{ManagementPermissions, PermissionsConfig};
    use alien_core::{
        ArtifactRegistry, CapacityGroup, ComputeCluster, Container, ContainerCode, DeploymentModel,
        EnvironmentVariablesSnapshot, ExternalBindings, HeartbeatsMode, KubernetesCluster,
        KubernetesClusterOwnership, KubernetesClusterProvider, KubernetesHeartbeatMode,
        ResourceEntry, ResourceLifecycle, ResourceSpec, StackSettings, StackState, Storage,
        TelemetryMode, Worker, WorkerCode,
    };

    fn empty_env_snapshot() -> EnvironmentVariablesSnapshot {
        EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    #[tokio::test]
    async fn test_auto_management_profile_generation() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .build();
        let storage = Storage::new("test-storage".to_string()).build();

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
        resources.insert(
            "test-storage".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(storage),
                lifecycle: ResourceLifecycle::Frozen, // Should get nothing (heartbeat only)
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
        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Check that management permissions were generated
        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                assert!(profile.0.contains_key("*"));
                let global_permissions = profile.0.get("*").unwrap();

                // Live worker gets provision; storage is frozen so no management.
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
                assert!(!permission_names.contains(&"storage/management".to_string()));
                assert!(!permission_names.contains(&"aws/tag-tamper-protection".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn kubernetes_cluster_cloud_metadata_heartbeat_is_explicit() {
        let cluster = KubernetesCluster::new("kubernetes".to_string())
            .provider(KubernetesClusterProvider::Eks)
            .ownership(KubernetesClusterOwnership::Managed)
            .namespace("default".to_string())
            .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
            .build();

        let stack = Stack::new("test-stack".to_string())
            .add(cluster, ResourceLifecycle::Frozen)
            .management(ManagementPermissions::Auto)
            .build();
        let stack_state = StackState::new(Platform::Kubernetes);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let result_stack = ManagementPermissionProfileMutation
            .mutate(stack, &stack_state, &config)
            .await
            .unwrap();

        let ManagementPermissions::Extend(profile) = result_stack.management() else {
            panic!("Expected Extend management permissions");
        };
        let permission_names: Vec<String> = profile
            .0
            .get("*")
            .unwrap()
            .iter()
            .map(|perm_ref| perm_ref.id().to_string())
            .collect();
        assert!(permission_names.contains(&"kubernetes-cluster/heartbeat".to_string()));
        assert!(!permission_names.contains(&"compute-cluster/heartbeat".to_string()));
    }

    #[tokio::test]
    async fn kubernetes_api_only_heartbeat_gets_no_cloud_metadata_permission() {
        let cluster = KubernetesCluster::new("kubernetes".to_string())
            .provider(KubernetesClusterProvider::Generic)
            .ownership(KubernetesClusterOwnership::External)
            .namespace("default".to_string())
            .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApi)
            .build();

        let stack = Stack::new("test-stack".to_string())
            .add(cluster, ResourceLifecycle::Frozen)
            .management(ManagementPermissions::Auto)
            .build();
        let stack_state = StackState::new(Platform::Kubernetes);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let result_stack = ManagementPermissionProfileMutation
            .mutate(stack, &stack_state, &config)
            .await
            .unwrap();

        assert!(
            result_stack.management().profile().is_none(),
            "API-only Kubernetes heartbeat should not author cloud IAM permissions"
        );
    }

    #[tokio::test]
    async fn test_extend_management_profile_merge() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
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

        // Create an extend profile with additional permissions
        let extend_profile = PermissionProfile::new().global(["storage/data-write"]);

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Extend(extend_profile),
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
        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Check that both auto-generated and extended permissions are present
        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should have auto-generated live provision and extended storage/data-write.
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
                assert!(permission_names.contains(&"storage/data-write".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_override_management_profile_unchanged() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
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

        // Create an override profile
        let override_profile =
            PermissionProfile::new().global(["storage/management", "worker/management"]);

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
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
        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Check that override profile is unchanged
        match result_stack.management() {
            ManagementPermissions::Override(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                assert_eq!(global_permissions.len(), 2);

                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                assert!(permission_names.contains(&"storage/management".to_string()));
                assert!(permission_names.contains(&"worker/management".to_string()));
                // Should NOT have auto-generated worker/provision
                assert!(!permission_names.contains(&"worker/provision".to_string()));
            }
            _ => panic!("Expected Override management permissions"),
        }
    }

    #[tokio::test]
    async fn test_pull_model_permissions() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .build();
        let storage = Storage::new("test-storage".to_string()).build();

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
        resources.insert(
            "test-storage".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(storage),
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

        // Create stack state with Pull model. Permission derivation is model
        // independent: the credentials are attached differently, but the
        // resource operations required by Alien are the same.
        let stack_settings = StackSettings {
            deployment_model: DeploymentModel::Pull,
            ..Default::default()
        };
        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(stack_settings)
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                assert!(profile.0.contains_key("*"));
                let global_permissions = profile.0.get("*").unwrap();

                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should contain heartbeat permissions for both resources
                assert!(permission_names.contains(&"worker/heartbeat".to_string()));
                assert!(permission_names.contains(&"storage/heartbeat".to_string()));

                // Should contain live resource mutation permissions in both
                // Pull and Push models.
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
                assert!(!permission_names.contains(&"storage/management".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_live_container_and_frozen_cluster_permissions() {
        let container = Container::new("web".to_string())
            .code(ContainerCode::Image {
                image: "test:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("test".to_string())
            .build();
        let cluster = ComputeCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: Some("m7g.large".to_string()),
                profile: None,
                min_size: 1,
                max_size: 3,
            })
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "web".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(container),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        resources.insert(
            "compute".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(cluster),
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

        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let permission_names: Vec<String> = profile
                    .0
                    .get("*")
                    .unwrap()
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                assert!(permission_names.contains(&"container/provision".to_string()));
                assert!(!permission_names.contains(&"container/management".to_string()));
                assert!(permission_names.contains(&"compute-cluster/management".to_string()));
                assert!(!permission_names.contains(&"compute-cluster/provision".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_frozen_artifact_registry_gets_management_permissions() {
        let registry = ArtifactRegistry::new("registry".to_string()).build();

        let mut resources = IndexMap::new();
        resources.insert(
            "registry".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(registry),
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

        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let permission_names: Vec<String> = profile
                    .0
                    .get("*")
                    .unwrap()
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                assert!(permission_names.contains(&"artifact-registry/management".to_string()));
                assert!(!permission_names.contains(&"artifact-registry/provision".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_push_model_permissions() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
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

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        // Create stack state with Push model (Manager deploys remotely)
        let stack_settings = StackSettings {
            deployment_model: DeploymentModel::Push,
            ..Default::default()
        };
        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(stack_settings)
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Check that both heartbeat and provision permissions were generated
        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should contain heartbeat plus live mutation permissions.
                assert!(permission_names.contains(&"worker/heartbeat".to_string()));
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_commands_enabled_function_permissions() {
        // Test AWS platform - should add resource-scoped worker/dispatch-command.
        let arc_function = Worker::new("arc-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .commands_enabled(true)
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "arc-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(arc_function),
                lifecycle: ResourceLifecycle::Live,
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
        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Check that Commands permissions were added
        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should have live resource permissions globally; command dispatch is resource-scoped.
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
                assert!(!permission_names.contains(&"worker/dispatch-command".to_string()));

                let worker_permissions = profile.0.get("arc-worker").unwrap();
                let worker_permission_names: Vec<String> = worker_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();
                assert!(worker_permission_names.contains(&"worker/dispatch-command".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }

        // Test GCP platform - should author an explicit command dispatch grant.
        let stack_state_gcp = StackState::new(Platform::Gcp);
        let config_gcp = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let stack_gcp = Stack {
            id: "test-stack-gcp".to_string(),
            resources: {
                let mut resources = IndexMap::new();
                let arc_function = Worker::new("arc-worker".to_string())
                    .code(WorkerCode::Image {
                        image: "test:latest".to_string(),
                    })
                    .permissions("test".to_string())
                    .commands_enabled(true)
                    .build();
                resources.insert(
                    "arc-worker".to_string(),
                    ResourceEntry {
                        config: alien_core::Resource::new(arc_function),
                        lifecycle: ResourceLifecycle::Live,
                        dependencies: Vec::new(),
                        remote_access: false,
                    },
                );
                resources
            },
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        let result_stack_gcp = mutation
            .mutate(stack_gcp, &stack_state_gcp, &config_gcp)
            .await
            .unwrap();

        match result_stack_gcp.management() {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should have live resource permissions globally; command dispatch is resource-scoped.
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
                assert!(!permission_names.contains(&"worker/dispatch-command".to_string()));

                let worker_permissions = profile.0.get("arc-worker").unwrap();
                let worker_permission_names: Vec<String> = worker_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();
                assert!(worker_permission_names.contains(&"worker/dispatch-command".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_disabled_heartbeat_no_permissions() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
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

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        // Disable heartbeat
        let stack_settings = StackSettings {
            heartbeats: HeartbeatsMode::Off,
            ..Default::default()
        };
        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(stack_settings)
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should NOT contain heartbeat permissions since heartbeat is disabled
                assert!(!permission_names.contains(&"worker/heartbeat".to_string()));
                // Should still have live mutation permissions.
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_disabled_telemetry_no_permissions() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
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

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        // Disable telemetry
        let stack_settings = StackSettings {
            telemetry: TelemetryMode::Off,
            ..Default::default()
        };
        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(stack_settings)
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should NOT contain telemetry permissions since telemetry is disabled
                assert!(!permission_names.contains(&"worker/telemetry".to_string()));
                // Should still have heartbeat and live mutation permissions.
                assert!(permission_names.contains(&"worker/heartbeat".to_string()));
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_requires_approval_still_creates_permissions() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
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

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        // Require approval for telemetry - permissions should still be created
        // Note: ApprovalRequired is a runtime check, permissions are infrastructure.
        // When telemetry is ApprovalRequired (not Off), permissions should be created
        // for telemetry collection. The approval gate happens at runtime.
        let stack_settings = StackSettings {
            telemetry: TelemetryMode::ApprovalRequired,
            ..Default::default()
        };
        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(stack_settings)
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should contain heartbeat permissions (heartbeat is On by default)
                assert!(permission_names.contains(&"worker/heartbeat".to_string()));
                // Should contain live mutation permissions.
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
                // Note: worker/telemetry permission set may not exist in registry,
                // but the code attempts to add it. If it exists, it would be added.
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }
}
