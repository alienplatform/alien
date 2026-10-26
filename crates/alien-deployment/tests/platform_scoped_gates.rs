//! Deployment lifecycle regression for a gate whose input is unavailable on the target.
use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus, Platform, ReleaseInfo,
    ResourceLifecycle, Stack, StackInputDefinition, Storage,
};

fn release(stack: Stack, version: &str) -> ReleaseInfo {
    ReleaseInfo {
        release_id: Some(version.into()),
        version: Some(version.into()),
        description: None,
        stack,
    }
}

fn stack_with_bucket(lifecycle: ResourceLifecycle) -> Stack {
    let mut input = StackInputDefinition::deployer_boolean(
        "managedBucket",
        "Managed bucket",
        "Create a bucket",
        Some(true),
    );
    input.platforms = Some(vec![Platform::Aws]);
    Stack::new("scoped-gate".into())
        .inputs(vec![input])
        .add_enabled_when(
            Storage::new("data".into()).build(),
            lifecycle,
            "managedBucket",
        )
        .build()
}

async fn run(mut state: DeploymentState, config: &DeploymentConfig) -> DeploymentState {
    for _ in 0..30 {
        state = alien_deployment::step(state, config.clone(), ClientConfig::Machines, None)
            .await
            .expect("deployment step")
            .state;
        println!("status={:?}, error={:?}", state.status, state.error);
        if state.status == DeploymentStatus::Running {
            return state;
        }
        assert!(
            state.error.is_none(),
            "deployment failed: {:?}",
            state.error
        );
    }
    panic!("did not reach Running: {:?}", state.status);
}

#[tokio::test]
async fn machines_create_and_upgrade_decline_aws_only_storage() {
    for lifecycle in [ResourceLifecycle::Frozen, ResourceLifecycle::Live] {
        for provided in [None, Some(true)] {
            let mut config = DeploymentConfig::builder()
                .stack_settings(alien_core::StackSettings::default())
                .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                    variables: vec![],
                    hash: String::new(),
                    created_at: String::new(),
                })
                .allow_frozen_changes(false)
                .external_bindings(alien_core::ExternalBindings::default())
                .build();
            if let Some(value) = provided {
                config
                    .input_values
                    .insert("managedBucket".into(), serde_json::json!(value));
            }
            let empty = Stack::new("scoped-gate".into()).build();
            let mut initial = DeploymentState {
                status: DeploymentStatus::Pending,
                platform: Platform::Machines,
                current_release: None,
                target_release: Some(release(empty, "1.0.0")),
                stack_state: None,
                error: None,
                environment_info: None,
                runtime_metadata: None,
                retry_requested: false,
                protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
            };
            let mut state = run(initial.clone(), &config).await;
            state.target_release = Some(release(stack_with_bucket(lifecycle), "2.0.0"));
            state.status = DeploymentStatus::UpdatePending;
            let updated = run(state, &config).await;
            assert!(!updated
                .stack_state
                .as_ref()
                .unwrap()
                .resources
                .contains_key("data"));
            assert!(!updated
                .runtime_metadata
                .as_ref()
                .unwrap()
                .prepared_stack
                .as_ref()
                .unwrap()
                .resources
                .contains_key("data"));
            initial.target_release = Some(release(stack_with_bucket(lifecycle), "2.0.0"));
            let created = run(initial, &config).await;
            assert!(!created
                .stack_state
                .as_ref()
                .unwrap()
                .resources
                .contains_key("data"));
        }
    }
}

fn config() -> DeploymentConfig {
    DeploymentConfig::builder()
        .stack_settings(alien_core::StackSettings::default())
        .environment_variables(alien_core::EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .allow_frozen_changes(false)
        .external_bindings(Default::default())
        .build()
}

#[tokio::test]
async fn setup_update_refuses_scope_narrowing_for_installed_storage() {
    for lifecycle in [ResourceLifecycle::Frozen, ResourceLifecycle::Live] {
        let mut old = stack_with_bucket(lifecycle);
        old.inputs[0].platforms = None;
        for gate in ["managedBucket", "replacementBucket"] {
            let mut target = old.clone();
            target.inputs[0].platforms = Some(vec![Platform::Gcp]);
            target.inputs[0].id = gate.into();
            target.resources.get_mut("data").unwrap().enabled_when = Some(gate.into());
            let mut installed = alien_core::StackState::new(Platform::Aws);
            let mut bucket = alien_core::StackResourceState::new_pending(
                "storage".into(),
                old.resources["data"].config.clone(),
                Some(lifecycle),
                vec![],
            );
            bucket.status = alien_core::ResourceStatus::Running;
            bucket.internal_state = Some(serde_json::json!({
                "type": "AwsStorageController", "_controllerStateVersion": 1,
                "state": "ready", "bucketName": "scoped-gate-data",
            }));
            installed.resources.insert("data".into(), bucket);
            for automatic_setup in [false, true] {
                let metadata = alien_core::RuntimeMetadata {
                    prepared_stack: Some(old.clone()),
                    automatic_setup,
                    persisted_gate_answers: [("managedBucket".into(), true)].into(),
                    ..Default::default()
                };
                let error = alien_deployment::prepare_direct_setup_update(
                    target.clone(),
                    &installed,
                    &config(),
                    &ClientConfig::Local {
                        state_directory: "/unused".into(),
                    },
                    &metadata,
                )
                .await
                .expect_err("scope narrowing must never authorize deletion, even during setup");
                assert!(
                    format!("{error:?}").contains("GATE_PLATFORM_SCOPE_NARROWED"),
                    "{error:?}"
                );
                assert!(
                    format!("{error:?}").contains("already installed"),
                    "{error:?}"
                );
                let current = DeploymentState {
                    status: DeploymentStatus::UpdatePending,
                    platform: Platform::Aws,
                    current_release: Some(release(old.clone(), "1.0.0")),
                    target_release: Some(release(target.clone(), "2.0.0")),
                    stack_state: Some(installed.clone()),
                    error: None,
                    environment_info: None,
                    runtime_metadata: Some(metadata),
                    retry_requested: false,
                    protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
                };
                let result = alien_deployment::step(
                    current,
                    config(),
                    ClientConfig::Local {
                        state_directory: "/unused".into(),
                    },
                    None,
                )
                .await;
                let failure = match result {
                    Err(error) => format!("{error:?}"),
                    Ok(result) => format!("{:?}", result.state.error.expect("update must fail")),
                };
                assert!(failure.contains("already installed"), "{failure}");
            }
        }
    }
}

#[tokio::test]
async fn widening_an_unasked_frozen_gate_can_enable_storage_during_setup() {
    let mut scoped = stack_with_bucket(ResourceLifecycle::Frozen);
    scoped.inputs[0].platforms = Some(vec![Platform::Gcp]);
    let state = alien_core::StackState::new(Platform::Aws);
    let client = ClientConfig::Local {
        state_directory: "/unused".into(),
    };
    let metadata = alien_deployment::prepare_direct_setup_update(
        scoped,
        &state,
        &config(),
        &client,
        &Default::default(),
    )
    .await
    .expect("excluded storage needs no setup");
    assert!(metadata.persisted_gate_answers.is_empty());
    assert!(!metadata
        .prepared_stack
        .as_ref()
        .unwrap()
        .resources
        .contains_key("data"));
    let mut config = config();
    config
        .input_values
        .insert("managedBucket".into(), serde_json::json!(true));
    let widened = alien_deployment::prepare_direct_setup_update(
        stack_with_bucket(ResourceLifecycle::Frozen),
        &state,
        &config,
        &client,
        &metadata,
    )
    .await
    .expect("the newly offered gate accepts true during setup");
    assert_eq!(
        widened.persisted_gate_answers.get("managedBucket"),
        Some(&true)
    );
    assert!(widened
        .prepared_stack
        .as_ref()
        .unwrap()
        .resources
        .contains_key("data"));
}

#[tokio::test]
async fn kubernetes_on_aws_declines_aws_scoped_gate() {
    let mut config = config();
    config.base_platform = Some(Platform::Aws);
    let prepared = alien_deployment::prepare_direct_setup_update(
        stack_with_bucket(ResourceLifecycle::Frozen),
        &alien_core::StackState::new(Platform::Kubernetes),
        &config,
        &ClientConfig::Local {
            state_directory: "/unused".into(),
        },
        &Default::default(),
    )
    .await
    .expect("base cloud does not make AWS inputs available on Kubernetes");
    assert!(prepared.persisted_gate_answers.is_empty());
    assert!(!prepared
        .prepared_stack
        .unwrap()
        .resources
        .contains_key("data"));
}
