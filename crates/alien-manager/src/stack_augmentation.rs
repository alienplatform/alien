//! Extension point for injecting operator-owned resources into a deployment's
//! desired stack for one reconcile pass, without ever persisting them to the
//! customer's release/stack record.
//!
//! Some operator-owned infrastructure (e.g. a push-mode operations worker)
//! needs the full resource-controller machinery — credentials, retry, state
//! persistence, teardown-on-absence — but is never something a customer
//! declares. [`register_stack_augmentation_extension`] lets downstream crates
//! plug in a callback that decides what to inject into the deployment's
//! in-memory [`Stack`] before it reaches the executor, run on every reconcile
//! pass. Because the executor diffs desired-stack resources against persisted
//! state and deletes anything that disappears between passes, a callback MUST
//! return a decision consistently for the resource's whole intended lifetime
//! — returning [`StackAugmentationDecision::Remove`] once the underlying
//! condition no longer holds is how teardown happens, not an optional step.
//!
//! The callback is decision-only (it never touches [`Stack`] directly):
//! [`augment_desired_stack`] applies the returned decision itself. This also
//! sidesteps a rustc limitation unifying parameter and future lifetimes for
//! `Box<dyn Fn(&T, &mut U) -> Pin<Box<dyn Future>>>` trait objects.
//!
//! Mirrors the `register_registry_extension` pattern in `alien-infra`, but
//! async (the decision typically requires a database lookup) and scoped to
//! one deployment's stack rather than the global controller registry.

use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;

use alien_core::{ResourceEntry, Stack};
use alien_error::{AlienError, GenericError};

use crate::traits::deployment_store::DeploymentRecord;

/// What a registered callback wants applied to one resource id in the
/// deployment's desired stack for this pass.
pub enum StackAugmentationDecision {
    /// Ensure `resource_id` is present with this config, inserting or
    /// replacing any existing entry.
    Upsert {
        resource_id: String,
        entry: ResourceEntry,
    },
    /// Ensure `resource_id` is absent. The executor deletes it (via the
    /// normal controller Delete flow) if it was present on a prior pass.
    Remove { resource_id: String },
}

/// Callback that decides what to inject into a deployment's in-memory desired
/// stack before it reaches the executor. Must be idempotent and
/// side-effect-free beyond whatever external state it reads (no writes) — it
/// may run on every reconcile tick.
pub type StackAugmentationCallback = Box<
    dyn Fn(
            &DeploymentRecord,
        )
            -> Pin<Box<dyn Future<Output = Result<StackAugmentationDecision, AlienError<GenericError>>> + Send>>
        + Send
        + Sync,
>;

static STACK_AUGMENTATION_EXTENSION: OnceLock<StackAugmentationCallback> = OnceLock::new();

/// Registers a callback invoked by [`augment_desired_stack`] on every
/// reconcile pass, for every deployment. Must be called before the deployment
/// loop starts (typically at process startup). Only the first registration
/// wins — a second call is a no-op, matching `register_registry_extension`.
pub fn register_stack_augmentation_extension(callback: StackAugmentationCallback) {
    let _ = STACK_AUGMENTATION_EXTENSION.set(callback);
}

/// Applies the registered augmentation callback's decision (if any) to
/// `stack` for `deployment`. A no-op when no callback is registered (OSS
/// builds).
pub(crate) async fn augment_desired_stack(
    deployment: &DeploymentRecord,
    stack: &mut Stack,
) -> Result<(), AlienError<GenericError>> {
    let Some(callback) = STACK_AUGMENTATION_EXTENSION.get() else {
        return Ok(());
    };
    match callback(deployment).await? {
        StackAugmentationDecision::Upsert { resource_id, entry } => {
            stack.resources.insert(resource_id, entry);
        }
        StackAugmentationDecision::Remove { resource_id } => {
            stack.resources.shift_remove(&resource_id);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{OperationsWorker, Platform, ResourceEntry, ResourceLifecycle, StackSettings};
    use chrono::Utc;

    fn test_deployment() -> DeploymentRecord {
        DeploymentRecord {
            id: "dep-1".to_string(),
            workspace_id: "ws-1".to_string(),
            project_id: "proj-1".to_string(),
            name: "test".to_string(),
            deployment_group_id: "dg-1".to_string(),
            platform: Platform::Aws,
            deployment_protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
            base_platform: None,
            status: "running".to_string(),
            stack_settings: Some(StackSettings::default()),
            stack_state: None,
            environment_info: None,
            runtime_metadata: None,
            current_release_id: None,
            desired_release_id: None,
            import_source: None,
            setup_method: None,
            setup_metadata: None,
            setup_target: None,
            setup_fingerprint: None,
            setup_fingerprint_version: None,
            user_environment_variables: None,
            management_config: None,
            deployment_config: None,
            deployment_token: None,
            input_values: Default::default(),
            retry_requested: false,
            locked_by: None,
            locked_at: None,
            created_at: Utc::now(),
            updated_at: None,
            error: None,
        }
    }

    fn operations_worker_entry() -> ResourceEntry {
        ResourceEntry {
            config: alien_core::Resource::new(
                OperationsWorker::new("operations".to_string())
                    .image("example@sha256:abc".to_string())
                    .build(),
            ),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        }
    }

    /// Applies a decision the same way `augment_desired_stack` does, without
    /// needing a full registration round-trip through the OnceLock.
    fn apply_decision(stack: &mut Stack, decision: StackAugmentationDecision) {
        match decision {
            StackAugmentationDecision::Upsert { resource_id, entry } => {
                stack.resources.insert(resource_id, entry);
            }
            StackAugmentationDecision::Remove { resource_id } => {
                stack.resources.shift_remove(&resource_id);
            }
        }
    }

    #[test]
    fn upsert_decision_inserts_into_the_stack() {
        let mut stack = Stack::new("stack-1".to_string()).build();
        apply_decision(
            &mut stack,
            StackAugmentationDecision::Upsert {
                resource_id: "operations".to_string(),
                entry: operations_worker_entry(),
            },
        );
        assert!(stack.resources.contains_key("operations"));
    }

    #[test]
    fn remove_decision_deletes_from_the_stack() {
        let mut stack = Stack::new("stack-1".to_string()).build();
        stack
            .resources
            .insert("operations".to_string(), operations_worker_entry());

        apply_decision(
            &mut stack,
            StackAugmentationDecision::Remove {
                resource_id: "operations".to_string(),
            },
        );
        assert!(!stack.resources.contains_key("operations"));
    }

    // Registration is process-global (OnceLock) and shared across every test
    // in this binary, so only ONE test in the whole crate may call
    // register_stack_augmentation_extension — putting it here, alongside the
    // no-callback-registered case, keeps that constraint in one place. Run
    // sequentially within this module (default `cargo test` behavior; no
    // `#[test]` in this file spawns threads that race the OnceLock).
    #[tokio::test]
    async fn registered_callback_decision_is_applied_and_absence_is_a_noop() {
        // Before registration: augment_desired_stack must not panic or error
        // when nothing is registered (the OSS/default case).
        let deployment = test_deployment();
        let mut stack = Stack::new("stack-1".to_string()).build();
        augment_desired_stack(&deployment, &mut stack)
            .await
            .expect("no-op augmentation must not error");
        assert!(stack.resources.is_empty());

        let callback: StackAugmentationCallback = Box::new(|_deployment: &DeploymentRecord| {
            Box::pin(async move {
                Ok(StackAugmentationDecision::Upsert {
                    resource_id: "operations".to_string(),
                    entry: operations_worker_entry(),
                })
            })
        });
        register_stack_augmentation_extension(callback);

        augment_desired_stack(&deployment, &mut stack)
            .await
            .expect("augmentation should succeed");
        assert!(stack.resources.contains_key("operations"));
    }
}
