#![cfg(all(feature = "platform", target_os = "linux"))]

use httpmock::{Method::{GET, PUT, DELETE}, MockServer};
use serde_json::json;
use std::process::Command;
use tempfile::TempDir;

const DEPLOYMENT: &str = "dep_0000000000000000000000000000";
const MANAGER: &str = "mgr_0000000000000000000000000000";
const PROJECT: &str = "prj_0000000000000000000000000000";
const GROUP: &str = "dg_0000000000000000000000000000";

#[test]
fn platform_vault_actions_use_the_deployments_manager_and_workspace() {
    for selector in [DEPLOYMENT, "demo/app"] {
        for action in ["set", "get", "delete"] {
            let api = MockServer::start();
            let manager = MockServer::start();
            api.mock(|when, then| {
                when.method(GET).path(format!("/v1/projects/{PROJECT}"));
                then.status(200).json_body(json!({"id":PROJECT,"name":"demo-project",
                    "workspaceId":"ws_test", "createdAt":"2026-01-01T00:00:00Z"}));
            });
            api.mock(|when, then| {
                when.method(GET).path("/v1/deployment-groups").query_param("search", "demo");
                then.status(200).json_body(json!({"items":[{"id":GROUP,"name":"demo",
                    "projectId":PROJECT,"workspaceId":"ws_test", "createdAt":"2026-01-01T00:00:00Z",
                    "updatedAt":"2026-01-01T00:00:00Z"}],"nextCursor":null}));
            });
            let deployment = json!({"id":DEPLOYMENT,"name":"app","status":"running",
                "projectId":PROJECT,"platform":"aws","deploymentProtocolVersion":1,
                "deploymentGroupId":GROUP,"purpose":"application","stackSettings":{},
                "releaseChannel":"production","retryRequested":false,
                "createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z",
                "managerId":MANAGER,"workspaceId":"ws_test"});
            api.mock(|when, then| {
                when.method(GET).path("/v1/deployments");
                then.status(200).json_body(json!({"items":[deployment.clone()],"nextCursor":null}));
            });
            let detail = api.mock(|when, then| {
                when.method(GET).path(format!("/v1/deployments/{DEPLOYMENT}"))
                    .query_param("workspace", "demo").header("authorization","Bearer test-key");
                then.status(200).json_body(deployment);
            });
            let discovery = api.mock(|when, then| {
                when.method(GET).path(format!("/v1/managers/{MANAGER}"))
                    .query_param("workspace", "demo").header("authorization","Bearer test-key");
                then.status(200).json_body(json!({"id":MANAGER,"name":"deployment manager",
                    "targets":["aws"],"managementConfigs":{},"isSystem":false,"workspaceId":"ws_test",
                    "status":"running","managedDeploymentCount":1,"defaultProjectCount":0,
                    "createdAt":"2026-01-01T00:00:00Z","url":manager.base_url()}));
            });
            let path = format!("/v1/deployments/{DEPLOYMENT}/vault/secrets/secrets/TEST_KEY");
            let wrong_host = api.mock(|when, then| {
                when.path(&path);
                then.status(404);
            });
            let request = manager.mock(|when, then| {
                when.method(match action { "set" => PUT, "get" => GET, _ => DELETE })
                    .path(&path).header("authorization", "Bearer test-key")
                    .header("x-alien-workspace", "demo");
                if action == "set" { when.json_body(json!({"value":"synthetic-value"})); }
                then.status(200).json_body(if action == "get" {
                    json!({"value":"synthetic-value"})
                } else { json!({"ok":true}) });
            });
            let config = TempDir::new().unwrap();
            let checkout = TempDir::new().unwrap();
            let mut command = Command::new(env!("CARGO_BIN_EXE_alien"));
            command.current_dir(checkout.path()).env("XDG_CONFIG_HOME", config.path())
                .env("ALIEN_API_KEY", "test-key")
                .args(["--base-url", &api.base_url(), "--workspace", "demo", "--project", PROJECT,
                    "vault", "--deployment", selector, action, "secrets", "TEST_KEY"]);
            for name in ["ALIEN_MANAGER_URL","ALIEN_PROJECT","ALIEN_BASE_URL","ALIEN_WORKSPACE",
                "ALIEN_LOG","ALIEN_LOG_FILE","RUST_LOG"] { command.env_remove(name); }
            if action == "set" { command.arg("synthetic-value"); }
            let output = command.output().unwrap();
            assert!(output.status.success(), "{selector} {action}: {}", String::from_utf8_lossy(&output.stderr));
            if action == "get" { assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "synthetic-value"); }
            detail.assert_hits(1);
            discovery.assert_hits(1);
            request.assert_hits(1);
            wrong_host.assert_hits(0);
        }
    }
}
