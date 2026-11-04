#![cfg(all(feature = "platform", target_os = "linux"))]

//! `alien deployments pin` takes the same deployment references as the other
//! deployment commands: an ID, or `<deployment-group-name>/<deployment-name>`.

use httpmock::{
    Method::{GET, POST},
    MockServer,
};
use serde_json::{json, Value};
use std::process::{Command, Output};
use tempfile::TempDir;

const PROJECT: &str = "prj_0000000000000000000000000000";
const WORKSPACE_ID: &str = "ws_000000000000000000000000";
const DEPLOYMENT_ID: &str = "dep_pinned0000000000000000000000";
const GROUP_ID: &str = "dg_customers0000000000000000000";
const RELEASE_ID: &str = "rel_0000000000000000000000000000";

fn run_pin(server: &MockServer, reference: &str) -> Output {
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
        .args(["deployments", "pin", reference, RELEASE_ID])
        .output()
        .unwrap()
}

fn deployment() -> Value {
    json!({
        "id": DEPLOYMENT_ID,
        "name": "acme",
        "status": "running",
        "projectId": PROJECT,
        "platform": "aws",
        "deploymentProtocolVersion": 1,
        "deploymentGroupId": GROUP_ID,
        "deploymentGroup": {"id": GROUP_ID, "name": "customers", "externalId": null},
        "purpose": "application",
        "releaseChannel": "production",
        "retryRequested": false,
        "updateBlocked": false,
        "createdAt": "2026-01-01T00:00:00Z",
        "updatedAt": "2026-01-01T00:00:00Z",
        "managerId": "mgr_0000000000000000000000000000",
        "workspaceId": WORKSPACE_ID,
    })
}

fn output_text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn mock_identity(api: &MockServer) {
    api.mock(|when, then| {
        when.method(GET)
            .path("/v1/whoami")
            .header("authorization", "Bearer test-key");
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
}

#[test]
fn pins_a_deployment_named_by_group_and_name() {
    let api = MockServer::start();
    mock_identity(&api);
    let groups = api.mock(|when, then| {
        when.method(GET)
            .path("/v1/deployment-groups")
            .query_param("project", PROJECT)
            .query_param("search", "customers");
        then.status(200).json_body(json!({
            "items": [
                // A search match whose name is only similar must not be picked.
                {"id": "dg_other00000000000000000000000", "name": "customers-eu",
                 "externalId": null, "projectId": PROJECT, "workspaceId": WORKSPACE_ID,
                 "maxDeployments": 100, "createdAt": "2026-01-01T00:00:00Z"},
                {"id": GROUP_ID, "name": "customers", "externalId": null,
                 "projectId": PROJECT, "workspaceId": WORKSPACE_ID,
                 "maxDeployments": 100, "createdAt": "2026-01-01T00:00:00Z"}
            ],
            "nextCursor": null
        }));
    });
    let deployments = api.mock(|when, then| {
        when.method(GET)
            .path("/v1/deployments")
            .query_param("deploymentGroup", GROUP_ID)
            .query_param("search", "acme");
        then.status(200)
            .json_body(json!({"items": [deployment()], "nextCursor": null}));
    });
    let detail = api.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/deployments/{DEPLOYMENT_ID}"));
        then.status(200).json_body(deployment());
    });
    let pin = api.mock(|when, then| {
        when.method(POST)
            .path(format!("/v1/deployments/{DEPLOYMENT_ID}/pin-release"))
            .json_body(json!({"releaseId": RELEASE_ID}));
        then.status(202)
            .json_body(json!({"message": "Pinned to the release."}));
    });

    let output = run_pin(&api, "customers/acme");
    let text = output_text(&output);

    assert!(output.status.success(), "{text}");
    assert!(text.contains("Deployment pin updated."), "{text}");
    groups.assert_hits(1);
    deployments.assert_hits(1);
    detail.assert_hits(1);
    pin.assert_hits(1);
}

#[test]
fn explains_a_malformed_deployment_id_before_calling_the_api() {
    let api = MockServer::start();
    mock_identity(&api);
    let pin = api.mock(|when, then| {
        when.method(POST).path_contains("/pin-release");
        then.status(202).json_body(json!({"message": "unexpected"}));
    });

    // One character short of a valid ID.
    let output = run_pin(&api, "dep_pinned000000000000000000000");
    let text = output_text(&output);

    assert!(!output.status.success(), "{text}");
    assert!(
        text.contains("'dep_pinned000000000000000000000' is not a valid deployment ID"),
        "{text}"
    );
    assert!(
        text.contains("<deployment-group-name>/<deployment-name>"),
        "{text}"
    );
    pin.assert_hits(0);
}
