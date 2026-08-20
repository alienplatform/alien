//! The Sandbox per-platform gate, driven through the preflight runner.
//!
//! `Sandbox::validate_for_platform` sat unregistered for the whole of this resource's
//! development, and its own unit tests never caught that because they called the method. These
//! run `PreflightRunner::run_compile_time_checks` against the built-in registry, so a check that
//! stops being registered fails here.

use alien_core::{
    PermissionProfile, PermissionsConfig, Platform, Sandbox, SandboxCode, SandboxEgress,
    SandboxLimits, SandboxSessionPolicy, Stack, Worker, WorkerCode,
};
use alien_preflights::runner::PreflightRunner;

fn stack_with(sandbox: Sandbox) -> Stack {
    // GCP is the platform whose ceilings are unenforceable, and it also requires a Cloud Run host
    // for any sandbox at all. The worker is here so the gate under test is the one that fires.
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
        .add(sandbox, alien_core::ResourceLifecycle::Frozen)
        .build()
}

fn sandbox(limits: Option<SandboxLimits>) -> Sandbox {
    let builder = Sandbox::new("agent".to_string())
        .code(SandboxCode::Image {
            image: "ubuntu:24.04".to_string(),
        })
        .egress(SandboxEgress::Deny)
        .session(SandboxSessionPolicy {
            max_lifetime_seconds: None,
            idle_suspend_seconds: None,
        });
    match limits {
        Some(limits) => builder.limits(limits).build(),
        None => builder.build(),
    }
}

/// A GCP sandbox runs as a subprocess of the app's own Cloud Run instance, which applies no
/// per-sandbox ceiling. Declaring one has to fail before anything is provisioned, or the stack
/// reads as bounded while the sandbox is not.
#[tokio::test]
async fn declared_ceilings_fail_preflight_on_a_platform_that_ignores_them() {
    let stack = stack_with(sandbox(Some(SandboxLimits {
        cpu: "1".to_string(),
        memory: "2Gi".to_string(),
        disk: "20Gi".to_string(),
        max_processes: None,
    })));

    let summary = PreflightRunner::new()
        .run_compile_time_checks(&stack, Platform::Gcp)
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
        rendered.contains("enforcedLimits"),
        "and the capability that is missing: {rendered}"
    );
}

/// The same stack without ceilings passes, so the failure above is the gate and not the fixture.
#[tokio::test]
async fn the_same_stack_without_ceilings_passes_preflight() {
    let summary = PreflightRunner::new()
        .run_compile_time_checks(&stack_with(sandbox(None)), Platform::Gcp)
        .await
        .expect("compile-time checks run");

    assert!(
        summary.success,
        "no declaration, nothing to refuse: {summary:?}"
    );
}
