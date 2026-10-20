#![cfg(all(feature = "platform", target_os = "linux"))]

//! `alien deploy --name X` on a machine with no stored key for X, in a project
//! that already has a deployment named X in another group.

use httpmock::{
    prelude::HttpMockRequest,
    Method::{GET, PUT},
    Mock, MockServer,
};
use serde_json::{json, Value};
use std::process::{Command, Output};
use tempfile::TempDir;

const PROJECT: &str = "prj_0000000000000000000000000000";
const WORKSPACE_ID: &str = "ws_000000000000000000000000";
const EXISTING_ID: &str = "dep_existing00000000000000000000";
const OTHER_ID: &str = "dep_other00000000000000000000000";
const GROUP_ID: &str = "dg_ntg0000000000000000000000000";

fn run_deploy(server: &MockServer) -> Output {
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
        .args(["deploy", "--name", "nt-1", "--machines"])
        .output()
        .unwrap()
}

fn deployment(id: &str, name: &str, status: &str, group: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "status": status,
        "projectId": PROJECT,
        "platform": "machines",
        "deploymentProtocolVersion": 1,
        "deploymentGroupId": GROUP_ID,
        "deploymentGroup": {"id": GROUP_ID, "name": group, "externalId": null},
        "purpose": "application",
        "releaseChannel": "production",
        "retryRequested": false,"updateBlocked":false,
        "createdAt": "2026-01-01T00:00:00Z",
        "updatedAt": "2026-01-01T00:00:00Z",
        "managerId": "mgr_0000000000000000000000000000",
        "workspaceId": WORKSPACE_ID,
    })
}

fn without_cursor(request: &HttpMockRequest) -> bool {
    request
        .query_params
        .as_ref()
        .map_or(true, |params| params.iter().all(|(key, _)| key != "cursor"))
}

/// Identity and project lookups, and two pages of `nt-1` search results.
/// Returns the mocks for each page.
fn mock_platform<'a>(
    api: &'a MockServer,
    first_page: Vec<Value>,
    second_page: Vec<Value>,
) -> (Mock<'a>, Mock<'a>) {
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
    let second = api.mock(|when, then| {
        when.method(GET)
            .path("/v1/deployments")
            .query_param("project", PROJECT)
            .query_param("search", "nt-1")
            .query_param("cursor", "page-2");
        then.status(200)
            .json_body(json!({"items": second_page, "nextCursor": null}));
    });
    let first = api.mock(|when, then| {
        when.method(GET)
            .path("/v1/deployments")
            .query_param("project", PROJECT)
            .query_param("search", "nt-1")
            .matches(without_cursor);
        then.status(200)
            .json_body(json!({"items": first_page, "nextCursor": "page-2"}));
    });
    (first, second)
}

#[test]
fn refuses_to_create_a_same_named_deployment_in_a_new_group() {
    let api = MockServer::start();
    let (first, second) = mock_platform(
        &api,
        vec![deployment(OTHER_ID, "nt-10", "running", "nt-g")],
        vec![deployment(EXISTING_ID, "nt-1", "running", "nt-g")],
    );
    let ensure_group = api.mock(|when, then| {
        when.method(PUT).path("/v1/deployment-groups/by-name");
        then.status(200)
            .json_body(json!({"id": "dg_new00000000000000000000000000", "projectId": PROJECT}));
    });

    let output = run_deploy(&api);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(!output.status.success(), "deploy should fail: {text}");
    assert!(
        text.contains("A deployment named 'nt-1' already exists in this project"),
        "{text}"
    );
    assert!(
        text.contains(&format!("{EXISTING_ID} in group 'nt-g'")),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "`alien deployments token {EXISTING_ID} > deployment-token.txt` and run `alien deploy --token-file deployment-token.txt --name nt-1`"
        )),
        "{text}"
    );
    first.assert_hits(1);
    second.assert_hits(1);
    // Nothing was created: no group, so no deployment either.
    ensure_group.assert_hits(0);
}

#[test]
fn creates_a_new_group_when_only_similar_or_deleted_names_exist() {
    let api = MockServer::start();
    let (first, second) = mock_platform(
        &api,
        vec![deployment(OTHER_ID, "nt-10", "running", "nt-g")],
        vec![deployment(EXISTING_ID, "nt-1", "deleted", "nt-g")],
    );
    // Stop the flow right after the check, at the first write.
    let ensure_group = api.mock(|when, then| {
        when.method(PUT)
            .path("/v1/deployment-groups/by-name")
            .json_body(json!({"name": "nt-1", "project": PROJECT, "maxDeployments": 1}));
        then.status(503).body("stop here");
    });

    let output = run_deploy(&api);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(!output.status.success(), "{text}");
    assert!(text.contains("Failed to ensure deployment group"), "{text}");
    assert!(!text.contains("already exists"), "{text}");
    first.assert_hits(1);
    second.assert_hits(1);
    ensure_group.assert_hits(1);
}
