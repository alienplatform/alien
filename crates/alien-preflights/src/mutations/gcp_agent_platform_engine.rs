//! Synthesizes the Agent Platform reasoning engine each GCP sandbox needs.
//!
//! The engine carries its sandbox's lifecycle, so one owner creates and destroys both: setup for
//! a Frozen pair, the runtime controllers for a Live one. The sandbox depends on its engine, so
//! the engine deploys first and tears down after the template and its sessions.

use crate::error::Result;
use crate::StackMutation;
use alien_core::{
    DeploymentConfig, GcpAgentPlatformEngine, Platform, Resource, ResourceEntry, ResourceLifecycle,
    ResourceRef, Sandbox, Stack, StackState,
};
use async_trait::async_trait;
use tracing::info;

pub struct GcpAgentPlatformEngineMutation;

impl GcpAgentPlatformEngineMutation {
    fn sandboxes(stack: &Stack) -> Vec<(String, ResourceLifecycle, Option<String>)> {
        stack
            .resources
            .iter()
            .filter(|(_, entry)| {
                entry.config.resource_type().as_ref() == Sandbox::RESOURCE_TYPE.as_ref()
            })
            .map(|(id, entry)| (id.clone(), entry.lifecycle, entry.enabled_when.clone()))
            .collect()
    }
}

#[async_trait]
impl StackMutation for GcpAgentPlatformEngineMutation {
    fn description(&self) -> &'static str {
        "Provision an Agent Platform reasoning engine for each GCP sandbox"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        // Gcp always means the Agent Platform sandbox backend; no other GCP backend exists to key on.
        stack_state.platform == Platform::Gcp && !Self::sandboxes(stack).is_empty()
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        _stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        for (sandbox_id, lifecycle, enabled_when) in Self::sandboxes(&stack) {
            let engine_id = GcpAgentPlatformEngine::id_for_sandbox(&sandbox_id);

            // Lifecycle and gate both come from the sandbox: the engine exists for that one
            // sandbox, so a deployer who declines it must not be billed for its parent, and a
            // grant setup places on the engine is only placeable while setup owns the engine.
            stack
                .resources
                .entry(engine_id.clone())
                .or_insert_with(|| ResourceEntry {
                    enabled_when,
                    config: Resource::new(GcpAgentPlatformEngine::new(engine_id.clone()).build()),
                    lifecycle,
                    dependencies: Vec::new(),
                    remote_access: false,
                });

            // The sandbox depends on its engine, so the engine deploys first (its id is available
            // when the template is built) and tears down last (after the template and sessions).
            let engine_ref =
                ResourceRef::new(GcpAgentPlatformEngine::RESOURCE_TYPE, engine_id.clone());
            if let Some(entry) = stack.resources.get_mut(&sandbox_id) {
                if !entry.dependencies.iter().any(|r| r.id == engine_id) {
                    entry.dependencies.push(engine_ref);
                    info!(sandbox=%sandbox_id, engine=%engine_id, "sandbox depends on its reasoning engine");
                }
            }
        }

        Ok(stack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        PermissionsConfig, SandboxCode, SandboxEgress, SandboxSessionPolicy, StackInputDefinition,
        StackSettings,
    };

    fn config() -> DeploymentConfig {
        DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: String::new(),
                created_at: "2024-01-01T00:00:00Z".to_string(),
            })
            .allow_frozen_changes(false)
            .external_bindings(alien_core::ExternalBindings::default())
            .build()
    }

    fn stack_with_sandbox(lifecycle: ResourceLifecycle) -> Stack {
        Stack::new("gcp-sandbox".to_string())
            .permissions(PermissionsConfig::new())
            .add(
                Sandbox::new("worker-sbx".to_string())
                    .code(SandboxCode::Image {
                        image: "python:3.12".to_string(),
                    })
                    .egress(SandboxEgress::Deny)
                    .session(SandboxSessionPolicy {
                        max_lifetime_seconds: None,
                        idle_suspend_seconds: None,
                    })
                    .build(),
                lifecycle,
            )
            .build()
    }

    fn gcp_state() -> StackState {
        StackState::new(Platform::Gcp)
    }

    #[tokio::test]
    async fn one_engine_is_synthesized_per_sandbox_and_the_sandbox_depends_on_it() {
        let stack = stack_with_sandbox(ResourceLifecycle::Live);
        let state = gcp_state();
        assert!(GcpAgentPlatformEngineMutation.should_run(&stack, &state, &config()));

        let mutated = GcpAgentPlatformEngineMutation
            .mutate(stack, &state, &config())
            .await
            .expect("mutation should succeed");

        let engine = mutated
            .resources
            .get("worker-sbx-engine")
            .expect("an engine is synthesized for the sandbox");
        assert_eq!(
            engine.config.resource_type().as_ref(),
            GcpAgentPlatformEngine::RESOURCE_TYPE.as_ref()
        );

        let sandbox = mutated.resources.get("worker-sbx").expect("the sandbox");
        assert!(
            sandbox
                .dependencies
                .iter()
                .any(|r| r.id == "worker-sbx-engine"),
            "the sandbox must depend on its engine for teardown ordering"
        );
    }

    /// A declined sandbox must not leave a billed engine behind, so the gate travels with the
    /// lifecycle. The engine exists for exactly one sandbox and has no meaning without it.
    #[tokio::test]
    async fn the_engine_takes_the_gate_of_the_sandbox_it_belongs_to() {
        let stack = Stack::new("gcp-sandbox".to_string())
            .permissions(PermissionsConfig::new())
            .inputs(vec![StackInputDefinition::deployer_boolean(
                "sandboxEnabled",
                "Enable the sandbox",
                "Whether to create the sandbox.",
                Some(true),
            )])
            .add_enabled_when(
                Sandbox::new("worker-sbx".to_string())
                    .code(SandboxCode::Image {
                        image: "python:3.12".to_string(),
                    })
                    .egress(SandboxEgress::Deny)
                    .session(SandboxSessionPolicy {
                        max_lifetime_seconds: None,
                        idle_suspend_seconds: None,
                    })
                    .build(),
                ResourceLifecycle::Frozen,
                "sandboxEnabled",
            )
            .build();

        let mutated = GcpAgentPlatformEngineMutation
            .mutate(stack, &gcp_state(), &config())
            .await
            .expect("mutation should succeed");

        assert_eq!(
            mutated
                .resources
                .get("worker-sbx-engine")
                .expect("an engine is synthesized for the sandbox")
                .enabled_when
                .as_deref(),
            Some("sandboxEnabled")
        );
    }

    /// One owner per pair. A Frozen sandbox's engine is created and destroyed by the setup stack,
    /// so an engine that came out Live would be created twice and deleted by the controller that
    /// never made it; a Live sandbox's engine that came out Frozen would never be created at all.
    #[tokio::test]
    async fn the_engine_takes_the_lifecycle_of_the_sandbox_it_belongs_to() {
        for lifecycle in [ResourceLifecycle::Frozen, ResourceLifecycle::Live] {
            let mutated = GcpAgentPlatformEngineMutation
                .mutate(stack_with_sandbox(lifecycle), &gcp_state(), &config())
                .await
                .expect("mutation should succeed");

            assert_eq!(
                mutated
                    .resources
                    .get("worker-sbx-engine")
                    .expect("an engine is synthesized for the sandbox")
                    .lifecycle,
                lifecycle,
                "a {lifecycle:?} sandbox must get a {lifecycle:?} engine"
            );
        }
    }

    #[tokio::test]
    async fn re_running_is_a_no_op() {
        let stack = stack_with_sandbox(ResourceLifecycle::Live);
        let state = gcp_state();
        let once = GcpAgentPlatformEngineMutation
            .mutate(stack, &state, &config())
            .await
            .expect("first pass");
        let twice = GcpAgentPlatformEngineMutation
            .mutate(once, &state, &config())
            .await
            .expect("second pass");

        assert_eq!(
            twice
                .resources
                .get("worker-sbx")
                .unwrap()
                .dependencies
                .len(),
            1,
            "the engine dependency is not appended twice"
        );
        assert_eq!(
            twice
                .resources
                .values()
                .filter(|e| e.config.resource_type().as_ref()
                    == GcpAgentPlatformEngine::RESOURCE_TYPE.as_ref())
                .count(),
            1,
            "no second engine is synthesized"
        );
    }

    #[tokio::test]
    async fn it_does_not_run_off_gcp_or_without_a_sandbox() {
        let stack = stack_with_sandbox(ResourceLifecycle::Live);
        let aws_state = StackState::new(Platform::Aws);
        assert!(!GcpAgentPlatformEngineMutation.should_run(&stack, &aws_state, &config()));

        let empty = Stack::new("empty".to_string())
            .permissions(PermissionsConfig::new())
            .build();
        let empty_state = gcp_state();
        assert!(!GcpAgentPlatformEngineMutation.should_run(&empty, &empty_state, &config()));
    }
}
