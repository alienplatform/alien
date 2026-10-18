//! Expiry owned by an external reconciler, with no requests from the sandbox's caller.
#![cfg(feature = "azure")]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use alien_azure_clients::azure::sandbox_data_plane::{
    AzureSandboxDataPlaneClient, CreateSandbox, SandboxDataPlaneApi,
};
use alien_azure_clients::AzureTokenCache;
use alien_core::{AzureClientConfig, AzureCredentials, SANDBOX_EXPIRY_LABEL};

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

#[tokio::test]
#[ignore = "requires an exclusively owned live test sandbox group; the reaper owns all its sessions"]
async fn external_reaper_deletes_omitted_widened_and_shortened_lifetimes() {
    let _ = dotenvy::from_path(workspace_root::get_workspace_root().join(".env.test"));
    let region = env("AZURE_SANDBOX_REGION");
    let group = env("AZURE_SANDBOX_GROUP");
    let client = Arc::new(AzureSandboxDataPlaneClient::new(
        reqwest::Client::new(),
        &region,
        &env("AZURE_RESOURCE_GROUP"),
        AzureTokenCache::new(AzureClientConfig {
            subscription_id: env("AZURE_TARGET_SUBSCRIPTION_ID"),
            tenant_id: env("AZURE_TARGET_TENANT_ID"),
            region: Some(region.clone()),
            credentials: AzureCredentials::ServicePrincipal {
                client_id: env("AZURE_TARGET_CLIENT_ID"),
                client_secret: env("AZURE_TARGET_CLIENT_SECRET"),
            },
            service_overrides: None,
        }),
    ));
    let mut ids = vec![];
    for lifetime in [None, Some(60000), Some(3000)] {
        let labels = lifetime
            .map(|ms| {
                BTreeMap::from([(
                    SANDBOX_EXPIRY_LABEL.to_string(),
                    (chrono::Utc::now().timestamp_millis() + ms).to_string(),
                )])
            })
            .unwrap_or_default();
        let session = client
            .create_sandbox(
                &group,
                CreateSandbox {
                    disk_image: "ubuntu".to_string(),
                    cpu: "1000m".to_string(),
                    memory: "2048Mi".to_string(),
                    labels,
                    ..Default::default()
                },
            )
            .await
            .expect("create");
        eprintln!("created {} requested lifetime {lifetime:?}", session.id);
        ids.push(session.id);
    }
    let reaper_client = client.clone();
    let reaper_group = group.clone();
    // Same function and interval as the controller. No binding or guest process owns this task.
    let reaper = tokio::spawn(async move {
        let mut removed = vec![];
        for _ in 0..6 {
            tokio::time::sleep(Duration::from_secs(5)).await;
            removed.extend(
                alien_infra::reap_expired_azure_sandboxes(
                    reaper_client.as_ref(),
                    &reaper_group,
                    Some(10),
                )
                .await
                .expect("reconciliation"),
            );
        }
        removed
    });
    // The client sends no traffic until the independent owner has finished six heartbeats.
    let result = reaper.await;
    let remaining = client
        .list_sandboxes(&group)
        .await
        .expect("cleanup inventory");
    for id in &ids {
        if remaining.iter().any(|s| &s.id == id) {
            client
                .delete_sandbox(&group, id)
                .await
                .expect("failure cleanup");
        }
    }
    let removed = result.expect("reaper task");
    for id in &ids {
        assert!(
            removed.contains(id),
            "reaper never accepted deletion of {id}"
        );
        assert!(
            !remaining.iter().any(|s| &s.id == id),
            "expired sandbox still exists: {id}"
        );
    }
    eprintln!("all expired sessions absent from platform list: {ids:?}");
}
