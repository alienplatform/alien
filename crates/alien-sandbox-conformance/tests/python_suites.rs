//! Runs the Python runner, log-verification and CNI listener tests, which are otherwise
//! only run by hand.

use std::path::Path;
use std::process::Command;

fn unittest(directory: &str, modules: &[&str]) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("python3")
        .args(["-m", "unittest"])
        .args(modules)
        // Keep bytecode caches out of the source tree.
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .current_dir(root.join(directory))
        .output()
        .expect("python3 runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn after_runner_and_log_verification() {
    unittest("after", &["test_after", "test_identity_map"]);
}

#[test]
fn cni_probe_listener() {
    unittest("tests", &["test_cni_probe"]);
}

#[test]
fn metadata_credential_response_controls() {
    unittest("entrypoint", &["test_metadata"]);
}
