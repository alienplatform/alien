//! The docs' controller binary as `alien dev` drives it: the step protocol over stdin and
//! stdout, for the zone's local controller only.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

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

fn step(platform: &str, state: Value) -> Value {
    json!({
        "protocol": 1, "stepId": format!("step-{platform}"), "deploymentId": "dep_acme",
        "resourceId": "zone", "resourceType": "acme/dns-zone", "platform": platform,
        "resourcePrefix": "q6aa0b26", "initialSetupAuthority": "importedHandoff",
        "lifecycle": "live",
        "resource": { "type": "acme/dns-zone", "id": "zone", "domain": "internal.acme.dev" },
        "previousResource": null, "dependencies": {},
        "deploymentConfig": {
            "stackSettings": {},
            "environmentVariables": { "variables": [], "hash": "", "createdAt": "" },
            "externalBindings": {},
            "allowFrozenChanges": false
        },
        "setupRun": false, "setupRunId": null, "state": state, "transitions": [],
        "credentials": null, "deadline": "2099-01-01T00:00:00Z"
    })
}

/// The binary runs the local controller alone: the zone's cloud controllers are in the crate's
/// library, which builds into the controller module.
#[test]
fn the_binary_lists_its_local_controller() {
    let output = Command::new(BINARY).arg("--controllers").output().unwrap();
    assert!(output.status.success());
    let listed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(listed, json!({ "acme/dns-zone": ["local"] }));
}

#[test]
fn the_local_controller_steps_over_stdin_and_stdout() {
    let mut controller = StdioController::start();

    // alien dev: a local create, with no credentials and no cloud call.
    let reply = controller.send(step("local", Value::Null));
    assert_eq!(reply["result"], "stepped", "{reply}");
    assert_eq!(reply["controller"]["status"], "running");
    assert_eq!(reply["controller"]["outputs"]["zoneId"], "q6aa0b26-zone");
    // The next step starts from the state the last one returned.
    let state = reply["controller"]["state"].clone();
    assert_eq!(state["state"], "ready");
    let mut next = step("local", state);
    next["stepId"] = json!("step-local-2");
    let reply = controller.send(next);
    assert_eq!(reply["suggestedDelayMs"], 60_000, "{reply}");

    let reply = controller.send(step("aws", Value::Null));
    assert_eq!(reply["result"], "refused", "{reply}");
}

/// The binary serves stdin and stdout only.
#[test]
fn the_binary_refuses_to_serve_http() {
    let output = Command::new(BINARY)
        .args(["--http", "127.0.0.1:0"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown argument `--http`"), "{stderr}");
}
