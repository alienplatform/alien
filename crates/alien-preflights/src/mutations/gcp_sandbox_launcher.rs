//! Marks the Cloud Run workers that host a GCP stack's sandbox sessions.
//!
//! GCP provisions nothing durable for a sandbox: a session is a subprocess of the Cloud Run
//! instance already running the application, and an instance can only launch one if its container
//! declares `sandboxLauncher`. Nothing in an application's own declaration says which worker that
//! is, so preflight decides it — the same reason `SandboxHostRequiredCheck` refuses a GCP sandbox
//! with no Cloud Run host at all.

use crate::error::Result;
use crate::StackMutation;
use alien_core::{DeploymentConfig, Platform, Sandbox, Stack, StackState, Worker};
use async_trait::async_trait;
use tracing::info;

pub struct GcpSandboxLauncherMutation;

impl GcpSandboxLauncherMutation {
    fn stack_has_a_sandbox(stack: &Stack) -> bool {
        stack.resources.values().any(|entry| {
            entry.config.resource_type().as_ref() == Sandbox::RESOURCE_TYPE.as_ref()
        })
    }
}

#[async_trait]
impl StackMutation for GcpSandboxLauncherMutation {
    fn description(&self) -> &'static str {
        "Let the Cloud Run workers hosting a GCP sandbox launch sessions"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        stack_state.platform == Platform::Gcp && Self::stack_has_a_sandbox(stack)
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        _stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        // Every worker, not a chosen one: a session is created through the binding, and any
        // worker holding that binding can be the one that asks. Marking a subset would make
        // which instance served the request decide whether the call worked.
        for (id, entry) in &mut stack.resources {
            let Some(worker) = entry.config.downcast_mut::<Worker>() else {
                continue;
            };
            if worker.sandbox_launcher {
                continue;
            }
            worker.sandbox_launcher = true;
            info!(worker = %id, "Worker may launch sandbox sessions");
        }

        Ok(stack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        PermissionsConfig, ResourceLifecycle, SandboxCode, SandboxEgress, SandboxSessionPolicy,
        StackSettings, WorkerCode,
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

    fn stack(with_sandbox: bool) -> Stack {
        let mut builder = Stack::new("gcp-sandbox".to_string())
            .permissions(PermissionsConfig::new())
            .add(
                Worker::new("api".to_string())
                    .permissions("execution".to_string())
                    .code(WorkerCode::Image {
                        image: "registry.example.com/api:latest".to_string(),
                    })
                    .build(),
                ResourceLifecycle::Live,
            );
        if with_sandbox {
            builder = builder.add(
                Sandbox::new("agent".to_string())
                    .code(SandboxCode::Image {
                        image: "ubuntu:24.04".to_string(),
                    })
                    .egress(SandboxEgress::Deny)
                    .session(SandboxSessionPolicy {
                        max_lifetime_seconds: None,
                        idle_suspend_seconds: None,
                    })
                    .build(),
                ResourceLifecycle::Frozen,
            );
        }
        builder.build()
    }

    fn launcher(stack: &Stack, id: &str) -> bool {
        stack
            .resources
            .get(id)
            .and_then(|entry| entry.config.downcast_ref::<Worker>())
            .expect("the worker")
            .sandbox_launcher
    }

    /// Without this the deploy succeeds and the first `create()` fails, which is the silent
    /// failure the capability contract exists to prevent.
    #[tokio::test]
    async fn a_gcp_worker_hosting_a_sandbox_is_marked_as_a_launcher() {
        let stack = stack(true);
        let state = StackState::new(Platform::Gcp);

        assert!(GcpSandboxLauncherMutation.should_run(&stack, &state, &config()));
        let mutated = GcpSandboxLauncherMutation
            .mutate(stack, &state, &config())
            .await
            .expect("mutation runs");

        assert!(launcher(&mutated, "api"));
    }

    /// The flag is a Cloud Run permission, so a stack that declares no sandbox must not acquire
    /// it — and no other platform reads it at all.
    #[tokio::test]
    async fn nothing_is_marked_without_a_sandbox_or_off_gcp() {
        let config = config();
        assert!(!GcpSandboxLauncherMutation.should_run(
            &stack(false),
            &StackState::new(Platform::Gcp),
            &config
        ));
        assert!(!GcpSandboxLauncherMutation.should_run(
            &stack(true),
            &StackState::new(Platform::Aws),
            &config
        ));
    }
}
