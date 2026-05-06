//! GCP compute & artifacts — function / build / artifact-registry.
//!
//! `container-cluster` is a platform-only resource (Phase 6d moved its
//! emitter to `alien-terraformx`); the OSS suite asserts the dispatch
//! registry produces a typed `ImportRegistrationMissing` error if the
//! extension is absent.

use super::helpers::{assert_terraform_valid, render, snapshot_module};
use alien_core::{
    ArtifactRegistry, Build, CapacityGroup, ContainerCluster, ErrorData, Function, FunctionCode,
    FunctionTrigger, Ingress, Platform, Queue, ResourceLifecycle, Stack, StackSettings, Storage,
};
use alien_terraform::{generate_terraform_module, TerraformOptions, TerraformTarget, TfRegistry};

#[test]
fn gcp_artifact_registry_renders_docker_repository() {
    let stack = Stack::new("acme-ar".to_string())
        .add(
            ArtifactRegistry::new("registry".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_artifact_registry", &module);
    assert_terraform_valid(&module, "gcp_artifact_registry");
}

#[test]
fn gcp_build_renders_cloud_build_trigger() {
    let stack = Stack::new("acme-build".to_string())
        .add(
            Build::new("builder".to_string())
                .permissions("execution".to_string())
                .environment([("PROFILE".to_string(), "release".to_string())].into())
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_build", &module);
    assert_terraform_valid(&module, "gcp_build");
}

#[test]
fn gcp_function_basic_cloud_run() {
    let stack = Stack::new("acme-fn".to_string())
        .add(
            Function::new("api".to_string())
                .code(FunctionCode::Image {
                    image: "us-central1-docker.pkg.dev/proj/app/api:1".to_string(),
                })
                .permissions("execution".to_string())
                .timeout_seconds(30)
                .memory_mb(256)
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_function_basic", &module);
    assert_terraform_valid(&module, "gcp_function_basic");
}

#[test]
fn gcp_function_public_ingress_emits_invoker_binding() {
    let stack = Stack::new("acme-public".to_string())
        .add(
            Function::new("public-api".to_string())
                .code(FunctionCode::Image {
                    image: "us-central1-docker.pkg.dev/proj/app/api:1".to_string(),
                })
                .permissions("execution".to_string())
                .ingress(Ingress::Public)
                .timeout_seconds(60)
                .memory_mb(512)
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_function_public", &module);
    assert_terraform_valid(&module, "gcp_function_public");
}

#[test]
fn gcp_function_with_queue_and_schedule_triggers() {
    let jobs = Queue::new("jobs".to_string()).build();
    let assets = Storage::new("assets".to_string()).build();
    let stack = Stack::new("acme-triggers".to_string())
        .add(jobs.clone(), ResourceLifecycle::Frozen)
        .add(assets.clone(), ResourceLifecycle::Frozen)
        .add(
            Function::new("worker".to_string())
                .code(FunctionCode::Image {
                    image: "us-central1-docker.pkg.dev/proj/app/worker:1".to_string(),
                })
                .permissions("execution".to_string())
                .trigger(FunctionTrigger::queue(&jobs))
                .trigger(FunctionTrigger::schedule("*/5 * * * *"))
                .trigger(FunctionTrigger::storage(
                    &assets,
                    vec!["created".to_string()],
                ))
                .timeout_seconds(60)
                .memory_mb(512)
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_function_with_triggers", &module);
    assert_terraform_valid(&module, "gcp_function_with_triggers");
}

#[test]
fn gcp_container_cluster_without_platform_extension_errors_cleanly() {
    let stack = Stack::new("acme-cluster".to_string())
        .add(
            ContainerCluster::new("compute".to_string())
                .capacity_group(CapacityGroup {
                    group_id: "general".to_string(),
                    instance_type: Some("e2-standard-4".to_string()),
                    profile: None,
                    min_size: 1,
                    max_size: 3,
                })
                .build(),
            ResourceLifecycle::Live,
        )
        .build();

    let registry = TfRegistry::built_in();
    let err = generate_terraform_module(
        &stack,
        TerraformTarget::Gcp,
        TerraformOptions {
            registry: &registry,
            stack_settings: StackSettings::default(),
            registration: None,
        },
    )
    .expect_err("OSS registry should not register container_cluster");

    match err.error.as_ref().expect("typed error") {
        ErrorData::ImportRegistrationMissing {
            resource_type,
            platform,
            ..
        } => {
            assert_eq!(resource_type.as_ref(), "container-cluster");
            assert_eq!(*platform, Platform::Gcp);
        }
        other => panic!("expected ImportRegistrationMissing, got {other:?}"),
    }
}
