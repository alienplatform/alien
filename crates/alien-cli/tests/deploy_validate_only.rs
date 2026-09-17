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
