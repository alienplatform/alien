//! A provider whose cached disk image is deleted under it, on a real group. Ignored: it needs the
//! `AZURE_TARGET_*` identity holding the data-plane role on `AZURE_RESOURCE_GROUP` /
//! `AZURE_SANDBOX_GROUP`, with `AZURE_SANDBOX_IMAGE` already built there.
//!
//! ```text
//! cargo test -p alien-bindings --features azure --test azure_sandbox_retired_image_live -- --ignored --nocapture
//! ```

#![cfg(feature = "azure")]

use std::collections::{BTreeMap, HashMap};

use alien_azure_clients::azure::sandbox_data_plane::{
    AzureSandboxDataPlaneClient, CreateDiskImage, SandboxDataPlaneApi,
};
use alien_azure_clients::AzureTokenCache;
use alien_bindings::traits::{CreateSandboxRequest, RunCommandRequest, Sandbox};
use alien_bindings::{BindingsProvider, BindingsProviderApi};
use alien_core::{azure_disk_image_label, AzureClientConfig, AzureCredentials, ClientConfig};
use futures::StreamExt;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

async fn os_release(sandbox: &dyn Sandbox, id: &str) -> String {
    let mut stream = sandbox
        .run_command(
            id,
            RunCommandRequest {
                command: "cat".to_string(),
                args: vec!["/etc/os-release".to_string()],
                cwd: None,
                env: BTreeMap::new(),
                timeout: std::time::Duration::from_secs(30),
            },
        )
        .await
        .expect("exec starts");
    let mut stdout = Vec::new();
    while let Some(frame) = stream.next().await {
        if let alien_bindings::traits::CommandOutput::Stdout { data, .. } = frame.expect("frame") {
            stdout.extend(data);
        }
    }
    String::from_utf8_lossy(&stdout).to_string()
}

#[tokio::test]
#[ignore = "needs a live Azure sandbox group with the image built"]
async fn live_a_deleted_cached_image_is_looked_up_again() {
    let region = std::env::var("AZURE_SANDBOX_REGION").unwrap_or_else(|_| "westus2".to_string());
    let image = env("AZURE_SANDBOX_IMAGE");
    let group = env("AZURE_SANDBOX_GROUP");
    let azure = AzureClientConfig {
        subscription_id: env("AZURE_TARGET_SUBSCRIPTION_ID"),
        tenant_id: env("AZURE_TARGET_TENANT_ID"),
        region: Some(region.clone()),
        credentials: AzureCredentials::ServicePrincipal {
            client_id: env("AZURE_TARGET_CLIENT_ID"),
            client_secret: env("AZURE_TARGET_CLIENT_SECRET"),
        },
        service_overrides: None,
    };
    let plane = AzureSandboxDataPlaneClient::new(
        reqwest::Client::new(),
        &region,
        &env("AZURE_RESOURCE_GROUP"),
        AzureTokenCache::new(azure.clone()),
    );
    let label = azure_disk_image_label(&image);
    let ready_ids = || async {
        plane
            .list_disk_images(&group)
            .await
            .expect("lists")
            .into_iter()
            .filter(|i| i.labels.get("alienImage") == Some(&label) && i.state() == Some("Ready"))
            .map(|i| i.id)
            .collect::<Vec<_>>()
    };
    let before = ready_ids().await;
    eprintln!("ready images under the label before: {before:?}");
    assert_eq!(before.len(), 1, "exactly one built image to start from");
    let old = before[0].clone();

    let binding = serde_json::json!({
        "service": "sandbox-azure",
        "sandboxGroup": group,
        "dataPlaneEndpoint": format!("https://management.{region}.azuredevcompute.io"),
        "region": region.clone(),
        "resourceGroup": env("AZURE_RESOURCE_GROUP"),
        "diskImage": image,
        "egress": { "mode": "allow" },
    });
    let provider = BindingsProvider::new(
        ClientConfig::Azure(Box::new(azure)),
        HashMap::from([("box".to_string(), binding)]),
    )
    .unwrap();
    let sandbox = provider.load_sandbox("box").await.expect("loads");

    // First create caches the old id.
    let first = sandbox
        .create(CreateSandboxRequest::default())
        .await
        .unwrap_or_else(|error| panic!("first create failed: {error}"));
    eprintln!("first session {} from {old}", first.sandbox_id);
    sandbox
        .terminate(&first.sandbox_id)
        .await
        .expect("terminate");

    // The controller's replacement: a new image under the same label, then the old one deleted.
    let new = plane
        .create_disk_image(
            &group,
            CreateDiskImage {
                base: image.clone(),
                labels: [("alienImage".to_string(), label.clone())].into(),
                registry_credentials: None,
            },
        )
        .await
        .expect("replacement build starts")
        .id;
    for _ in 0..90 {
        let state = plane.get_disk_image(&group, &new).await.expect("get");
        if state.state() == Some("Ready") {
            break;
        }
        assert_ne!(state.state(), Some("Failed"), "{state:?}");
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
    let mut deleted = false;
    for attempt in 0..12 {
        match plane.delete_disk_image(&group, &old).await {
            Ok(()) => {
                deleted = true;
                break;
            }
            Err(error) => {
                eprintln!("delete {old} attempt {attempt}: {error}");
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }
        }
    }
    assert!(
        deleted,
        "the old image must go for this test to mean anything"
    );
    let gone = plane.get_disk_image(&group, &old).await;
    eprintln!("old image after delete: {gone:?}");
    assert_eq!(ready_ids().await, vec![new.clone()]);

    // What the data plane actually answers for a create naming the deleted image.
    let raw = plane
        .create_sandbox(
            &group,
            alien_azure_clients::azure::sandbox_data_plane::CreateSandbox {
                disk_image: image.clone(),
                disk_image_id: Some(old.clone()),
                cpu: "1000m".to_string(),
                memory: "2048Mi".to_string(),
                ..Default::default()
            },
        )
        .await;
    match &raw {
        Ok(sandbox) => {
            eprintln!("UNEXPECTED: create from the deleted image succeeded: {sandbox:?}");
            let _ = plane.delete_sandbox(&group, &sandbox.id).await;
        }
        Err(error) => eprintln!("raw create from deleted image: {error:?}"),
    }
    assert!(raw.is_err(), "a deleted image must be refused");

    // Same provider, cached id now gone: the create must resolve the replacement.
    let started = std::time::Instant::now();
    let second = sandbox
        .create(CreateSandboxRequest::default())
        .await
        .unwrap_or_else(|error| panic!("second create after the delete failed: {error}"));
    eprintln!(
        "second session {} in {:?} (replacement {new})",
        second.sandbox_id,
        started.elapsed()
    );
    let os = os_release(sandbox.as_ref(), &second.sandbox_id).await;
    sandbox
        .terminate(&second.sandbox_id)
        .await
        .expect("terminate");
    eprintln!("os-release:\n{os}");
    assert!(os.contains("ID="), "{os}");
}
