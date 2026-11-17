//! Runs the Python tests of the CNI probe's client pods (timing of policy enforcement).

use std::path::Path;
use std::process::Command;

#[test]
fn probe_client_waits_for_enforcement_and_still_fails_without_it() {
    let output = Command::new("python3")
        .args(["-m", "unittest", "test_client", "test_wait"])
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cni_probe"))
        .output()
        .expect("python3 runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
