#![cfg(all(feature = "platform", target_os = "linux"))]

use httpmock::{
    Method::{DELETE, GET, POST},
    MockServer,
};
use serde_json::{json, Value};
use std::{
    fs,
    process::{Command, Output},
};
use tempfile::TempDir;

const PROJECT: &str = "prj_0000000000000000000000000000";
const KEY: &str = "apikey_0000000000000000000000000000";

fn run_cli(server: &MockServer, args: &[&str], explicit_workspace: bool) -> Output {
    let config = TempDir::new().unwrap();
    let checkout = TempDir::new().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_alien"));
    command
        .current_dir(checkout.path())
        .args(["--base-url", &server.base_url()])
        .env("XDG_CONFIG_HOME", config.path())
        .env("ALIEN_API_KEY", "test-key");
    for name in [
        "ALIEN_MANAGER_URL",
        "ALIEN_PROJECT",
        "ALIEN_BASE_URL",
        "ALIEN_WORKSPACE",
        "ALIEN_LOG",
        "RUST_LOG",
    ] {
        command.env_remove(name);
    }
    if explicit_workspace {
        command.args(["--workspace", "demo"]);
    }
    let output = command.args(args).output().unwrap();
    assert!(!config.path().join("alien/profile.json").exists());
    assert!(!checkout.path().join(".alien/project.json").exists());
    // Log files are allowed; no authentication or workspace profile is saved.
    if config.path().join("alien").exists() {
        for entry in fs::read_dir(config.path().join("alien")).unwrap() {
            let entry = entry.unwrap();
            assert!(
                entry.file_type().unwrap().is_dir(),
                "unexpected profile file: {:?}",
                entry.path()
            );
        }
    }
    output
}

fn subject() -> Value {
    json!({"kind":"serviceAccount", "id":KEY, "workspaceId":"ws_000000000000000000000000",
        "workspaceName":"demo", "scope":{"type":"project", "projectId":PROJECT}, "role":"project.admin"})
}

#[test]
fn api_key_commands_discover_workspace_and_explicit_workspace_still_works() {
    for explicit in [false, true] {
        for operation in ["list", "revoke", "logs"] {
            let api = MockServer::start();
            let whoami = api.mock(|when, then| {
                when.method(GET)
                    .path("/v1/whoami")
                    .header("authorization", "Bearer test-key");
                then.status(200).json_body(subject());
            });
            let project = api.mock(|when, then| {
                when.method(GET).path("/v1/projects/demo-project").header("authorization", "Bearer test-key");
                then.status(200).json_body(json!({"id":PROJECT,"name":"demo-project",
                    "workspaceId":"ws_000000000000000000000000", "createdAt":"2026-01-01T00:00:00Z"}));
            });
            let list = api.mock(|when, then| {
                when.method(GET)
                    .path("/v1/api-keys")
                    .query_param("workspace", "demo")
                    .query_param("project", PROJECT)
                    .header("authorization", "Bearer test-key");
                then.status(200)
                    .json_body(json!({"items":[], "nextCursor":null}));
            });
            let revoke = api.mock(|when, then| {
                when.method(DELETE)
                    .path(format!("/v1/api-keys/{KEY}"))
                    .query_param("workspace", "demo")
                    .header("authorization", "Bearer test-key");
                then.status(204);
            });
            let token = api.mock(|when, then| {
                when.method(POST)
                    .path("/v1/managers/mgr_0000000000000000000000000000/token")
                    .query_param("workspace", "demo")
                    .header("authorization", "Bearer test-key")
                    .json_body(json!({"project":PROJECT}));
                then.status(200).json_body(
                    json!({"accessToken":"test-query-token", "expiresIn":3600,
                    "tokenType":"Bearer", "managerUrl":api.base_url(), "databaseId":"db_test",
                    "controlPlaneUrl":api.url("/v1")}),
                );
            });
            let splits = api.mock(|when, then| {
                when.method(GET)
                    .path("/v1/databases/db_test/splits")
                    .header("authorization", "Bearer test-query-token");
                then.status(200).json_body(json!({"splits":[]}));
            });
            let search = api.mock(|when, then| {
                when.method(POST)
                    .path("/v1/logs/search")
                    .header("authorization", "Bearer test-query-token");
                then.status(200).json_body(
                    json!({"hits":[], "num_hits":0, "elapsed_time_micros":1, "errors":[]}),
                );
            });
            let args = match operation {
                "list" => vec!["api-keys", "--project", "demo-project", "--json", "list"],
                "revoke" => vec![
                    "api-keys",
                    "--project",
                    "demo-project",
                    "--json",
                    "revoke",
                    KEY,
                    "--yes",
                ],
                _ => vec![
                    "logs",
                    "--project",
                    "demo-project",
                    "--manager",
                    "mgr_0000000000000000000000000000",
                    "--json",
                ],
            };
            let output = run_cli(&api, &args, explicit);
            assert!(
                output.status.success(),
                "{operation}, explicit={explicit}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let result: Value = serde_json::from_slice(&output.stdout).unwrap();
            match operation {
                "list" => assert_eq!(result, json!([])),
                "revoke" => assert_eq!(result, Value::Null),
                _ => {
                    assert_eq!(result["numHits"], 0);
                    assert_eq!(result["logs"], json!([]));
                }
            }
            whoami.assert_hits(if explicit { 0 } else { 2 });
            project.assert_hits(1);
            list.assert_hits(usize::from(operation == "list"));
            revoke.assert_hits(usize::from(operation == "revoke"));
            token.assert_hits(if operation == "logs" { 2 } else { 0 });
            splits.assert_hits(usize::from(operation == "logs"));
            search.assert_hits(usize::from(operation == "logs"));
        }
    }
}

#[test]
fn unavailable_identity_or_missing_workspace_stops_before_key_requests() {
    for status in [200, 401, 503] {
        let api = MockServer::start();
        let identity = api.mock(|when, then| {
            when.method(GET)
                .path("/v1/whoami")
                .header("authorization", "Bearer test-key");
            if status == 200 {
                let mut body = subject();
                body.as_object_mut().unwrap().remove("workspaceName");
                then.status(status).json_body(body);
            } else {
                then.status(status).json_body(
                    json!({"code":"IDENTITY_UNAVAILABLE", "message":"Identity request rejected",
                    "retryable":status==503, "internal":false,"httpStatusCode":status}),
                );
            }
        });
        let downstream = api.mock(|when, then| {
            when.path("/v1/api-keys");
            then.status(200)
                .json_body(json!({"items":[],"nextCursor":null}));
        });
        let output = run_cli(
            &api,
            &["api-keys", "--project", "demo-project", "--json", "list"],
            false,
        );
        assert!(!output.status.success());
        identity.assert_hits(1);
        downstream.assert_hits(0);
    }
}
