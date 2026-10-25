use std::process::Command;

#[test]
fn json_validation_error_is_one_stderr_document_with_empty_stdout() {
    let config = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_alien"))
        .args(["logs", "--limit", "0", "--json"])
        .current_dir(config.path())
        .env("XDG_CONFIG_HOME", config.path())
        .env_remove("RUST_LOG")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        output.stdout.is_empty(),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["code"], "VALIDATION_ERROR");
    assert_eq!(error["httpStatusCode"], 400);
    assert_eq!(error["context"]["field"], "limit");
}
