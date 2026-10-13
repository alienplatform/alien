use httpmock::{
    Method::{GET, POST},
    MockServer,
};
use serde_json::{json, Value};
use std::process::Command;
use tempfile::TempDir;

const MESSAGE: &str = "Recovery accepted. Rerun deployment setup and select compute.";

fn retry(message: Option<&str>, json_output: bool, refused: bool) {
    let manager = MockServer::start();
    let read = manager.mock(|when, then| {
        when.method(GET)
            .path("/v1/deployments/dep_test")
            .header("authorization", "Bearer test-key");
        then.status(200)
            .json_body(json!({"id":"dep_test", "name":"test", "platform":"aws",
            "status":"update-pending", "deploymentGroupId":"dg_test", "deploymentProtocolVersion":1,
            "projectId":"prj_test", "workspaceId":"ws_test", "retryRequested":false,
            "createdAt":"2026-01-01T00:00:00Z"}));
    });
    let write = manager.mock(|when, then| {
        when.method(POST)
            .path("/v1/deployments/dep_test/retry")
            .header("authorization", "Bearer test-key");
        if refused {
            then.status(400).json_body(
                json!({"code":"BAD_REQUEST", "message":"Pending work cannot safely recover",
                "retryable":false,"internal":false,"httpStatusCode":400}),
            );
        } else {
            let mut body = json!({"success":true});
            if let Some(message) = message {
                body["message"] = json!(message);
            }
            then.status(200).json_body(body);
        }
    });
    let config = TempDir::new().unwrap();
    let checkout = TempDir::new().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_alien"));
    command
        .current_dir(checkout.path())
        .args(["deployments", "retry", "dep_test"])
        .env("XDG_CONFIG_HOME", config.path())
        .env("ALIEN_MANAGER_URL", manager.base_url())
        .env("ALIEN_API_KEY", "test-key")
        .env("NO_COLOR", "1");
    for key in [
        "ALIEN_PROJECT",
        "ALIEN_BASE_URL",
        "ALIEN_WORKSPACE",
        "ALIEN_LOG",
        "ALIEN_LOG_FILE",
        "RUST_LOG",
    ] {
        command.env_remove(key);
    }
    if json_output {
        command.arg("--json");
    }
    let output = command.output().unwrap();
    read.assert_hits(1);
    write.assert_hits(1);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    if refused {
        assert!(!output.status.success(), "{stdout} {stderr}");
        assert!(!stdout.contains("Retry requested."), "{stdout}");
        assert!(
            stderr.contains("Pending work cannot safely recover"),
            "{stderr}"
        );
    } else {
        let expected_message = message.filter(|message| !message.trim().is_empty());
        assert!(output.status.success(), "{stdout} {stderr}");
        if json_output {
            let response: Value = serde_json::from_str(&stdout).unwrap();
            assert_eq!(response["deploymentId"], "dep_test");
            assert_eq!(response["action"], "retry");
            assert_eq!(response["accepted"], true);
            if let Some(message) = expected_message {
                assert_eq!(response["message"], message);
            } else {
                assert!(response.get("message").is_none(), "{response}");
            }
        } else {
            assert!(
                stdout.contains(expected_message.unwrap_or("Retry requested.")),
                "{stdout}"
            );
            assert!(
                stdout.contains("alien deployments get dep_test"),
                "{stdout}"
            );
        }
    }
}

#[test]
fn accepted_retry_prints_server_guidance() {
    retry(Some(MESSAGE), false, false);
}
#[test]
fn accepted_retry_json_preserves_server_guidance() {
    retry(Some(MESSAGE), true, false);
}
#[test]
fn older_manager_without_guidance_still_works() {
    for json in [false, true] {
        retry(None, json, false);
    }
}
#[test]
fn refused_retry_keeps_the_error() {
    retry(None, false, true);
}

#[test]
fn blank_guidance_uses_the_existing_fallback() {
    for message in ["", " \t\n "] {
        for json in [false, true] {
            retry(Some(message), json, false);
        }
    }
}
