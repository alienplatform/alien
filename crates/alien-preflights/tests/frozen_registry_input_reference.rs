//! A release that adds the optional registry-regions input keeps installed stacks updating.
//!
//! The "before" fixture is the setup-owned part of a stack as the SDK wrote it before
//! the registry took its replication regions from a deployer input; "after" is the same stack
//! once it does. Both were generated with the SDK from the authored stack file (Live workloads
//! dropped). An installed stack recorded the "before" shape, so the first update after the
//! release compares it with the "after" shape. Each test prepares both with the real mutations
//! and runs the update compatibility checks, as an update does.

use alien_core::{
    AwsManagementConfig, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings,
    ManagementConfig, Platform, Stack, StackSettings, StackState,
};
use alien_preflights::runner::PreflightRunner;
use serde_json::json;

fn fixture(name: &str) -> Stack {
    let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).expect("fixture exists"))
        .expect("fixture is a stack")
}

fn before() -> Stack {
    fixture("installed_stack_before_registry_regions")
}

fn after() -> Stack {
    fixture("installed_stack_after_registry_regions")
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
    update_errors_between(before(), after(), regions).await
}

async fn update_errors_between(
    installed: Stack,
    release: Stack,
    regions: Option<serde_json::Value>,
) -> Vec<String> {
    let installed = prepare(installed, &deployment_config(None)).await;
    let config = deployment_config(regions);
    let next = prepare(release, &config).await;
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
async fn an_installed_stack_takes_the_release_when_no_regions_are_listed() {
    for answer in [None, Some(json!([]))] {
        let errors = update_errors(answer.clone()).await;
        assert!(
            errors.is_empty(),
            "no regions listed (answer {answer:?}) must not ask for a setup update: {errors:?}"
        );
    }
}

#[tokio::test]
async fn listing_regions_on_an_installed_stack_asks_for_a_setup_update() {
    let errors = update_errors(Some(json!(["us-west-2"]))).await;
    assert!(
        errors.iter().any(|error| error
            .starts_with("Frozen resource 'artifact-registry' was modified")
            && error.contains("replicationRegionsInput")),
        "listing regions needs setup and names the field: {errors:?}"
    );
}

fn registry(stack: &mut Stack) -> &mut alien_core::ArtifactRegistry {
    stack
        .resources
        .get_mut("artifact-registry")
        .expect("the stack has a registry")
        .config
        .downcast_mut::<alien_core::ArtifactRegistry>()
        .expect("it is an artifact registry")
}

fn registry_modified(errors: &[String]) -> bool {
    errors
        .iter()
        .any(|error| error.starts_with("Frozen resource 'artifact-registry' was modified"))
}

#[tokio::test]
async fn dropping_the_regions_a_setup_recorded_needs_a_setup_update() {
    // Recorded with the reference, new without: the excuse is one-directional.
    let errors = update_errors_between(after(), before(), None).await;
    assert!(registry_modified(&errors), "{errors:?}");
}

#[tokio::test]
async fn a_reference_to_an_undeclared_input_is_still_a_change() {
    let mut release = after();
    release.inputs.clear();
    let errors = update_errors_between(before(), release, Some(json!([]))).await;
    assert!(registry_modified(&errors), "{errors:?}");
}

#[tokio::test]
async fn another_registry_change_is_not_excused_by_the_empty_reference() {
    let mut release = after();
    registry(&mut release).replication_regions = vec!["eu-west-1".to_string()];
    let errors = update_errors_between(before(), release, Some(json!([]))).await;
    assert!(registry_modified(&errors), "{errors:?}");
}

#[tokio::test]
async fn a_declared_default_with_regions_applies_when_nothing_was_answered() {
    let mut release = after();
    let input = release
        .inputs
        .iter_mut()
        .find(|input| input.id == "deploymentRegions")
        .expect("the input is declared");
    input.default = Some(alien_core::StackInputDefaultValue::StringList(vec![
        "us-west-2".to_string(),
    ]));
    let errors = update_errors_between(before(), release.clone(), None).await;
    assert!(registry_modified(&errors), "{errors:?}");
    // An explicit empty answer overrides that default.
    let errors = update_errors_between(before(), release, Some(json!([]))).await;
    assert!(errors.is_empty(), "{errors:?}");
}

#[tokio::test]
async fn blank_entries_and_a_comma_string_count_like_the_list() {
    let errors = update_errors(Some(json!([" ", ""]))).await;
    assert!(errors.is_empty(), "{errors:?}");
    let errors = update_errors(Some(json!("us-west-2, eu-west-1"))).await;
    assert!(registry_modified(&errors), "{errors:?}");
}

/// A null answer is no answer: the declared default applies, as the input resolver treats it.
#[tokio::test]
async fn a_null_answer_uses_the_declared_default() {
    let mut release = after();
    release
        .inputs
        .iter_mut()
        .find(|input| input.id == "deploymentRegions")
        .expect("the input is declared")
        .default = Some(alien_core::StackInputDefaultValue::StringList(vec![
        "us-west-2".to_string(),
    ]));
    let errors = update_errors_between(before(), release, Some(serde_json::Value::Null)).await;
    assert!(registry_modified(&errors), "{errors:?}");
}

#[tokio::test]
async fn regions_listed_after_an_accepted_empty_update_ask_for_a_setup_update() {
    // First update: accepted with no regions. The stack recorded for it is the update target.
    let first = deployment_config(Some(json!([])));
    let mut recorded = prepare(after(), &first).await;
    alien_preflights::compatibility::resolve_empty_frozen_input_references(&mut recorded, &first);
    // Second update to the same release: the deployer now lists a region.
    let second = deployment_config(Some(json!(["us-west-2"])));
    let next = prepare(after(), &second).await;
    let errors: Vec<String> = PreflightRunner::new()
        .run_compatibility_checks(&recorded, &next, &second, Platform::Aws)
        .await
        .expect("compatibility checks should run")
        .results
        .iter()
        .flat_map(|result| result.errors.clone())
        .collect();
    assert!(registry_modified(&errors), "{errors:?}");
}
