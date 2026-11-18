//! First setup of an imported Azure install against the next deployment: the stack the setup
//! records (old) against the stack the deployment plans for the same release (new). A
//! difference in the compute cluster reads as "Frozen resource 'compute' was modified".

use super::*;
use alien_core::{Container, ContainerCode, PermissionProfile, ResourceSpec, Storage, Vault};

fn source_stack(ephemeral_storage: Option<&str>) -> Stack {
    source_stack_with_memory("512Mi", ephemeral_storage)
}

fn source_stack_with_memory(memory: &str, ephemeral_storage: Option<&str>) -> Stack {
    let mut container = Container::new("test-alien-ts-container".to_string())
        .code(ContainerCode::Image {
            image: "example.com/web:latest".to_string(),
        })
        .cpu(ResourceSpec {
            min: "0.5".into(),
            desired: "0.5".into(),
        })
        .memory(ResourceSpec {
            min: memory.into(),
            desired: memory.into(),
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
        &Default::default(),
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

/// Offers only the machines `fits` accepts, as the subscription's quota would.
struct QuotaRoom<F: Fn(&str) -> bool>(F);

impl<F: Fn(&str) -> bool> alien_core::compute_planner::MachineRoom for QuotaRoom<F> {
    fn has_room(
        &self,
        _pool_id: &str,
        spec: &alien_core::instance_catalog::InstanceTypeSpec,
        _machines: u32,
    ) -> bool {
        (self.0)(spec.name)
    }

    fn commit(
        &self,
        _pool_id: &str,
        _spec: &alien_core::instance_catalog::InstanceTypeSpec,
        _machines: u32,
    ) {
    }
}

fn compute_of(stack: &Stack) -> alien_core::ComputeCluster {
    stack.resources["compute"]
        .config
        .downcast_ref::<alien_core::ComputeCluster>()
        .unwrap()
        .clone()
}

/// What registration records for an imported install: every setup-owned resource in the
/// prepared stack, the cluster still being provisioned.
fn registered_state(prepared: &Stack) -> StackState {
    let mut state = StackState::new(Platform::Azure);
    for (id, entry) in &prepared.resources {
        if entry.lifecycle != ResourceLifecycle::Frozen {
            continue;
        }
        let mut resource = alien_core::StackResourceState::new_pending(
            entry.config.resource_type().to_string(),
            entry.config.clone(),
            Some(entry.lifecycle),
            entry.combined_dependencies(),
        );
        resource.status = if id == "compute" {
            resource.internal_state = Some(serde_json::json!({"imported": true}));
            ResourceStatus::Provisioning
        } else {
            ResourceStatus::Running
        };
        state.resources.insert(id.clone(), resource);
    }
    state
}

/// The whole imported first setup in one pass: registration, the quota's pick (a machine
/// smaller than the default, with counts the registration did not have), the pick carried
/// into the registered state, the setup's next step, and the first deployment's check. The
/// cluster is the same at every hand-off, and no step asks for setup again.
#[tokio::test]
async fn imported_first_setup_hands_the_same_cluster_from_step_to_step() {
    // A workload that makes the registration default a large machine.
    let source = source_stack_with_memory("6Gi", None);
    let settings = with_pool(alien_core::ComputePoolSelection::Autoscale {
        min: 1,
        max: 3,
        machine: None,
        failure_domains: None,
        data_disk_gib: None,
    });

    // 1. register
    let registered =
        prepare_setup_stack(source.clone(), &registration_request(), &default_settings())
            .await
            .unwrap();
    let default_machine = compute_of(&registered).capacity_groups[0]
        .instance_type
        .clone();
    let mut state = registered_state(&registered);

    // 2. the quota refuses the default machine and offers only a smaller one (8 GiB against the default's)
    let planning =
        alien_preflights::runner::PreflightRunner::first_setup_planning_stack(&source, &registered);
    let chosen = alien_preflights::mutations::ComputeClusterMutation
        .choose_pool_machines(
            planning,
            &state,
            &deploy_config(&settings),
            &QuotaRoom(|machine| machine == "Standard_D2s_v5"),
        )
        .unwrap();
    let picked = alien_preflights::runner::PreflightRunner::with_chosen_machines(
        &registered,
        &state,
        &chosen,
        &alien_preflights::runner::PreflightRunner::declared_cluster_ids(&source),
    )
    .unwrap()
    .expect("the pick differs from the registered default");
    let group = &compute_of(&picked).capacity_groups[0];
    assert_ne!(group.instance_type, default_machine);
    assert_eq!(group.instance_type.as_deref(), Some("Standard_D2s_v5"));
    assert_eq!((group.min_size, group.max_size), (1, 3));

    // 3. carry the pick into the registered state
    alien_deployment::carry_selection_into_imported_state(&mut state, &picked);
    assert_eq!(
        state.resources["compute"].config, picked.resources["compute"].config,
        "the registered cluster equals the picked one"
    );

    // 4. the next setup step (`StackExecutor::continue_imported`) refuses a registered resource
    // whose config or dependencies differ from the stack it plans from. The executor itself
    // cannot run here (the Azure cluster controller is not part of this repository), so its
    // comparison is made directly, for every setup-owned resource.
    for (id, entry) in &picked.resources {
        if entry.lifecycle != ResourceLifecycle::Frozen {
            continue;
        }
        let registered = &state.resources[id];
        assert_eq!(registered.config, entry.config, "{id}: config");
        assert_eq!(
            registered.dependencies,
            entry.combined_dependencies(),
            "{id}: dependencies"
        );
    }

    // 5. the first deployment plans the same release against the installed state
    let runner = alien_preflights::runner::PreflightRunner::new();
    let mut picked_config = deploy_config(&settings);
    picked_config.stack_settings = chosen.stack_settings.clone();
    let planned = runner
        .apply_mutations(source, &state, &picked_config)
        .await
        .unwrap();
    assert_eq!(compute_of(&planned), compute_of(&picked));
    let summary = runner
        .run_compatibility_checks(&picked, &planned, &picked_config, Platform::Azure)
        .await
        .unwrap();
    let errors: Vec<String> = summary
        .results
        .iter()
        .flat_map(|r| r.errors.clone())
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
}
