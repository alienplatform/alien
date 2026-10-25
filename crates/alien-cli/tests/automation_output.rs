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
    let events: Vec<_> = (0..100)
        .map(|i| {
            json!({
                "id":format!("event_{i:028}"), "deploymentId":ID,
                "data":{"type":"DeploymentCheckRequested","deploymentId":ID},
                "state":"success", "projectId":"prj_0000000000000000000000000000",
                "workspaceId":"ws_000000000000000000000000", "createdAt":"2026-01-01T00:00:00Z"
            })
        })
        .collect();
    let event_response = json!({"items":events,"nextCursor":"next-page","releases":[{
        "id":"rel_0000000000000000000000000000", "version":"1.0.0",
        "title":"First release", "createdAt":"2026-01-01T00:00:00Z"
    }]});
    let first = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/events")
            .query_param("limit", "100");
        then.json_body(event_response.clone());
    });
    for (operation, expected) in [
        ("machines", json!([])),
        ("events", event_response),
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
            command.args(["--limit", "100"]);
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
}

#[test]
fn json_logs_suppress_the_cross_workspace_notice_and_search_once() {
    let server = MockServer::start();
    const MANAGER: &str = "mgr_0000000000000000000000000000";
    server.mock(|when, then| {
        when.method(GET).path(format!("/v1/managers/{MANAGER}"));
        then.json_body(
            json!({"id":MANAGER,"name":"acme-manager","url":server.base_url(),
            "workspaceId":"ws_000000000000000000000000","createdAt":"2026-01-01T00:00:00Z",
            "defaultProjectCount":0,"managedDeploymentCount":1,"managementConfigs":{},
            "isSystem":false,"status":"healthy","targets":["machines"]}),
        );
    });
    server.mock(|when, then| {
        when.method(POST).path(format!("/v1/managers/{MANAGER}/token"));
        then.json_body(json!({"accessToken":"test-token","expiresIn":3600,"tokenType":"Bearer",
            "managerUrl":server.base_url(),"databaseId":"db_test","controlPlaneUrl":server.url("/v1")}));
    });
    server.mock(|when, then| {
        when.method(GET).path("/v1/databases/db_test/splits");
        then.json_body(json!({"splits":[]}));
    });
    let search = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/logs/search")
            .json_body_partial(json!({"max_hits":7}).to_string());
        then.json_body(json!({"num_hits":0,"hits":[],"elapsed_time_micros":1,"errors":[]}));
    });
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join(".alien")).unwrap();
    std::fs::write(
        directory.path().join(".alien/project.json"),
        json!({
            "workspace":"other-workspace", "projectId":"prj_0000000000000000000000000000",
            "projectName":"linked-app", "rootDirectory":null
        })
        .to_string(),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_alien"))
        .args([
            "--base-url",
            &server.base_url(),
            "--workspace",
            "acme",
            "logs",
            "--manager",
            MANAGER,
            "--limit",
            "7",
            "--json",
        ])
        .current_dir(directory.path())
        .env("XDG_CONFIG_HOME", directory.path())
        .env("ALIEN_API_KEY", "test-key")
        .env_remove("ALIEN_MANAGER_URL")
        .env_remove("ALIEN_PROJECT")
        .env_remove("RUST_LOG")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    assert_eq!(value["logs"], json!([]));
    search.assert_hits(1);
}
