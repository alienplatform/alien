//! The step log a controller host writes when it is given one: one whole JSON object per line,
//! however many steps it handles at once, and no secret from the step's credentials.

use alien_infra::step_protocol::protocol::StepRequest;
use alien_infra::step_protocol::test_controller::ControllerHost;
use serde_json::{json, Value};
use std::sync::Arc;
use tempfile::TempDir;

/// A step for a type with no controller, so the host refuses it at once. The long step id makes
/// each line large enough that a line written in pieces would interleave with another.
fn request(i: usize) -> StepRequest {
    serde_json::from_value(json!({
        "protocol": 1, "stepId": format!("step-{i}-{}", "x".repeat(20_000)),
        "deploymentId": "dep_log", "resourceId": "zone",
        "resourceType": "acme/unknown", "platform": "aws",
        "resourcePrefix": "q6aa0b26", "initialSetupAuthority": "importedHandoff",
        "lifecycle": "live", "resource": { "type": "acme/unknown", "id": "zone" },
        "previousResource": null, "dependencies": {},
        "deploymentConfig": {}, "setupRun": false, "setupRunId": null, "state": null,
        "transitions": [],
        "credentials": {
            "platform": "aws", "accountId": "123456789012", "region": "us-east-1",
            "accessKeyId": "ASIALOGKEY", "secretAccessKey": "LOGSECRETVALUE",
            "sessionToken": "LOGSESSIONVALUE", "expiresAt": null
        },
        "deadline": "2099-01-01T00:00:00Z"
    }))
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_steps_keep_the_log_one_object_per_line() {
    let dir = TempDir::new().unwrap();
    let log = dir.path().join("log.jsonl");
    let host = Arc::new(ControllerHost::new(dir.path(), Some(log.clone())));
    let tasks: Vec<_> = (0..64)
        .map(|i| {
            let host = host.clone();
            tokio::spawn(async move { host.handle(request(i)).await })
        })
        .collect();
    for task in tasks {
        task.await.unwrap();
    }
    let text = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 64);
    let unreadable = lines
        .iter()
        .filter(|line| serde_json::from_str::<Value>(line).is_err())
        .count();
    assert_eq!(unreadable, 0, "interleaved log lines");
}

#[tokio::test]
async fn the_log_never_holds_the_steps_secrets() {
    let dir = TempDir::new().unwrap();
    let log = dir.path().join("log.jsonl");
    let host = ControllerHost::new(dir.path(), Some(log.clone()));
    host.handle(request(0)).await;
    let text = std::fs::read_to_string(&log).unwrap();
    for secret in ["LOGSECRETVALUE", "LOGSESSIONVALUE"] {
        assert!(!text.contains(secret), "the log holds {secret}");
    }
    let line: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    let credentials = &line["request"]["credentials"];
    assert_eq!(credentials["accessKeyId"], "ASIALOGKEY");
    assert_eq!(credentials["secretAccessKey"], "<redacted>");
    assert_eq!(credentials["sessionToken"], "<redacted>");
}

fn step_on(platform: &str, credentials: Value) -> StepRequest {
    serde_json::from_value(json!({
        "protocol": 1, "stepId": format!("step-{platform}"),
        "deploymentId": "dep_log", "resourceId": "zone",
        "resourceType": "acme/unknown", "platform": platform,
        "resourcePrefix": "q6aa0b26", "initialSetupAuthority": "importedHandoff",
        "lifecycle": "live", "resource": { "type": "acme/unknown", "id": "zone" },
        "previousResource": null, "dependencies": {},
        "deploymentConfig": {}, "setupRun": false, "setupRunId": null, "state": null,
        "transitions": [], "credentials": credentials,
        "deadline": "2099-01-01T00:00:00Z"
    }))
    .unwrap()
}

#[tokio::test]
async fn the_log_never_holds_a_gcp_or_azure_token() {
    let dir = TempDir::new().unwrap();
    let log = dir.path().join("log.jsonl");
    let host = ControllerHost::new(dir.path(), Some(log.clone()));
    host.handle(step_on(
        "gcp",
        json!({ "platform": "gcp", "projectId": "proj", "region": "us-central1",
                "accessToken": "GCPTOKENVALUE", "expiresAt": "2099-01-01T00:00:00Z" }),
    ))
    .await;
    host.handle(step_on(
        "azure",
        json!({ "platform": "azure", "subscriptionId": "sub", "tenantId": "ten",
                "accessToken": "AZURETOKENVALUE", "expiresAt": "2099-01-01T00:00:00Z" }),
    ))
    .await;
    let text = std::fs::read_to_string(&log).unwrap();
    for secret in ["GCPTOKENVALUE", "AZURETOKENVALUE"] {
        assert!(!text.contains(secret), "the log holds {secret}");
    }
    let lines: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    for (line, platform) in lines.iter().zip(["gcp", "azure"]) {
        let credentials = &line["request"]["credentials"];
        assert_eq!(credentials["platform"], platform);
        assert_eq!(credentials["accessToken"], "<redacted>");
    }
}
