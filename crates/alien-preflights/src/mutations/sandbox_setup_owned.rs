//! A Live sandbox on Azure and Google Cloud is created by setup, like a Frozen one.
//!
//! On AWS a sandbox's runtime controller creates it. On Azure the sandbox group, and on Google
//! Cloud the engine and template, are parents a remote grant names, so they must exist when the
//! setup package applies and only that package can create them. A stack that declares a sandbox
//! `live` for AWS therefore runs unchanged on the other two clouds, where it takes the Frozen
//! lifecycle: setup owns it, and a change to it goes through setup.

use crate::error::Result;
use crate::StackMutation;
use alien_core::{DeploymentConfig, Platform, ResourceLifecycle, Sandbox, Stack, StackState};
use async_trait::async_trait;

/// The build notice, shared with the compile-time check so both say the same thing.
pub const SETUP_OWNED_SANDBOX_NOTICE: &str = "on Azure and Google Cloud a live sandbox is created by \
     setup, like frozen; changes to it go through setup";

pub fn takes_setup_lifecycle(platform: Platform) -> bool {
    matches!(platform, Platform::Azure | Platform::Gcp)
}

fn is_live_sandbox(entry: &alien_core::ResourceEntry) -> bool {
    entry.config.downcast_ref::<Sandbox>().is_some() && entry.lifecycle == ResourceLifecycle::Live
}

pub struct SandboxSetupOwnedMutation;

#[async_trait]
impl StackMutation for SandboxSetupOwnedMutation {
    fn description(&self) -> &'static str {
        "Let setup create a sandbox declared live on Azure and Google Cloud"
    }

    fn should_run(&self, stack: &Stack, state: &StackState, _config: &DeploymentConfig) -> bool {
        takes_setup_lifecycle(state.platform) && stack.resources().any(|(_, e)| is_live_sandbox(e))
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        _state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        for entry in stack.resources.values_mut() {
            if is_live_sandbox(entry) {
                entry.lifecycle = ResourceLifecycle::Frozen;
            }
        }
        Ok(stack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        PermissionsConfig, SandboxCode, SandboxEgress, SandboxLifecyclePolicy, StackSettings,
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

    fn stack(lifecycle: ResourceLifecycle) -> Stack {
        Stack::new("s".to_string())
            .permissions(PermissionsConfig::new())
            .add(
                Sandbox::new("box".to_string())
                    .code(SandboxCode::Image {
                        image: "python:3.12".to_string(),
                        bundled_from: None,
                    })
                    .egress(SandboxEgress::None)
                    .lifecycle(SandboxLifecyclePolicy {
                        max_lifetime_seconds: None,
                        idle_pause_seconds: None,
                    })
                    .build(),
                lifecycle,
            )
            .build()
    }

    #[tokio::test]
    async fn a_live_sandbox_becomes_frozen_on_azure_and_gcp_only() {
        for platform in [Platform::Azure, Platform::Gcp] {
            let state = StackState::new(platform);
            let live = stack(ResourceLifecycle::Live);
            assert!(SandboxSetupOwnedMutation.should_run(&live, &state, &config()));
            let mutated = SandboxSetupOwnedMutation
                .mutate(live, &state, &config())
                .await
                .unwrap();
            assert_eq!(mutated.resources["box"].lifecycle, ResourceLifecycle::Frozen);
        }
        for platform in [Platform::Aws, Platform::Kubernetes] {
            let state = StackState::new(platform);
            assert!(!SandboxSetupOwnedMutation.should_run(
                &stack(ResourceLifecycle::Live),
                &state,
                &config()
            ));
        }
        // A Frozen sandbox (every install that exists today) is left alone.
        let state = StackState::new(Platform::Azure);
        assert!(!SandboxSetupOwnedMutation.should_run(
            &stack(ResourceLifecycle::Frozen),
            &state,
            &config()
        ));
    }
}
