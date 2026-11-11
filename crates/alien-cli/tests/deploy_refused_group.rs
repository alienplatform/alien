#![cfg(all(feature = "platform", target_os = "linux"))]

//! `alien deploy` without `--deployment-group` makes a group for the new
//! deployment. When the platform then refuses the deployment, the run must not
//! leave that empty group behind, and it must never delete a group it did not
//! make.

use httpmock::{
    Method::{DELETE, GET, PATCH, POST, PUT},
    Mock, MockServer,
};
use serde_json::{json, Value};
use std::process::Command;
use tempfile::TempDir;

const PROJECT: &str = "prj_0000000000000000000000000000";
const WORKSPACE_ID: &str = "ws_000000000000000000000000";
const GROUP_ID: &str = "dg_refused0000000000000000000000";

struct Run {
    text: String,
    success: bool,
}

fn run_deploy(server: &MockServer) -> Run {
    run_deploy_with(server, &["--machines"])
}

fn run_deploy_with(server: &MockServer, extra_args: &[&str]) -> Run {
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
    let output = command
        .args(["--base-url", &server.base_url()])
        .args(["--project", "demo-project"])
        .args(["deploy", "--name", "refused-1"])
        .args(extra_args)
        .output()
        .unwrap();
    Run {
        text: format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
        success: output.status.success(),
    }
}

/// A platform that knows the project, has no deployment named `refused-1`, hands
/// out the group `ensure_group` describes, and refuses the release preparation
/// that comes before the deployment is created.
fn mock_refusing_platform(api: &MockServer, ensure_group: Value) {
    mock_refusing_platform_with(
        api,
        ensure_group,
        json!({
            "code": "SETUP_METHOD_CANNOT_INSTALL_RESOURCES",
            "message": "This stack cannot be set up from the CLI."
        }),
    );
}

fn mock_refusing_platform_with(api: &MockServer, ensure_group: Value, refusal: Value) {
    mock_platform_without_prepare(api, ensure_group);
    api.mock(|when, then| {
        when.method(POST).path("/v1/deployment-info/prepare-stack");
        then.status(400).json_body(refusal);
    });
}

fn mock_platform_without_prepare(api: &MockServer, ensure_group: Value) {
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
        when.method(GET).path("/v1/deployments");
        then.status(200)
            .json_body(json!({"items": [], "nextCursor": null}));
    });
    api.mock(|when, then| {
        when.method(PUT).path("/v1/deployment-groups/by-name");
        then.status(200).json_body(ensure_group);
    });
    api.mock(|when, then| {
        when.method(POST).path(format!(
            "/v1/deployment-groups/{GROUP_ID}/first-party-session"
        ));
        then.status(200)
            .json_body(json!({"token": "session-token"}));
    });
    api.mock(|when, then| {
        when.method(PUT).path("/v1/deployments/first-party-inputs");
        then.status(200).json_body(json!({}));
    });
}

fn mock_delete_group(api: &MockServer, status: u16) -> Mock<'_> {
    api.mock(|when, then| {
        when.method(DELETE)
            .path(format!("/v1/deployment-groups/{GROUP_ID}"));
        then.status(status).json_body(json!({}));
    })
}

#[test]
fn deletes_the_group_it_made_when_the_deployment_is_refused() {
    let api = MockServer::start();
    mock_refusing_platform(
        &api,
        json!({"id": GROUP_ID, "projectId": PROJECT, "created": true}),
    );
    let delete = mock_delete_group(&api, 200);

    let run = run_deploy(&api);

    assert!(!run.success, "deploy should fail: {}", run.text);
    delete.assert_hits(1);
    assert!(
        run.text.contains("SETUP_METHOD_CANNOT_INSTALL_RESOURCES"),
        "the refusal is still shown: {}",
        run.text
    );
    assert!(
        run.text
            .contains(&format!("Deleted the empty deployment group {GROUP_ID}")),
        "{}",
        run.text
    );
}

#[test]
fn keeps_a_group_that_already_existed() {
    let api = MockServer::start();
    mock_refusing_platform(
        &api,
        json!({"id": GROUP_ID, "projectId": PROJECT, "created": false}),
    );
    let delete = mock_delete_group(&api, 200);

    let run = run_deploy(&api);

    assert!(!run.success, "deploy should fail: {}", run.text);
    delete.assert_hits(0);
    assert!(!run.text.contains("empty deployment group"), "{}", run.text);
}

#[test]
fn deletes_nothing_and_names_the_group_when_the_manager_does_not_say() {
    let api = MockServer::start();
    mock_refusing_platform(&api, json!({"id": GROUP_ID, "projectId": PROJECT}));
    let delete = mock_delete_group(&api, 200);

    let run = run_deploy(&api);

    assert!(!run.success, "deploy should fail: {}", run.text);
    delete.assert_hits(0);
    assert!(
        run.text
            .contains(&format!("`alien deployment-groups delete {GROUP_ID}`")),
        "{}",
        run.text
    );
}

#[test]
fn reports_a_group_it_could_not_delete() {
    let api = MockServer::start();
    mock_refusing_platform(
        &api,
        json!({"id": GROUP_ID, "projectId": PROJECT, "created": true}),
    );
    let delete = mock_delete_group(&api, 500);

    let run = run_deploy(&api);

    assert!(!run.success, "deploy should fail: {}", run.text);
    delete.assert_hits(1);
    assert!(
        run.text.contains("SETUP_METHOD_CANNOT_INSTALL_RESOURCES"),
        "{}",
        run.text
    );
    assert!(
        run.text.contains(&format!(
            "Could not delete the empty deployment group {GROUP_ID}"
        )),
        "{}",
        run.text
    );
}

#[test]
fn says_a_group_that_holds_a_deployment_was_kept() {
    let api = MockServer::start();
    mock_refusing_platform(
        &api,
        json!({"id": GROUP_ID, "projectId": PROJECT, "created": true}),
    );
    let delete = mock_delete_group(&api, 409);

    let run = run_deploy(&api);

    assert!(!run.success, "deploy should fail: {}", run.text);
    delete.assert_hits(1);
    assert!(
        run.text.contains("SETUP_METHOD_CANNOT_INSTALL_RESOURCES"),
        "{}",
        run.text
    );
    assert!(
        run.text.contains(&format!(
            "Deployment group {GROUP_ID} already holds a deployment"
        )),
        "{}",
        run.text
    );
    assert!(
        !run.text.contains("No deployment was created"),
        "{}",
        run.text
    );
}

/// The deployment is created, then saving the answers held back from the
/// create fails. The deployment exists, so the group is not empty and must stay.
#[test]
fn keeps_the_group_when_saving_inputs_fails_after_the_deployment_was_created() {
    const DEPLOYMENT_ID: &str = "dep_0c0ffee0c0ffee0c0ffee0c0ffee";
    let api = MockServer::start();
    mock_refusing_platform(
        &api,
        json!({"id": GROUP_ID, "projectId": PROJECT, "created": true}),
    );
    // A local deployment is not prepared before it is created, so the create
    // itself goes through.
    api.mock(|when, then| {
        when.method(POST).path("/v1/deployments");
        then.status(200).json_body(json!({
            "deployment": {"id": DEPLOYMENT_ID},
            "token": "deployment-token"
        }));
    });
    api.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/deployments/{DEPLOYMENT_ID}/inputs"));
        then.status(200).json_body(json!({
            "inputs": [], "providedInputIds": [], "heldInputIds": [], "values": {}
        }));
    });
    api.mock(|when, then| {
        when.method(PATCH)
            .path(format!("/v1/deployments/{DEPLOYMENT_ID}/inputs"));
        then.status(500)
            .json_body(json!({"code": "INTERNAL", "message": "storage is down"}));
    });
    let delete = mock_delete_group(&api, 200);

    let run = run_deploy_with(
        &api,
        &["--platform", "local", "--secret-input", "api_key=hunter2"],
    );

    assert!(!run.success, "deploy should fail: {}", run.text);
    delete.assert_hits(0);
    assert!(
        run.text.contains(&format!(
            "Deployment '{DEPLOYMENT_ID}' was created, but the answers for api_key were not stored"
        )),
        "the existing message names the deployment: {}",
        run.text
    );
    assert!(
        run.text
            .contains(&format!("alien deployments set-inputs {DEPLOYMENT_ID}")),
        "{}",
        run.text
    );
    assert!(
        !run.text.contains("No deployment was created"),
        "{}",
        run.text
    );
    assert!(!run.text.contains("empty deployment group"), "{}", run.text);
}

#[test]
fn tells_the_platform_it_can_create_keys() {
    let api = MockServer::start();
    mock_platform_without_prepare(
        &api,
        json!({"id": GROUP_ID, "projectId": PROJECT, "created": false}),
    );
    // Only a request that says the CLI can create keys is answered.
    let capable = api.mock(|when, then| {
        when.method(POST)
            .path("/v1/deployment-info/prepare-stack")
            .json_body_partial(json!({"setupCapabilities": ["create-keys"]}).to_string());
        then.status(400).json_body(json!({
            "code": "SETUP_METHOD_CANNOT_INSTALL_RESOURCES",
            "message": "seen"
        }));
    });

    let run = run_deploy(&api);

    assert!(!run.success, "{}", run.text);
    assert!(capable.hits() >= 1, "no prepare request carried the capability: {}", run.text);
}

#[test]
fn explains_a_platform_that_cannot_create_keys_yet() {
    let api = MockServer::start();
    // What a platform from before CLI keys answers for a stack with a key.
    mock_refusing_platform_with(
        &api,
        json!({"id": GROUP_ID, "projectId": PROJECT, "created": false}),
        json!({
            "code": "SETUP_METHOD_CANNOT_INSTALL_RESOURCES",
            "message": "The 'cli' setup cannot install key resources (data-key): they enter a deployment through stack import. Use the CloudFormation or Terraform setup instead."
        }),
    );

    let run = run_deploy(&api);

    assert!(!run.success, "{}", run.text);
    assert!(
        run.text.contains("cannot yet create encryption keys during a CLI setup"),
        "{}",
        run.text
    );
}
