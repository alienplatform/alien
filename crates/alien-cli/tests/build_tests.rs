use snapbox::cmd::Command;
use tempfile::TempDir;

// Re-use the shared test utilities from the main crate
#[path = "../src/test_utils.rs"]
mod test_utils;

use test_utils::*;

#[test]
fn test_build_command_basic_platforms() {
    let temp_app = create_temp_alien_app(&create_basic_alien_ts());
    let temp_path = temp_app.path();

    // Test AWS platform
    let assert = Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("build")
        .arg("--platform")
        .arg("aws")
        .env_remove("RUST_LOG")
        .assert();

    let output = assert.get_output();
    assert!(output.status.success());
    let output_dir = temp_path.join(".alien").join("build").join("aws");
    assert!(
        output_dir.exists(),
        "AWS platform output directory should exist after successful build command"
    );

    // Test GCP platform
    let assert = Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("build")
        .arg("--platform")
        .arg("gcp")
        .env_remove("RUST_LOG")
        .assert();

    let output = assert.get_output();
    assert!(output.status.success());
    let output_dir = temp_path.join(".alien").join("build").join("gcp");
    assert!(
        output_dir.exists(),
        "GCP platform output directory should exist after successful build command"
    );

    // Test Azure platform
    let assert = Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("build")
        .arg("--platform")
        .arg("azure")
        .env_remove("RUST_LOG")
        .assert();

    let output = assert.get_output();
    assert!(output.status.success());
    let output_dir = temp_path.join(".alien").join("build").join("azure");
    assert!(
        output_dir.exists(),
        "Azure platform output directory should exist after successful build command"
    );
}

#[test]
fn test_build_command_with_aws_managing_account() {
    let temp_app = create_temp_alien_app(&create_basic_alien_ts());
    let temp_path = temp_app.path();

    let assert = Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("build")
        .arg("--platform")
        .arg("aws")
        .arg("--aws-managing-account-id")
        .arg("123456789012")
        .env_remove("RUST_LOG")
        .assert();

    let output = assert.get_output();
    assert!(output.status.success());
    let output_dir = temp_path.join(".alien").join("build").join("aws");
    assert!(
        output_dir.exists(),
        "AWS platform output directory should exist after successful build command"
    );
}

#[test]
fn test_build_command_registry_auth() {
    let temp_app = create_temp_alien_app(&create_basic_alien_ts());
    let temp_path = temp_app.path();

    // Build no longer accepts registry auth flags; ensure we error cleanly.
    let assert = Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("build")
        .arg("--platform")
        .arg("aws")
        .arg("--image-repo")
        .arg("myregistry.com/my-repo")
        .arg("--registry-auth")
        .arg("anonymous")
        .env_remove("RUST_LOG")
        .assert();

    let output = assert.get_output();
    assert!(!output.status.success());

    // Test basic auth (also rejected)
    let assert = Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("build")
        .arg("--platform")
        .arg("aws")
        .arg("--image-repo")
        .arg("myregistry.com/my-repo")
        .arg("--registry-auth")
        .arg("basic")
        .arg("--registry-username")
        .arg("testuser")
        .arg("--registry-password")
        .arg("testpass")
        .env_remove("RUST_LOG")
        .assert();

    let output = assert.get_output();
    assert!(!output.status.success());
}

#[test]
fn test_build_command_validation_errors() {
    let temp_app = create_temp_alien_app(&create_basic_alien_ts());
    let temp_path = temp_app.path();

    // Invalid platform
    Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("build")
        .arg("--platform")
        .arg("invalid-platform")
        .env_remove("RUST_LOG")
        .assert()
        .failure();

    // Missing username for basic auth
    Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("build")
        .arg("--platform")
        .arg("aws")
        .arg("--image-repo")
        .arg("myregistry.com/my-repo")
        .arg("--registry-auth")
        .arg("basic")
        .arg("--registry-password")
        .arg("testpass")
        .env_remove("RUST_LOG")
        .assert()
        .failure();

    // Missing password for basic auth
    Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("build")
        .arg("--platform")
        .arg("aws")
        .arg("--image-repo")
        .arg("myregistry.com/my-repo")
        .arg("--registry-auth")
        .arg("basic")
        .arg("--registry-username")
        .arg("testuser")
        .env_remove("RUST_LOG")
        .assert()
        .failure();

    // Invalid auth type
    Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("build")
        .arg("--platform")
        .arg("aws")
        .arg("--image-repo")
        .arg("myregistry.com/my-repo")
        .arg("--registry-auth")
        .arg("invalid-auth")
        .env_remove("RUST_LOG")
        .assert()
        .failure();
}

#[test]
fn test_build_command_no_config_file() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("build")
        .arg("--platform")
        .arg("aws")
        .env_remove("RUST_LOG")
        .assert()
        .failure();
}

#[test]
fn test_build_command_tty_environments() {
    let temp_app = create_temp_alien_app(&create_basic_alien_ts());
    let temp_path = temp_app.path();

    // Test with TTY
    let assert = Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("build")
        .arg("--platform")
        .arg("aws")
        .env("TERM", "xterm-256color")
        .env_remove("RUST_LOG")
        .assert();

    let output = assert.get_output();
    assert!(output.status.success());
    let output_dir = temp_path.join(".alien").join("build").join("aws");
    assert!(
        output_dir.exists(),
        "AWS platform output directory should exist after successful build command"
    );

    // Test without TTY
    let assert = Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("build")
        .arg("--platform")
        .arg("aws")
        .env_remove("TERM")
        .env_remove("RUST_LOG")
        .assert();

    let output = assert.get_output();
    assert!(output.status.success());
    let output_dir = temp_path.join(".alien").join("build").join("aws");
    assert!(
        output_dir.exists(),
        "AWS platform output directory should exist after successful build command"
    );
}

#[test]
fn test_build_command_custom_output_dir() {
    let temp_app = create_temp_alien_app(&create_basic_alien_ts());
    let temp_path = temp_app.path();
    let custom_output_dir = temp_path.join("custom-output");

    let assert = Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("build")
        .arg("--platform")
        .arg("aws")
        .arg("--output-dir")
        .arg(custom_output_dir.to_str().unwrap())
        .env_remove("RUST_LOG")
        .assert();

    let output = assert.get_output();
    assert!(output.status.success());
    assert!(
        custom_output_dir.exists(),
        "Custom output directory should exist after successful build command"
    );
}
