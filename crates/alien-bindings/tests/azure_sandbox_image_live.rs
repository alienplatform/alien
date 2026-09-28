//! A session started from an Azure sandbox binding on a real group. Ignored: it needs the
//! `AZURE_TARGET_*` identity holding the data-plane role on the group `AZURE_RESOURCE_GROUP` and
//! `AZURE_SANDBOX_GROUP` name; `AZURE_SANDBOX_IMAGE` must already be built there if a reference.
//!
//! ```text
//! cargo test -p alien-bindings --features azure --test azure_sandbox_image_live -- --ignored --nocapture
//! ```

#![cfg(feature = "azure")]

use std::collections::{BTreeMap, HashMap};

use alien_bindings::traits::{CreateSandboxRequest, RunCommandRequest};
use alien_bindings::{BindingsProvider, BindingsProviderApi};
use alien_core::{AzureClientConfig, AzureCredentials, ClientConfig};
use futures::StreamExt;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

#[tokio::test]
#[ignore = "needs a live Azure sandbox group and a published binding"]
async fn live_session_from_published_binding() {
    let region = std::env::var("AZURE_SANDBOX_REGION").unwrap_or_else(|_| "westus2".to_string());
    let image = std::env::var("AZURE_SANDBOX_IMAGE")
        .unwrap_or_else(|_| "docker.io/library/python:3.14-slim".to_string());
    let binding = serde_json::json!({
        "service": "sandbox-azure",
        "sandboxGroup": env("AZURE_SANDBOX_GROUP"),
        "dataPlaneEndpoint": format!("https://management.{region}.azuredevcompute.io"),
        "region": region.clone(),
        "resourceGroup": env("AZURE_RESOURCE_GROUP"),
        "diskImage": image,
        "egress": { "mode": "allow" },
    });
    let config = ClientConfig::Azure(Box::new(AzureClientConfig {
        subscription_id: env("AZURE_TARGET_SUBSCRIPTION_ID"),
        tenant_id: env("AZURE_TARGET_TENANT_ID"),
        region: Some(region.clone()),
        credentials: AzureCredentials::ServicePrincipal {
            client_id: env("AZURE_TARGET_CLIENT_ID"),
            client_secret: env("AZURE_TARGET_CLIENT_SECRET"),
        },
        service_overrides: None,
    }));
    let provider =
        BindingsProvider::new(config, HashMap::from([("box".to_string(), binding)])).unwrap();
    let sandbox = provider.load_sandbox("box").await.expect("loads");

    let started = std::time::Instant::now();
    let instance = sandbox
        .create(CreateSandboxRequest::default())
        .await
        .unwrap_or_else(|error| panic!("create failed: {error}"));
    eprintln!("created {} in {:?}", instance.sandbox_id, started.elapsed());

    let result = async {
        let mut stream = sandbox
            .run_command(
                &instance.sandbox_id,
                RunCommandRequest {
                    command: "cat".to_string(),
                    args: vec!["/etc/os-release".to_string()],
                    cwd: None,
                    env: BTreeMap::new(),
                    timeout: std::time::Duration::from_secs(30),
                },
            )
            .await?;
        let mut stdout = Vec::new();
        while let Some(frame) = stream.next().await {
            eprintln!(
                "frame: {:?}",
                frame
                    .as_ref()
                    .map(|f| format!("{f:?}").chars().take(200).collect::<String>())
            );
            if let alien_bindings::traits::CommandOutput::Stdout { data, .. } = frame? {
                stdout.extend(data);
            }
        }
        let os = String::from_utf8_lossy(&stdout).to_string();
        eprintln!("os-release:\n{os}");

        sandbox
            .write_files(
                &instance.sandbox_id,
                BTreeMap::from([("/tmp/alien/round.txt".to_string(), b"round-trip".to_vec())]),
            )
            .await?;
        let back = sandbox
            .read_file(&instance.sandbox_id, "/tmp/alien/round.txt")
            .await?;
        eprintln!("file back: {:?}", String::from_utf8_lossy(&back));
        Ok::<_, alien_error::AlienError<alien_bindings::ErrorData>>((os, back))
    }
    .await;

    sandbox
        .terminate(&instance.sandbox_id)
        .await
        .expect("terminate");
    let (os, back) = result.unwrap_or_else(|error| panic!("session failed: {error}"));
    assert!(os.contains("ID="), "{os}");
    assert_eq!(back, b"round-trip");
}
