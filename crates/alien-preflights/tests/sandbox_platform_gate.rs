//! The Sandbox per-platform gate, driven through the preflight runner.
//!
//! `Sandbox::validate_for_platform` sat unregistered for the whole of this resource's
//! development, and its own unit tests never caught that because they called the method. These
//! run `PreflightRunner::run_compile_time_checks` against the built-in registry, so a check that
//! stops being registered fails here.

use alien_core::{
    PermissionProfile, PermissionsConfig, Platform, Sandbox, SandboxCode, SandboxEgress,
    SandboxLifecyclePolicy, SandboxLimits, Stack, ToolchainConfig, Worker, WorkerCode,
};
use alien_preflights::runner::PreflightRunner;

fn stack_with(sandbox: Sandbox) -> Stack {
    stack_with_lifecycle(sandbox, alien_core::ResourceLifecycle::Frozen)
}

fn stack_with_lifecycle(sandbox: Sandbox, lifecycle: alien_core::ResourceLifecycle) -> Stack {
    // Azure's sandbox ceilings are unenforceable, so a declared ceiling must fail the gate. The
    // worker rounds out the stack; the gate under test is the sandbox capability one.
    Stack::new("sandbox-gate".to_string())
        .permissions(PermissionsConfig::new().with_profile("execution", PermissionProfile::new()))
        .add(
            Worker::new("api".to_string())
                .permissions("execution".to_string())
                .code(WorkerCode::Image {
                    image: "registry.example.com/api:latest".to_string(),
                })
                .build(),
            alien_core::ResourceLifecycle::Live,
        )
        .add(sandbox, lifecycle)
        .build()
}

fn sandbox(limits: Option<SandboxLimits>) -> Sandbox {
    let builder = Sandbox::new("agent".to_string())
        .code(SandboxCode::Image {
            image: "ubuntu".to_string(),
            bundled_from: None,
        })
        .egress(SandboxEgress::None)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
            ..Default::default()
        });
    match limits {
        Some(limits) => builder.limits(limits).build(),
        None => builder.build(),
    }
}

/// A ceiling the platform will not allocate must fail before anything is provisioned, or the
/// stack reads as bounded while the sandbox is not. Azure sizes cpu in steps of 250m, so `333m`
/// is refused here rather than at the first session.
#[tokio::test]
async fn declared_ceilings_fail_preflight_when_the_platform_will_not_allocate_them() {
    let stack = stack_with(sandbox(Some(SandboxLimits {
        cpu: "333m".to_string(),
        memory: "512Mi".to_string(),
        disk: "5120Mi".to_string(),
        max_processes: None,
    })));

    let summary = PreflightRunner::new()
        .run_compile_time_checks(&stack, Platform::Azure)
        .await
        .expect("compile-time checks run");

    assert!(
        !summary.success,
        "unenforceable ceilings must fail preflight, not reach a deployment"
    );
    let rendered = format!("{summary:?}");
    assert!(
        rendered.contains("agent"),
        "the failure must name the sandbox to change: {rendered}"
    );
    assert!(
        rendered.contains("cpu"),
        "and the field to change: {rendered}"
    );
}

/// The same stack without ceilings passes, so the failure above is the gate and not the fixture.
#[tokio::test]
async fn the_same_stack_without_ceilings_passes_preflight() {
    let summary = PreflightRunner::new()
        .run_compile_time_checks(&stack_with(sandbox(None)), Platform::Azure)
        .await
        .expect("compile-time checks run");

    assert!(
        summary.success,
        "no declaration, nothing to refuse: {summary:?}"
    );
}

fn source_sandbox() -> Sandbox {
    Sandbox::new("agent".to_string())
        .code(SandboxCode::Source {
            src: "./sandbox".to_string(),
            toolchain: ToolchainConfig::Docker {
                dockerfile: None,
                target: None,
                build_args: None,
            },
        })
        .egress(SandboxEgress::None)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
            ..Default::default()
        })
        .build()
}

/// `alien build` turns a sandbox's source into an image on AWS and on Kubernetes. Driven through
/// the runner rather than the method, because the runtime's empty-image fallback rests on this
/// gate being registered, not merely on the method refusing when called.
///
/// Live, because the release pushes a source build to a private repository and a Frozen one is
/// built before the registry can open it.
#[tokio::test]
async fn source_reaches_aws_and_kubernetes_through_the_runner() {
    let live = alien_core::ResourceLifecycle::Live;
    for platform in [Platform::Kubernetes, Platform::Aws] {
        let summary = PreflightRunner::new()
            .run_compile_time_checks(&stack_with_lifecycle(source_sandbox(), live), platform)
            .await
            .expect("compile-time checks run");
        assert!(summary.success, "{platform} builds it: {summary:?}");
    }
}

/// Azure and GCP build a source sandbox too, and Frozen: Azure's controller pulls the pushed image
/// with the deployment's own credentials, and GCP's template is created by the release, which runs
/// after the deployment's registry access exists.
#[tokio::test]
async fn source_reaches_azure_and_gcp_frozen_through_the_runner() {
    let frozen = alien_core::ResourceLifecycle::Frozen;
    for platform in [Platform::Azure, Platform::Gcp] {
        let summary = PreflightRunner::new()
            .run_compile_time_checks(&stack_with_lifecycle(source_sandbox(), frozen), platform)
            .await
            .expect("compile-time checks run");
        assert!(summary.success, "{platform} builds it: {summary:?}");
    }
}

#[tokio::test]
async fn unsupported_internet_sandboxes_fail_planning_without_optional_limits() {
    for platform in [Platform::Local] {
        let mut declared = sandbox(None);
        declared.egress = SandboxEgress::Internet;
        let result = PreflightRunner::new()
            .run_compile_time_checks(&stack_with(declared), platform)
            .await
            .expect("preflight runs");
        assert!(
            !result.success,
            "{platform} cannot host the gateway: {result:?}"
        );
        let rendered = format!("{result:?}");
        assert!(
            rendered.contains("SANDBOX_EGRESS_GATEWAY_UNAVAILABLE"),
            "{rendered}"
        );
        assert!(rendered.contains("agent"), "{rendered}");

        let mut legacy = sandbox(None);
        legacy.egress = SandboxEgress::Allow;
        let result = PreflightRunner::new()
            .run_compile_time_checks(&stack_with(legacy), platform)
            .await
            .expect("legacy preflight runs");
        assert!(
            result.success,
            "released Allow stays available on {platform}: {result:?}"
        );
    }
}

/// Azure hosts the egress gateway in the deployment's VNet, so a catalog image (no agent) is
/// refused for `internet`. An image that carries the agent plans on the gateway this release
/// installs. A stack whose installed gateway predates the admin protocol is still refused, for
/// that reason only.
#[tokio::test]
async fn azure_internet_needs_an_agent_image_not_a_catalog_entry() {
    let mut catalog = sandbox(None);
    catalog.egress = SandboxEgress::Internet;
    let refused = PreflightRunner::new()
        .run_compile_time_checks(&stack_with(catalog), Platform::Azure)
        .await
        .expect("preflight runs");
    assert!(!refused.success, "{refused:?}");
    let rendered = format!("{refused:?}");
    assert!(
        rendered.contains("SANDBOX_EGRESS_GATEWAY_UNAVAILABLE"),
        "{rendered}"
    );
    assert!(rendered.contains("Alien sandbox agent"), "{rendered}");

    let mut registry = sandbox(None);
    registry.code = SandboxCode::Image {
        image: "example.azurecr.io/box:1".to_string(),
        bundled_from: None,
    };
    registry.egress = SandboxEgress::Internet;
    let agent_image = PreflightRunner::new()
        .run_compile_time_checks(&stack_with(registry.clone()), Platform::Azure)
        .await
        .expect("preflight runs");
    assert!(agent_image.success, "{agent_image:?}");

    let old_gateway = PreflightRunner::new()
        .run_compile_time_checks(
            &with_installed_gateway(
                stack_with(registry),
                &format!(
                    "ghcr.io/alienplatform/alien-egress-gateway@{}",
                    alien_core::sandbox_egress::NO_ADMIN_PROTOCOL_GATEWAY_DIGESTS[0]
                ),
            ),
            Platform::Azure,
        )
        .await
        .expect("preflight runs");
    // The installed gateway Container stands in for one an earlier release wrote, so the
    // checks of user-declared resources refuse it too; read only the sandbox platform check.
    let sandbox_check = old_gateway
        .results
        .iter()
        .find(|result| {
            result.check_description.as_deref()
                == Some("A Sandbox must declare only what its target platform can enforce")
        })
        .expect("the sandbox platform check runs");
    assert_eq!(
        sandbox_check.errors,
        vec![format!(
            "Sandbox 'agent': {}",
            alien_core::sandbox_egress::AZURE_INTERNET_SANDBOX_UNAVAILABLE
        )]
    );
}

/// `stack` with the managed gateway Container already installed, running `image`.
fn with_installed_gateway(mut stack: Stack, image: &str) -> Stack {
    let gateway_id = alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID;
    stack.resources.insert(
        gateway_id.into(),
        alien_core::ResourceEntry {
            config: alien_core::Resource::new(
                alien_core::Container::new(gateway_id.into())
                    .code(alien_core::ContainerCode::Image {
                        image: image.into(),
                    })
                    .cpu(alien_core::ResourceSpec {
                        min: "0.25".into(),
                        desired: "0.25".into(),
                    })
                    .memory(alien_core::ResourceSpec {
                        min: "512Mi".into(),
                        desired: "512Mi".into(),
                    })
                    .replicas(1)
                    .permissions(gateway_id.into())
                    .port(3128)
                    .build(),
            ),
            lifecycle: alien_core::ResourceLifecycle::Live,
            dependencies: vec![],
            display_name: None,
            description: None,
            remote_access: false,
            enabled_when: None,
        },
    );
    stack
}

/// With the switch flipped, Google Cloud's `internet` plans like every cloud's: through the
/// gateway, so a default policy that names hosts is accepted at plan time too.
#[tokio::test]
async fn gcp_plans_internet_and_an_allowlist_default() {
    let _on = alien_core::gcp_sandbox_gateway::enable_internet_egress_for_test();
    let mut declared = sandbox(None);
    declared.egress = SandboxEgress::Internet;
    // A Google Cloud internet sandbox needs a profile that holds execute on it.
    let granted = |sandbox: Sandbox| {
        let mut stack = stack_with(sandbox);
        stack.permissions.profiles.insert(
            "operator".to_string(),
            PermissionProfile::new().resource("agent", ["sandbox/execute"]),
        );
        stack
    };
    let result = PreflightRunner::new()
        .run_compile_time_checks(&granted(declared.clone()), Platform::Gcp)
        .await
        .expect("preflight runs");
    assert!(result.success, "{result:?}");

    declared.default_policy = Some(alien_core::EgressPolicy::Allow {
        allow: vec!["api.example.com".into()],
    });
    let result = PreflightRunner::new()
        .run_compile_time_checks(&granted(declared), Platform::Gcp)
        .await
        .expect("preflight runs");
    assert!(result.success, "{result:?}");
}

/// Image permissions and the image's own pause and resume steps are AWS Lambda MicroVM only.
/// Every other platform refuses them when the stack is planned, with an error naming the
/// platform, rather than deploying a sandbox the agent would never honor.
#[tokio::test]
async fn image_permissions_and_lifecycle_steps_fail_planning_off_aws() {
    let mut mounts = sandbox(None);
    mounts.image_permissions = Some(alien_core::SandboxImagePermissions {
        mounts: vec![alien_core::SandboxImageMount {
            mount_type: alien_core::SandboxImageMountType::Tmpfs,
            path: "/scratch".to_string(),
            size: Some("1Gi".to_string()),
            from: None,
        }],
        connections: vec![],
    });
    let mut on_pause = sandbox(None);
    on_pause.lifecycle.on_pause = Some(vec!["/bin/sync".to_string()]);
    let mut on_resume = sandbox(None);
    on_resume.lifecycle.on_resume = Some(vec!["/bin/true".to_string()]);

    let live = alien_core::ResourceLifecycle::Live;
    for (field, declared) in [
        ("imagePermissions", mounts),
        ("onPause", on_pause),
        ("onResume", on_resume),
    ] {
        let summary = PreflightRunner::new()
            .run_compile_time_checks(&stack_with_lifecycle(declared.clone(), live), Platform::Aws)
            .await
            .expect("compile-time checks run");
        assert!(summary.success, "AWS plans {field}: {summary:?}");

        for platform in [
            Platform::Gcp,
            Platform::Azure,
            Platform::Kubernetes,
            Platform::Local,
        ] {
            let summary = PreflightRunner::new()
                .run_compile_time_checks(&stack_with_lifecycle(declared.clone(), live), platform)
                .await
                .expect("compile-time checks run");
            assert!(
                !summary.success,
                "{platform} must refuse {field}: {summary:?}"
            );
            let rendered = format!("{summary:?}");
            assert!(rendered.contains(field), "{platform}: {rendered}");
            assert!(
                rendered.contains(&platform.to_string()),
                "the refusal names {platform}: {rendered}"
            );
            assert!(rendered.contains("agent"), "and the sandbox: {rendered}");
        }
    }
}
