//! One sandbox declaration runs on every cloud. On Azure and Google Cloud a sandbox declared
//! `live` is made by setup, so the package a customer applies must be the one a `frozen`
//! declaration of the same stack renders: no new resource, grant or difference, and nothing
//! left for the runtime to create that setup cannot place.

use super::helpers::{normalized, prepare_for_target, render};
use alien_core::{
    PermissionProfile, ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress,
    SandboxLifecyclePolicy, Stack, StackSettings,
};
use alien_terraform::TerraformTarget;

fn stack(lifecycle: ResourceLifecycle, grant: &str) -> Stack {
    let profile = if grant == "*" {
        PermissionProfile::new().global(["sandbox/execute", "sandbox/management"])
    } else {
        PermissionProfile::new().resource(grant, ["sandbox/execute", "sandbox/management"])
    };
    Stack::new("one-declaration".to_string())
        .permission("execution", profile)
        .add(
            Sandbox::new("box".to_string())
                .code(SandboxCode::Image {
                    image: "ubuntu".to_string(),
                    bundled_from: None,
                })
                .egress(SandboxEgress::None)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .build(),
            lifecycle,
        )
        .build()
}

#[tokio::test]
async fn a_live_sandbox_renders_the_package_a_frozen_one_does_on_azure_and_google_cloud() {
    for target in [TerraformTarget::Azure, TerraformTarget::Gcp] {
        for grant in ["*", "box"] {
            let rendered = |lifecycle| async move {
                let prepared = prepare_for_target(stack(lifecycle, grant), target).await;
                normalized(&render(&prepared, target, StackSettings::default()))
            };
            let live = rendered(ResourceLifecycle::Live).await;
            let frozen = rendered(ResourceLifecycle::Frozen).await;
            assert!(
                live.contains("sandbox"),
                "{target:?} {grant}: the package carries the sandbox"
            );
            assert_eq!(
                live, frozen,
                "{target:?} grant {grant}: a live declaration renders the frozen package"
            );
        }
    }
}
