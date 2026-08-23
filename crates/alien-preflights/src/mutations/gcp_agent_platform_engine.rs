//! Synthesizes the Agent Platform reasoning engine each GCP sandbox needs.
//!
//! Vertex exposes no Terraform resource for a reasoning engine, so it cannot be emitted; a
//! controller creates it via the API instead, and this mutation adds the resource that controller
//! reconciles — one engine per sandbox, with the sandbox depending on it so teardown orders the
//! engine after the template and sessions.

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
    fn sandbox_ids(stack: &Stack) -> Vec<String> {
        stack
            .resources
            .iter()
            .filter(|(_, entry)| {
                entry.config.resource_type().as_ref() == Sandbox::RESOURCE_TYPE.as_ref()
            })
            .map(|(id, _)| id.clone())
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
        // Keys on Gcp + sandbox; correct only once Cloud Run is removed as the GCP sandbox backend,
        // which is why this mutation stays unregistered until the cutover.
        stack_state.platform == Platform::Gcp && !Self::sandbox_ids(stack).is_empty()
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        _stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        for sandbox_id in Self::sandbox_ids(&stack) {
            let engine_id = GcpAgentPlatformEngine::id_for_sandbox(&sandbox_id);

            // Live: the engine is Alien-owned and created with provision permissions, not setup.
            stack
                .resources
                .entry(engine_id.clone())
                .or_insert_with(|| ResourceEntry {
                    enabled_when: None,
                    config: Resource::new(GcpAgentPlatformEngine::new(engine_id.clone()).build()),
                    lifecycle: ResourceLifecycle::Live,
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
        PermissionsConfig, SandboxCode, SandboxEgress, SandboxSessionPolicy, StackSettings,
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

    fn stack_with_sandbox() -> Stack {
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
                ResourceLifecycle::Live,
            )
            .build()
    }

    fn gcp_state() -> StackState {
        StackState::new(Platform::Gcp)
    }

    #[tokio::test]
    async fn one_engine_is_synthesized_per_sandbox_and_the_sandbox_depends_on_it() {
        let stack = stack_with_sandbox();
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
        assert_eq!(engine.lifecycle, ResourceLifecycle::Live);
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

    #[tokio::test]
    async fn re_running_is_a_no_op() {
        let stack = stack_with_sandbox();
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
        let stack = stack_with_sandbox();
        let aws_state = StackState::new(Platform::Aws);
        assert!(!GcpAgentPlatformEngineMutation.should_run(&stack, &aws_state, &config()));

        let empty = Stack::new("empty".to_string())
            .permissions(PermissionsConfig::new())
            .build();
        let empty_state = gcp_state();
        assert!(!GcpAgentPlatformEngineMutation.should_run(&empty, &empty_state, &config()));
    }
}
