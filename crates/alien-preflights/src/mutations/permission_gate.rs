//! Applies deploy-time permission gates declared on the stack.
//!
//! A `PermissionGate` ties a profile's permission-set reference to a stack
//! input: the reference is kept only while the input's resolved value matches
//! the gate's enabled value. This runs before `ServiceAccountMutation` so
//! service accounts are built from the pruned profiles.

use crate::error::Result;
use crate::StackMutation;
use alien_core::{DeploymentConfig, Stack, StackState};
use async_trait::async_trait;
use tracing::{debug, info};

/// Removes gated permission-set references whose gating input resolved to a
/// different value.
pub struct PermissionGateMutation;

#[async_trait]
impl StackMutation for PermissionGateMutation {
    fn description(&self) -> &'static str {
        "Apply deploy-time gates to profile permission-set references"
    }

    fn should_run(
        &self,
        stack: &Stack,
        _stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        !stack.permissions.gates.is_empty()
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        _stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<Stack> {
        let gates = stack.permissions.gates.clone();
        let mut references_removed = 0;

        for gate in &gates {
            // A gate prunes only on a definite mismatch. Template generation
            // runs mutations with an empty environment snapshot, so an
            // unresolvable value must keep the reference: the frozen role is a
            // superset and the live deploy path does the pruning.
            let Some(resolved_value) = resolve_gate_value(&stack, config, &gate.input_id) else {
                debug!(
                    input_id = %gate.input_id,
                    permission_set_id = %gate.permission_set_id,
                    "Permission gate input has no resolved value; keeping reference"
                );
                continue;
            };

            if resolved_value == gate.enabled_value {
                continue;
            }

            // A gate whose profile or resource is absent (a stale or externally
            // built gate) prunes nothing and falls through to the ungated superset.
            // The setup emitters skip such a gate the same way — they walk the real
            // profiles and never reach it — so failing here would split-brain the
            // generated template against the live deploy path.
            let Some(profile) = stack.permissions.profiles.get_mut(&gate.profile) else {
                debug!(
                    profile = %gate.profile,
                    "Skipping permission gate for nonexistent profile"
                );
                continue;
            };
            let Some(references) = profile.0.get_mut(&gate.resource) else {
                debug!(
                    profile = %gate.profile,
                    resource = %gate.resource,
                    "Skipping permission gate for nonexistent profile resource"
                );
                continue;
            };

            let before = references.len();
            references.retain(|reference| reference.id() != gate.permission_set_id);
            let removed = before - references.len();
            references_removed += removed;

            if removed > 0 {
                info!(
                    profile = %gate.profile,
                    resource = %gate.resource,
                    permission_set_id = %gate.permission_set_id,
                    input_id = %gate.input_id,
                    // Log the gate's configured value, not the resolved input
                    // value, which could carry a secret input's contents.
                    enabled_value = %gate.enabled_value,
                    "Removed gated permission-set reference"
                );
            }
        }

        if references_removed > 0 {
            info!(
                "Removed {} gated permission-set references",
                references_removed
            );
        }

        Ok(stack)
    }
}

/// Resolve a gating input's value through its environment mapping.
///
/// Inputs reach the deployment engine only as environment variables, so the
/// gate reads the input's first env mapping from the resolved snapshot.
fn resolve_gate_value(
    stack: &Stack,
    config: &DeploymentConfig,
    input_id: &str,
) -> Option<String> {
    let input = stack.inputs.iter().find(|input| input.id == input_id)?;
    let env_name = &input.env.first()?.name;
    config
        .environment_variables
        .variables
        .iter()
        .find(|variable| &variable.name == env_name)
        .map(|variable| variable.value.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::permissions::{
        ManagementPermissions, PermissionGate, PermissionProfile, PermissionsConfig,
    };
    use alien_core::{
        EnvironmentVariable, EnvironmentVariablesSnapshot, EnvironmentVariableType,
        ExternalBindings, Platform, StackInputDefinition, StackInputEnvironmentMapping,
        StackInputKind, StackInputProvider, StackSettings,
    };
    use indexmap::IndexMap;

    fn gated_stack() -> Stack {
        let mut profiles = IndexMap::new();
        profiles.insert(
            "execution".to_string(),
            PermissionProfile::new().global(["queue/data-write", "kv/data-write"]),
        );
        Stack {
            id: "test-stack".to_string(),
            resources: IndexMap::new(),
            permissions: PermissionsConfig {
                profiles,
                management: ManagementPermissions::Auto,
                gates: vec![PermissionGate {
                    profile: "execution".to_string(),
                    resource: "*".to_string(),
                    permission_set_id: "queue/data-write".to_string(),
                    input_id: "dataWriteEnabled".to_string(),
                    enabled_value: "true".to_string(),
                }],
            },
            supported_platforms: None,
            inputs: vec![StackInputDefinition {
                id: "dataWriteEnabled".to_string(),
                kind: StackInputKind::Boolean,
                provided_by: vec![StackInputProvider::Deployer],
                required: false,
                label: "Data write access".to_string(),
                description: "Whether the workload may write data".to_string(),
                placeholder: None,
                default: None,
                platforms: None,
                validation: None,
                env: vec![StackInputEnvironmentMapping {
                    name: "APP_DATA_WRITE_ENABLED".to_string(),
                    target_resources: None,
                    var_type: None,
                }],
            }],
        }
    }

    fn empty_stack() -> Stack {
        Stack {
            id: "test-stack".to_string(),
            resources: IndexMap::new(),
            permissions: PermissionsConfig::new(),
            supported_platforms: None,
            inputs: vec![],
        }
    }

    fn config_with_env(value: Option<&str>) -> DeploymentConfig {
        let variables = value
            .map(|value| {
                vec![EnvironmentVariable {
                    name: "APP_DATA_WRITE_ENABLED".to_string(),
                    value: value.to_string(),
                    var_type: EnvironmentVariableType::Plain,
                    target_resources: None,
                }]
            })
            .unwrap_or_default();
        DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables,
                hash: "hash".to_string(),
                created_at: String::new(),
            })
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build()
    }

    fn profile_permission_ids(stack: &Stack) -> Vec<String> {
        stack.permissions.profiles["execution"].0["*"]
            .iter()
            .map(|reference| reference.id().to_string())
            .collect()
    }

    #[tokio::test]
    async fn keeps_reference_when_value_matches() {
        let stack = gated_stack();
        let config = config_with_env(Some("true"));

        let mutated = PermissionGateMutation
            .mutate(stack, &StackState::new(Platform::Aws), &config)
            .await
            .expect("mutation should succeed");

        assert_eq!(profile_permission_ids(&mutated), ["queue/data-write", "kv/data-write"]);
    }

    #[tokio::test]
    async fn removes_reference_when_value_differs() {
        let stack = gated_stack();
        let config = config_with_env(Some("false"));

        let mutated = PermissionGateMutation
            .mutate(stack, &StackState::new(Platform::Aws), &config)
            .await
            .expect("mutation should succeed");

        assert_eq!(profile_permission_ids(&mutated), ["kv/data-write"]);
    }

    #[tokio::test]
    async fn keeps_reference_when_value_unresolved() {
        // Template generation runs with an empty env snapshot: the frozen role
        // must stay a superset, so the gate must not prune here.
        let stack = gated_stack();
        let config = config_with_env(None);

        let mutated = PermissionGateMutation
            .mutate(stack, &StackState::new(Platform::Aws), &config)
            .await
            .expect("mutation should succeed");

        assert_eq!(profile_permission_ids(&mutated), ["queue/data-write", "kv/data-write"]);
    }

    #[tokio::test]
    async fn other_references_survive_untouched() {
        let mut stack = gated_stack();
        stack.permissions.profiles.insert(
            "other".to_string(),
            PermissionProfile::new().global(["queue/data-write"]),
        );
        let config = config_with_env(Some("false"));

        let mutated = PermissionGateMutation
            .mutate(stack, &StackState::new(Platform::Aws), &config)
            .await
            .expect("mutation should succeed");

        assert_eq!(profile_permission_ids(&mutated), ["kv/data-write"]);
        assert_eq!(
            mutated.permissions.profiles["other"].0["*"]
                .iter()
                .map(|reference| reference.id())
                .collect::<Vec<_>>(),
            ["queue/data-write"]
        );
    }

    #[test]
    fn should_run_only_with_gates() {
        let config = config_with_env(None);
        let state = StackState::new(Platform::Aws);

        assert!(PermissionGateMutation.should_run(&gated_stack(), &state, &config));
        assert!(!PermissionGateMutation.should_run(&empty_stack(), &state, &config));
    }
}
