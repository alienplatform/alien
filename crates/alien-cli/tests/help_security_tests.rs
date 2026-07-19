use std::process::Command;

#[path = "../src/test_utils.rs"]
mod test_utils;

use test_utils::get_alien_cli_binary;

#[test]
fn api_key_environment_value_is_absent_from_subcommand_help() {
    const SENTINEL: &str = "sentinel-api-key-must-never-be-rendered";

    let output = Command::new(get_alien_cli_binary())
        .args(["projects", "--help"])
        .env("ALIEN_API_KEY", SENTINEL)
        .env_remove("RUST_LOG")
        .output()
        .expect("alien help subprocess should run");

    assert!(
        output.status.success(),
        "help should succeed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("help stdout should be UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("help stderr should be UTF-8");

    assert!(
        stdout.contains("ALIEN_API_KEY"),
        "the API key option should still document its environment variable"
    );
    assert!(!stdout.contains(SENTINEL), "help stdout leaked the API key");
    assert!(!stderr.contains(SENTINEL), "help stderr leaked the API key");
}
