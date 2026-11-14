//! Refuses a cloud Redis resource on a protected install.
//!
//! A protected install's workloads read the endpoint of a Redis they bind to only from the
//! cloud's own read of it or from the signed config, and the cloud Redis controllers write the
//! group's password and network rules as management. Until that has its own protected rules, a
//! stack that mixes the two is refused where the install is set up and updated, instead of
//! creating a store no protected workload can bind.

use crate::{
    error::{ErrorData, Result},
    mutations::runs_on_platform_or_base,
    StackMutation,
};
use alien_core::{DeploymentConfig, Platform, Redis, Stack, StackState};
use alien_error::AlienError;
use async_trait::async_trait;

pub struct ProtectedRedisRefusalMutation;

fn on_a_cloud(stack_state: &StackState, config: &DeploymentConfig) -> bool {
    [Platform::Aws, Platform::Gcp, Platform::Azure]
        .into_iter()
        .any(|cloud| runs_on_platform_or_base(stack_state, config, cloud))
}

fn first_redis(stack: &Stack) -> Option<&str> {
    stack
        .resources
        .iter()
        .find(|(_, entry)| entry.config.resource_type() == Redis::RESOURCE_TYPE)
        .map(|(id, _)| id.as_str())
}

#[async_trait]
impl StackMutation for ProtectedRedisRefusalMutation {
    fn description(&self) -> &'static str {
        "Refuse a cloud Redis resource on a protected install"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> bool {
        config.stack_settings.signing.is_some()
            && on_a_cloud(stack_state, config)
            && first_redis(stack).is_some()
    }

    async fn mutate(
        &self,
        stack: Stack,
        _stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        let id = first_redis(&stack).map(str::to_string);
        Err(AlienError::new(ErrorData::StackMutationFailed {
            mutation_name: self.description().to_string(),
            message: format!(
                "resource '{}' is a Redis, which a protected install does not support on AWS, GCP or Azure yet: its workloads cannot bind it. Remove it, or install without release signing",
                id.as_deref().unwrap_or("redis")
            ),
            resource_id: id,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        EnvironmentVariablesSnapshot, ExternalBindings, ResourceLifecycle, StackSettings,
        StackSigning,
    };

    fn config(signing: bool) -> DeploymentConfig {
        DeploymentConfig::builder()
            .stack_settings(StackSettings {
                signing: signing.then(|| StackSigning {
                    fingerprint: "SHA256:00".to_string(),
                    channel: "production".to_string(),
                }),
                ..StackSettings::default()
            })
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: String::new(),
                created_at: "2024-01-01T00:00:00Z".to_string(),
            })
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build()
    }

    fn with_redis() -> Stack {
        Stack::new("shop".to_string())
            .add(
                Redis::new("cache".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build()
    }

    async fn prepared(
        stack: Stack,
        platform: Platform,
        signing: bool,
    ) -> std::result::Result<Stack, String> {
        crate::runner::PreflightRunner::new()
            .apply_mutations(stack, &StackState::new(platform), &config(signing))
            .await
            .map_err(|error| error.to_string())
    }

    #[tokio::test]
    async fn a_protected_cloud_stack_with_redis_is_refused_by_name() {
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let error = prepared(with_redis(), platform, true)
                .await
                .expect_err(&format!(
                    "{platform:?}: a protected stack with Redis is refused"
                ));
            assert!(error.contains("'cache'"), "{platform:?}: {error}");
            assert!(error.contains("protected"), "{platform:?}: {error}");
        }
    }

    #[tokio::test]
    async fn redis_is_unaffected_without_signing_or_on_another_platform() {
        prepared(with_redis(), Platform::Aws, false)
            .await
            .expect("an unprotected install keeps Redis");
        prepared(with_redis(), Platform::Local, true)
            .await
            .expect("a local install is not a cloud install");
    }
}
