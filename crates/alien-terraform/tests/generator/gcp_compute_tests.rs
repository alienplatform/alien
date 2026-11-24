//! GCP compute & artifacts — function / build / artifact-registry.
//!
//! `compute-cluster` is a platform-only resource (Phase 6d moved its
//! emitter to `alien-terraformx`); the OSS suite asserts the dispatch
//! registry produces a typed `ImportRegistrationMissing` error if the
//! extension is absent.

use super::helpers::{assert_terraform_valid, render, snapshot_module};
use alien_core::{
    ArtifactRegistry, Build, CapacityGroup, ComputeCluster, ErrorData, GcpAgentPlatformEngine,
    Platform, Queue, RemoteBindings, ResourceLifecycle, ResourceRef, Sandbox, SandboxCode,
    SandboxEgress, SandboxLifecyclePolicy, ServiceAccount, Stack, StackSettings, Storage, Worker,
    WorkerCode, WorkerTrigger,
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
                    data_disk_gib_range: None,
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
            setup_identity: None,
            pins: alien_core::SetupPins::compiled_in(),
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
                    bundled_from: None,
                })
                .egress(SandboxEgress::None)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                    ..Default::default()
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

/// A Frozen sandbox's manager creates and replaces its templates, so the preflight hands it
/// `sandbox/templates` scoped to that sandbox. It must land on the engine setup creates: at project
/// scope it would let the manager replace any sibling sandbox's image.
#[test]
fn a_frozen_gcp_sandbox_manager_gets_template_verbs_on_its_engine_only() {
    let stack = Stack::new("byo-sandbox".to_string())
        .management(alien_core::permissions::ManagementPermissions::extend(
            alien_core::PermissionProfile::new()
                .global(["sandbox/heartbeat", "sandbox/management"])
                .resource("agents", ["sandbox/templates"]),
        ))
        .add(
            GcpAgentPlatformEngine::new("agents-engine".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            alien_core::RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Sandbox::new("agents".to_string())
                .code(SandboxCode::Image {
                    image: "python:3.12".to_string(),
                    bundled_from: None,
                })
                .egress(SandboxEgress::None)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                    ..Default::default()
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    let rendered = module
        .files
        .values()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");

    let member = rendered
        .split(r#"resource "google_vertex_ai_reasoning_engine_iam_member""#)
        .nth(1)
        .expect("the template grant renders an engine-scoped IAM member")
        .split("\nresource ")
        .next()
        .expect("block ends");
    assert!(
        member.contains("google_vertex_ai_reasoning_engine.agents_engine.name"),
        "{member}"
    );
    assert!(
        member.contains("google_service_account.management.email"),
        "{member}"
    );

    let custom_roles: Vec<&str> = rendered
        .split(r#"resource "google_project_iam_custom_role""#)
        .skip(1)
        .map(|block| block.split("\nresource ").next().expect("block ends"))
        .collect();
    let templates_role = custom_roles
        .iter()
        .find(|role| role.contains("aiplatform.sandboxEnvironmentTemplates.create"))
        .expect("the template verbs render as a custom role");
    assert!(
        !templates_role.contains("aiplatform.reasoningEngines.")
            && !templates_role.contains("aiplatform.sandboxEnvironments."),
        "the template role may not reach the engine or a session:\n{templates_role}"
    );
    let templates_role_ref = format!(
        "google_project_iam_custom_role.{}",
        templates_role
            .trim_start()
            .split('"')
            .nth(1)
            .expect("the role block is labelled")
    );
    for block in rendered
        .split(r#"resource "google_project_iam_member""#)
        .skip(1)
    {
        let block = block.split("\nresource ").next().expect("block ends");
        assert!(
            !block.contains(&templates_role_ref),
            "the template role is bound at project scope:\n{block}"
        );
    }
    assert!(member.contains(&templates_role_ref), "{member}");

    assert_terraform_valid(&module, "gcp frozen sandbox template management");
    snapshot_module("gcp_frozen_sandbox_template_management", &module);
}

/// Two sandboxes that declare preview ports share one project-wide custom role for signing the
/// edge credential: its id is fixed, so one Terraform address per sandbox would create the same
/// role twice and fail at apply. Every signer binding points to the one address.
#[test]
fn two_gcp_preview_sandboxes_share_one_signing_role() {
    let sandbox = |name: &str, engine: &str| {
        (
            GcpAgentPlatformEngine::new(engine.to_string()).build(),
            Sandbox::new(name.to_string())
                .code(SandboxCode::Image {
                    image: "python:3.12".to_string(),
                    bundled_from: None,
                })
                .egress(SandboxEgress::None)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                    ..Default::default()
                })
                .preview_ports(vec![8080])
                .build(),
            ResourceRef::new(GcpAgentPlatformEngine::RESOURCE_TYPE, engine.to_string()),
        )
    };
    let (engine_a, sandbox_a, depends_a) = sandbox("agents", "agents-engine");
    let (engine_b, sandbox_b, depends_b) = sandbox("jobs", "jobs-engine");
    let stack = Stack::new("acme-two".to_string())
        .add(
            ServiceAccount::new("worker-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(engine_a, ResourceLifecycle::Frozen)
        .add(engine_b, ResourceLifecycle::Frozen)
        .add_with_dependencies(sandbox_a, ResourceLifecycle::Frozen, vec![depends_a])
        .add_with_dependencies(sandbox_b, ResourceLifecycle::Frozen, vec![depends_b])
        .permission(
            "worker",
            alien_core::permissions::PermissionProfile::new()
                .resource("agents", ["sandbox/execute"])
                .resource("jobs", ["sandbox/execute"]),
        )
        .build();

    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    let rendered = module
        .files
        .values()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let roles = rendered
        .matches("role_%s_sandbox_preview_sign\", local.gcp_custom_role_prefix) title")
        .count();
    assert_eq!(roles, 1, "one signing role for the stack:\n{rendered}");
    assert_eq!(
        rendered
            .matches(
                "resource \"google_project_iam_custom_role\" \"gcp_role_sandbox_preview_sign\""
            )
            .count(),
        1,
        "{rendered}"
    );
    // Each signer account has its own binding, and both name the one role.
    for signer in ["agents_preview", "jobs_preview"] {
        let binding =
            format!("resource \"google_service_account_iam_member\" \"{signer}_sign_worker_sa\"");
        let block = rendered
            .split(&binding)
            .nth(1)
            .unwrap_or_else(|| panic!("{signer} has a signing binding:\n{rendered}"))
            .split(" resource ")
            .next()
            .unwrap();
        assert!(
            block.contains("google_project_iam_custom_role.gcp_role_sandbox_preview_sign[0].name"),
            "{signer}: {block}"
        );
    }

    assert_terraform_valid(&module, "two gcp preview sandboxes");
    snapshot_module("gcp_two_preview_sandboxes", &module);
}
