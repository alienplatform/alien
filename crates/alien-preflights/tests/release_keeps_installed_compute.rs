//! A release prepares the setup-owned compute cluster the installation already has.
//!
//! These tests run the real mutations for an install and for the next release from the same
//! deployment settings, then the update compatibility checks (the Frozen-resource comparison)
//! between the two prepared stacks, as an update does.

use alien_core::{
    compute_planner::plan_compute,
    permissions::{PermissionProfile, PermissionsConfig},
    AwsManagementConfig, ComputeCluster, ComputePoolSelection, ComputeSettings, Container,
    ContainerCode, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings,
    FailureDomainSelection, ManagementConfig, PersistentStorage, Platform, ResourceLifecycle,
    ResourceSpec, Stack, StackResourceState, StackSettings, StackState, VolumeBackups,
};
use alien_preflights::{runner::PreflightRunner, PreflightSummary};

fn container(id: &str, image: &str, persistent: bool) -> Container {
    let builder = Container::new(id.to_string())
        .code(ContainerCode::Image {
            image: image.to_string(),
        })
        .cpu(ResourceSpec {
            min: "1".to_string(),
            desired: "1".to_string(),
        })
        .memory(ResourceSpec {
            min: "1Gi".to_string(),
            desired: "1Gi".to_string(),
        })
        .port(8080)
        .permissions("app".to_string());
    if persistent {
        builder
            .persistent_storage(PersistentStorage {
                size: "20Gi".to_string(),
                mount_path: "/data".to_string(),
                backups: VolumeBackups::default(),
            })
            .stateful(true)
            .replicas(1)
            .build()
    } else {
        builder.build()
    }
}

/// An API container in the generated `general` pool and a database in the generated `stateful`
/// pool. Releases differ only in the database image.
fn release(database_image: &str) -> Stack {
    Stack::new("test-stack".to_string())
        .add(container("api", "api:1", false), ResourceLifecycle::Live)
        .add(
            container("database", database_image, true),
            ResourceLifecycle::Live,
        )
        .permissions(PermissionsConfig::new().with_profile(
            "app",
            PermissionProfile::new().global(["storage/data-read"]),
        ))
        .build()
}

fn with_failure_domains(
    selection: &ComputePoolSelection,
    domains: Option<FailureDomainSelection>,
) -> ComputePoolSelection {
    let mut selection = selection.clone();
    match &mut selection {
        ComputePoolSelection::Fixed {
            failure_domains, ..
        }
        | ComputePoolSelection::Autoscale {
            failure_domains, ..
        } => *failure_domains = domains,
    }
    selection
}

/// Settings as a CLI or API install saves them: the planner's machine choice for every pool,
/// and no failure-domain choice for the stateful pool.
fn settings_without_failure_domains(stack: &Stack) -> ComputeSettings {
    let plan = plan_compute(stack, Platform::Aws, None).expect("compute plan should build");
    ComputeSettings {
        pools: plan
            .pools
            .iter()
            .map(|pool| {
                (
                    pool.pool_id.clone(),
                    with_failure_domains(&pool.recommended, None),
                )
            })
            .collect(),
    }
}

fn deployment_config(compute: ComputeSettings) -> DeploymentConfig {
    DeploymentConfig::builder()
        .stack_settings(StackSettings {
            compute: Some(compute),
            ..StackSettings::default()
        })
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
        .build()
}

/// The stack state an install leaves: every prepared resource recorded with its config.
fn installed_state(prepared: &Stack) -> StackState {
    let mut state = StackState::new(Platform::Aws);
    for (id, entry) in &prepared.resources {
        state.resources.insert(
            id.clone(),
            StackResourceState::new_pending(
                entry.config.resource_type().to_string(),
                entry.config.clone(),
                Some(entry.lifecycle),
                entry.dependencies.clone(),
            ),
        );
    }
    state
}

fn cluster(prepared: &Stack) -> &ComputeCluster {
    prepared.resources["compute"]
        .config
        .downcast_ref::<ComputeCluster>()
        .expect("the stack should have the generated compute cluster")
}

fn errors(summary: &PreflightSummary) -> Vec<String> {
    summary
        .results
        .iter()
        .flat_map(|result| result.errors.clone())
        .collect()
}

struct Installed {
    prepared: Stack,
    state: StackState,
    config: DeploymentConfig,
}

async fn install() -> Installed {
    let stack = release("database:1");
    let config = deployment_config(settings_without_failure_domains(&stack));
    let prepared = PreflightRunner::new()
        .apply_mutations(stack, &StackState::new(Platform::Aws), &config)
        .await
        .expect("install should prepare");
    // The new stateful pool gets the planner's single-domain default at install.
    let installed_cluster = cluster(&prepared);
    assert_eq!(
        prepared.resources["compute"].lifecycle,
        ResourceLifecycle::Frozen
    );
    assert_eq!(
        installed_cluster.failure_domain_spread.get("stateful"),
        Some(&1)
    );
    assert!(!installed_cluster
        .failure_domain_spread
        .contains_key("general"));
    let state = installed_state(&prepared);
    Installed {
        prepared,
        state,
        config,
    }
}

/// The next release from the same settings changes only a Live container. It must prepare the
/// installed compute cluster unchanged, and the update must not ask for setup.
#[tokio::test]
async fn release_that_does_not_touch_compute_keeps_the_installed_cluster() {
    let installed = install().await;
    let runner = PreflightRunner::new();

    let next = runner
        .apply_mutations(release("database:2"), &installed.state, &installed.config)
        .await
        .expect("the release should prepare");

    let summary = runner
        .run_compatibility_checks(&installed.prepared, &next, &installed.config, Platform::Aws)
        .await
        .expect("compatibility checks should run");
    assert!(
        summary.success,
        "an unchanged-compute release must not need setup: {:?}",
        errors(&summary)
    );
    assert_eq!(
        serde_json::to_value(cluster(&next)).unwrap(),
        serde_json::to_value(cluster(&installed.prepared)).unwrap(),
        "the release must prepare the installed compute cluster"
    );

    // The release after that, on the state the second one leaves, stays stable too.
    let third = runner
        .apply_mutations(
            release("database:3"),
            &installed_state(&next),
            &installed.config,
        )
        .await
        .expect("the third release should prepare");
    let summary = runner
        .run_compatibility_checks(&next, &third, &installed.config, Platform::Aws)
        .await
        .expect("compatibility checks should run");
    assert!(summary.success, "{:?}", errors(&summary));
}

/// A real topology change to the setup-owned cluster is still refused as a setup change.
#[tokio::test]
async fn release_that_changes_the_stateful_pool_topology_still_needs_setup() {
    let installed = install().await;
    let runner = PreflightRunner::new();

    let mut compute = installed
        .config
        .stack_settings
        .compute
        .clone()
        .expect("install has compute settings");
    let stateful = compute.pools["stateful"].clone();
    compute.pools.insert(
        "stateful".to_string(),
        with_failure_domains(
            &stateful,
            Some(FailureDomainSelection {
                spread: 1,
                selected_failure_domains: vec!["us-east-1b".to_string()],
            }),
        ),
    );
    let pinned_zone = deployment_config(compute);

    let next = runner
        .apply_mutations(release("database:2"), &installed.state, &pinned_zone)
        .await
        .expect("the release should prepare");
    assert_eq!(
        cluster(&next).selected_failure_domains["stateful"],
        ["us-east-1b"]
    );

    let summary = runner
        .run_compatibility_checks(&installed.prepared, &next, &pinned_zone, Platform::Aws)
        .await
        .expect("compatibility checks should run");
    assert!(!summary.success, "a topology change must need setup");
    let errors = errors(&summary);
    assert!(
        errors
            .iter()
            .any(|error| error.starts_with("Frozen resource 'compute' was modified")),
        "{errors:?}"
    );
}
