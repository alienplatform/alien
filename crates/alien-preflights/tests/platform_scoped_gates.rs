use alien_core::{
    DeploymentConfig, EnvironmentVariablesSnapshot, Platform, ResourceLifecycle, Stack,
    StackInputDefinition, StackSettings, StackState, Storage,
};
use alien_preflights::runner::PreflightRunner;

#[tokio::test]
async fn setup_preparation_only_derives_resources_for_an_available_gate() {
    let mut input = StackInputDefinition::deployer_boolean(
        "managedBucket",
        "Managed bucket",
        "Create a bucket",
        Some(true),
    );
    input.platforms = Some(vec![Platform::Aws]);
    let stack = Stack::new("scoped-gate".into())
        .inputs(vec![input])
        .add_enabled_when(
            Storage::new("data".into()).build(),
            ResourceLifecycle::Frozen,
            "managedBucket",
        )
        .build();
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(Default::default())
        .allow_frozen_changes(false)
        .build();
    let runner = PreflightRunner::new();
    for platform in [
        Platform::Aws,
        Platform::Machines,
        Platform::Kubernetes,
        Platform::Azure,
    ] {
        runner
            .run_template_preflights(&stack, platform)
            .await
            .expect("template checks");
        let prepared = runner
            .apply_mutations(stack.clone(), &StackState::new(platform), &config)
            .await
            .expect("setup preparation");
        if platform == Platform::Aws {
            assert!(prepared.resources.contains_key("data"));
        } else {
            assert!(
                prepared.resources.is_empty(),
                "{platform}: {:?}",
                prepared.resources.keys()
            );
        }
    }
}

#[tokio::test]
async fn render_preparation_removes_only_excluded_container_selections() {
    let mut input = StackInputDefinition::deployer_boolean(
        "enabled",
        "Enabled",
        "Create the container",
        Some(true),
    );
    input.platforms = Some(vec![Platform::Gcp]);
    let stack = Stack::new("scoped-gate".into())
        .inputs(vec![input])
        .add_enabled_when(
            alien_core::Container::new("api".into())
                .code(alien_core::ContainerCode::Image {
                    image: "example.com/api:latest".into(),
                })
                .permissions("execution".into())
                .cpu(alien_core::ResourceSpec {
                    min: "1".into(),
                    desired: "1".into(),
                })
                .memory(alien_core::ResourceSpec {
                    min: "1Gi".into(),
                    desired: "1Gi".into(),
                })
                .build(),
            ResourceLifecycle::Live,
            "enabled",
        )
        .add_enabled_when(
            Storage::new("data".into()).build(),
            ResourceLifecycle::Frozen,
            "enabled",
        )
        .build();
    let mut config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(Default::default())
        .allow_frozen_changes(false)
        .build();
    let selections: alien_core::ComputeSettings = serde_json::from_value(serde_json::json!({
        "containers": { "api": {} }
    }))
    .unwrap();
    config.stack_settings.compute = Some(selections.clone());
    let runner = PreflightRunner::new();
    let state = StackState::new(Platform::Aws);
    let prepared = runner
        .apply_mutations(stack.clone(), &state, &config)
        .await
        .expect("excluded container selection must not prevent rendering");
    assert!(prepared.resources.is_empty());

    for invalid in ["missing", "data"] {
        let mut compute = selections.clone();
        compute
            .containers
            .insert(invalid.into(), Default::default());
        config.stack_settings.compute = Some(compute);
        let error = runner
            .apply_mutations(stack.clone(), &state, &config)
            .await
            .expect_err("unknown and non-container selections must still fail");
        assert!(
            format!("{error:?}").contains("CONTAINER_RESOURCE_SELECTION_INVALID"),
            "{error:?}"
        );
    }
}

#[tokio::test]
async fn build_checks_ignore_excluded_public_container() {
    let mut input = StackInputDefinition::deployer_boolean(
        "enabled",
        "Enabled",
        "Create container",
        Some(true),
    );
    input.platforms = Some(vec![Platform::Aws]);
    let container = alien_core::Container::new("api".into())
        .code(alien_core::ContainerCode::Image {
            image: "example.com/api:latest".into(),
        })
        .permissions("execution".into())
        .cpu(alien_core::ResourceSpec {
            min: "1".into(),
            desired: "1".into(),
        })
        .memory(alien_core::ResourceSpec {
            min: "1Gi".into(),
            desired: "1Gi".into(),
        })
        .public_endpoint(alien_core::PublicEndpoint {
            name: "http".into(),
            port: 8080,
            protocol: alien_core::ExposeProtocol::Http,
            host_label: None,
            wildcard_subdomains: false,
        })
        .build();
    let mut stack = Stack::new("scoped-gate".into())
        .inputs(vec![input])
        .add_enabled_when(container, ResourceLifecycle::Live, "enabled")
        .build();
    stack
        .permissions
        .profiles
        .insert("execution".into(), alien_core::PermissionProfile::new());
    let runner = PreflightRunner::new();
    runner
        .run_build_time_preflights(&stack, Platform::Machines)
        .await
        .expect("excluded public container does not require Machines endpoint support");
    runner
        .run_prebuilt_release_checks(&stack, Platform::Machines)
        .await
        .expect("excluded container also passes prebuilt release checks");
    let mut unscoped = stack;
    unscoped.inputs[0].platforms = None;
    let error = runner
        .run_build_time_preflights(&unscoped, Platform::Machines)
        .await
        .expect_err("an available public container remains unsupported");
    assert!(
        format!("{error:?}").contains("MACHINES_CONTAINER_PUBLIC_ENDPOINT"),
        "{error:?}"
    );
}
