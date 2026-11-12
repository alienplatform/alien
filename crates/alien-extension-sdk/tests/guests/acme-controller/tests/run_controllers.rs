//! The docs' controller binary as Alien drives it: the step protocol over stdin and stdout, and
//! over HTTP with a token. No step here makes a cloud call.

use alien_infra::step_protocol::token::{self, Claims};
use base64::Engine as _;
use ed25519_compact::{KeyPair, Seed};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Duration;

const BINARY: &str = env!("CARGO_BIN_EXE_acme-controller");

struct StdioController {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl StdioController {
    fn start() -> Self {
        let mut child = Command::new(BINARY)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        Self {
            stdin: child.stdin.take().unwrap(),
            stdout: BufReader::new(child.stdout.take().unwrap()),
            child,
        }
    }

    fn send(&mut self, message: Value) -> Value {
        writeln!(self.stdin, "{message}").unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }
}

impl Drop for StdioController {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn step(platform: &str, credentials: Value, state: Value, pending_import: Option<Value>) -> Value {
    let mut request = json!({
        "protocol": 1, "stepId": format!("step-{platform}"), "deploymentId": "dep_acme",
        "resourceId": "zone", "resourceType": "acme/dns-zone", "platform": platform,
        "resourcePrefix": "q6aa0b26", "initialSetupAuthority": "importedHandoff",
        "lifecycle": "frozen",
        "resource": { "type": "acme/dns-zone", "id": "zone", "domain": "internal.acme.dev" },
        "previousResource": null, "dependencies": {},
        "deploymentConfig": {
            "stackSettings": {},
            "environmentVariables": { "variables": [], "hash": "", "createdAt": "" },
            "externalBindings": {},
            "allowFrozenChanges": false
        },
        "setupRun": false, "setupRunId": null, "state": state, "transitions": [],
        "credentials": credentials, "deadline": "2099-01-01T00:00:00Z"
    });
    if let Some(imported) = pending_import {
        request["pendingImport"] = imported;
    }
    request
}

fn aws_credentials() -> Value {
    json!({
        "platform": "aws", "accountId": "123456789012", "region": "us-east-1",
        "accessKeyId": "ASIASTEPKEY", "secretAccessKey": "step-secret",
        "sessionToken": null, "expiresAt": null
    })
}

#[test]
fn the_binary_lists_its_controllers() {
    let output = Command::new(BINARY).arg("--controllers").output().unwrap();
    assert!(output.status.success());
    let listed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        listed,
        json!({ "acme/dns-zone": ["aws", "azure", "gcp", "local"] })
    );
}

#[test]
fn an_importers_fields_load_into_the_aws_controller() {
    let mut controller = StdioController::start();
    let reply = controller.send(json!({
        "protocol": 1, "op": "load", "resourceType": "acme/dns-zone", "platform": "aws",
        "state": { "state": "ready", "zoneId": "Z123" }
    }));
    let view = &reply["view"];
    assert_eq!(view["status"], "running", "{reply}");
    assert_eq!(
        view["outputs"],
        json!({ "type": "acme/dns-zone", "zoneId": "Z123" })
    );
    assert_eq!(view["state"]["type"], "AwsDnsZoneController");

    let refused = controller.send(json!({
        "protocol": 1, "op": "load", "resourceType": "acme/dns-zone", "platform": "aws",
        "state": { "state": "running", "zoneId": "Z123" }
    }));
    assert!(
        refused["refused"]
            .as_str()
            .unwrap()
            .contains("unknown variant `running`"),
        "{refused}"
    );
}

#[test]
fn the_controllers_step_over_stdin_and_stdout() {
    let mut controller = StdioController::start();

    // alien dev: a local create, with no credentials and no cloud call.
    let reply = controller.send(step("local", Value::Null, Value::Null, None));
    assert_eq!(reply["result"], "stepped", "{reply}");
    assert_eq!(reply["controller"]["status"], "running");
    assert_eq!(reply["controller"]["outputs"]["zoneId"], "q6aa0b26-zone");
    // The next step starts from the state the last one returned.
    let state = reply["controller"]["state"].clone();
    assert_eq!(state["state"], "ready");
    let mut next = step("local", Value::Null, state, None);
    next["stepId"] = json!("step-local-2");
    let reply = controller.send(next);
    assert_eq!(reply["suggestedDelayMs"], 60_000, "{reply}");

    // An imported AWS zone's first step is a refresh from Ready, not a create.
    let reply = controller.send(step(
        "aws",
        aws_credentials(),
        Value::Null,
        Some(json!({ "state": "ready", "zoneId": "Z123" })),
    ));
    assert_eq!(reply["result"], "stepped", "{reply}");
    assert_eq!(reply["controller"]["status"], "running");
    assert_eq!(reply["suggestedDelayMs"], 60_000);

    let reply = controller.send(step("kubernetes", Value::Null, Value::Null, None));
    assert_eq!(reply["result"], "refused", "{reply}");
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_binary_serves_steps_over_http_for_the_keys_it_trusts() {
    let manager = KeyPair::from_seed(Seed::new([7; 32]));
    let stranger = KeyPair::from_seed(Seed::new([8; 32]));
    let port = free_port();
    let url = format!("http://127.0.0.1:{port}");
    let _server = KillOnDrop(
        Command::new(BINARY)
            .args(["--http", &format!("127.0.0.1:{port}"), "--audience", &url])
            .args(["--issuer", "https://manager.example.com"])
            .args(["--project", "prj_acme"])
            .args([
                "--trusted-key",
                &base64::engine::general_purpose::STANDARD.encode(manager.pk.as_ref()),
            ])
            .spawn()
            .unwrap(),
    );
    let client = reqwest::Client::new();
    let post = |key: &KeyPair, body: Value| {
        let now = chrono::Utc::now().timestamp();
        let claims = Claims {
            iss: "https://manager.example.com".into(),
            aud: url.clone(),
            project: "prj_acme".into(),
            deployment_id: body["deploymentId"].as_str().unwrap().into(),
            extension: "acme".into(),
            resource_id: "zone".into(),
            step_id: body["stepId"].as_str().unwrap().into(),
            body_sha256: token::body_sha256(&serde_json::to_vec(&body).unwrap()),
            iat: now,
            exp: now + 300,
            jti: "j1".into(),
        };
        client
            .post(format!("{url}/v1/step"))
            .bearer_auth(token::mint(&claims, &key.sk).unwrap())
            .json(&body)
            .send()
    };
    // The first start of a freshly linked binary can stall most of a minute while macOS scans it.
    let mut answer = None;
    for _ in 0..1200 {
        match post(&manager, step("local", Value::Null, Value::Null, None)).await {
            Ok(response) => {
                answer = Some(response);
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    let answer = answer.expect("the server starts");
    assert_eq!(answer.status(), 200);
    let reply: Value = answer.json().await.unwrap();
    assert_eq!(reply["result"], "stepped", "{reply}");

    let stranger_answer = post(&stranger, step("local", Value::Null, Value::Null, None))
        .await
        .unwrap();
    assert_eq!(stranger_answer.status(), 401);

    let refused = post(&manager, step("kubernetes", Value::Null, Value::Null, None))
        .await
        .unwrap();
    assert_eq!(refused.status(), 400);
}

/// Stops the server when the test ends, failed assertion or not.
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
