//! Refused disk-image builds as the Azure sandbox controller reports them, on a real group.
//! Ignored: same environment as `azure_sandbox_image_live`, plus `AZURE_PRIVATE_IMAGE`.
//!
//! ```text
//! cargo test -p alien-infra --features all-platforms,test-utils --test azure_sandbox_image_refusals_live -- --ignored --nocapture --test-threads=1
//! ```

#![cfg(feature = "azure")]

use alien_core::{
    AzureClientConfig, AzureCredentials, ClientConfig, Platform, ResourceLifecycle, Sandbox,
    SandboxCode, SandboxEgress, SandboxLifecyclePolicy,
};
use alien_infra::controller_test::SingleControllerExecutor;
use alien_infra::AzureSandboxController;

/// The token `SingleControllerExecutor` hands the controller as the deployment token.
const DEPLOYMENT_TOKEN: &str = "test-deployment-token";

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

fn region() -> String {
    std::env::var("AZURE_SANDBOX_REGION").unwrap_or_else(|_| "westus2".to_string())
}

async fn executor(image: &str) -> SingleControllerExecutor {
    let controller: AzureSandboxController = serde_json::from_value(serde_json::json!({
        "state": "ready",
        "sandboxGroup": env("AZURE_SANDBOX_GROUP"),
        "region": region(),
        "resourceGroup": env("AZURE_RESOURCE_GROUP"),
        "diskImage": null,
        "egress": { "mode": "allow" },
    }))
    .unwrap();
    SingleControllerExecutor::builder()
        .resource(
            Sandbox::new("agents".to_string())
                .code(SandboxCode::Image {
                    image: image.to_string(),
                })
                .egress(SandboxEgress::Allow)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .build(),
        )
        .controller(controller)
        .platform(Platform::Azure)
        .resource_lifecycle(ResourceLifecycle::Frozen)
        .client_config(ClientConfig::Azure(Box::new(AzureClientConfig {
            subscription_id: env("AZURE_TARGET_SUBSCRIPTION_ID"),
            tenant_id: env("AZURE_TARGET_TENANT_ID"),
            region: Some(region()),
            credentials: AzureCredentials::ServicePrincipal {
                client_id: env("AZURE_TARGET_CLIENT_ID"),
                client_secret: env("AZURE_TARGET_CLIENT_SECRET"),
            },
            service_overrides: None,
        })))
        .build()
        .await
        .expect("executor builds")
}

async fn refused(image: &str, expected: &[&str]) {
    let mut executor = executor(image).await;
    executor.step().await.expect("Ready routes to the build");
    let error = executor.step().await.expect_err("the build is refused");
    let rendered = error.to_string();
    let serialized = serde_json::to_string(&error).unwrap();
    eprintln!(
        "[{image}] retryable={} rendered: {rendered}",
        error.retryable
    );
    eprintln!("[{image}] serialized: {serialized}");
    for text in expected {
        assert!(rendered.contains(text), "missing {text:?}: {rendered}");
    }
    assert!(
        !error.retryable,
        "the executor must not retry a registry refusal"
    );
    let group = env("AZURE_SANDBOX_GROUP");
    assert!(
        !rendered.contains(&format!("Resource '{group}' not found")),
        "{rendered}"
    );
    assert!(
        !rendered.contains("Access denied to Resource"),
        "{rendered}"
    );
    assert!(!serialized.contains(DEPLOYMENT_TOKEN), "{serialized}");
}

#[tokio::test]
#[ignore = "needs a live Azure sandbox group"]
async fn live_controller_missing_tag() {
    refused(
        "docker.io/library/python:0.0.0-does-not-exist",
        &["ImageNotFound", "not found in the registry"],
    )
    .await;
}

#[tokio::test]
#[ignore = "needs a live Azure sandbox group"]
async fn live_controller_private_image_without_credentials() {
    refused(&env("AZURE_PRIVATE_IMAGE"), &["Azure refused to build"]).await;
}

/// A reference on the proxy host is sent with the deployment token; whatever Azure answers, the
/// token stays out of the error.
#[tokio::test]
#[ignore = "needs a live Azure sandbox group"]
async fn live_controller_proxy_host_image_keeps_the_token_out() {
    let image = std::env::var("AZURE_PROXY_IMAGE")
        .unwrap_or_else(|_| "test-manager.alien.dev/artifacts/prj_test/app:v1".to_string());
    let mut executor = executor(&image).await;
    executor.step().await.expect("Ready routes to the build");
    let mut errors = 0;
    for step in 0..40 {
        let result = executor.step().await;
        let state = serde_json::to_value(
            executor
                .internal_state::<AzureSandboxController>()
                .expect("controller"),
        )
        .unwrap();
        let text = match &result {
            Ok(_) => "ok".to_string(),
            Err(error) => {
                errors += 1;
                format!(
                    "retryable={} {} | {}",
                    error.retryable,
                    error,
                    serde_json::to_string(error).unwrap()
                )
            }
        };
        eprintln!(
            "step {step}: {text} state={} diskImageId={} retired={}",
            state["state"], state["diskImageId"], state["retiredDiskImages"]
        );
        assert!(!text.contains(DEPLOYMENT_TOKEN), "{text}");
        if errors >= 2 || (result.is_ok() && state["state"] == "ready") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}
