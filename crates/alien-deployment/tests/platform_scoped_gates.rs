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
