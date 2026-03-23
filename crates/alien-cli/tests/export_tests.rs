use snapbox::cmd::Command;
use std::fs;
use tempfile::TempDir;

// Re-use the shared test utilities from the main crate
#[path = "../src/test_utils.rs"]
mod test_utils;

use test_utils::*;

fn export_command_available() -> bool {
    let output = std::process::Command::new(get_alien_cli_binary())
        .arg("--help")
        .output();
    match output {
        Ok(output) => {
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("export")
        }
        Err(_) => false,
    }
}

/// Helper to build an app first (required before export)
fn build_app_for_platform(temp_path: &std::path::Path, platform: &str) {
    let assert = Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("build")
        .arg("--platform")
        .arg(platform)
        .env_remove("RUST_LOG")
        .assert();

    let output = assert.get_output();
    if !output.status.success() {
        eprintln!("STDOUT: {}", String::from_utf8_lossy(&output.stdout));
        eprintln!("STDERR: {}", String::from_utf8_lossy(&output.stderr));
    }
    assert!(
        output.status.success(),
        "Build command should succeed before export"
    );

    let output_dir = temp_path.join(".alien").join("build").join(platform);
    assert!(
        output_dir.exists(),
        "Platform output directory should exist after packaging"
    );
    assert!(
        output_dir.join("stack.json").exists(),
        "stack.json should exist after packaging"
    );
}

#[test]
fn test_export_alien_template_basic_platforms() {
    if !export_command_available() {
        eprintln!("Skipping export tests: 'alien export' command not available");
        return;
    }

    let temp_app = create_temp_alien_app(&create_basic_alien_ts());
    let temp_path = temp_app.path();

    // Test AWS platform
    build_app_for_platform(temp_path, "aws");

    let assert = Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("export")
        .arg("--local")
        .arg("alien")
        .arg("--platform")
        .arg("aws")
        .env_remove("RUST_LOG")
        .assert();

    let output = assert.get_output();
    assert!(output.status.success());

    // Verify output is valid JSON
    let stdout = String::from_utf8_lossy(&output.stdout);
    let _: serde_json::Value =
        serde_json::from_str(&stdout).expect("Export output should be valid JSON");

    // Test GCP platform
    build_app_for_platform(temp_path, "gcp");

    let assert = Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("export")
        .arg("--local")
        .arg("alien")
        .arg("--platform")
        .arg("gcp")
        .env_remove("RUST_LOG")
        .assert();

    let output = assert.get_output();
    assert!(output.status.success());

    // Verify output is valid JSON
    let stdout = String::from_utf8_lossy(&output.stdout);
    let _: serde_json::Value =
        serde_json::from_str(&stdout).expect("Export output should be valid JSON");

    // Test Azure platform
    build_app_for_platform(temp_path, "azure");

    let assert = Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("export")
        .arg("--local")
        .arg("alien")
        .arg("--platform")
        .arg("azure")
        .env_remove("RUST_LOG")
        .assert();

    let output = assert.get_output();
    assert!(output.status.success());

    // Verify output is valid JSON
    let stdout = String::from_utf8_lossy(&output.stdout);
    let _: serde_json::Value =
        serde_json::from_str(&stdout).expect("Export output should be valid JSON");
}

#[test]
fn test_export_alien_template_to_file() {
    if !export_command_available() {
        eprintln!("Skipping export tests: 'alien export' command not available");
        return;
    }

    let temp_app = create_temp_alien_app(&create_basic_alien_ts());
    let temp_path = temp_app.path();
    let output_file = temp_path.join("exported-stack.json");

    // Build first
    build_app_for_platform(temp_path, "aws");

    // Export to file
    let assert = Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("export")
        .arg("--local")
        .arg("--output")
        .arg(output_file.to_str().unwrap())
        .arg("alien")
        .arg("--platform")
        .arg("aws")
        .env_remove("RUST_LOG")
        .assert();

    let output = assert.get_output();
    assert!(output.status.success());
    assert!(output_file.exists(), "Output file should be created");

    // Verify file content is valid JSON
    let file_content = fs::read_to_string(&output_file).unwrap();
    let _: serde_json::Value =
        serde_json::from_str(&file_content).expect("Exported file should contain valid JSON");
}

#[test]
fn test_export_cloudformation_template_basic() {
    if !export_command_available() {
        eprintln!("Skipping export tests: 'alien export' command not available");
        return;
    }

    let temp_app = create_temp_alien_app(&create_basic_alien_ts());
    let temp_path = temp_app.path();

    // Build first for AWS
    build_app_for_platform(temp_path, "aws");

    // Export CloudFormation template
    let assert = Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("export")
        .arg("--local")
        .arg("cloudformation")
        .env_remove("RUST_LOG")
        .assert();

    let output = assert.get_output();
    if !output.status.success() {
        eprintln!("STDOUT: {}", String::from_utf8_lossy(&output.stdout));
        eprintln!("STDERR: {}", String::from_utf8_lossy(&output.stderr));
    }
    assert!(output.status.success());

    // Verify output contains CloudFormation structure
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Verify basic CloudFormation structure
    assert!(stdout.contains("Description"), "Should contain Description");
    assert!(
        stdout.contains("Resources"),
        "Should contain Resources section"
    );
    assert!(
        stdout.contains("Transform"),
        "Should contain Transform section"
    );
    assert!(
        stdout.contains("Parameters"),
        "Should contain Parameters section"
    );
    assert!(stdout.contains("Outputs"), "Should contain Outputs section");

    // Check for ManagingAccountId parameter (key feature for cross-account management)
    assert!(
        stdout.contains("ManagingAccountId"),
        "Should contain ManagingAccountId parameter"
    );

    // Check for management resources created by stack processing
    assert!(
        stdout.contains("ManagementRole"),
        "Should contain ManagementRole for cross-account access"
    );
    assert!(
        stdout.contains("ManagementServiceAccount"),
        "Should contain ManagementServiceAccount for permissions"
    );
    assert!(
        stdout.contains("ManagementRoleArn"),
        "Should contain ManagementRoleArn output"
    );

    // Check for specific resources based on our basic config
    assert!(
        stdout.contains("TestStorage"),
        "Should contain test storage resource"
    );
    assert!(
        stdout.contains("AWS::S3::Bucket"),
        "Should contain S3 bucket type"
    );
}

#[test]
fn test_export_cloudformation_with_all_resources() {
    if !export_command_available() {
        eprintln!("Skipping export tests: 'alien export' command not available");
        return;
    }

    let temp_app = create_temp_alien_app(&create_basic_alien_ts());
    let temp_path = temp_app.path();

    // Build first for AWS
    build_app_for_platform(temp_path, "aws");

    // Export CloudFormation template with all resources
    let assert = Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("export")
        .arg("--local")
        .arg("cloudformation")
        .arg("--all-resources")
        .env_remove("RUST_LOG")
        .assert();

    let output = assert.get_output();
    assert!(output.status.success());

    // Verify output contains CloudFormation structure with all resources
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Description"), "Should contain Description");
    assert!(
        stdout.contains("Resources"),
        "Should contain Resources section with all resources"
    );
    assert!(
        stdout.contains("Transform"),
        "Should contain Transform section"
    );
    assert!(
        stdout.contains("Parameters"),
        "Should contain Parameters section"
    );
    assert!(
        stdout.contains("ManagingAccountId"),
        "Should contain ManagingAccountId parameter"
    );

    // Should include resources from all lifecycles (Frozen, LiveOnSetup, Live)
    assert!(
        stdout.contains("TestStorage"),
        "Should contain test storage resource"
    );
    assert!(
        stdout.contains("ManagementRole"),
        "Should contain ManagementRole for cross-account access"
    );
    assert!(
        stdout.contains("ManagementServiceAccount"),
        "Should contain ManagementServiceAccount for permissions"
    );
}

#[test]
fn test_export_cloudformation_with_managing_account() {
    if !export_command_available() {
        eprintln!("Skipping export tests: 'alien export' command not available");
        return;
    }

    let temp_app = create_temp_alien_app(&create_basic_alien_ts());
    let temp_path = temp_app.path();

    // Build first for AWS
    build_app_for_platform(temp_path, "aws");

    // Export CloudFormation template with managing account ID
    let assert = Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("export")
        .arg("--local")
        .arg("cloudformation")
        .arg("--default-managing-account-id")
        .arg("123456789012")
        .env_remove("RUST_LOG")
        .assert();

    let output = assert.get_output();
    assert!(output.status.success());

    // Verify output contains CloudFormation structure
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Description"), "Should contain Description");
    assert!(
        stdout.contains("Resources"),
        "Should contain Resources section"
    );
    assert!(
        stdout.contains("Transform"),
        "Should contain Transform section"
    );
    assert!(
        stdout.contains("Parameters"),
        "Should contain Parameters section"
    );
    assert!(
        stdout.contains("ManagingAccountId"),
        "Should contain ManagingAccountId parameter"
    );

    // Should contain the default value when specified
    assert!(
        stdout.contains("123456789012"),
        "Should contain the specified managing account ID"
    );
}

#[test]
fn test_export_cloudformation_to_file() {
    if !export_command_available() {
        eprintln!("Skipping export tests: 'alien export' command not available");
        return;
    }

    let temp_app = create_temp_alien_app(&create_basic_alien_ts());
    let temp_path = temp_app.path();
    let output_file = temp_path.join("template.yaml");

    // Build first for AWS
    build_app_for_platform(temp_path, "aws");

    // Export CloudFormation template to file
    let assert = Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("export")
        .arg("--local")
        .arg("--output")
        .arg(output_file.to_str().unwrap())
        .arg("cloudformation")
        .env_remove("RUST_LOG")
        .assert();

    let output = assert.get_output();
    assert!(output.status.success());
    assert!(output_file.exists(), "Output file should be created");

    // Verify file content contains CloudFormation structure
    let file_content = fs::read_to_string(&output_file).unwrap();
    assert!(
        file_content.contains("Description"),
        "Should contain Description"
    );
    assert!(
        file_content.contains("Resources"),
        "Should contain Resources section"
    );
    assert!(
        file_content.contains("Transform"),
        "Should contain Transform section"
    );
    assert!(
        file_content.contains("Parameters"),
        "Should contain Parameters section"
    );
    assert!(
        file_content.contains("ManagingAccountId"),
        "Should contain ManagingAccountId parameter"
    );
}

#[test]
fn test_export_error_cases() {
    if !export_command_available() {
        eprintln!("Skipping export tests: 'alien export' command not available");
        return;
    }

    let temp_app = create_temp_app_dir("json");
    let temp_path = temp_app.path();

    // Test export without packaging first
    Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("export")
        .arg("--local")
        .arg("alien")
        .arg("--platform")
        .arg("aws")
        .env_remove("RUST_LOG")
        .assert()
        .failure();

    // Build first for further tests
    build_app_for_platform(temp_path, "aws");

    // Test CloudFormation export with invalid argument (platform not supported)
    Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("export")
        .arg("--local")
        .arg("cloudformation")
        .arg("--platform")
        .arg("gcp")
        .env_remove("RUST_LOG")
        .assert()
        .failure();

    // Test SaaS mode (not supported)
    Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("export")
        .arg("alien")
        .arg("--platform")
        .arg("aws")
        .env_remove("RUST_LOG")
        .assert()
        .failure();
}

#[test]
fn test_export_no_config_file() {
    if !export_command_available() {
        eprintln!("Skipping export tests: 'alien export' command not available");
        return;
    }

    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    // Test export without any config files
    Command::new(get_alien_cli_binary())
        .current_dir(temp_path)
        .arg("export")
        .arg("--local")
        .arg("alien")
        .arg("--platform")
        .arg("aws")
        .env_remove("RUST_LOG")
        .assert()
        .failure();
}

#[test]
fn test_export_different_config_types() {
    if !export_command_available() {
        eprintln!("Skipping export tests: 'alien export' command not available");
        return;
    }

    // Test with TypeScript config
    let temp_app_ts = create_temp_app_dir("ts");
    let temp_path_ts = temp_app_ts.path();

    build_app_for_platform(temp_path_ts, "aws");

    let assert = Command::new(get_alien_cli_binary())
        .current_dir(temp_path_ts)
        .arg("export")
        .arg("--local")
        .arg("alien")
        .arg("--platform")
        .arg("aws")
        .env_remove("RUST_LOG")
        .assert();

    let output = assert.get_output();
    assert!(output.status.success());

    // Test with JSON config
    let temp_app_json = create_temp_app_dir("json");
    let temp_path_json = temp_app_json.path();

    build_app_for_platform(temp_path_json, "aws");

    let assert = Command::new(get_alien_cli_binary())
        .current_dir(temp_path_json)
        .arg("export")
        .arg("--local")
        .arg("alien")
        .arg("--platform")
        .arg("aws")
        .env_remove("RUST_LOG")
        .assert();

    let output = assert.get_output();
    assert!(output.status.success());
}
