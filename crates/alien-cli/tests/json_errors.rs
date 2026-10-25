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
    assert_eq!(String::from_utf8_lossy(&output.stderr).lines().count(), 1);
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["code"], "VALIDATION_ERROR");
    assert_eq!(error["httpStatusCode"], 400);
    assert_eq!(error["context"]["field"], "limit");
}

#[test]
fn json_build_keeps_progress_on_stderr_and_ends_failure_with_one_error_line() {
    let project = tempfile::tempdir().unwrap();
    let config_path = project.path().join("alien.json");
    std::fs::write(
        &config_path,
        serde_json::to_vec(&alien_core::Stack::new("docproc".to_string()).build()).unwrap(),
    )
    .unwrap();

    for succeeds in [true, false] {
        if !succeeds {
            std::fs::write(&config_path, "{ invalid json }").unwrap();
        }
        let output = Command::new(env!("CARGO_BIN_EXE_alien"))
            .args(["build", "--platform", "local", "--json"])
            .current_dir(project.path())
            .env("XDG_CONFIG_HOME", project.path())
            .env_remove("RUST_LOG")
            .output()
            .unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert_eq!(output.status.success(), succeeds, "{stderr}");
        assert!(stderr.contains("Loading configuration"), "{stderr}");
        assert!(!stderr.contains('\u{1b}'), "{stderr}");
        assert!(!stderr.contains('\r'), "{stderr}");
        if succeeds {
            let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(result["platform"], "local");
        } else {
            assert!(output.stdout.is_empty());
            let error: serde_json::Value =
                serde_json::from_str(stderr.lines().last().unwrap()).unwrap();
            assert_eq!(error["code"], "CONFIGURATION_ERROR");
            assert_eq!(
                stderr
                    .lines()
                    .filter(|line| serde_json::from_str::<serde_json::Value>(line).is_ok())
                    .count(),
                1,
                "{stderr}"
            );
        }
    }
}
