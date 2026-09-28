//! Disk-image builds Azure refuses, against a real sandbox group. Ignored: it needs the
//! `AZURE_TARGET_*` service principal holding `sandbox/images` on the group `AZURE_RESOURCE_GROUP`
//! and `AZURE_SANDBOX_GROUP` name, and `AZURE_PRIVATE_IMAGE`, an image on a registry that refuses
//! anonymous pulls.
//!
//! ```text
//! cargo test -p alien-azure-clients --test azure_disk_image_refusals_live -- --ignored --nocapture
//! ```

use alien_azure_clients::azure::sandbox_data_plane::{
    AzureSandboxDataPlaneClient, CreateDiskImage, SandboxDataPlaneApi,
};
use alien_azure_clients::AzureTokenCache;
use alien_azure_clients::{AzureClientConfig, AzureCredentials};

const TOKEN: &str = "v43r2-registry-token-must-not-leak";

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

fn client() -> AzureSandboxDataPlaneClient {
    let region = std::env::var("AZURE_SANDBOX_REGION").unwrap_or_else(|_| "westus2".to_string());
    let config = AzureClientConfig {
        subscription_id: env("AZURE_TARGET_SUBSCRIPTION_ID"),
        tenant_id: env("AZURE_TARGET_TENANT_ID"),
        region: Some(region.clone()),
        credentials: AzureCredentials::ServicePrincipal {
            client_id: env("AZURE_TARGET_CLIENT_ID"),
            client_secret: env("AZURE_TARGET_CLIENT_SECRET"),
        },
        service_overrides: None,
    };
    AzureSandboxDataPlaneClient::new(
        reqwest::Client::new(),
        &region,
        &env("AZURE_RESOURCE_GROUP"),
        AzureTokenCache::new(config),
    )
}

/// Each refusal names the registry's answer, is not retried, never blames the group, and never
/// carries the registry token, neither rendered nor serialized.
async fn refused(base: String, credentials: bool, expected: &[&str]) {
    let group = env("AZURE_SANDBOX_GROUP");
    let error = client()
        .create_disk_image(
            &group,
            CreateDiskImage {
                base: base.clone(),
                labels: [("alienImage".to_string(), "v43r2-refusal".to_string())].into(),
                registry_credentials: credentials
                    .then(|| ("deployment".to_string(), TOKEN.to_string())),
            },
        )
        .await
        .expect_err("Azure refuses the build");
    let rendered = error.to_string();
    let serialized = serde_json::to_string(&error).expect("the error serializes");
    eprintln!(
        "[{base}] retryable={} rendered: {rendered}",
        error.retryable
    );
    eprintln!("[{base}] serialized: {serialized}");
    for text in expected {
        assert!(rendered.contains(text), "missing {text:?}: {rendered}");
    }
    assert!(!error.retryable, "a registry refusal is not retried");
    assert!(!rendered.contains(&format!("Resource '{group}' not found")));
    assert!(!rendered.contains("Access denied to Resource"));
    assert!(!rendered.contains(TOKEN) && !serialized.contains(TOKEN));
}

#[tokio::test]
#[ignore = "needs a live Azure sandbox group"]
async fn live_missing_tag_names_image_not_found() {
    refused(
        "docker.io/library/python:0.0.0-v43r2-nope".to_string(),
        false,
        &["ImageNotFound", "not found in the registry"],
    )
    .await;
}

#[tokio::test]
#[ignore = "needs a live Azure sandbox group"]
async fn live_private_image_without_credentials_names_the_registry() {
    refused(
        env("AZURE_PRIVATE_IMAGE"),
        false,
        &["Azure refused to build"],
    )
    .await;
}

#[tokio::test]
#[ignore = "needs a live Azure sandbox group"]
async fn live_private_image_with_a_bad_token_names_the_registry() {
    refused(
        env("AZURE_PRIVATE_IMAGE"),
        true,
        &["Azure refused to build"],
    )
    .await;
}

#[tokio::test]
#[ignore = "needs a live Azure sandbox group"]
async fn live_no_refused_build_leaves_an_image_behind() {
    let images = client()
        .list_disk_images(&env("AZURE_SANDBOX_GROUP"))
        .await
        .expect("the group lists");
    let left: Vec<_> = images
        .iter()
        .filter(|image| image.labels.get("alienImage").map(String::as_str) == Some("v43r2-refusal"))
        .map(|image| (image.id.clone(), image.state().map(str::to_string)))
        .collect();
    eprintln!("refusal-labelled images: {left:?}");
    assert!(left.is_empty(), "{left:?}");
}
