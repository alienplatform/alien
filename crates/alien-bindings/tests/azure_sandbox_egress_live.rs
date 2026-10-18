//! Native egress lifecycle proof against a setup-owned Azure sandbox group with a VNet connection.
//! Set AZURE_TARGET_* credentials, AZURE_RESOURCE_GROUP, AZURE_SANDBOX_GROUP,
//! AZURE_SANDBOX_REGION and AZURE_SANDBOX_NAT_IP, then run this ignored test explicitly.
#![cfg(feature = "azure")]

use alien_bindings::traits::{
    CommandOutput, CreateSandboxRequest, RunCommandRequest, Sandbox, SandboxState,
};
use alien_bindings::{BindingsProvider, BindingsProviderApi};
use alien_core::{AzureClientConfig, AzureCredentials, ClientConfig, SandboxEgress};
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
fn allow(targets: &[&str]) -> SandboxEgress {
    SandboxEgress::Allow {
        allow: targets.iter().map(|v| (*v).to_string()).collect(),
    }
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
                timeout: Duration::from_secs(30),
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

async fn status(boxes: &dyn Sandbox, id: &str, url: &str) -> ProbeResult<String> {
    shell(
        boxes,
        id,
        &format!("curl -sS --max-time 8 -o /dev/null -w '%{{http_code}}' '{url}'"),
    )
    .await
}

#[tokio::test]
#[ignore = "requires a dedicated Azure sandbox group, VNet connection, NSG and NAT gateway"]
async fn native_narrowing_update_reconnect_and_pause_use_the_deployment_network() {
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
    let bound = allow(&["*.ipify.org", "example.com:80"]);
    let provider = BindingsProvider::new(ClientConfig::Azure(Box::new(azure)), HashMap::from([("box".to_string(), serde_json::json!({
        "service":"sandbox-azure", "sandboxGroup":env("AZURE_SANDBOX_GROUP"), "region":region,
        "resourceGroup":env("AZURE_RESOURCE_GROUP"), "dataPlaneEndpoint":format!("https://management.{region}.azuredevcompute.io"),
        "diskImage":"ubuntu", "egress":bound,
    }))])).expect("binding config");
    let boxes = provider.load_sandbox("box").await.expect("binding loads");
    let created = boxes
        .create(CreateSandboxRequest {
            egress: Some(allow(&["api.ipify.org"])),
            ..Default::default()
        })
        .await
        .expect("narrowed create");
    let id = &created.sandbox_id;
    eprintln!("live egress sandbox {id}");
    let proof: ProbeResult<()> = async {
        let ip = shell(
            boxes.as_ref(),
            id,
            "curl -fsS --max-time 8 https://api.ipify.org",
        )
        .await?;
        ensure!(
            ip.trim() == env("AZURE_SANDBOX_NAT_IP"),
            "source differs from NAT: {ip}"
        );
        ensure!(
            status(boxes.as_ref(), id, "https://example.com").await? == "403",
            "unlisted HTTPS allowed"
        );
        boxes.update_egress(id, bound.clone()).await?;
        ensure!(
            status(boxes.as_ref(), id, "https://api.ipify.org").await? == "200",
            "wildcard child blocked"
        );
        ensure!(
            status(boxes.as_ref(), id, "https://ipify.org").await? == "403",
            "wildcard apex allowed"
        );
        ensure!(
            status(boxes.as_ref(), id, "http://example.com").await? == "200",
            "explicit HTTP blocked"
        );
        ensure!(
            status(boxes.as_ref(), id, "https://example.com").await? == "403",
            "HTTP target permits HTTPS"
        );
        let widening = boxes
            .update_egress(id, SandboxEgress::Internet)
            .await
            .expect_err("widening is refused");
        ensure!(
            widening.code == "SANDBOX_EGRESS_WIDENS",
            "wrong widening error"
        );
        boxes.pause(id).await?;
        for _ in 0..30 {
            if boxes
                .get(id)
                .await?
                .is_some_and(|s| s.state == SandboxState::Paused)
            {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        boxes.update_egress(id, SandboxEgress::None).await?;
        ensure!(
            boxes
                .get(id)
                .await?
                .is_some_and(|s| s.state == SandboxState::Paused),
            "update did not restore pause"
        );
        boxes.resume(id).await?;
        ensure!(
            status(boxes.as_ref(), id, "https://api.ipify.org").await? == "403",
            "none update permits former target"
        );
        let reopened = boxes
            .get_or_create(CreateSandboxRequest {
                sandbox_id: Some(id.clone()),
                egress: Some(allow(&["api.ipify.org"])),
                ..Default::default()
            })
            .await?;
        ensure!(
            !reopened.created && reopened.sandbox.sandbox_id == *id,
            "reconnect replaced a narrowed sandbox"
        );
        ensure!(
            status(boxes.as_ref(), id, "https://api.ipify.org").await? == "200",
            "reconnect narrowing did not apply"
        );
        eprintln!("native narrowing/update/wildcard/protocol/NAT/pause/reconnect PASS");
        Ok(())
    }
    .await;
    boxes.terminate(id).await.expect("owned sandbox cleanup");
    assert!(boxes.get(id).await.expect("cleanup read").is_none());
    proof.expect("native egress proof");
}
