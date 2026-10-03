use alien_core::{
    ComputeCluster, ComputePoolSelection, ComputeSettings, Container, ContainerCode,
    DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, Platform, ResourceLifecycle,
    ResourceSpec, Stack, StackSettings, StackState,
};
use alien_preflights::compatibility::FrozenResourcesUnchangedCheck;
use alien_preflights::mutations::ComputeClusterMutation;
use alien_preflights::{StackCompatibilityCheck, StackMutation};

fn release_stack() -> Stack {
    let container = Container::new("api".to_string())
        .code(ContainerCode::Image {
            image: "test:latest".to_string(),
        })
        .cpu(ResourceSpec {
            min: "0.25".to_string(),
            desired: "0.25".to_string(),
        })
        .memory(ResourceSpec {
            min: "256Mi".to_string(),
            desired: "256Mi".to_string(),
        })
        .ephemeral_storage("40Gi".to_string())
        .port(8080)
        .permissions("test".to_string())
        .build();
    Stack::new("stack".to_string())
        .add(container, ResourceLifecycle::Live)
        .build()
}

fn config(machine: &str, machines: u32) -> DeploymentConfig {
    DeploymentConfig::builder()
        .stack_settings(StackSettings {
            compute: Some(ComputeSettings {
                pools: [(
                    "general".to_string(),
                    ComputePoolSelection::Fixed {
                        machines,
                        machine: Some(machine.to_string()),
                        failure_domains: None,
                    },
                )]
                .into_iter()
                .collect(),
            }),
            ..StackSettings::default()
        })
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build()
}

async fn prepare(stack: Stack, machine: &str, machines: u32) -> Stack {
    let state = StackState::new(Platform::Aws);
    let config = config(machine, machines);
    let mutation = ComputeClusterMutation;
    assert!(mutation.should_run(&stack, &state, &config));
    mutation.mutate(stack, &state, &config).await.unwrap()
}

fn group_machine(stack: &Stack) -> (Option<String>, u64) {
    let cluster = stack
        .resources
        .values()
        .find_map(|entry| entry.config.downcast_ref::<ComputeCluster>())
        .expect("compute cluster");
    let group = &cluster.capacity_groups[0];
    (
        group.instance_type.clone(),
        group.profile.as_ref().unwrap().ephemeral_storage_bytes,
    )
}

async fn frozen_check_passes(old: &Stack, new: &Stack) -> bool {
    FrozenResourcesUnchangedCheck {
        platform: Platform::Aws,
    }
    .check(old, new)
    .await
    .unwrap()
    .success
}

/// Real mutation output (catalog profile grown to the workload's storage) on both sides.
#[tokio::test]
async fn mutated_same_arch_machine_change_passes_and_cross_arch_fails() {
    let installed = prepare(release_stack(), "t4g.micro", 1).await;
    let (machine, storage) = group_machine(&installed);
    assert_eq!(machine.as_deref(), Some("t4g.micro"));
    assert_eq!(storage, 40 * 1024 * 1024 * 1024);

    let resized = prepare(release_stack(), "c7g.medium", 1).await;
    assert_eq!(group_machine(&resized).0.as_deref(), Some("c7g.medium"));
    assert!(frozen_check_passes(&installed, &resized).await);

    let rematerialized = prepare(installed.clone(), "t4g.small", 1).await;
    assert_eq!(
        group_machine(&rematerialized).0.as_deref(),
        Some("t4g.small")
    );
    assert!(frozen_check_passes(&installed, &rematerialized).await);

    let cross_arch = prepare(release_stack(), "m7i.large", 1).await;
    assert!(!frozen_check_passes(&installed, &cross_arch).await);
}

/// Off a fixed disk the installed profile records no request, so the check reads the containers.
#[tokio::test]
async fn mutated_fixed_disk_machine_moves_to_the_requested_disk() {
    let installed = prepare(release_stack(), "i4i.xlarge", 1).await;
    assert_eq!(group_machine(&installed).0.as_deref(), Some("i4i.xlarge"));

    let moved = prepare(release_stack(), "m7i.large", 1).await;
    assert_eq!(
        group_machine(&moved),
        (Some("m7i.large".to_string()), 40 * 1024 * 1024 * 1024)
    );
    assert!(frozen_check_passes(&installed, &moved).await);
}
