//! GCP compute & artifacts — function / build / artifact-registry.
//!
//! `compute-cluster` is a platform-only resource (Phase 6d moved its
//! emitter to `alien-terraformx`); the OSS suite asserts the dispatch
//! registry produces a typed `ImportRegistrationMissing` error if the
//! extension is absent.

use super::helpers::{assert_terraform_valid, render, snapshot_module};
use alien_core::{
    ArtifactRegistry, Build, CapacityGroup, ComputeCluster, ErrorData, GcpAgentPlatformEngine,
    Platform, Queue, ResourceLifecycle, ResourceRef, Sandbox, SandboxCode, SandboxEgress,
    SandboxSessionPolicy, ServiceAccount, Stack, StackSettings, Storage, Worker, WorkerCode,
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
                .expect("literal Worker timeout is within supported range")
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
                .public_endpoint(alien_core::WorkerPublicEndpoint {
                    name: "api".to_string(),
                    host_label: None,
                    wildcard_subdomains: false,
                })
                .timeout_seconds(60)
                .expect("literal Worker timeout is within supported range")
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
                .expect("literal Worker timeout is within supported range")
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
                    scale_policy: None,
                    nested_virtualization: None,
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
            supported_aws_regions: Vec::new(),
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

/// The stack the `GcpAgentPlatformEngineMutation` produces: one engine per sandbox, carrying the
/// sandbox's lifecycle, with the sandbox depending on it.
fn gcp_sandbox_stack(lifecycle: ResourceLifecycle) -> Stack {
    Stack::new("acme-sbx".to_string())
        .add(
            GcpAgentPlatformEngine::new("agents-engine".to_string()).build(),
            lifecycle,
        )
        .add_with_dependencies(
            Sandbox::new("agents".to_string())
                .code(SandboxCode::Image {
                    image: "python:3.12".to_string(),
                })
                .egress(SandboxEgress::Deny)
                .session(SandboxSessionPolicy {
                    max_lifetime_seconds: None,
                    idle_suspend_seconds: None,
                })
                .build(),
            lifecycle,
            vec![ResourceRef::new(
                GcpAgentPlatformEngine::RESOURCE_TYPE,
                "agents-engine".to_string(),
            )],
        )
        .build()
}

/// A Frozen sandbox's engine is created by the stack the customer applies, because a resource-level
/// IAM binding cannot name an engine that does not exist yet. `terraform validate` is what proves
/// the `google-beta` provider was declared *and* configured: the type resolves nowhere else, and a
/// `required_providers` entry without a `provider` block fails at plan rather than at emit.
#[test]
fn a_frozen_gcp_sandbox_gets_a_setup_owned_reasoning_engine() {
    let module = render(
        &gcp_sandbox_stack(ResourceLifecycle::Frozen),
        TerraformTarget::Gcp,
        StackSettings::default(),
    );

    let engine_tf = module
        .get("agents_engine.tf")
        .expect("the engine renders into its own file");
    // HCL pads `=` to align an attribute with its siblings, so assertions compare on collapsed
    // whitespace rather than the rendered columns.
    let engine_tf = engine_tf.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        engine_tf.contains("resource \"google_vertex_ai_reasoning_engine\" \"agents_engine\""),
        "{engine_tf}"
    );
    assert!(engine_tf.contains("provider = google-beta"), "{engine_tf}");

    // The registered id is read off the created engine. Registering a derived name instead would
    // hand the controller an engine that does not exist, and every session under it would 404.
    let locals = module
        .get("locals.tf")
        .expect("locals render")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        locals.contains("engineId = google_vertex_ai_reasoning_engine.agents_engine.name"),
        "{locals}"
    );
    // The sandbox registers no engine or template path of its own. A derived one would be a name
    // nothing created: the engine's is server-assigned and the template is built after apply.
    assert!(
        !locals.contains("reasoningEngines"),
        "no path is derived from the setup label:\n{locals}"
    );

    assert_terraform_valid(&module, "gcp frozen sandbox engine");
    snapshot_module("gcp_frozen_sandbox_engine", &module);
}

/// A Live engine belongs to its controller. Setup rendering one would create a second engine the
/// controller never learns about, and `terraform destroy` would then take the parent of sessions
/// that controller still believes it owns.
#[test]
fn a_live_gcp_sandbox_gets_no_engine_and_no_google_beta_provider() {
    let module = render(
        &gcp_sandbox_stack(ResourceLifecycle::Live),
        TerraformTarget::Gcp,
        StackSettings::default(),
    );

    let rendered = module
        .iter()
        .map(|(_, contents)| contents.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !rendered.contains("google_vertex_ai_reasoning_engine"),
        "setup renders nothing for a controller-owned engine:\n{rendered}"
    );
    assert!(
        !rendered.contains("google-beta"),
        "a stack with no beta resource declares no beta provider:\n{rendered}"
    );

    assert_terraform_valid(&module, "gcp live sandbox engine");
}
