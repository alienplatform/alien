//! A GCP deployment whose setup predates the Cloud KMS activation keeps updating.
//!
//! Preparation now adds a Frozen `enable-cloud-kms` activation for every GCP Key. A deployment set
//! up before that saved a prepared stack and a stack state without it. These tests run the real
//! mutations from that state and the update compatibility checks, as an update does.

use alien_core::{
    DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, Key, Kv, Platform,
    ResourceLifecycle, ResourceRef, Stack, StackResourceState, StackSettings, StackState, Storage,
};
use alien_preflights::{runner::PreflightRunner, PreflightSummary};

const KMS_ACTIVATION: &str = "enable-cloud-kms";

/// Cloud Storage encrypted with a customer Key, plus a Live table the release can change freely.
fn release() -> Stack {
    Stack::new("test-stack".to_string())
        .add(
            Key::new("customer-key".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Storage::new("customer-data".to_string())
                .encryption_key(ResourceRef::new(Key::RESOURCE_TYPE, "customer-key"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Kv::new("cache".to_string()).build(),
            ResourceLifecycle::Live,
        )
        .build()
}

fn deployment_config() -> DeploymentConfig {
    DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build()
}

async fn prepare(state: &StackState) -> Stack {
    PreflightRunner::new()
        .apply_mutations(release(), state, &deployment_config())
        .await
        .expect("the stack should prepare")
}

/// The stack state an install leaves: every prepared resource recorded with its config.
fn installed_state(prepared: &Stack) -> StackState {
    let mut state = StackState::new(Platform::Gcp);
    for (id, entry) in &prepared.resources {
        state.resources.insert(
            id.clone(),
            StackResourceState::new_pending(
                entry.config.resource_type().to_string(),
                entry.config.clone(),
                Some(entry.lifecycle),
                entry.dependencies.clone(),
            ),
        );
    }
    state
}

/// A fresh install's prepared stack as a setup from before the activation saved it.
fn saved_before_kms_activation(prepared: &Stack) -> Stack {
    let mut saved = prepared.clone();
    saved
        .resources
        .shift_remove(KMS_ACTIVATION)
        .expect("a fresh install enables Cloud KMS");
    for entry in saved.resources.values_mut() {
        entry
            .dependencies
            .retain(|dependency| dependency.id() != KMS_ACTIVATION);
    }
    saved
}

fn key_depends_on_activation(prepared: &Stack) -> bool {
    prepared.resources["customer-key"]
        .dependencies
        .iter()
        .any(|dependency| dependency.id() == KMS_ACTIVATION)
}

fn errors(summary: &PreflightSummary) -> Vec<String> {
    summary
        .results
        .iter()
        .flat_map(|result| result.errors.clone())
        .collect()
}

#[tokio::test]
async fn a_fresh_install_enables_cloud_kms_before_the_key() {
    let prepared = prepare(&StackState::new(Platform::Gcp)).await;
    assert_eq!(
        prepared.resources[KMS_ACTIVATION].lifecycle,
        ResourceLifecycle::Frozen
    );
    assert!(key_depends_on_activation(&prepared));
}

#[tokio::test]
async fn an_update_without_key_changes_does_not_require_setup() {
    let installed = saved_before_kms_activation(&prepare(&StackState::new(Platform::Gcp)).await);
    let state = installed_state(&installed);
    let next = prepare(&state).await;

    let summary = PreflightRunner::new()
        .run_compatibility_checks(&installed, &next, &deployment_config(), Platform::Gcp)
        .await
        .expect("compatibility checks should run");
    assert!(
        summary.success,
        "the update must not need setup: {:?}",
        errors(&summary)
    );
    assert!(!next.resources.contains_key(KMS_ACTIVATION));
    assert!(!key_depends_on_activation(&next));
}

#[tokio::test]
async fn a_setup_that_installed_the_activation_keeps_it() {
    let installed = prepare(&StackState::new(Platform::Gcp)).await;
    let next = prepare(&installed_state(&installed)).await;

    let summary = PreflightRunner::new()
        .run_compatibility_checks(&installed, &next, &deployment_config(), Platform::Gcp)
        .await
        .expect("compatibility checks should run");
    assert!(summary.success, "{:?}", errors(&summary));
    assert!(next.resources.contains_key(KMS_ACTIVATION));
    assert!(key_depends_on_activation(&next));
}
