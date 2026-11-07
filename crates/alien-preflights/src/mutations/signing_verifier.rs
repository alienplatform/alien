//! Adds a protected install's verifier, the only principal that changes what runs.

use crate::{
    error::{ErrorData, Result},
    StackMutation,
};
use alien_core::{
    ComputeCluster, DeploymentConfig, Platform, ResourceEntry, ResourceLifecycle, SigningVerifier,
    Stack, StackState, Worker,
};
use alien_error::AlienError;
use async_trait::async_trait;
use tracing::info;

/// Adds the verifier to a stack with signing settings on AWS, GCP or Azure that has a function
/// (a worker) or a machine cluster: the two things only the verifier may write. Setup creates
/// it, so it is Frozen. A stack with neither, or a Kubernetes one, gets none.
pub struct SigningVerifierMutation;

fn is_written_by_the_verifier(entry: &ResourceEntry) -> bool {
    let resource_type = entry.config.resource_type();
    resource_type == Worker::RESOURCE_TYPE || resource_type == ComputeCluster::RESOURCE_TYPE
}

#[async_trait]
impl StackMutation for SigningVerifierMutation {
    fn description(&self) -> &'static str {
        "Add the verifier of a protected install"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> bool {
        config.stack_settings.signing.is_some()
            && matches!(
                stack_state.platform,
                Platform::Aws | Platform::Gcp | Platform::Azure
            )
            && stack.resources.values().any(is_written_by_the_verifier)
            && !stack
                .resources
                .values()
                .any(|entry| entry.config.resource_type() == SigningVerifier::RESOURCE_TYPE)
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        let id = SigningVerifier::ID;
        if let Some(existing) = stack.resources.get(id) {
            return Err(AlienError::new(ErrorData::StackMutationFailed {
                mutation_name: self.description().to_string(),
                message: format!(
                    "resource ID '{id}' is reserved for the install's verifier, but is already used by resource type '{}'",
                    existing.config.resource_type()
                ),
                resource_id: Some(id.to_string()),
            }));
        }
        info!(platform = ?stack_state.platform, "Adding the install's verifier");
        stack.resources.insert(
            id.to_string(),
            ResourceEntry {
                enabled_when: None,
                config: alien_core::Resource::new(SigningVerifier::new(id.to_string()).build()),
                lifecycle: ResourceLifecycle::Frozen,
                // What it names of other resources (Azure: the identity and vault the
                // management resource creates), it references, which orders setup.
                dependencies: Vec::new(),
                display_name: Some("Verifier".to_string()),
                description: Some(
                    "Checks every change to what runs against the release the vendor signed."
                        .to_string(),
                ),
                remote_access: false,
            },
        );
        Ok(stack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        EnvironmentVariablesSnapshot, ExternalBindings, Platform, Stack, StackSettings,
        StackSigning, Storage, WorkerCode,
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

    fn with_worker() -> Stack {
        Stack::new("shop".to_string())
            .add(
                Worker::new("api".to_string())
                    .code(WorkerCode::Image {
                        image: "ghcr.io/shop/api@sha256:1".to_string(),
                    })
                    .permissions("execution".to_string())
                    .build(),
                ResourceLifecycle::Live,
            )
            .build()
    }

    fn storage_only() -> Stack {
        Stack::new("shop".to_string())
            .add(
                Storage::new("assets".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build()
    }

    /// Whether the full preflight pipeline gives `stack` a verifier on `platform`.
    async fn verifier(stack: Stack, platform: Platform, signing: bool) -> Option<ResourceEntry> {
        let config = config(signing);
        let state = StackState::new(platform);
        let runner = crate::runner::PreflightRunner::new();
        let prepared = runner
            .apply_mutations(stack, &state, &config)
            .await
            .expect("preflights");
        let repeated = runner
            .apply_mutations(prepared.clone(), &state, &config)
            .await
            .expect("preflights again");
        assert_eq!(
            prepared
                .resources
                .get(SigningVerifier::ID)
                .map(|e| e.lifecycle),
            repeated
                .resources
                .get(SigningVerifier::ID)
                .map(|e| e.lifecycle),
            "preflights are idempotent"
        );
        prepared.resources.get(SigningVerifier::ID).cloned()
    }

    #[tokio::test]
    async fn a_protected_stack_with_a_function_gets_a_frozen_verifier() {
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let entry = verifier(with_worker(), platform, true)
                .await
                .unwrap_or_else(|| panic!("{platform:?}: no verifier"));
            assert_eq!(entry.lifecycle, ResourceLifecycle::Frozen);
            assert_eq!(entry.config.resource_type(), SigningVerifier::RESOURCE_TYPE);
        }
    }

    #[tokio::test]
    async fn only_a_protected_cloud_stack_that_runs_something_gets_one() {
        assert!(verifier(with_worker(), Platform::Aws, false)
            .await
            .is_none());
        assert!(verifier(storage_only(), Platform::Aws, true)
            .await
            .is_none());
        assert!(verifier(with_worker(), Platform::Kubernetes, true)
            .await
            .is_none());
    }
}
