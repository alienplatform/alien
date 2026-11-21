#![cfg(target_os = "linux")]

//! `alien deployments enrollment-code` issues a new Operator enrollment code,
//! shows it once, and words the server's refusals plainly.

use httpmock::{
    Method::{GET, POST},
    Mock, MockServer,
};
use serde_json::{json, Value};
use std::process::{Command, Output};
use tempfile::TempDir;

const DEPLOYMENT_ID: &str = "dep_a";
const CODE: &str = "enroll_fixture_code_0123456789";
const AUDIENCE: &str = "https://manager.example.test";

struct Run {
    output: Output,
    log_file: String,
}

fn run(server: &MockServer, extra: &[&str]) -> Run {
    let home = TempDir::new().unwrap();
    let checkout = TempDir::new().unwrap();
    let log_path = home.path().join("alien.log");
    let mut command = Command::new(env!("CARGO_BIN_EXE_alien"));
    command
        .current_dir(checkout.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("ALIEN_MANAGER_URL", server.base_url())
        .env("ALIEN_API_KEY", "test-key")
        .env("RUST_LOG", "trace")
        .env("ALIEN_LOG_FILE", &log_path);
    for name in [
        "ALIEN_PROJECT",
        "ALIEN_BASE_URL",
        "ALIEN_WORKSPACE",
        "ALIEN_LOG",
    ] {
        command.env_remove(name);
    }
    let output = command
        .args(["deployments", "enrollment-code", DEPLOYMENT_ID])
        .args(extra)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let log_file = std::fs::read_to_string(&log_path).expect("should read ALIEN_LOG_FILE");
    Run { output, log_file }
}

fn stdout(run: &Run) -> String {
    String::from_utf8_lossy(&run.output.stdout).into_owned()
}

fn stderr(run: &Run) -> String {
    String::from_utf8_lossy(&run.output.stderr).into_owned()
}

fn deployment(status: &str) -> Value {
    json!({
        "id": DEPLOYMENT_ID,
        "name": "acme",
        "status": status,
        "projectId": "default",
        "platform": "kubernetes",
        "deploymentProtocolVersion": 1,
        "deploymentGroupId": "dg_a",
        "purpose": "application",
        "stackSettings": {},
        "releaseChannel": "production",
        "retryRequested": false,
        "updateBlocked": false,
        "protected": false,
        "signingChannel": null,
        "createdAt": "2026-01-01T00:00:00Z",
        "updatedAt": "2026-01-01T00:00:00Z",
        "workspaceId": "default",
    })
}

fn mock_deployment<'a>(server: &'a MockServer, status: &str) -> Mock<'a> {
    let body = deployment(status);
    server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/deployments/{DEPLOYMENT_ID}"));
        then.status(200).json_body(body);
    })
}

fn mock_issue<'a>(server: &'a MockServer, status: u16, body: Value) -> Mock<'a> {
    server.mock(|when, then| {
        when.method(POST)
            .path(format!("/v1/deployments/{DEPLOYMENT_ID}/enrollment-code"));
        then.status(status).json_body(body);
    })
}

fn issued() -> Value {
    let expires = chrono::Utc::now() + chrono::Duration::minutes(60);
    json!({
        "deploymentId": DEPLOYMENT_ID,
        "code": CODE,
        "expiresAt": expires.to_rfc3339(),
        "managerAudience": AUDIENCE,
    })
}

fn error_body(code: &str, message: &str) -> Value {
    json!({"code": code, "message": message, "retryable": false, "internal": false})
}

#[test]
fn prints_the_code_once_with_expiry_audience_and_helm_flags() {
    let server = MockServer::start();
    mock_deployment(&server, "delete-failed");
    let issue = mock_issue(&server, 200, issued());

    let run = run(&server, &[]);
    let out = stdout(&run);

    assert!(run.output.status.success(), "{out}{}", stderr(&run));
    issue.assert_hits(1);
    assert_eq!(
        out.matches(CODE).count(),
        2,
        "code only in the Code line and the Helm line: {out}"
    );
    assert!(
        out.contains("expires in 59 minutes") || out.contains("expires in 60 minutes"),
        "{out}"
    );
    assert!(out.contains(AUDIENCE), "{out}");
    assert!(
        out.contains(&format!(
            "--set management.enrollmentCode={CODE} --set management.audience={AUDIENCE}"
        )),
        "{out}"
    );
    assert!(out.contains("works once"), "{out}");
}

#[test]
fn json_has_the_four_fields() {
    let server = MockServer::start();
    mock_deployment(&server, "deleting");
    mock_issue(&server, 200, issued());

    let run = run(&server, &["--json"]);
    assert!(run.output.status.success(), "{}", stderr(&run));
    let value: Value = serde_json::from_str(&stdout(&run)).unwrap();
    let object = value.as_object().unwrap();
    assert_eq!(object.len(), 4, "{value}");
    assert_eq!(value["deploymentId"], DEPLOYMENT_ID);
    assert_eq!(value["code"], CODE);
    assert_eq!(value["managerAudience"], AUDIENCE);
    assert!(value["expiresAt"].is_string());
}

#[test]
fn an_existing_operator_key_is_refused_once_in_plain_words() {
    let server = MockServer::start();
    mock_deployment(&server, "delete-pending");
    let issue = mock_issue(
        &server,
        409,
        error_body("ENROLLMENT_KEY_EXISTS", "The Operator already holds a key"),
    );

    let run = run(&server, &[]);
    let text = format!("{}{}", stdout(&run), stderr(&run));

    assert!(!run.output.status.success(), "{text}");
    assert!(
        text.contains("This deployment's Operator still holds a key; an administrator credential is needed to issue a new code."),
        "{text}"
    );
    issue.assert_hits(1);
}

#[test]
fn a_forbidden_credential_is_refused_in_plain_words() {
    let server = MockServer::start();
    mock_deployment(&server, "delete-pending");
    let issue = mock_issue(
        &server,
        403,
        error_body("FORBIDDEN", "Cannot issue an enrollment code"),
    );

    let run = run(&server, &[]);
    let text = format!("{}{}", stdout(&run), stderr(&run));

    assert!(!run.output.status.success(), "{text}");
    assert!(text.contains("can't issue enrollment codes"), "{text}");
    issue.assert_hits(1);
}

#[test]
fn a_running_deployment_without_yes_and_without_a_terminal_is_refused_before_any_post() {
    let server = MockServer::start();
    mock_deployment(&server, "running");
    let issue = mock_issue(&server, 200, issued());

    let run = run(&server, &[]);
    let text = format!("{}{}", stdout(&run), stderr(&run));

    assert!(!run.output.status.success(), "{text}");
    assert!(text.contains("--yes"), "{text}");
    assert!(!text.contains(CODE), "{text}");
    issue.assert_hits(0);
}

#[test]
fn yes_skips_the_confirmation_for_a_running_deployment() {
    let server = MockServer::start();
    mock_deployment(&server, "running");
    let issue = mock_issue(&server, 200, issued());

    let run = run(&server, &["--yes"]);

    assert!(run.output.status.success(), "{}", stderr(&run));
    issue.assert_hits(1);
}

#[test]
fn the_code_never_reaches_tracing_or_error_output() {
    let server = MockServer::start();
    mock_deployment(&server, "delete-failed");
    mock_issue(&server, 200, issued());

    let run = run(&server, &[]);

    assert!(run.output.status.success(), "{}", stderr(&run));
    assert!(stdout(&run).contains(CODE));
    assert!(!stderr(&run).contains(CODE), "stderr leaked the code");
    // The log must hold this run's request, or "code absent" proves nothing.
    assert!(!run.log_file.trim().is_empty(), "log file is empty");
    assert!(
        run.log_file
            .contains(&format!("connected to {}", server.address())),
        "log file has no connection line for the mock manager: {}",
        run.log_file
    );
    assert!(!run.log_file.contains(CODE), "log file leaked the code");
}
