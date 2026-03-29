use snapbox::cmd::Command;
use std::fs;
use tempfile::TempDir;

// Re-use the shared test utilities from the main crate
#[path = "../src/test_utils.rs"]
mod test_utils;

use test_utils::get_alien_cli_binary;

#[test]
fn test_unlink_force_removes_project_link() {
    let temp_dir = TempDir::new().unwrap();
    let alien_dir = temp_dir.path().join(".alien");
    fs::create_dir_all(&alien_dir).unwrap();
    fs::write(
        alien_dir.join("project.json"),
        r#"{
  "workspace": "demo-workspace",
  "projectId": "project_123",
  "projectName": "demo-project",
  "rootDirectory": null
}"#,
    )
    .unwrap();

    Command::new(get_alien_cli_binary())
        .current_dir(temp_dir.path())
        .arg("unlink")
        .arg("--force")
        .env_remove("RUST_LOG")
        .assert()
        .success();

    assert!(!alien_dir.join("project.json").exists());
}
