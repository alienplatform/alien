use alien_core::{
    CapacityGroup, ComputeCluster, Container, ContainerCode, ResourceLifecycle, ResourceSpec,
    Stack, StackSettings,
};
use alien_helm::{generate_helm_chart, HelmOptions, HelmRegistry};

/// Declaring existing-node placement must not add installer inputs, workloads, or RBAC.
/// Runtime pool validation and namespace access are exercised by controller tests.
#[test]
fn frozen_compute_and_referenced_container_keep_helm_install_requirements() {
    let container = Container::new("api".to_string())
        .code(ContainerCode::Image {
            image: "registry.example.com/api:1".to_string(),
        })
        .cpu(ResourceSpec {
            min: "1".to_string(),
            desired: "1".to_string(),
        })
        .memory(ResourceSpec {
            min: "128Mi".to_string(),
            desired: "128Mi".to_string(),
        })
        .permissions("default".to_string())
        .build();
    let baseline = Stack::new("existing-cluster".to_string())
        .add(container.clone(), ResourceLifecycle::Live)
        .build();
    let compute = ComputeCluster::new("compute".to_string())
        .capacity_group(CapacityGroup {
            group_id: "apps".to_string(),
            instance_type: None,
            profile: None,
            min_size: 1,
            max_size: 3,
            scale_policy: None,
            nested_virtualization: None,
        })
        .dynamic_container_pool("apps".to_string())
        .build();
    let referenced = Container {
        cluster: Some("compute".to_string()),
        pool: Some("apps".to_string()),
        ..container
    };
    let portable = Stack::new("existing-cluster".to_string())
        .add(compute, ResourceLifecycle::Frozen)
        .add(referenced, ResourceLifecycle::Live)
        .build();
    let registry = HelmRegistry::built_in();
    let generate = |stack: &Stack| {
        generate_helm_chart(
            stack,
            HelmOptions {
                registry: &registry,
                stack_settings: StackSettings::default(),
                chart_name: "existing-cluster".to_string(),
            },
        )
        .expect("portable stack produces an installable chart")
    };
    let mut portable_files = generate(&portable).files;
    let mut baseline_files = generate(&baseline).files;
    // Stack metadata must retain the declarations for runtime admission. It is the
    // only expected difference; every installer input and rendered template matches.
    let embedded = portable_files
        .shift_remove("files/stack.json")
        .expect("embedded stack metadata");
    let restored: Stack = serde_json::from_str(&embedded).expect("valid embedded stack");
    assert!(alien_core::kubernetes_container_pool(
        &restored,
        restored.resources["api"]
            .config
            .downcast_ref::<Container>()
            .expect("embedded api must remain a Container"),
    )
    .expect("retained pool reference")
    .is_some());
    baseline_files.shift_remove("files/stack.json");
    assert_eq!(portable_files, baseline_files);
}
