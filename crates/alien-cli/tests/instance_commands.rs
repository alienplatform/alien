#![cfg(feature = "platform")]

use httpmock::{Method::GET, MockServer};
use serde_json::{json, Value};
use std::process::Command;

#[test]
fn instance_and_legacy_commands_share_the_api_and_json_output() {
    let server = MockServer::start();
    let id = "mgr_0000000000000000000000000000";
    let list = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/managers")
            .query_param("workspace", "sample")
            .header("authorization", "Bearer test-key");
        then.json_body(json!([{
            "id": id,
            "name": "production",
            "workspaceId": "ws_000000000000000000000000",
            "createdAt": "2026-01-01T00:00:00Z",
            "defaultProjectCount": 1,
            "managedDeploymentCount": 2,
            "managementConfigs": {},
            "isSystem": false,
            "status": "healthy",
            "targets": ["aws"],
            "cloud": "aws",
            "region": "us-east-1"
        }]));
    });
    let mut first = None;
    for name in ["instances", "managers", "instance", "manager"] {
        let directory = tempfile::tempdir().unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_alien"))
            .current_dir(directory.path())
            .args([
                "--base-url",
                &server.base_url(),
                "--workspace",
                "sample",
                name,
                "list",
                "--json",
            ])
            // macOS resolves the config directory from HOME, not XDG_CONFIG_HOME.
            .env("HOME", directory.path())
            .env("XDG_CONFIG_HOME", directory.path())
            .env("ALIEN_API_KEY", "test-key")
            .env_remove("ALIEN_MANAGER_URL")
            .env_remove("ALIEN_PROJECT")
            .env_remove("ALIEN_BASE_URL")
            .env_remove("ALIEN_WORKSPACE")
            .env_remove("RUST_LOG")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result[0]["id"], id);
        assert_eq!(result[0]["name"], "production");
        assert_eq!(result[0]["managedDeploymentCount"], 2);
        assert_eq!(result[0]["defaultProjectCount"], 1);
        if let Some(expected) = &first {
            assert_eq!(&result, expected);
        } else {
            first = Some(result);
        }
    }
    list.assert_hits(4);
}
