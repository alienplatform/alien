//! A release that only resizes a compute pool's disks is applied by the runtime
//! controller, so the update must not ask for setup.
//!
//! These tests run the real compile-time checks and mutations for an install
//! and for the next release from the same deployment settings, then the update
//! compatibility checks between the two prepared stacks, as an update does.

use alien_core::{
    permissions::{PermissionProfile, PermissionsConfig},
    AwsManagementConfig, CapacityGroup, ComputeCluster, ComputePoolSelection, ComputeSettings,
    Container, ContainerCode, DataDiskRequirement, DeploymentConfig, EnvironmentVariablesSnapshot,
    ExternalBindings, MachineProfile, ManagementConfig, Platform, ResourceLifecycle, ResourceSpec,
    Stack, StackResourceState, StackSettings, StackState,
};
use alien_preflights::{runner::PreflightRunner, PreflightSummary};

const GIB: u64 = 1024 * 1024 * 1024;

/// A nested virtualization pool declared in source, with a container on it.
fn release(ephemeral_gib: u64, data_disk_gib: Option<u64>, machine_arch_x86: bool) -> Stack {
    let cluster = ComputeCluster::new("compute".to_string())
        .capacity_group(CapacityGroup {
            group_id: "sandbox".to_string(),
            instance_type: None,
            profile: Some(MachineProfile {
                cpu: "2".to_string(),
                memory_bytes: 4 * GIB,
                ephemeral_storage_bytes: ephemeral_gib * GIB,
                architecture: None,
                gpu: None,
                data_disk: data_disk_gib.map(|gib| DataDiskRequirement {
                    size_bytes: gib * GIB,
                }),
            }),
            min_size: 1,
            max_size: 1,
            scale_policy: None,
            nested_virtualization: machine_arch_x86.then_some(true),
        })
        .build();
    let container = Container::new("vm-host".to_string())
        .code(ContainerCode::Image {
            image: "vm-host:1".to_string(),
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
        .permissions("app".to_string())
        .cluster("compute".to_string())
        .pool("sandbox".to_string())
        .build();
    Stack::new("test-stack".to_string())
        .add(cluster, ResourceLifecycle::Frozen)
        .add(container, ResourceLifecycle::Live)
        .permissions(PermissionsConfig::new().with_profile(
            "app",
            PermissionProfile::new().global(["storage/data-read"]),
        ))
        .build()
}

fn deployment_config(machine: &str) -> DeploymentConfig {
    DeploymentConfig::builder()
        .stack_settings(StackSettings {
            compute: Some(ComputeSettings {
                containers: Default::default(),
                pools: [(
                    "sandbox".to_string(),
                    ComputePoolSelection::Fixed {
                        machines: 1,
                        machine: Some(machine.to_string()),
                        failure_domains: None,
                    },
                )]
                .into(),
            }),
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

fn pool(prepared: &Stack) -> &CapacityGroup {
    &prepared.resources["compute"]
        .config
        .downcast_ref::<ComputeCluster>()
        .expect("the stack declares the compute cluster")
        .capacity_groups[0]
}

fn errors(summary: &PreflightSummary) -> Vec<String> {
    summary
        .results
        .iter()
        .flat_map(|result| result.errors.clone())
        .collect()
}

async fn prepare(stack: Stack, state: &StackState, config: &DeploymentConfig) -> Stack {
    let runner = PreflightRunner::new();
    let checks = runner
        .run_compile_time_checks(&stack, Platform::Aws)
        .await
        .expect("compile-time checks should run");
    assert!(checks.success, "{:?}", errors(&checks));
    runner
        .apply_mutations(stack, state, config)
        .await
        .expect("the release should prepare")
}

async fn update(installed: &Stack, next: &Stack, config: &DeploymentConfig) -> PreflightSummary {
    PreflightRunner::new()
        .run_compatibility_checks(installed, next, config, Platform::Aws)
        .await
        .expect("compatibility checks should run")
}

/// A nested virtualization pool installed without a data disk has the volume such
/// pools always received. Declaring a data disk, then growing it, then growing
/// root storage each change only the volume sizes the controller applies.
#[tokio::test]
async fn data_disk_and_storage_changes_need_no_setup() {
    let config = deployment_config("c8i.large");
    let installed = prepare(
        release(20, None, true),
        &StackState::new(Platform::Aws),
        &config,
    )
    .await;
    assert_eq!(pool(&installed).instance_type.as_deref(), Some("c8i.large"));
    assert_eq!(pool(&installed).aws_root_volume_size_gib(), 37);
    assert_eq!(
        pool(&installed).aws_runtime_data_volume_size_gib(),
        Some(25)
    );

    let mut previous = installed;
    for (ephemeral_gib, data_disk_gib, root, data) in [
        (20, Some(30), 37, 30),
        (20, Some(50), 37, 50),
        (40, Some(50), 62, 50),
    ] {
        let next = prepare(
            release(ephemeral_gib, data_disk_gib, true),
            &installed_state(&previous),
            &config,
        )
        .await;
        let summary = update(&previous, &next, &config).await;
        assert!(
            summary.success,
            "{ephemeral_gib} GiB storage with a {data_disk_gib:?} GiB data disk must not need setup: {:?}",
            errors(&summary)
        );
        let group = pool(&next);
        assert_eq!(group.instance_type.as_deref(), Some("c8i.large"));
        assert_eq!(
            group.profile.as_ref().unwrap().data_disk,
            data_disk_gib.map(|gib| DataDiskRequirement {
                size_bytes: gib * GIB
            })
        );
        assert_eq!(group.aws_root_volume_size_gib(), root);
        assert_eq!(group.aws_runtime_data_volume_size_gib(), Some(data));
        previous = next;
    }
}

/// A data disk on a pool without nested virtualization is the only extra volume.
#[tokio::test]
async fn a_data_disk_on_an_ordinary_pool_is_prepared_and_resizable() {
    let config = deployment_config("m7i.large");
    let installed = prepare(
        release(20, Some(30), false),
        &StackState::new(Platform::Aws),
        &config,
    )
    .await;
    assert_eq!(
        pool(&installed).aws_runtime_data_volume_size_gib(),
        Some(30)
    );

    let next = prepare(
        release(20, None, false),
        &installed_state(&installed),
        &config,
    )
    .await;
    assert_eq!(pool(&next).aws_runtime_data_volume_size_gib(), None);
    let summary = update(&installed, &next, &config).await;
    assert!(summary.success, "{:?}", errors(&summary));
}

/// A disk change does not let a machine change across CPU architectures skip setup.
#[tokio::test]
async fn a_disk_change_with_an_architecture_change_still_needs_setup() {
    let installed = prepare(
        release(20, Some(30), false),
        &StackState::new(Platform::Aws),
        &deployment_config("m7i.large"),
    )
    .await;
    let config = deployment_config("m7g.large");
    let next = prepare(
        release(20, Some(50), false),
        &installed_state(&installed),
        &config,
    )
    .await;
    let summary = update(&installed, &next, &config).await;
    assert!(!summary.success);
    let errors = errors(&summary);
    assert!(
        errors.iter().any(|error| error
            .contains("capacity group 'sandbox' changes machine from 'm7i.large' to 'm7g.large'")),
        "{errors:?}"
    );
}

/// A data disk larger than gp3 allows is refused before anything is prepared.
#[tokio::test]
async fn an_over_limit_data_disk_is_refused_at_build() {
    let summary = PreflightRunner::new()
        .run_compile_time_checks(&release(20, Some(64 * 1024 + 1), true), Platform::Aws)
        .await
        .expect("compile-time checks should run");
    assert!(!summary.success);
    assert_eq!(
        errors(&summary),
        ["ComputeCluster 'compute' pool 'sandbox': requirements.dataDisk.size is 65537 GiB, but the largest data disk on platform 'aws' is 65536 GiB."]
    );
}
