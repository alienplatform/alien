use std::process::Command;

#[test]
fn process_failure_is_reported_as_structured_json() {
    let output = Command::new(env!("CARGO_BIN_EXE_alien-worker-runtime"))
        .env("ALIEN_DEPLOYMENT_TYPE", "local")
        .args([
            "--bindings-address",
            "127.0.0.1:0",
            "--transport",
            "local",
            "--",
            "/definitely-not-a-real-alien-test-binary",
        ])
        .output()
        .expect("alien-worker-runtime should start");

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).expect("runtime stdout should be UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("runtime stderr should be UTF-8");
    assert!(!stdout.contains("AlienError {"));
    assert!(!stderr.contains("AlienError {"));

    let event = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|event| event["fields"]["error_code"] == "PROCESS_FAILED")
        .expect("runtime should emit a structured PROCESS_FAILED event");
    assert_eq!(
        event["fields"]["message"],
        "Process failed: Application exited with code 7"
    );

    let serialized = event["fields"]["alien_error"]
        .as_str()
        .expect("structured event should include serialized AlienError");
    let error: serde_json::Value =
        serde_json::from_str(serialized).expect("AlienError field should be valid JSON");
    assert_eq!(error["code"], "PROCESS_FAILED");
    assert_eq!(error["context"]["exit_code"], serde_json::Value::Null);
    assert!(error["context"]["message"]
        .as_str()
        .expect("process error should contain a message")
        .contains("Failed to start application"));
    assert_eq!(error["retryable"], false);
    assert_eq!(error["internal"], false);
    assert_eq!(error["httpStatusCode"], 500);
}
