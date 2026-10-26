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
