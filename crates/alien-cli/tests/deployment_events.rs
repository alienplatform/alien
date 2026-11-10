#![cfg(all(feature = "platform", target_os = "linux"))]

//! `alien deployments events` lists the status changes of a deployment, so a
//! deployment that waited for a secret shows when the wait began and ended.

use httpmock::{Method::GET, Mock, MockServer};
use serde_json::{json, Value};
use std::process::{Command, Output};
use tempfile::TempDir;

const PROJECT: &str = "prj_0000000000000000000000000000";
const WORKSPACE_ID: &str = "ws_000000000000000000000000";
const DEPLOYMENT_ID: &str = "dep_events0000000000000000000000";
const GROUP_ID: &str = "dg_customers0000000000000000000";

fn run_events(server: &MockServer, extra: &[&str]) -> Output {
    let home = TempDir::new().unwrap();
    let checkout = TempDir::new().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_alien"));
    command
        .current_dir(checkout.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("ALIEN_API_KEY", "test-key");
    for name in [
        "ALIEN_MANAGER_URL",
        "ALIEN_PROJECT",
        "ALIEN_BASE_URL",
        "ALIEN_WORKSPACE",
        "ALIEN_LOG",
        "ALIEN_LOG_FILE",
        "RUST_LOG",
    ] {
        command.env_remove(name);
    }
    command
        .args(["--base-url", &server.base_url()])
        .args(["--project", "demo-project"])
        .args(["deployments", "events", DEPLOYMENT_ID])
        .args(extra)
        .output()
        .unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn event(id: &str, time: &str, data: Value) -> Value {
    json!({
        "id": id,
        "createdAt": time,
        "workspaceId": WORKSPACE_ID,
        "deploymentId": DEPLOYMENT_ID,
        "state": "success",
        "data": data,
    })
}

fn status_change(id: &str, time: &str, previous: &str, status: &str) -> Value {
    event(
        id,
        time,
        json!({
            "type": "DeploymentStatusChanged",
            "deploymentId": DEPLOYMENT_ID,
            "previousStatus": previous,
            "status": status,
        }),
    )
}

/// A deployment that started an update, waited 12 minutes for a secret, and finished.
fn timeline() -> Vec<Value> {
    vec![
        status_change("evt_5", "2026-01-01T12:00:00Z", "updating", "running"),
        status_change(
            "evt_4",
            "2026-01-01T11:50:14Z",
            "waiting-for-secrets",
            "updating",
        ),
        status_change(
            "evt_3",
            "2026-01-01T11:38:13Z",
            "updating",
            "waiting-for-secrets",
        ),
        status_change("evt_2", "2026-01-01T11:37:47Z", "running", "updating"),
        event(
            "evt_1",
            "2026-01-01T09:00:00Z",
            json!({"type": "DeploymentCreated", "deploymentId": DEPLOYMENT_ID}),
        ),
    ]
}

/// Identity, project and deployment lookups. Returns nothing: the events mocks
/// are added by each test.
fn mock_platform(api: &MockServer) {
    api.mock(|when, then| {
        when.method(GET).path("/v1/whoami");
        then.status(200).json_body(json!({
            "kind": "serviceAccount", "id": "apikey_0000000000000000000000000000",
            "workspaceId": WORKSPACE_ID, "workspaceName": "demo",
            "scope": {"type": "project", "projectId": PROJECT}, "role": "project.admin"
        }));
    });
    api.mock(|when, then| {
        when.method(GET).path("/v1/projects/demo-project");
        then.status(200)
            .json_body(json!({"id": PROJECT, "name": "demo-project",
            "workspaceId": WORKSPACE_ID, "createdAt": "2026-01-01T00:00:00Z"}));
    });
    api.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/deployments/{DEPLOYMENT_ID}"));
        then.status(200).json_body(json!({
            "id": DEPLOYMENT_ID,
            "name": "acme",
            "status": "running",
            "projectId": PROJECT,
            "platform": "aws",
            "deploymentProtocolVersion": 1,
            "deploymentGroupId": GROUP_ID,
            "deploymentGroup": {"id": GROUP_ID, "name": "customers", "externalId": null},
            "purpose": "application",
            "stackSettings": {},
            "releaseChannel": "production",
            "retryRequested": false,
            "updateBlocked": false,
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-01T00:00:00Z",
            "managerId": "mgr_0000000000000000000000000000",
            "workspaceId": WORKSPACE_ID,
        }));
    });
}

fn mock_events<'a>(api: &'a MockServer, with_status_changes: bool, items: Vec<Value>) -> Mock<'a> {
    api.mock(|when, then| {
        let when = when
            .method(GET)
            .path("/v1/events")
            .query_param("deploymentId", DEPLOYMENT_ID);
        if with_status_changes {
            when.query_param("include", "statusChanges");
        }
        then.status(200)
            .json_body(json!({"items": items, "nextCursor": null}));
    })
}

#[test]
fn shows_when_the_wait_for_a_secret_began_and_ended() {
    let api = MockServer::start();
    mock_platform(&api);
    let events = mock_events(&api, true, timeline());

    let output = run_events(&api, &[]);
    let text = text(&output);

    assert!(output.status.success(), "{text}");
    events.assert_hits(1);
    for expected in [
        "2026-01-01 11:37:47 UTC",
        "Update Started",
        "2026-01-01 11:38:13 UTC",
        "Waiting For Secrets",
        "2026-01-01 11:50:14 UTC",
        "Update Resumed",
        "Update Ended",
        "Deployment Created",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in:\n{text}");
    }
}

#[test]
fn json_output_keeps_the_status_change_events() {
    let api = MockServer::start();
    mock_platform(&api);
    mock_events(&api, true, timeline());

    let output = run_events(&api, &["--json"]);

    assert!(output.status.success(), "{}", text(&output));
    let body: Value = serde_json::from_slice(&output.stdout).expect("JSON output");
    let waiting = &body["items"][2]["data"];
    assert_eq!(waiting["type"], "DeploymentStatusChanged");
    assert_eq!(waiting["status"], "waiting-for-secrets");
}

#[test]
fn works_against_a_platform_that_does_not_know_status_changes() {
    let api = MockServer::start();
    mock_platform(&api);
    // An older platform refuses an `include` value it does not know.
    let refused = api.mock(|when, then| {
        when.method(GET)
            .path("/v1/events")
            .query_param("include", "statusChanges");
        then.status(400).json_body(json!({
            "error": {"code": "INVALID_INPUT", "message": "Invalid enum value for include"}
        }));
    });
    let events = mock_events(&api, false, vec![timeline().pop().unwrap()]);

    let output = run_events(&api, &[]);
    let text = text(&output);

    assert!(output.status.success(), "{text}");
    refused.assert_hits(1);
    events.assert_hits(1);
    assert!(text.contains("Deployment Created"), "{text}");
}

#[test]
fn a_refused_list_is_still_an_error() {
    let api = MockServer::start();
    mock_platform(&api);
    api.mock(|when, then| {
        when.method(GET).path("/v1/events");
        then.status(403).json_body(json!({
            "error": {"code": "FORBIDDEN", "message": "No access to this deployment"}
        }));
    });

    let output = run_events(&api, &[]);

    assert!(!output.status.success());
    assert!(
        text(&output).contains("No access to this deployment"),
        "{}",
        text(&output)
    );
}
