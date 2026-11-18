//! Reproduces the "Frozen resource 'compute' was modified" preflight failure offline: the
//! stack a setup registration records (old) against the stack the deployment plans for the
//! same release (new), for the comprehensive-typescript container app on Azure.

use super::*;
use alien_core::{Container, ContainerCode, PermissionProfile, ResourceSpec, Storage, Vault};

fn source_stack() -> Stack {
    let container = Container::new("test-alien-ts-container".to_string())
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

async fn run_case(
    label: &str,
    old_settings: &StackSettings,
    new_settings: &StackSettings,
) -> Vec<String> {
    let source = source_stack();
    let req = PrepareSetupStack {
        platform: Platform::Azure,
        base_platform: None,
        deployment_name: "deployment",
        management_config: None,
        imported_quota_activation: false,
        imported_management_identity: false,
    };
    let old = prepare_setup_stack(source.clone(), &req, old_settings)
        .await
        .unwrap_or_else(|e| panic!("registration stack: {}", e.message));
    let runner = alien_preflights::runner::PreflightRunner::new();
    let new = runner
        .apply_mutations(
            source,
            &StackState::new(Platform::Azure),
            &deploy_config(new_settings),
        )
        .await
        .unwrap_or_else(|e| panic!("deployment stack: {e:?}"));
    let old_compute = serde_json::to_value(&old.resources["compute"].config).unwrap();
    let new_compute = serde_json::to_value(&new.resources["compute"].config).unwrap();
    let mut diff = Vec::new();
    json_diff("compute", &old_compute, &new_compute, &mut diff);
    let summary = runner
        .run_compatibility_checks(&old, &new, &deploy_config(new_settings), Platform::Azure)
        .await
        .unwrap();
    let errors: Vec<String> = summary
        .results
        .iter()
        .flat_map(|r| r.errors.clone())
        .collect();
    println!("CASE {label}\nDIFF {diff:#?}\nCOMPAT {errors:#?}");
    errors
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

#[tokio::test]
async fn compute_cluster_registered_for_azure_matches_the_deployment_plan() {
    let default = StackSettings {
        deployment_model: alien_core::DeploymentModel::Pull,
        ..Default::default()
    };
    let fixed = |machine: &str| {
        with_pool(alien_core::ComputePoolSelection::Fixed {
            machines: 1,
            machine: Some(machine.to_string()),
            failure_domains: None,
            data_disk_gib: None,
        })
    };
    let mut failures = Vec::new();
    for (label, old, new) in [
        ("default/default", default.clone(), default.clone()),
        ("default/D4s_v5", default.clone(), fixed("Standard_D4s_v5")),
        (
            "D4s_v5/D4s_v5",
            fixed("Standard_D4s_v5"),
            fixed("Standard_D4s_v5"),
        ),
        (
            "B2s/D2s_v5",
            fixed("Standard_B2s"),
            fixed("Standard_D2s_v5"),
        ),
        ("default/B2s", default.clone(), fixed("Standard_B2s")),
    ] {
        if !run_case(label, &old, &new).await.is_empty() {
            failures.push(label);
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

struct SkipSeries {
    skip: usize,
    seen: std::cell::RefCell<Vec<&'static str>>,
}

impl alien_core::compute_planner::MachineRoom for SkipSeries {
    fn has_room(
        &self,
        _pool_id: &str,
        spec: &alien_core::instance_catalog::InstanceTypeSpec,
        _machines: u32,
    ) -> bool {
        let mut seen = self.seen.borrow_mut();
        if !seen.contains(&spec.series) {
            seen.push(spec.series);
        }
        seen.iter().position(|s| *s == spec.series).unwrap() >= self.skip
    }
    fn commit(&self, _: &str, _: &alien_core::instance_catalog::InstanceTypeSpec, _: u32) {}
}

#[tokio::test]
async fn quota_chosen_machine_against_the_registered_default() {
    let default = StackSettings {
        deployment_model: alien_core::DeploymentModel::Pull,
        ..Default::default()
    };
    let source = source_stack();
    let runner = alien_preflights::runner::PreflightRunner::new();
    let mut failures = Vec::new();
    for skip in 0..10 {
        let room = SkipSeries {
            skip,
            seen: Default::default(),
        };
        let chosen = alien_preflights::mutations::ComputeClusterMutation.choose_pool_machines(
            source.clone(),
            &StackState::new(Platform::Azure),
            &deploy_config(&default),
            &room,
        );
        let chosen = match chosen {
            Ok(c) => c,
            Err(e) => {
                println!("SKIP {skip}: choose failed {}", e.message);
                continue;
            }
        };
        println!(
            "SKIP {skip}: pools {:?}",
            chosen.stack_settings.compute.as_ref().map(|c| &c.pools)
        );
        let errors = run_case(
            &format!("auto skip {skip}"),
            &default,
            &chosen.stack_settings,
        )
        .await;
        if !errors.is_empty() {
            failures.push((skip, errors));
        }
    }
    let _ = runner;
    assert!(failures.is_empty(), "{failures:#?}");
}

/// First-setup machine selection writes only the machine into the registered stack
/// (`PreflightRunner::with_chosen_machines`); the next deployment derives the profile from it.
#[tokio::test]
async fn first_setup_machine_selection_records_the_profile_of_the_chosen_machine() {
    let default = StackSettings {
        deployment_model: alien_core::DeploymentModel::Pull,
        ..Default::default()
    };
    let req = PrepareSetupStack {
        platform: Platform::Azure,
        base_platform: None,
        deployment_name: "deployment",
        management_config: None,
        imported_quota_activation: false,
        imported_management_identity: false,
    };
    let registered = prepare_setup_stack(source_stack(), &req, &default)
        .await
        .unwrap();
    let chosen = with_pool(alien_core::ComputePoolSelection::Autoscale {
        min: 1,
        max: 2,
        machine: Some("Standard_D2s_v5".to_string()),
        failure_domains: None,
        data_disk_gib: None,
    });
    // what first setup does for a pool the quota moved to Standard_D2s_v5
    let old = alien_preflights::runner::PreflightRunner::with_chosen_machines(
        &registered,
        &StackState::new(Platform::Azure),
        &deploy_config(&chosen),
    )
    .expect("the machine differs from the registered default");
    let runner = alien_preflights::runner::PreflightRunner::new();
    let new = runner
        .apply_mutations(
            source_stack(),
            &StackState::new(Platform::Azure),
            &deploy_config(&chosen),
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
        .run_compatibility_checks(&old, &new, &deploy_config(&chosen), Platform::Azure)
        .await
        .unwrap();
    let errors: Vec<String> = summary
        .results
        .iter()
        .flat_map(|r| r.errors.clone())
        .collect();
    println!("DIFF {diff:#?}\nCOMPAT {errors:#?}");
    assert!(errors.is_empty(), "{errors:#?}");
}
