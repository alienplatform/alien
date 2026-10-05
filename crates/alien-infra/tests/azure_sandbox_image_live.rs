//! The Azure sandbox controller building disk images on a real sandbox group. Ignored: it needs
//! the `AZURE_TARGET_*` service principal holding `sandbox/images` on the group that
//! `AZURE_RESOURCE_GROUP` and `AZURE_SANDBOX_GROUP` name, in `AZURE_SANDBOX_REGION` (westus2).
//!
//! ```text
//! cargo test -p alien-infra --features all-platforms,test-utils --test azure_sandbox_image_live -- --ignored --nocapture --test-threads=1
//! ```

#![cfg(feature = "azure")]

use alien_core::{
    AzureClientConfig, AzureCredentials, ClientConfig, Platform, ResourceLifecycle, Sandbox,
    SandboxCode, SandboxEgress, SandboxLifecyclePolicy,
};
use alien_infra::controller_test::SingleControllerExecutor;
use alien_infra::AzureSandboxController;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

fn region() -> String {
    std::env::var("AZURE_SANDBOX_REGION").unwrap_or_else(|_| "westus2".to_string())
}

fn client_config() -> ClientConfig {
    ClientConfig::Azure(Box::new(AzureClientConfig {
        subscription_id: env("AZURE_TARGET_SUBSCRIPTION_ID"),
        tenant_id: env("AZURE_TARGET_TENANT_ID"),
        region: Some(region()),
        credentials: AzureCredentials::ServicePrincipal {
            client_id: env("AZURE_TARGET_CLIENT_ID"),
            client_secret: env("AZURE_TARGET_CLIENT_SECRET"),
        },
        service_overrides: None,
    }))
}

fn sandbox(image: &str) -> Sandbox {
    Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: image.to_string(),
        })
        .egress(SandboxEgress::Allow)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build()
}

/// The controller as the importer seeds it for a setup-created group.
fn adopted(disk_image: Option<&str>) -> AzureSandboxController {
    serde_json::from_value(serde_json::json!({
        "state": if disk_image.is_some() { "ready" } else { "ensureDiskImage" },
        "sandboxGroup": env("AZURE_SANDBOX_GROUP"),
        "region": region(),
        "resourceGroup": env("AZURE_RESOURCE_GROUP"),
        "diskImage": disk_image,
        "egress": { "mode": "allow" },
    }))
    .unwrap_or_else(|error| {
        let shape = serde_json::to_string(&AzureSandboxController::default()).unwrap();
        panic!("controller seed does not deserialize: {error}; shape {shape}")
    })
}

async fn executor(image: &str, controller: AzureSandboxController) -> SingleControllerExecutor {
    SingleControllerExecutor::builder()
        .real_delays()
        .resource(sandbox(image))
        .controller(controller)
        .platform(Platform::Azure)
        .resource_lifecycle(ResourceLifecycle::Frozen)
        .client_config(client_config())
        .build()
        .await
        .expect("executor builds")
}

fn internal(executor: &SingleControllerExecutor) -> serde_json::Value {
    serde_json::to_value(
        executor
            .internal_state::<AzureSandboxController>()
            .expect("controller"),
    )
    .unwrap()
}

/// Steps until the controller sits in `Ready` with nothing pending, or a step fails.
async fn drive(executor: &mut SingleControllerExecutor, label: &str) -> Result<(), String> {
    for step in 0..80 {
        let result = executor.step().await;
        let state = internal(executor);
        eprintln!(
            "[{label}] step {step}: {:?} state={} diskImage={} diskImageId={} retired={}",
            result.as_ref().map(|_| "ok").map_err(|e| e.to_string()),
            state["state"],
            state["diskImage"],
            state["diskImageId"],
            state["retiredDiskImages"]
        );
        if let Err(error) = result {
            return Err(error.to_string());
        }
        if state["state"] == "ready" && step > 0 {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
    Err("did not settle".to_string())
}

const PY314: &str = "docker.io/library/python:3.14-slim";
const PY313: &str = "docker.io/library/python:3.13-slim";

#[tokio::test]
#[ignore = "needs a live Azure sandbox group"]
async fn live_registry_image_build_reuse_rebuild_and_retire() {
    // First build from an imported state: nothing built yet, no binding.
    let mut first = executor(PY314, adopted(None)).await;
    assert!(internal(&first)["diskImage"].is_null());
    drive(&mut first, "build-314")
        .await
        .expect("the build succeeds");
    let built = internal(&first);
    let id_314 = built["diskImageId"].as_str().expect("an id").to_string();
    assert_eq!(built["diskImage"], PY314);

    // A second controller whose create response was "lost": it must adopt, not rebuild.
    let mut again = executor(PY314, adopted(None)).await;
    drive(&mut again, "reuse-314")
        .await
        .expect("the retry adopts");
    assert_eq!(internal(&again)["diskImageId"], id_314.as_str());

    // Change the reference: builds anew and retires the old one, which a Ready tick keeps for
    // the retention window. The id is left in the group for the teardown to take.
    first.update(sandbox(PY313)).expect("update starts");
    drive(&mut first, "rebuild-313")
        .await
        .expect("the rebuild succeeds");
    let rebuilt = internal(&first);
    assert_eq!(rebuilt["diskImage"], PY313);
    assert_ne!(rebuilt["diskImageId"], id_314.as_str());
    first.step().await.expect("the Ready tick keeps it");
    assert_eq!(
        internal(&first)["retiredDiskImages"][0]["id"],
        id_314.as_str()
    );
}

#[tokio::test]
#[ignore = "needs a live Azure sandbox group"]
async fn live_arm64_only_image_fails_with_azures_reason() {
    let mut executor = executor("docker.io/arm64v8/alpine:3.19", adopted(None)).await;
    let error = drive(&mut executor, "arm64")
        .await
        .expect_err("an arm64-only image is refused");
    eprintln!("arm64 error: {error}");
    assert!(error.contains("amd64"), "{error}");
}

#[tokio::test]
#[ignore = "needs a live Azure sandbox group"]
async fn live_missing_tag_fails_with_azures_reason() {
    let mut executor = executor(
        "docker.io/library/python:0.0.0-does-not-exist",
        adopted(None),
    )
    .await;
    let error = drive(&mut executor, "missing-tag")
        .await
        .expect_err("a missing tag is refused");
    assert!(error.contains("not found in the registry"), "{error}");
}

#[tokio::test]
#[ignore = "needs a live Azure sandbox group"]
async fn live_catalog_name_publishes_at_once() {
    let mut executor = executor("ubuntu", adopted(Some("ubuntu"))).await;
    drive(&mut executor, "catalog")
        .await
        .expect("a catalog name serves");
    let state = internal(&executor);
    assert_eq!(state["diskImage"], "ubuntu");
    assert!(state["diskImageId"].is_null());
}
