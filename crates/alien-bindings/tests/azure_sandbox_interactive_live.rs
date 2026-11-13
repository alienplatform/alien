//! An interactive command (stdin round trip) on a real Azure sandbox running the packaged agent.
//! Ignored: it needs short-lived tokens and a built agent image in a registry the group can pull.
//!
//! Env: `AZURE_TARGET_SUBSCRIPTION_ID`, `AZURE_TARGET_TENANT_ID`, `AZURE_RESOURCE_GROUP`,
//! `AZURE_SANDBOX_GROUP`, `AZURE_SANDBOX_IMAGE` (registry reference), `AZURE_SANDBOX_REGISTRY_TOKEN`
//! (ACR refresh token), `AZURE_ARM_TOKEN`, `AZURE_DATAPLANE_TOKEN`; optional `AZURE_SANDBOX_REGION`.
//!
//! ```text
//! cargo test -p alien-bindings --features azure --test azure_sandbox_interactive_live -- --ignored --nocapture
//! ```

#![cfg(feature = "azure")]

use std::collections::{BTreeMap, HashMap};

use alien_azure_clients::azure::sandbox_data_plane::{
    AzureSandboxDataPlaneClient, CreateDiskImage, SandboxDataPlaneApi,
};
use alien_azure_clients::AzureTokenCache;
use alien_bindings::traits::{CommandOutput, CreateSandboxRequest, RunCommandRequest};
use alien_bindings::{BindingsProvider, BindingsProviderApi};
use alien_core::{azure_disk_image_label, AzureClientConfig, AzureCredentials, ClientConfig};
use futures::StreamExt;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

#[tokio::test]
#[ignore = "needs a live Azure sandbox group, tokens and an agent image"]
async fn live_interactive_command_round_trips_stdin() {
    let region = std::env::var("AZURE_SANDBOX_REGION").unwrap_or_else(|_| "eastus".to_string());
    let image = env("AZURE_SANDBOX_IMAGE");
    let group = env("AZURE_SANDBOX_GROUP");
    let resource_group = env("AZURE_RESOURCE_GROUP");
    let azure = AzureClientConfig {
        subscription_id: env("AZURE_TARGET_SUBSCRIPTION_ID"),
        tenant_id: env("AZURE_TARGET_TENANT_ID"),
        region: Some(region.clone()),
        credentials: AzureCredentials::ScopedAccessTokens {
            tokens: HashMap::from([
                (
                    "https://management.azure.com/.default".to_string(),
                    env("AZURE_ARM_TOKEN"),
                ),
                (
                    "https://dynamicsessions.io/.default".to_string(),
                    env("AZURE_DATAPLANE_TOKEN"),
                ),
            ]),
        },
        service_overrides: None,
    };
    let plane = AzureSandboxDataPlaneClient::new(
        reqwest::Client::new(),
        &region,
        &resource_group,
        AzureTokenCache::new(azure.clone()),
    );

    // The controller's build step: a disk image from the registry image, found later by label.
    let label = azure_disk_image_label(&image);
    let built = plane
        .create_disk_image(
            &group,
            CreateDiskImage {
                base: image.clone(),
                labels: [("alienImage".to_string(), label)].into(),
                registry_credentials: Some((
                    "00000000-0000-0000-0000-000000000000".to_string(),
                    env("AZURE_SANDBOX_REGISTRY_TOKEN"),
                )),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("image build did not start: {error}"));
    eprintln!("disk image {} building", built.id);
    let mut ready = false;
    for _ in 0..120 {
        let state = plane.get_disk_image(&group, &built.id).await.expect("get");
        match state.state() {
            Some("Ready") => {
                ready = true;
                break;
            }
            Some("Failed") => panic!("image build failed: {state:?}"),
            _ => tokio::time::sleep(std::time::Duration::from_secs(5)).await,
        }
    }
    assert!(ready, "image never became Ready");

    let binding = serde_json::json!({
        "service": "sandbox-azure",
        "agent": true,
        "sandboxGroup": group,
        "dataPlaneEndpoint": format!("https://management.{region}.azuredevcompute.io"),
        "region": region.clone(),
        "resourceGroup": resource_group,
        "diskImage": image,
        "egress": { "mode": "allow" },
    });
    let provider = BindingsProvider::new(
        ClientConfig::Azure(Box::new(azure)),
        HashMap::from([("box".to_string(), binding)]),
    )
    .unwrap();
    let sandbox = provider.load_sandbox("box").await.expect("loads");
    assert!(sandbox.capabilities().interactive_commands);

    let instance = sandbox
        .create(CreateSandboxRequest::default())
        .await
        .unwrap_or_else(|error| panic!("create failed: {error}"));
    eprintln!("created {}", instance.sandbox_id);

    let result = async {
        let command = sandbox
            .run_interactive(
                &instance.sandbox_id,
                RunCommandRequest {
                    command: "sh".to_string(),
                    args: vec![
                        "-c".to_string(),
                        "printf 'name? '; read n; echo \"hello $n\"; exit 7".to_string(),
                    ],
                    cwd: None,
                    env: BTreeMap::new(),
                    timeout: std::time::Duration::from_secs(60),
                },
            )
            .await?;
        let mut output = command.output;
        let mut stdout = Vec::new();
        let mut exit = None;
        let mut wrote = false;
        while let Some(frame) = output.next().await {
            match frame? {
                CommandOutput::Stdout { data, .. } => {
                    stdout.extend(data);
                    if !wrote && String::from_utf8_lossy(&stdout).contains("name? ") {
                        wrote = true;
                        command.input.write(b"alien\n").await?;
                    }
                }
                CommandOutput::Exit { code, .. } => exit = Some(code),
                other => eprintln!("frame: {other:?}"),
            }
        }
        Ok::<_, alien_error::AlienError<alien_bindings::ErrorData>>((stdout, exit))
    }
    .await;

    sandbox
        .terminate(&instance.sandbox_id)
        .await
        .expect("terminate");
    let (stdout, exit) = result.unwrap_or_else(|error| panic!("interactive failed: {error}"));
    let text = String::from_utf8_lossy(&stdout).to_string();
    eprintln!("interactive stdout: {text:?} exit: {exit:?}");
    assert!(text.contains("name? "), "{text}");
    assert!(text.contains("hello alien"), "{text}");
    assert_eq!(exit, Some(7));

    // The disk image this test built is left for the supervisor-recorded cleanup of the group.
    eprintln!("built disk image id: {}", built.id);
}
