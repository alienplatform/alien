//! A frozen resource behind a boolean input: an existing deployment can decline it without
//! running setup, and a later setup run can turn it on. Nothing else changes the answer.
use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus,
    EnvironmentVariablesSnapshot, Key, Platform, ReleaseInfo, ResourceLifecycle, ResourceStatus,
    RuntimeMetadata, Stack, StackInputDefinition, StackResourceState, StackState,
};
use serde_json::json;

const GATE: &str = "keyEnabled";

fn client() -> ClientConfig {
    ClientConfig::Local {
        state_directory: "/unused".into(),
    }
}

/// The release the deployment was set up with: no gated resource yet.
fn installed_release() -> Stack {
    Stack::new("gate-update".into())
        .add(
            Key::new("retained".into()).build(),
            ResourceLifecycle::Frozen,
        )
        .build()
}

/// The next release: adds frozen keys behind one boolean input, which defaults to yes.
fn next_release(gated_keys: &[&str]) -> Stack {
    let mut builder = Stack::new("gate-update".into())
        .inputs(vec![StackInputDefinition::deployer_boolean(
            GATE,
            "Encryption key",
            "Create the optional encryption key",
            Some(true),
        )])
        .add(
            Key::new("retained".into()).build(),
            ResourceLifecycle::Frozen,
        );
    for id in gated_keys {
        builder = builder.add_enabled_when(
            Key::new((*id).into()).build(),
            ResourceLifecycle::Frozen,
            GATE,
        );
    }
    builder.build()
}

fn config(input: Option<bool>) -> DeploymentConfig {
    let mut config = DeploymentConfig::builder()
        .stack_settings(Default::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(Default::default())
        .allow_frozen_changes(false)
        .build();
    if let Some(value) = input {
        config.input_values.insert(GATE.into(), json!(value));
    }
    config
}

fn release(stack: Stack, version: &str) -> ReleaseInfo {
    ReleaseInfo {
        release_id: Some(version.into()),
        version: Some(version.into()),
        description: None,
        stack,
        signed: None,
        signer_lists: Vec::new(),
    }
}

/// Mark every resource of a prepared stack as installed and running.
fn installed_state(prepared: &Stack) -> StackState {
    let mut state = StackState::new(Platform::Aws);
    for (id, entry) in prepared.resources() {
        let mut resource = StackResourceState::new_pending(
            entry.config.resource_type().to_string(),
            entry.config.clone(),
            Some(entry.lifecycle),
            entry.dependencies.clone(),
        );
        resource.status = ResourceStatus::Running;
        resource.internal_state = Some(json!({}));
        state.resources.insert(id.clone(), resource);
    }
    state
}

/// A running deployment set up with `setup_stack` and `setup_input`, with `next` as its target.
async fn deployment(setup_stack: Stack, setup_input: Option<bool>, next: Stack) -> DeploymentState {
    let metadata = alien_deployment::prepare_direct_setup_update(
        setup_stack.clone(),
        &StackState::new(Platform::Aws),
        &config(setup_input),
        &client(),
        &RuntimeMetadata::default(),
    )
    .await
    .expect("initial setup preparation");
    let installed = installed_state(metadata.prepared_stack.as_ref().unwrap());
    DeploymentState {
        status: DeploymentStatus::UpdatePending,
        platform: Platform::Aws,
        current_release: Some(release(setup_stack, "1")),
        target_release: Some(release(next, "2")),
        stack_state: Some(installed),
        error: None,
        environment_info: None,
        runtime_metadata: Some(metadata),
        retry_requested: false,
        protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
    }
}

/// An existing deployment that never answered the gate (set up before the release added it).
async fn unanswered_deployment(gated_keys: &[&str]) -> DeploymentState {
    deployment(installed_release(), None, next_release(gated_keys)).await
}

async fn update(state: DeploymentState, input: Option<bool>) -> DeploymentState {
    alien_deployment::step(state, config(input), client(), None)
        .await
        .expect("update step")
        .state
}

fn pending_stack(state: &DeploymentState) -> &Stack {
    state
        .runtime_metadata
        .as_ref()
        .unwrap()
        .pending_prepared_stack
        .as_ref()
        .expect("the update prepared a target stack")
}

fn recorded(state: &DeploymentState) -> Option<bool> {
    state
        .runtime_metadata
        .as_ref()
        .unwrap()
        .persisted_gate_answers
        .get(GATE)
        .copied()
}

/// Run setup again against the metadata of a deployment that finished an update.
async fn rerun_setup(
    state: &DeploymentState,
    target: Stack,
    input: Option<bool>,
) -> RuntimeMetadata {
    let mut previous = state.runtime_metadata.clone().unwrap();
    previous.prepared_stack = previous.pending_prepared_stack.take();
    alien_deployment::prepare_direct_setup_update(
        target,
        state.stack_state.as_ref().unwrap(),
        &config(input),
        &client(),
        &previous,
    )
    .await
    .expect("setup rerun")
}

#[tokio::test]
async fn declining_an_unanswered_gate_updates_without_setup_and_setup_turns_it_on() {
    let state = unanswered_deployment(&["optional"]).await;
    assert_eq!(recorded(&state), None);

    let updated = update(state, Some(false)).await;
    assert_eq!(
        updated.status,
        DeploymentStatus::Updating,
        "{:?}",
        updated.error
    );
    assert!(updated.error.is_none());
    assert!(!pending_stack(&updated).resources.contains_key("optional"));
    assert!(pending_stack(&updated).resources.contains_key("retained"));
    assert!(!updated
        .stack_state
        .as_ref()
        .unwrap()
        .resources
        .contains_key("optional"));
    assert_eq!(recorded(&updated), Some(false), "the decline is recorded");

    // A default never turns it on.
    let by_default = rerun_setup(&updated, next_release(&["optional"]), None).await;
    assert_eq!(by_default.persisted_gate_answers.get(GATE), Some(&false));
    assert!(!by_default
        .prepared_stack
        .as_ref()
        .unwrap()
        .resources
        .contains_key("optional"));

    // An explicit yes does.
    let turned_on = rerun_setup(&updated, next_release(&["optional"]), Some(true)).await;
    assert_eq!(turned_on.persisted_gate_answers.get(GATE), Some(&true));
    assert!(turned_on
        .prepared_stack
        .as_ref()
        .unwrap()
        .resources
        .contains_key("optional"));
}

#[tokio::test]
async fn an_unanswered_gate_with_nothing_set_still_requires_setup() {
    let state = unanswered_deployment(&["optional"]).await;
    // The new frozen resource stays in the target stack, so the frozen-compatibility
    // check stops the update and asks for setup.
    let error = alien_deployment::step(state, config(None), client(), None)
        .await
        .expect_err("the update must be refused");
    let cause = error.source.as_ref().expect("the preflight cause");
    assert_eq!(cause.code, "DEPLOYMENT_SETUP_REQUIRED", "{error:?}");
}

#[tokio::test]
async fn set_inputs_cannot_turn_a_declined_gate_on_or_decline_an_installed_resource() {
    // Declined at setup, then asked to turn on through an update.
    let declined = deployment(
        next_release(&["optional"]),
        Some(false),
        next_release(&["optional"]),
    )
    .await;
    assert_eq!(recorded(&declined), Some(false));
    let error = alien_deployment::step(declined, config(Some(true)), client(), None)
        .await
        .expect_err("an update cannot create a frozen resource");
    assert_eq!(error.code, "FROZEN_GATE_ANSWER_CHANGED");

    // The resource exists but this deployment has no recorded answer: a decline would
    // name a resource that is installed.
    let mut legacy = deployment(
        next_release(&["optional"]),
        Some(true),
        next_release(&["optional"]),
    )
    .await;
    legacy
        .runtime_metadata
        .as_mut()
        .unwrap()
        .persisted_gate_answers
        .clear();
    let error = alien_deployment::step(legacy.clone(), config(Some(false)), client(), None)
        .await
        .expect_err("an installed resource cannot be declined");
    assert_eq!(error.code, "FROZEN_GATE_ANSWER_UNDERIVABLE");
    assert!(error.to_string().contains("already exists"), "{error}");

    // And an unanswered gate cannot be turned on through an update either.
    let unanswered = unanswered_deployment(&["optional"]).await;
    let error = alien_deployment::step(unanswered, config(Some(true)), client(), None)
        .await
        .expect_err("an update cannot create a frozen resource");
    assert_eq!(error.code, "FROZEN_GATE_ANSWER_UNDERIVABLE");
}

#[tokio::test]
async fn direct_setup_refuses_to_turn_a_recorded_yes_into_a_no() {
    let state = deployment(
        next_release(&["optional"]),
        Some(true),
        next_release(&["optional"]),
    )
    .await;
    let mut previous = state.runtime_metadata.clone().unwrap();
    previous.pending_prepared_stack = previous.prepared_stack.clone();
    let error = alien_deployment::prepare_direct_setup_update(
        next_release(&["optional"]),
        state.stack_state.as_ref().unwrap(),
        &config(Some(false)),
        &client(),
        &previous,
    )
    .await
    .expect_err("setup cannot delete a frozen resource by answering no");
    assert_eq!(error.code, "FROZEN_GATE_ANSWER_CHANGED");
}

#[tokio::test]
async fn every_resource_behind_one_gate_follows_the_same_answer() {
    let state = unanswered_deployment(&["optional", "optional-copy"]).await;
    let updated = update(state, Some(false)).await;
    assert_eq!(
        updated.status,
        DeploymentStatus::Updating,
        "{:?}",
        updated.error
    );
    for id in ["optional", "optional-copy"] {
        assert!(!pending_stack(&updated).resources.contains_key(id), "{id}");
    }

    let turned_on = rerun_setup(
        &updated,
        next_release(&["optional", "optional-copy"]),
        Some(true),
    )
    .await;
    for id in ["optional", "optional-copy"] {
        assert!(
            turned_on
                .prepared_stack
                .as_ref()
                .unwrap()
                .resources
                .contains_key(id),
            "{id}"
        );
    }
}
