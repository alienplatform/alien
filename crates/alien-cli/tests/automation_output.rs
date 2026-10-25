#![cfg(feature = "platform")]

use httpmock::{
    Method::{GET, POST},
    MockServer,
};
use serde_json::{json, Value};
use std::process::Command;

const ID: &str = "dep_0000000000000000000000000000";

#[test]
fn machine_inventory_and_token_are_json_without_a_linked_project() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v1/whoami");
        then.json_body(
            json!({"kind":"serviceAccount", "id":"ak_test", "workspaceId":"ws_000000000000000000000000",
            "workspaceName":"acme", "scope":{"type":"workspace"}, "role":"workspace.admin"}),
        );
    });
    server.mock(|when, then| {
        when.method(GET).path(format!("/v1/deployments/{ID}"));
        then.json_body(json!({"id":ID,"name":"docproc","platform":"machines","status":"running",
            "deploymentGroupId":"dg_0000000000000000000000000000","projectId":"prj_0000000000000000000000000000",
            "workspaceId":"ws_000000000000000000000000","managerId":"mgr_0000000000000000000000000000","purpose":"application",
            "releaseChannel":"production","stackSettings":{},"deploymentProtocolVersion":1,"retryRequested":false,"updateBlocked":false,
            "createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z"}));
    });
    let machines = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/deployments/{ID}/machines"));
        then.json_body(json!({"machines":[]}));
    });
    let token = server.mock(|when, then| {
        when.method(POST)
            .path(format!("/v1/deployments/{ID}/token"));
        then.status(201)
            .json_body(json!({"deploymentId":ID,"token":"test-deployment-token"}));
    });
    let events: Vec<_> = (0..101)
        .map(|i| {
            json!({
                "id":format!("event_{i:028}"), "deploymentId":ID,
                "data":{"type":"DeploymentCheckRequested","deploymentId":ID},
                "state":"success", "projectId":"prj_0000000000000000000000000000",
                "workspaceId":"ws_000000000000000000000000", "createdAt":"2026-01-01T00:00:00Z"
            })
        })
        .collect();
    let first = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/events")
            .query_param("limit", "100");
        then.json_body(json!({"items":&events[..100],"nextCursor":"next-page"}));
    });
    let second = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/events")
            .query_param("limit", "1")
            .query_param("cursor", "next-page");
        then.json_body(json!({"items":&events[100..],"nextCursor":null}));
    });
    for (operation, expected) in [
        ("machines", json!([])),
        ("events", json!({"items": events, "nextCursor":null})),
        (
            "token",
            json!({"deploymentId":ID,"token":"test-deployment-token"}),
        ),
    ] {
        let config = tempfile::tempdir().unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_alien"));
        command
            .args([
                "--base-url",
                &server.base_url(),
                "--workspace",
                "acme",
                "deployments",
                operation,
                ID,
                "--json",
            ])
            .current_dir(config.path())
            .env("XDG_CONFIG_HOME", config.path())
            .env("ALIEN_API_KEY", "test-key")
            .env_remove("ALIEN_PROJECT")
            .env_remove("RUST_LOG");
        if operation == "events" {
            command.args(["--limit", "101"]);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{operation}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value, expected);
    }
    machines.assert();
    token.assert();
    first.assert();
    second.assert();
}
