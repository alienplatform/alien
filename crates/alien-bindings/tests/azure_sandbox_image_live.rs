//! Sessions started from the binding an Azure sandbox controller published, on a real group.
//!
//! `#[ignore]`: needs the binding JSON the controller live test wrote (`V43_BINDING`) and an
//! identity holding the data-plane role the provider runs under.
//!
//! ```text
//! V43_BINDING=/path/binding.json cargo test -p alien-bindings --test azure_sandbox_image_live -- --ignored --nocapture
//! ```

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
    let binding: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(env("V43_BINDING")).unwrap()).unwrap();
    eprintln!("binding: {binding}");
    let config = ClientConfig::Azure(Box::new(AzureClientConfig {
        subscription_id: env("AZURE_TARGET_SUBSCRIPTION_ID"),
        tenant_id: env("AZURE_TARGET_TENANT_ID"),
        region: Some("westus2".to_string()),
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
    eprintln!(
        "created {} in {:?}",
        instance.sandbox_id,
        started.elapsed()
    );

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
            eprintln!("frame: {:?}", frame.as_ref().map(|f| format!("{f:?}").chars().take(200).collect::<String>()));
            if let alien_bindings::traits::CommandOutput::Stdout { data, .. } = frame? {
                stdout.extend(data);
            }
        }
        let os = String::from_utf8_lossy(&stdout).to_string();
        eprintln!("os-release:\n{os}");

        sandbox
            .write_files(
                &instance.sandbox_id,
                BTreeMap::from([("/tmp/v43/round.txt".to_string(), b"round-trip-v43".to_vec())]),
            )
            .await?;
        let back = sandbox
            .read_file(&instance.sandbox_id, "/tmp/v43/round.txt")
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
    if let Ok(expect) = std::env::var("V43_EXPECT_OS") {
        assert!(os.contains(&expect), "expected {expect} in {os}");
    }
    assert_eq!(back, b"round-trip-v43");
}
