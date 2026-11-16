//! A frozen identity that is new in a release and unused while its gate is off must not reach
//! the update executor's target: the executor plans from `pending_prepared_stack`, and creating
//! the identity would need setup's credentials.
use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus,
    EnvironmentVariablesSnapshot, PermissionProfile, PermissionsConfig, Platform, ReleaseInfo,
    ResourceLifecycle, ResourceStatus, RuntimeMetadata, ServiceAccount, Stack,
    StackInputDefinition, StackResourceState, StackState, Storage, Worker, WorkerCode,
};
use serde_json::json;

const GATE: &str = "feature";

fn client() -> ClientConfig {
    ClientConfig::Local {
        state_directory: "/unused".into(),
    }
}

fn config(input: Option<bool>) -> DeploymentConfig {
    alien_deployment::set_secrets_digest_key_if_unset([9; 32]);
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

fn gate() -> StackInputDefinition {
    StackInputDefinition::deployer_boolean(GATE, "Feature", "Turn the feature on", Some(false))
}

fn worker(id: &str, profile: &str) -> Worker {
    Worker::new(id.to_string())
        .permissions(profile.to_string())
        .code(WorkerCode::Image {
            image: "example.com/app:latest".to_string(),
        })
        .build()
}

fn base(profiles: &[&str]) -> alien_core::StackBuilder {
    let mut permissions = PermissionsConfig::new();
    for profile in profiles {
        permissions = permissions.with_profile(*profile, PermissionProfile::new());
    }
    Stack::new("unused-identity".into())
        .permissions(permissions)
        .add(
            Storage::new("kept".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(worker("o", "base"), ResourceLifecycle::Live)
}

/// What setup installed: an ungated worker and its identity, no `app` identity.
fn installed_release() -> Stack {
    base(&["base"]).build()
}

/// The next release: the identity `app-sa` of profile `app`, whose only user is gated.
fn next_release() -> Stack {
    base(&["base", "app"])
        .inputs(vec![gate()])
        .add(
            ServiceAccount::new("app-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_enabled_when(worker("w", "app"), ResourceLifecycle::Live, GATE)
        .build()
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

async fn deployment() -> DeploymentState {
    deployment_between(installed_release(), next_release()).await
}

async fn deployment_between(setup: Stack, next: Stack) -> DeploymentState {
    let metadata = alien_deployment::prepare_direct_setup_update(
        setup.clone(),
        &StackState::new(Platform::Aws),
        &config(None),
        &client(),
        &RuntimeMetadata::default(),
    )
    .await
    .expect("initial setup preparation");
    let mut installed = StackState::new(Platform::Aws);
    for (id, entry) in metadata.prepared_stack.as_ref().unwrap().resources() {
        let mut resource = StackResourceState::new_pending(
            entry.config.resource_type().to_string(),
            entry.config.clone(),
            Some(entry.lifecycle),
            entry.dependencies.clone(),
        );
        resource.status = ResourceStatus::Running;
        resource.internal_state = Some(json!({}));
        installed.resources.insert(id.clone(), resource);
    }
    DeploymentState {
        operator_request: None,
        status: DeploymentStatus::UpdatePending,
        platform: Platform::Aws,
        current_release: Some(release(setup, "1")),
        target_release: Some(release(next, "2")),
        stack_state: Some(installed),
        error: None,
        environment_info: None,
        runtime_metadata: Some(metadata),
        retry_requested: false,
        protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
    }
}

#[tokio::test]
async fn the_update_target_has_no_identity_nothing_uses() {
    let result = alien_deployment::step(deployment().await, config(None), client(), None)
        .await
        .expect("update step")
        .state;
    assert_eq!(
        result.status,
        DeploymentStatus::Updating,
        "{:?}",
        result.error
    );
    let target = result
        .runtime_metadata
        .as_ref()
        .unwrap()
        .pending_prepared_stack
        .as_ref()
        .expect("the update prepared a target stack");
    // The executor creates whatever its target holds that the state lacks.
    assert!(target.resources.contains_key("kept"));
    assert!(
        !target.resources.contains_key("app-sa"),
        "{:?}",
        target.resources.keys().collect::<Vec<_>>()
    );
    assert!(!target.resources.contains_key("w"));
    assert!(!result
        .stack_state
        .as_ref()
        .unwrap()
        .resources
        .contains_key("app-sa"));
}

#[tokio::test]
async fn with_the_gate_on_the_comparison_still_refuses_the_identity() {
    let result =
        alien_deployment::step(deployment().await, config(Some(true)), client(), None).await;
    let message = match result {
        Ok(step) => format!("{:?} {:?}", step.state.status, step.state.error),
        Err(error) => format!("{error:?}"),
    };
    assert!(
        message.contains("app-sa") || message.contains("SETUP") || message.contains("Setup"),
        "{message}"
    );
    assert!(!message.contains("Updating"), "{message}");
}

mod registry_regions {
    //! An update accepted while a registry's regions input is empty must record the registry
    //! without the reference, as setup installed it. Otherwise a later answer that lists regions
    //! compares equal to the recorded reference and never asks for setup.
    use super::*;
    use alien_core::{ArtifactRegistry, StackInputKind};

    const REGIONS: &str = "regions";

    fn regions_input() -> StackInputDefinition {
        let mut input = StackInputDefinition::deployer_boolean(REGIONS, "Regions", "Regions", None);
        input.kind = StackInputKind::StringList;
        input.required = false;
        input
    }

    fn registry(reference: bool) -> ArtifactRegistry {
        let mut registry = ArtifactRegistry::new("registry".to_string()).build();
        if reference {
            registry.replication_regions_input = Some(REGIONS.to_string());
        }
        registry
    }

    fn stack(reference: bool) -> Stack {
        let mut builder = Stack::new("registry-regions".into())
            .add(registry(reference), ResourceLifecycle::Frozen);
        if reference {
            builder = builder.inputs(vec![regions_input()]);
        }
        builder.build()
    }

    #[tokio::test]
    async fn an_accepted_empty_reference_is_not_recorded() {
        let state = deployment_between(stack(false), stack(true)).await;
        let mut answers = config(None);
        answers.input_values.insert(REGIONS.into(), json!([]));
        let result = alien_deployment::step(state, answers, client(), None)
            .await
            .expect("update step")
            .state;
        assert_eq!(
            result.status,
            DeploymentStatus::Updating,
            "{:?}",
            result.error
        );
        let target = result
            .runtime_metadata
            .as_ref()
            .unwrap()
            .pending_prepared_stack
            .as_ref()
            .unwrap();
        let registry = target
            .resources
            .get("registry")
            .unwrap()
            .config
            .downcast_ref::<ArtifactRegistry>()
            .unwrap();
        assert_eq!(registry.replication_regions_input, None);
    }
}
