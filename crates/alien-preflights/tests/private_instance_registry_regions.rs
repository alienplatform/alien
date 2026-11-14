//! A release that adds the optional registry-regions input keeps installed instances updating.
//!
//! The "before" fixture is the setup-owned part of an instance stack as the SDK wrote it before
//! the registry took its replication regions from a deployer input; "after" is the same stack
//! once it does. Both were generated with the SDK from the authored stack file (Live workloads
//! dropped). An installed instance recorded the "before" shape, so the first update after the
//! release compares it with the "after" shape. Each test prepares both with the real mutations
//! and runs the update compatibility checks, as an update does.

use alien_core::{
    AwsManagementConfig, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings,
    ManagementConfig, Platform, Stack, StackSettings, StackState,
};
use alien_preflights::runner::PreflightRunner;
use serde_json::json;

fn fixture(name: &str) -> Stack {
    let path = format!(
        "{}/tests/fixtures/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("fixture exists"))
        .expect("fixture is a stack")
}

fn before() -> Stack {
    fixture("private_instance_stack_before_registry_regions")
}

fn after() -> Stack {
    fixture("private_instance_stack_after_registry_regions")
}

fn deployment_config(regions: Option<serde_json::Value>) -> DeploymentConfig {
    let mut config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .management_config(ManagementConfig::Aws(AwsManagementConfig {
            managing_role_arn: "arn:aws:iam::111122223333:role/alien-management".to_string(),
        }))
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    if let Some(regions) = regions {
        config
            .input_values
            .insert("deploymentRegions".to_string(), regions);
    }
    config
}

async fn prepare(stack: Stack, config: &DeploymentConfig) -> Stack {
    PreflightRunner::new()
        .apply_mutations(stack, &StackState::new(Platform::Aws), config)
        .await
        .expect("the stack should prepare")
}

/// Compatibility errors of updating the installed `before` stack to the `after` release, with
/// the deployer's answer for `deploymentRegions` (`None`: never answered).
async fn update_errors(regions: Option<serde_json::Value>) -> Vec<String> {
    let installed = prepare(before(), &deployment_config(None)).await;
    let config = deployment_config(regions);
    let next = prepare(after(), &config).await;
    PreflightRunner::new()
        .run_compatibility_checks(&installed, &next, &config, Platform::Aws)
        .await
        .expect("compatibility checks should run")
        .results
        .iter()
        .flat_map(|result| result.errors.clone())
        .collect()
}

#[tokio::test]
async fn an_installed_instance_takes_the_release_when_no_regions_are_listed() {
    for answer in [None, Some(json!([]))] {
        let errors = update_errors(answer.clone()).await;
        assert!(
            errors.is_empty(),
            "no regions listed (answer {answer:?}) must not ask for a setup update: {errors:?}"
        );
    }
}

#[tokio::test]
async fn listing_regions_on_an_installed_instance_asks_for_a_setup_update() {
    let errors = update_errors(Some(json!(["us-west-2"]))).await;
    assert!(
        errors
            .iter()
            .any(|error| error.starts_with("Frozen resource 'artifact-registry' was modified")
                && error.contains("replicationRegionsInput")),
        "listing regions needs setup and names the field: {errors:?}"
    );
}
