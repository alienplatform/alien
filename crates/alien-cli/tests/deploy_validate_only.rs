use std::process::Command;

fn valid_config() -> tempfile::NamedTempFile {
    let file = tempfile::NamedTempFile::new().expect("create config file");
    std::fs::write(file.path(), "name = \"production\"\nplatform = \"aws\"\n")
        .expect("write config file");
    file
}

#[test]
fn validate_only_ignores_inherited_manager_without_credentials() {
    let config = valid_config();
    let output = Command::new(env!("CARGO_BIN_EXE_alien"))
        .args(["deploy", "--config"])
        .arg(config.path())
        .arg("--validate-only")
        .env("ALIEN_MANAGER_URL", "http://manager.invalid")
        .env_remove("ALIEN_API_KEY")
        .output()
        .expect("run alien");

    assert!(
        output.status.success(),
        "validate-only failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "Deployment config is valid."
    );
}

#[test]
fn validate_only_checks_supplied_token_files_without_authentication() {
    let config = valid_config();
    let directory = tempfile::tempdir().expect("token directory");
    let token = directory.path().join("token");
    for (contents, expected_error) in [
        (None, Some("Failed to read token file")),
        (Some(" \n\t"), Some("is empty")),
        (Some(" ax_test\n"), None),
    ] {
        if let Some(contents) = contents {
            std::fs::write(&token, contents).expect("write token fixture");
        }
        let output = Command::new(env!("CARGO_BIN_EXE_alien"))
            .args(["deploy", "--config"])
            .arg(config.path())
            .arg("--token-file")
            .arg(&token)
            .arg("--validate-only")
            .env("ALIEN_MANAGER_URL", "http://manager.invalid")
            .env_remove("ALIEN_API_KEY")
            .output()
            .expect("run offline validation");
        if let Some(message) = expected_error {
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains(message));
            assert!(!String::from_utf8_lossy(&output.stdout).contains("Deployment config is valid"));
        } else {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    let output = Command::new(env!("CARGO_BIN_EXE_alien"))
        .args(["deploy", "--config"])
        .arg(config.path())
        .arg("--token-file")
        .arg(&token)
        .args(["--validate-only", "--public-subdomain", "example"])
        .env("ALIEN_MANAGER_URL", "http://manager.invalid")
        .env_remove("ALIEN_API_KEY")
        .output()
        .expect("validate existing-token restrictions");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--public-subdomain is only supported")
    );
}
