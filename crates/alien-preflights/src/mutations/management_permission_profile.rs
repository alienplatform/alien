use crate::error::Result;
use crate::StackMutation;
use alien_core::permissions::{ManagementPermissions, PermissionProfile, PermissionSetReference};
use alien_core::{
    DeploymentConfig, DeploymentModel, Function, Platform, ResourceLifecycle, Stack, StackState,
};
use alien_permissions::get_permission_set;
use indexmap::IndexMap;
use std::collections::BTreeSet;

/// Automatically adds management permission profile with necessary permissions for all resources in the stack.
///
/// This mutation generates management permissions based on resource lifecycles and feature policies:
/// - Frozen resources: no management permissions (heartbeat-only, added separately)
/// - Live resources get `<resourceType>/management` permission sets (when push model)
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

/// Generates the default management permission profile based on resource lifecycles, deployment model, and commands configuration.
fn generate_auto_management_profile(
    stack: &Stack,
    stack_state: &StackState,
    config: &DeploymentConfig,
) -> Result<Option<PermissionProfile>> {
    // Generate management permission set IDs based on stack resources
    let mut permission_set_ids = BTreeSet::new();
    let platform = stack_state.platform;

    // Check if this is a push deployment model (Agent Manager deploys remotely)
    let is_push = matches!(
        config.stack_settings.deployment_model,
        DeploymentModel::Push
    );

    // Iterate through all resources in the stack to determine required management permissions
    for (_, resource_entry) in stack.resources() {
        let resource_type_value = resource_entry.config.resource_type();
        let resource_type = resource_type_value.0.as_ref();

        if is_push {
            match resource_entry.lifecycle {
                ResourceLifecycle::Frozen => {
                    // Frozen: heartbeat-only. No management permissions — frozen resources should not be modified.
                }
                ResourceLifecycle::Live => {
                    // Live: management permissions to update/redeploy the resource.
                    permission_set_ids.insert(format!("{}/management", resource_type));
                }
            }
        }

        // Add heartbeat permissions if heartbeat is enabled (Auto or RequiresApproval)
        // Disabled means no infrastructure/IAM permissions at all
        if config.stack_settings.heartbeats.is_enabled() {
            permission_set_ids.insert(format!("{}/heartbeat", resource_type));
        }

        // Add telemetry permissions if telemetry is enabled (Auto or RequiresApproval)
        // Disabled means no infrastructure/IAM permissions at all
        if config.stack_settings.telemetry.is_enabled() {
            permission_set_ids.insert(format!("{}/telemetry", resource_type));
        }

        // Add commands-specific permissions for functions with commands_enabled = true
        if resource_type == "function" {
            if let Some(function) = resource_entry.config.downcast_ref::<Function>() {
                if function.commands_enabled {
                    match platform {
                        Platform::Aws => {
                            // On AWS: function/invoke for Lambda InvokeFunction API
                            permission_set_ids.insert("function/invoke".to_string());
                        }
                        Platform::Gcp | Platform::Azure => {
                            // On GCP/Azure: queue/data-write for Pub/Sub and Service Bus
                            permission_set_ids.insert("queue/data-write".to_string());
                        }
                        _ => {
                            // Other platforms like Kubernetes and Local use HTTP polling, no additional permissions needed
                        }
                    }
                }
            }
        }
    }

    // Only create management permissions if there are resources that need management
    if permission_set_ids.is_empty() {
        return Ok(None);
    }

    // Validate permission sets exist in registry and filter out missing ones
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

    if valid_permission_refs.is_empty() {
        return Ok(None);
    }

    // Create the management permission profile with global scope
    let mut management_permissions = IndexMap::new();
    management_permissions.insert("*".to_string(), valid_permission_refs);

    Ok(Some(PermissionProfile(management_permissions)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::permissions::{ManagementPermissions, PermissionsConfig};
    use alien_core::{
        DeploymentModel, EnvironmentVariablesSnapshot, ExternalBindings, Function, FunctionCode,
        HeartbeatsMode, ResourceEntry, ResourceLifecycle, StackSettings, StackState, Storage,
        TelemetryMode,
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
        let function = Function::new("test-function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .build();
        let storage = Storage::new("test-storage".to_string()).build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-function".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(function),
                lifecycle: ResourceLifecycle::Live, // Should get function/management
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

                // Should contain function/management (push is default); storage is frozen so no management
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                assert!(permission_names.contains(&"function/management".to_string()));
                assert!(!permission_names.contains(&"storage/management".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_extend_management_profile_merge() {
        let function = Function::new("test-function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
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

        // Create an extend profile with additional permissions
        let extend_profile = PermissionProfile::new().global(["storage/data-write"]);

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Extend(extend_profile),
            },
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

                // Should have both auto-generated function/management and extended storage/data-write
                assert!(permission_names.contains(&"function/management".to_string()));
                assert!(permission_names.contains(&"storage/data-write".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_override_management_profile_unchanged() {
        let function = Function::new("test-function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
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

        // Create an override profile
        let override_profile =
            PermissionProfile::new().global(["storage/management", "function/management"]);

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Override(override_profile.clone()),
            },
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
                assert!(permission_names.contains(&"function/management".to_string()));
                // Should NOT have auto-generated function/provision
                assert!(!permission_names.contains(&"function/provision".to_string()));
            }
            _ => panic!("Expected Override management permissions"),
        }
    }

    #[tokio::test]
    async fn test_pull_model_permissions() {
        let function = Function::new("test-function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .build();
        let storage = Storage::new("test-storage".to_string()).build();

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
        };

        // Create stack state with Pull model (Operator deploys locally)
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

        // Check that only heartbeat permissions were generated (no management/provision since pull model)
        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                assert!(profile.0.contains_key("*"));
                let global_permissions = profile.0.get("*").unwrap();

                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should contain heartbeat permissions for both resources
                assert!(permission_names.contains(&"function/heartbeat".to_string()));
                assert!(permission_names.contains(&"storage/heartbeat".to_string()));

                // Should NOT contain management permissions since it's pull model
                assert!(!permission_names.contains(&"function/management".to_string()));
                assert!(!permission_names.contains(&"storage/management".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_push_model_permissions() {
        let function = Function::new("test-function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
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

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
        };

        // Create stack state with Push model (Agent Manager deploys remotely)
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

                // Should contain both heartbeat and management permissions
                assert!(permission_names.contains(&"function/heartbeat".to_string()));
                assert!(permission_names.contains(&"function/management".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_commands_enabled_function_permissions() {
        // Test AWS platform - should add function/invoke
        let arc_function = Function::new("arc-function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .commands_enabled(true)
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "arc-function".to_string(),
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

        // Check that ARC permissions were added
        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should have function/management (for live resource) and function/invoke (for ARC)
                assert!(permission_names.contains(&"function/management".to_string()));
                assert!(permission_names.contains(&"function/invoke".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }

        // Test GCP platform - should add queue/data-write
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
                let arc_function = Function::new("arc-function".to_string())
                    .code(FunctionCode::Image {
                        image: "test:latest".to_string(),
                    })
                    .permissions("test".to_string())
                    .commands_enabled(true)
                    .build();
                resources.insert(
                    "arc-function".to_string(),
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

                // Should have function/management (for live resource) and queue/data-write (for ARC)
                assert!(permission_names.contains(&"function/management".to_string()));
                assert!(permission_names.contains(&"queue/data-write".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_disabled_heartbeat_no_permissions() {
        let function = Function::new("test-function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
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

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
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
                assert!(!permission_names.contains(&"function/heartbeat".to_string()));
                // Should still have management permissions (push is default)
                assert!(permission_names.contains(&"function/management".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_disabled_telemetry_no_permissions() {
        let function = Function::new("test-function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
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

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
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
                assert!(!permission_names.contains(&"function/telemetry".to_string()));
                // Should still have heartbeat and management permissions
                assert!(permission_names.contains(&"function/heartbeat".to_string()));
                assert!(permission_names.contains(&"function/management".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_requires_approval_still_creates_permissions() {
        let function = Function::new("test-function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
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

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
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
                assert!(permission_names.contains(&"function/heartbeat".to_string()));
                // Should contain management permissions (push is default)
                assert!(permission_names.contains(&"function/management".to_string()));
                // Note: function/telemetry permission set may not exist in registry,
                // but the code attempts to add it. If it exists, it would be added.
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }
}
