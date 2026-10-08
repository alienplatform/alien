//! Where `sandbox/images` may be granted on Azure, driven through the built-in preflight registry
//! so the check fails here if it stops being registered.

use alien_core::{
    ManagementPermissions, PermissionProfile, PermissionsConfig, Platform, ResourceLifecycle,
    Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy, Stack, Worker, WorkerCode,
};
use alien_preflights::runner::PreflightRunner;

fn sandbox(id: &str, image: &str) -> Sandbox {
    Sandbox::new(id.to_string())
        .code(SandboxCode::Image {
            image: image.to_string(),
        })
        .egress(SandboxEgress::Deny)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build()
}

fn stack(image: &str, management: ManagementPermissions) -> Stack {
    Stack::new("images-gate".to_string())
        .permissions(PermissionsConfig::new().with_profile("execution", PermissionProfile::new()))
        .add(
            Worker::new("api".to_string())
                .permissions("execution".to_string())
                .code(WorkerCode::Image {
                    image: "registry.example.com/api:latest".to_string(),
                })
                .build(),
            ResourceLifecycle::Live,
        )
        .add(sandbox("agents", image), ResourceLifecycle::Frozen)
        .management(management)
        .build()
}

async fn run(stack: &Stack) -> (bool, String) {
    let summary = PreflightRunner::new()
        .run_compile_time_checks(stack, Platform::Azure)
        .await
        .expect("compile-time checks run");
    (summary.success, format!("{summary:#?}"))
}

const PYTHON: &str = "docker.io/library/python:3.14-slim";

#[tokio::test]
async fn misplaced_image_grants_fail_preflight_with_the_fix() {
    let cases = [
        (
            "at *",
            ManagementPermissions::Extend(PermissionProfile::new().global(["sandbox/images"])),
            "cannot be granted at '*'",
        ),
        (
            "under a worker",
            ManagementPermissions::Extend(
                PermissionProfile::new().resource("api", ["sandbox/images"]),
            ),
            "granted under 'api', which is not a sandbox",
        ),
        (
            "override without it",
            ManagementPermissions::override_(
                PermissionProfile::new().global(["sandbox/heartbeat"]),
            ),
            "add 'sandbox/images' under 'agents'",
        ),
    ];
    for (name, management, expected) in cases {
        let (success, rendered) = run(&stack(PYTHON, management)).await;
        eprintln!("--- {name} ---\n{rendered}");
        assert!(!success, "{name} must fail preflight");
        assert!(rendered.contains(expected), "{name}: {rendered}");
    }
}

#[tokio::test]
async fn the_shapes_that_bind_pass() {
    let cases = [
        ("auto, registry", PYTHON, ManagementPermissions::Auto),
        ("auto, catalog", "ubuntu", ManagementPermissions::Auto),
        (
            "override granting it per sandbox",
            PYTHON,
            ManagementPermissions::override_(
                PermissionProfile::new().resource("agents", ["sandbox/images"]),
            ),
        ),
        (
            "override, catalog, no images",
            "ubuntu",
            ManagementPermissions::override_(
                PermissionProfile::new().global(["sandbox/heartbeat"]),
            ),
        ),
    ];
    for (name, image, management) in cases {
        let (_, rendered) = run(&stack(image, management)).await;
        assert!(
            !rendered.contains("sandbox/images"),
            "{name} must not trip the images check: {rendered}"
        );
    }
}
