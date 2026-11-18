//! First setup of an imported Azure install against the next deployment: the stack the setup
//! records (old) against the stack the deployment plans for the same release (new). A
//! difference in the compute cluster reads as "Frozen resource 'compute' was modified".

use super::*;
use alien_core::{Container, ContainerCode, PermissionProfile, ResourceSpec, Storage, Vault};

fn source_stack(ephemeral_storage: Option<&str>) -> Stack {
    let mut container = Container::new("test-alien-ts-container".to_string())
        .code(ContainerCode::Image {
            image: "example.com/web:latest".to_string(),
        })
        .cpu(ResourceSpec {
            min: "0.5".into(),
            desired: "0.5".into(),
        })
        .memory(ResourceSpec {
            min: "512Mi".into(),
            desired: "512Mi".into(),
        })
        .port(3000)
        .public_endpoint(alien_core::PublicEndpoint {
            name: "http".into(),
            port: 3000,
            protocol: alien_core::ExposeProtocol::Http,
            host_label: None,
            wildcard_subdomains: false,
        })
        .permissions("execution".to_string())
        .build();
    container.ephemeral_storage = ephemeral_storage.map(str::to_string);
    Stack::new("test-alien-ts-stack".to_string())
        .permission("execution", PermissionProfile::new())
        .add(container, ResourceLifecycle::Live)
        .add(
            Storage::new("test-alien-storage".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Vault::new("test-vault".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build()
}

fn deploy_config(settings: &StackSettings) -> DeploymentConfig {
    DeploymentConfig::builder()
        .deployment_name("deployment".to_string())
        .stack_settings(settings.clone())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: "empty".to_string(),
            created_at: "1970-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build()
}

fn json_diff(path: &str, a: &serde_json::Value, b: &serde_json::Value, out: &mut Vec<String>) {
    match (a, b) {
        (serde_json::Value::Object(x), serde_json::Value::Object(y)) => {
            for key in x.keys().chain(y.keys().filter(|k| !x.contains_key(*k))) {
                json_diff(
                    &format!("{path}.{key}"),
                    x.get(key).unwrap_or(&serde_json::Value::Null),
                    y.get(key).unwrap_or(&serde_json::Value::Null),
                    out,
                );
            }
        }
        _ if a != b => out.push(format!("{path}: old={a} new={b}")),
        _ => {}
    }
}

fn with_pool(selection: alien_core::ComputePoolSelection) -> StackSettings {
    let mut settings = StackSettings {
        deployment_model: alien_core::DeploymentModel::Pull,
        ..Default::default()
    };
    settings.compute = Some(alien_core::ComputeSettings {
        pools: HashMap::from([("general".to_string(), selection)]),
        containers: HashMap::new(),
    });
    settings
}

fn registration_request() -> PrepareSetupStack<'static> {
    PrepareSetupStack {
        platform: Platform::Azure,
        base_platform: None,
        deployment_name: "deployment",
        management_config: None,
        imported_quota_activation: false,
        imported_management_identity: false,
    }
}

fn default_settings() -> StackSettings {
    StackSettings {
        deployment_model: alien_core::DeploymentModel::Pull,
        ..Default::default()
    }
}

fn chosen_d2s_v5() -> StackSettings {
    with_pool(alien_core::ComputePoolSelection::Autoscale {
        min: 1,
        max: 2,
        machine: Some("Standard_D2s_v5".to_string()),
        failure_domains: None,
        data_disk_gib: None,
    })
}

/// Compatibility errors between `old` and what the deployment plans for the release.
async fn errors_against_the_plan(
    source: Stack,
    old: &Stack,
    settings: &StackSettings,
) -> (Vec<String>, Vec<String>) {
    let runner = alien_preflights::runner::PreflightRunner::new();
    let new = runner
        .apply_mutations(
            source,
            &StackState::new(Platform::Azure),
            &deploy_config(settings),
        )
        .await
        .unwrap();
    let mut diff = Vec::new();
    json_diff(
        "compute",
        &serde_json::to_value(&old.resources["compute"].config).unwrap(),
        &serde_json::to_value(&new.resources["compute"].config).unwrap(),
        &mut diff,
    );
    let summary = runner
        .run_compatibility_checks(old, &new, &deploy_config(settings), Platform::Azure)
        .await
        .unwrap();
    let errors = summary
        .results
        .iter()
        .flat_map(|r| r.errors.clone())
        .collect();
    (diff, errors)
}

/// First-setup machine selection (`PreflightRunner::with_chosen_machines`) records the whole
/// pool the way the next deployment derives it, so the cluster does not differ at all.
#[tokio::test]
async fn first_setup_machine_selection_records_the_profile_of_the_chosen_machine() {
    let registered = prepare_setup_stack(
        source_stack(None),
        &registration_request(),
        &default_settings(),
    )
    .await
    .unwrap();
    // what first setup does for a pool the quota moved to Standard_D2s_v5
    let old = alien_preflights::runner::PreflightRunner::with_chosen_machines(
        &registered,
        &StackState::new(Platform::Azure),
        &deploy_config(&chosen_d2s_v5()),
    )
    .unwrap()
    .expect("the machine differs from the registered default");
    let (diff, errors) = errors_against_the_plan(source_stack(None), &old, &chosen_d2s_v5()).await;
    assert!(diff.is_empty(), "{diff:#?}");
    assert!(errors.is_empty(), "{errors:#?}");
}

/// An install first set up before the fix holds the chosen machine with the registration
/// default's profile. The planner's own output for the next release (a workload asking for
/// 50 GiB of disk, more than the catalog's 20) is accepted, without a rerun of setup.
#[tokio::test]
async fn an_install_recorded_with_the_default_profile_is_adopted_by_the_next_release() {
    let source = source_stack(Some("50Gi"));
    let mut stuck =
        prepare_setup_stack(source.clone(), &registration_request(), &default_settings())
            .await
            .unwrap();
    let cluster = stuck
        .resources
        .get_mut("compute")
        .unwrap()
        .config
        .downcast_mut::<alien_core::ComputeCluster>()
        .unwrap();
    let default_profile = cluster.capacity_groups[0].profile.clone();
    // the old behaviour: only the machine was written
    cluster.capacity_groups[0].instance_type = Some("Standard_D2s_v5".to_string());
    let (diff, errors) = errors_against_the_plan(source.clone(), &stuck, &chosen_d2s_v5()).await;
    assert!(
        diff.iter().any(|line| line.contains("memoryBytes")),
        "the stuck record differs in the profile: {diff:#?}"
    );
    assert_eq!(
        default_profile.as_ref().map(|p| p.ephemeral_storage_bytes),
        Some(50 * 1024 * 1024 * 1024)
    );
    assert!(errors.is_empty(), "{errors:#?}");
}
