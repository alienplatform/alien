#![cfg(all(feature = "platform", target_os = "linux"))]

//! `alien deployments set-inputs --secret-input-file` sends a file's contents,
//! line breaks included, so a PEM certificate and key arrive intact.

use httpmock::{
    Method::{GET, PATCH, POST},
    Mock, MockServer,
};
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Output, Stdio};
use tempfile::TempDir;

const PROJECT: &str = "prj_0000000000000000000000000000";
const WORKSPACE_ID: &str = "ws_000000000000000000000000";
const DEPLOYMENT_ID: &str = "dep_secretfile000000000000000000";
const GROUP_ID: &str = "dg_customers0000000000000000000";
const RELEASE_ID: &str = "rel_0000000000000000000000000000";
const PEM: &str = "-----BEGIN CERTIFICATE-----\n\
MIIBszCCAVmgAwIBAgIUQ2VydGlmaWNhdGVGb3JUZXN0aW5nMAoGCCqGSM49BAMC\n\
-----END CERTIFICATE-----\n\
-----BEGIN PRIVATE KEY-----\n\
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgVGVzdEtleU1hdGVy\n\
-----END PRIVATE KEY-----\n";

fn run_set_inputs(server: &MockServer, secret_input_file: &str, stdin: Option<&str>) -> Output {
    let home = TempDir::new().expect("home dir");
    let checkout = TempDir::new().expect("checkout dir");
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
        .args(["deployments", "set-inputs", DEPLOYMENT_ID])
        .args(["--secret-input-file", secret_input_file])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("alien starts");
    let mut child_stdin = child.stdin.take().expect("piped stdin");
    if let Some(contents) = stdin {
        child_stdin
            .write_all(contents.as_bytes())
            .expect("stdin written");
    }
    drop(child_stdin);
    child.wait_with_output().expect("alien exits")
}

fn output_text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
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
        "stackSettings": {},
        "releaseChannel": "production",
        "retryRequested": false,
        "updateBlocked": false,
        "createdAt": "2026-01-01T00:00:00Z",
        "updatedAt": "2026-01-01T00:00:00Z",
        "managerId": "mgr_0000000000000000000000000000",
        "workspaceId": WORKSPACE_ID,
    })
}

fn inputs() -> Value {
    json!({
        "inputs": [{
            "id": "tlsCertificate",
            "kind": "secret",
            "label": "TLS certificate",
            "description": "PEM certificate and key.",
            "providedBy": ["deployer"],
            "required": false
        }],
        "values": {},
        "providedInputIds": []
    })
}

/// Mocks the API the command calls. The returned save mock only matches a
/// request carrying `PEM` exactly; any other body gets a 404 and fails.
fn mock_api(api: &MockServer) -> Mock<'_> {
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
    api.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/deployments/{DEPLOYMENT_ID}"));
        then.status(200).json_body(deployment());
    });
    api.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/deployments/{DEPLOYMENT_ID}/inputs"));
        then.status(200).json_body(inputs());
    });
    api.mock(|when, then| {
        when.method(POST)
            .path(format!("/v1/deployments/{DEPLOYMENT_ID}/secret-access"));
        then.status(409).json_body(json!({
            "code": "SECRET_VALUES_NOT_SERVED_BY_INSTANCE",
            "message": "Send its secret values to the Platform API.",
            "retryable": false,
            "internal": false
        }));
    });
    let save = api.mock(|when, then| {
        when.method(PATCH)
            .path(format!("/v1/deployments/{DEPLOYMENT_ID}/inputs"))
            .json_body_partial(json!({"inputValues": {"tlsCertificate": PEM}}).to_string());
        let mut response = inputs();
        response["providedInputIds"] = json!(["tlsCertificate"]);
        response["runtimeUpdateRequested"] = json!(true);
        response["outcome"] = json!("accepted");
        response["operation"] = json!({
            "id": "duop_0000000000000000000000000000",
            "status": "queued",
            "reasons": ["configuration"],
            "targetReleaseId": RELEASE_ID,
            "changedKeys": ["tlsCertificate"],
            "requestedAt": "2026-01-01T00:00:00Z"
        });
        then.status(200).json_body(response);
    });
    save
}

#[test]
fn sends_a_pem_file_with_its_line_breaks() {
    let api = MockServer::start();
    let save = mock_api(&api);
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("tls.pem");
    std::fs::write(&path, PEM).expect("write pem");

    let output = run_set_inputs(&api, &format!("tlsCertificate={}", path.display()), None);
    let text = output_text(&output);

    assert!(output.status.success(), "{text}");
    assert!(text.contains("Stack inputs saved."), "{text}");
    save.assert_hits(1);
}

#[test]
fn reads_a_secret_from_stdin() {
    let api = MockServer::start();
    let save = mock_api(&api);

    let output = run_set_inputs(&api, "tlsCertificate=-", Some(PEM));
    let text = output_text(&output);

    assert!(output.status.success(), "{text}");
    save.assert_hits(1);
}

#[test]
fn fails_on_a_missing_file_before_calling_the_api() {
    let api = MockServer::start();
    // Matches every request: the command must fail before sending any.
    let any_request = api.mock(|_when, then| {
        then.status(500);
    });
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("missing.pem");

    let output = run_set_inputs(&api, &format!("tlsCertificate={}", path.display()), None);
    let text = output_text(&output);

    assert!(!output.status.success(), "{text}");
    assert!(text.contains(&path.display().to_string()), "{text}");
    any_request.assert_hits(0);
}

#[test]
fn refuses_empty_stdin_before_calling_the_api() {
    let api = MockServer::start();
    let any_request = api.mock(|_when, then| {
        then.status(500);
    });

    let output = run_set_inputs(&api, "tlsCertificate=-", Some(""));
    let text = output_text(&output);

    assert!(!output.status.success(), "{text}");
    assert!(text.contains("'tlsCertificate' is empty"), "{text}");
    any_request.assert_hits(0);
}
