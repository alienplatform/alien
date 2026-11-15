//! Renders the Terraform module and the prepared stack for the Azure `internet` live proof.
//! Ignored: it only writes files for `scripts/azure-internet-live.sh`, which applies the module
//! and runs the live test against it. The module is what `alien render --format terraform
//! --target azure` emits for this stack, so the proof runs on the shipped emitters.
//!
//! Env: `AZURE_INTERNET_OUT` (an empty directory), `AZURE_INTERNET_IMAGE` (the registry image the
//! sandbox group runs, for example `<registry>.azurecr.io/<repository>:<tag>`).

use super::helpers::{prepare_for_target, render};
use alien_core::{
    ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy, Stack,
    StackSettings,
};
use alien_terraform::TerraformTarget;
use std::path::PathBuf;

#[tokio::test]
#[ignore = "writes files for the live proof; see the module docs"]
async fn render_the_azure_internet_live_module() {
    let out =
        PathBuf::from(std::env::var("AZURE_INTERNET_OUT").expect("AZURE_INTERNET_OUT must be set"));
    let image = std::env::var("AZURE_INTERNET_IMAGE").expect("AZURE_INTERNET_IMAGE must be set");
    assert!(
        !out.exists() || std::fs::read_dir(&out).expect("readable").next().is_none(),
        "{} must be empty",
        out.display()
    );
    // The image is assembled with the agent and ends as root, which a release records as the
    // image's uid; without it the sandbox keeps the legacy native boot and no agent runs.
    let mut sandbox = Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image,
            bundled_from: None,
        })
        .egress(SandboxEgress::Internet)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build();
    sandbox.image_user = Some(0);
    let stack = Stack::new("azint".to_string())
        .add(sandbox, ResourceLifecycle::Frozen)
        .build();
    let prepared = prepare_for_target(stack, TerraformTarget::Azure).await;
    let module = render(&prepared, TerraformTarget::Azure, StackSettings::default());
    std::fs::create_dir_all(&out).expect("output directory");
    for (path, contents) in module.iter() {
        let target = out.join("module").join(path);
        std::fs::create_dir_all(target.parent().expect("a file has a parent"))
            .expect("module directory");
        std::fs::write(target, contents).expect("module file");
    }
    std::fs::write(
        out.join("stack.json"),
        serde_json::to_vec_pretty(&prepared).expect("the prepared stack serializes"),
    )
    .expect("stack file");
}
