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
        })
        .egress(SandboxEgress::Deny)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
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
        .egress(SandboxEgress::Deny)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build()
}

/// `alien build` turns a sandbox's source into an image on AWS, and nowhere else. Driven through
/// the runner rather than the method, because the runtime's empty-image fallback on Kubernetes
/// rests on this gate being registered, not merely on the method refusing when called.
///
/// Live, because the release pushes a source build to a private repository and a Frozen one is
/// built before the registry can open it.
#[tokio::test]
async fn source_reaches_no_platform_but_aws_through_the_runner() {
    let live = alien_core::ResourceLifecycle::Live;
    let summary = PreflightRunner::new()
        .run_compile_time_checks(
            &stack_with_lifecycle(source_sandbox(), live),
            Platform::Kubernetes,
        )
        .await
        .expect("compile-time checks run");

    assert!(
        !summary.success,
        "source has no builder off AWS: {summary:?}"
    );
    let rendered = format!("{summary:?}");
    assert!(
        rendered.contains("agent"),
        "the failure must name the sandbox: {rendered}"
    );

    let on_aws = PreflightRunner::new()
        .run_compile_time_checks(&stack_with_lifecycle(source_sandbox(), live), Platform::Aws)
        .await
        .expect("compile-time checks run");

    assert!(on_aws.success, "AWS builds it: {on_aws:?}");
}
