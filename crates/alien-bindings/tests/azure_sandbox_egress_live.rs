//! Released native allow lifecycle proof against a dedicated Azure sandbox group.
//! Set AZURE_TARGET_* credentials, AZURE_RESOURCE_GROUP, AZURE_SANDBOX_GROUP,
//! AZURE_SANDBOX_REGION, then run this ignored test explicitly.
#![cfg(feature = "azure")]

use alien_azure_clients::azure::{
    sandbox_data_plane::{AzureSandboxDataPlaneClient, SandboxDataPlaneApi},
    token_cache::AzureTokenCache,
};
use alien_bindings::traits::{CommandOutput, CreateSandboxRequest, RunCommandRequest, Sandbox};
use alien_bindings::{BindingsProvider, BindingsProviderApi};
use alien_core::{
    bindings::VaultBinding, AzureClientConfig, AzureCredentials, ClientConfig, EgressPolicy,
    SandboxEgress,
};
use futures::StreamExt;
use std::{
    collections::{BTreeMap, HashMap},
    time::Duration,
};

type ProbeResult<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
macro_rules! ensure {
    ($condition:expr, $($message:tt)*) => {
        if !$condition { return Err(std::io::Error::other(format!($($message)*)).into()); }
    };
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}
async fn shell(boxes: &dyn Sandbox, id: &str, script: &str) -> ProbeResult<String> {
    let mut output = Vec::new();
    let mut stream = boxes
        .run_command(
            id,
            RunCommandRequest {
                command: "sh".to_string(),
                args: vec!["-c".to_string(), script.to_string()],
                cwd: None,
                env: BTreeMap::new(),
                timeout: Duration::from_secs(60),
            },
        )
        .await?;
    let mut exit = None;
    while let Some(frame) = stream.next().await {
        match frame? {
            CommandOutput::Stdout { data, .. } => output.extend(data),
            CommandOutput::Stderr { data, .. } => eprint!("{}", String::from_utf8_lossy(&data)),
            CommandOutput::Exit { code, .. } => exit = Some(code),
        }
    }
    ensure!(
        exit == Some(0),
        "probe did not finish successfully: {exit:?}"
    );
    Ok(String::from_utf8(output)?)
}

#[tokio::test]
#[ignore = "requires a dedicated Azure sandbox group and target credentials"]
async fn legacy_allow_keeps_native_networking_and_refuses_runtime_policies() {
    let _ = dotenvy::from_path(workspace_root::get_workspace_root().join(".env.test"));
    let region = env("AZURE_SANDBOX_REGION");
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

    let binding = |egress| {
        serde_json::json!({
            "service":"sandbox-azure", "sandboxGroup":env("AZURE_SANDBOX_GROUP"), "region":region,
            "resourceGroup":env("AZURE_RESOURCE_GROUP"), "dataPlaneEndpoint":format!("https://management.{region}.azuredevcompute.io"),
            "diskImage":"ubuntu", "egress":egress,
        })
    };
    // A reconnect must reject the missing attachment before reading a signing key
    // or contacting the gateway. These endpoints deliberately have no service.
    let mut attached_binding = binding(SandboxEgress::Internet);
    attached_binding["egress"] = serde_json::json!({
        "container": "gateway", "audience": "live-test/sandbox", "kid": "unused",
        "signingKey": {"vault": "unused-vault", "key": "unused"},
    });
    let provider = BindingsProvider::new(
        ClientConfig::Azure(Box::new(azure.clone())),
        HashMap::from([
            ("legacy".to_string(), binding(SandboxEgress::Allow)),
            ("new-internet".to_string(), binding(SandboxEgress::Internet)),
            ("attached".to_string(), attached_binding),
            (
                "gateway".to_string(),
                serde_json::json!({
                    "service": "horizon", "containerName": "unused",
                    "internalUrl": "http://127.0.0.1:9",
                    "internalUrls": {"3128": "http://127.0.0.1:9", "3129": "http://127.0.0.1:9"},
                }),
            ),
            (
                "unused-vault".to_string(),
                serde_json::to_value(VaultBinding::key_vault("unused-live-test-vault"))
                    .expect("vault binding"),
            ),
        ]),
    )
    .expect("binding config");
    let error = provider
        .load_sandbox("new-internet")
        .await
        .err()
        .expect("new internet requires a gateway");
    assert_eq!(error.code, "SANDBOX_EGRESS_GATEWAY_UNAVAILABLE");
    let client = AzureSandboxDataPlaneClient::new(
        reqwest::Client::new(),
        &region,
        &env("AZURE_RESOURCE_GROUP"),
        AzureTokenCache::new(azure),
    );
    let boxes = provider
        .load_sandbox("legacy")
        .await
        .expect("legacy binding loads");
    let created = boxes
        .create(CreateSandboxRequest {
            ..Default::default()
        })
        .await
        .expect("native create");
    let id = &created.sandbox_id;
    eprintln!("live egress sandbox {id}");
    let proof: ProbeResult<()> = async {
        let observed = client.get_sandbox(&env("AZURE_SANDBOX_GROUP"), id).await?;
        ensure!(
            observed.vnet_connection_name.is_none(),
            "legacy allow acquired a new VNet attachment"
        );
        let ip = shell(
            boxes.as_ref(),
            id,
            "curl -fsS --connect-timeout 30 --max-time 45 https://api.ipify.org",
        )
        .await?;
        ensure!(
            ip.trim().parse::<std::net::Ipv4Addr>().is_ok(),
            "native internet did not return an IPv4 address: {ip}"
        );
        let error = boxes
            .update_egress(id, EgressPolicy::Allow { allow: vec![] })
            .await
            .expect_err("runtime policy requires a gateway");
        ensure!(
            error.code == "SANDBOX_EGRESS_REQUIRES_INTERNET",
            "wrong policy error"
        );
        let reopened = boxes
            .get_or_create(CreateSandboxRequest {
                sandbox_id: Some(id.clone()),
                ..Default::default()
            })
            .await?;
        ensure!(
            !reopened.created && reopened.sandbox.sandbox_id == *id,
            "reconnect replaced sandbox"
        );
        let attached = provider.load_sandbox("attached").await?;
        let error = attached.get(id).await.err().ok_or_else(|| {
            std::io::Error::other("new Internet accepted a sandbox without a VNet attachment")
        })?;
        ensure!(
            error.code == "SANDBOX_NOT_AS_DECLARED"
                && matches!(
                    error.error.as_ref(),
                    Some(alien_bindings::ErrorData::SandboxNotAsDeclared {
                        sandbox_id, restriction, ..
                    }) if sandbox_id == id && restriction == "deployment network"
                ),
            "missing VNet produced the wrong refusal: {}",
            error.code
        );
        ensure!(
            boxes.get(id).await?.is_some(),
            "refused reconnect changed the legacy sandbox"
        );
        eprintln!("legacy native lifecycle and new Internet missing-VNet refusal PASS");
        Ok(())
    }
    .await;
    boxes.terminate(id).await.expect("owned sandbox cleanup");
    assert!(boxes.get(id).await.expect("cleanup read").is_none());
    proof.expect("native network proof");
}
