#![cfg(target_os = "linux")]

use httpmock::{Method::GET, MockServer};
use serde_json::json;
use std::process::Command;
use tempfile::TempDir;

const DEPLOYMENT: &str = "dep_0000000000000000000000000000";

#[test]
fn standalone_vault_accepts_deployment_ids_and_names() {
    for selector in ["demo", DEPLOYMENT] {
        let manager = MockServer::start();
        let list = manager.mock(|when, then| {
            when.method(GET).path("/v1/deployments");
            then.status(200)
                .json_body(json!({"items":[{"id":DEPLOYMENT,"name":"demo"}],"nextCursor":null}));
        });
        let secret = manager.mock(|when, then| {
            when.method(GET)
                .path(format!(
                    "/v1/deployments/{DEPLOYMENT}/vault/secrets/secrets/TEST_KEY"
                ))
                .header("authorization", "Bearer test-key");
            then.status(200)
                .json_body(json!({"value":"synthetic-value"}));
        });
        let config = TempDir::new().unwrap();
        let checkout = TempDir::new().unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_alien"));
        command
            .current_dir(checkout.path())
            .env("XDG_CONFIG_HOME", config.path())
            .env("ALIEN_MANAGER_URL", manager.base_url())
            .env("ALIEN_API_KEY", "test-key")
            .args([
                "vault",
                "get",
                "--deployment",
                selector,
                "secrets",
                "TEST_KEY",
            ]);
        for name in [
            "ALIEN_PROJECT",
            "ALIEN_BASE_URL",
            "ALIEN_WORKSPACE",
            "ALIEN_LOG",
            "ALIEN_LOG_FILE",
            "RUST_LOG",
        ] {
            command.env_remove(name);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{selector}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            "synthetic-value"
        );
        secret.assert_hits(1);
        list.assert_hits(usize::from(selector == "demo"));
    }
}

#[tokio::test]
async fn local_vault_accepts_deployment_ids_and_names() {
    for selector in ["demo", DEPLOYMENT] {
        let manager = MockServer::start_async().await;
        manager
            .mock_async(|when, then| {
                when.method(GET).path("/health");
                then.status(200).json_body(json!({"status":"ok"}));
            })
            .await;
        manager
            .mock_async(|when, then| {
                when.method(GET).path("/v1/deployment-groups");
                then.status(200)
                    .json_body(json!({"items":[{"id":"dg_local", "name":"local-dev",
                "maxDeployments":100,"deploymentCount":1,"createdAt":"2026-01-01T00:00:00Z",
                "projectId":"default","workspaceId":"default"}]}));
            })
            .await;
        let list = manager
            .mock_async(|when, then| {
                when.method(GET).path("/v1/deployments");
                then.status(200)
                    .json_body(json!({"items":[{"id":DEPLOYMENT,"name":"demo",
                "platform":"local","status":"running","deploymentGroupId":"dg_local",
                "deploymentProtocolVersion":1,"projectId":"default","workspaceId":"default",
                "retryRequested":false,"createdAt":"2026-01-01T00:00:00Z"}]}));
            })
            .await;
        let directory = TempDir::new().unwrap();
        alien_cli::commands::vault_task(
            alien_cli::commands::VaultArgs {
                deployment: selector.to_string(),
                state_dir: directory.path().to_string_lossy().to_string(),
                action: alien_cli::commands::vault::VaultAction::Set {
                    vault_name: "secrets".to_string(),
                    secret_name: "TEST_KEY".to_string(),
                    value: Some("synthetic-value".to_string()),
                    stdin: false,
                    file: None,
                },
            },
            manager.port(),
        )
        .await
        .expect("the selector must resolve to the same local deployment");
        let stored: serde_json::Value = serde_json::from_slice(
            &std::fs::read(
                directory
                    .path()
                    .join(DEPLOYMENT)
                    .join("vault/secrets/secrets.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(stored["TEST_KEY"], "synthetic-value");
        list.assert_async().await;
    }
}
