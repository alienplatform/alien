use alien_cli::Cli;
use clap::Parser;
#[cfg(feature = "platform")]
use httpmock::MockServer;
use std::process::Command;

#[test]
fn release_and_token_subcommands_recognize_json_mode() {
    for prefix in [vec!["alien", "releases"], vec!["alien", "dev", "releases"]] {
        for args in [
            vec!["ls"],
            vec!["get", "rel_test"],
            vec!["promote", "rel_test"],
            vec!["channels"],
            vec!["create-channel", "preview"],
        ] {
            let mut command = prefix.clone();
            command.extend(args);
            assert!(!Cli::try_parse_from(&command).unwrap().wants_json_output());
            command.push("--json");
            assert!(Cli::try_parse_from(&command).unwrap().wants_json_output());
        }
    }
    for args in [
        vec!["alien", "tokens", "ls"],
        vec!["alien", "tokens", "create", "--tunnel"],
    ] {
        assert!(!Cli::try_parse_from(&args).unwrap().wants_json_output());
        let mut args = args;
        args.push("--json");
        assert!(Cli::try_parse_from(&args).unwrap().wants_json_output());
    }
}

#[test]
#[cfg(feature = "platform")]
fn release_api_failure_ends_with_one_json_error_line() {
    let api = MockServer::start();
    let denied = api.mock(|when, then| {
        when.any_request();
        then.status(401).json_body(serde_json::json!({
            "code": "UNAUTHORIZED", "message": "Test request denied",
            "retryable": false, "internal": false, "httpStatusCode": 401
        }));
    });
    let config = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_alien"))
        .args([
            "--base-url",
            &api.base_url(),
            "--workspace",
            "acme",
            "--project",
            "acme",
            "releases",
            "ls",
            "--json",
        ])
        .current_dir(config.path())
        .env("XDG_CONFIG_HOME", config.path())
        .env("ALIEN_API_KEY", "test-key")
        .env_remove("ALIEN_MANAGER_URL")
        .env_remove("RUST_LOG")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    let error: serde_json::Value = serde_json::from_str(stderr.lines().last().unwrap()).unwrap();
    assert!(error["code"].is_string(), "{stderr}");
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(denied.hits() > 0);
}

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
