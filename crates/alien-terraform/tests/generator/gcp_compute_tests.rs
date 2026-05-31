//! GCP compute & artifacts — function / build / artifact-registry.
//!
//! `compute-cluster` is a platform-only resource (Phase 6d moved its
//! emitter to `alien-terraformx`); the OSS suite asserts the dispatch
//! registry produces a typed `ImportRegistrationMissing` error if the
//! extension is absent.

use super::helpers::{assert_terraform_valid, render, snapshot_module};
use alien_core::{
    ArtifactRegistry, Build, CapacityGroup, ComputeCluster, ErrorData, Ingress, Platform, Queue,
    ResourceLifecycle, ServiceAccount, Stack, StackSettings, Storage, Worker, WorkerCode,
    WorkerTrigger,
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
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Build::new("builder".to_string())
                .permissions("execution".to_string())
                .environment([("PROFILE".to_string(), "release".to_string())].into())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_build", &module);
    let build_tf = module
        .get("builder.tf")
        .expect("builder terraform file should render");
    assert!(
        build_tf.contains("service_account = google_service_account.execution_sa.name"),
        "Cloud Build trigger must use the fully-qualified service-account name",
    );
    assert_terraform_valid(&module, "gcp_build");
}

#[test]
fn gcp_function_basic_cloud_run() {
    let stack = Stack::new("acme-fn".to_string())
        .add(
            Worker::new("api".to_string())
                .code(WorkerCode::Image {
                    image: "us-central1-docker.pkg.dev/proj/app/api:1".to_string(),
                })
                .permissions("execution".to_string())
                .timeout_seconds(30)
                .memory_mb(256)
                .build(),
            ResourceLifecycle::Frozen,
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
            Worker::new("public-api".to_string())
                .code(WorkerCode::Image {
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
            Worker::new("worker".to_string())
                .code(WorkerCode::Image {
                    image: "us-central1-docker.pkg.dev/proj/app/worker:1".to_string(),
                })
                .permissions("execution".to_string())
                .trigger(WorkerTrigger::queue(&jobs))
                .trigger(WorkerTrigger::schedule("*/5 * * * *"))
                .trigger(WorkerTrigger::storage(&assets, vec!["created".to_string()]))
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
            ComputeCluster::new("compute".to_string())
                .capacity_group(CapacityGroup {
                    group_id: "general".to_string(),
                    instance_type: Some("e2-standard-4".to_string()),
                    profile: None,
                    min_size: 1,
                    max_size: 3,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let registry = TfRegistry::built_in();
    let err = generate_terraform_module(
        &stack,
        TerraformTarget::Gcp,
        TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings: StackSettings::default(),
            registration: None,
            helm_install: None,
        },
    )
    .expect_err("OSS registry should not register container_cluster");

    match err.error.as_ref().expect("typed error") {
        ErrorData::ImportRegistrationMissing {
            resource_type,
            platform,
            ..
        } => {
            assert_eq!(resource_type.as_ref(), "compute-cluster");
            assert_eq!(*platform, Platform::Gcp);
        }
        other => panic!("expected ImportRegistrationMissing, got {other:?}"),
    }
}
