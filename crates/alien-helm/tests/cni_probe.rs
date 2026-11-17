//! Runs the Python tests of the network policy probe script (client verdicts, waits, time budget).

use std::path::Path;
use std::process::Command;

#[test]
fn probe_script_python_tests_pass() {
    let output = Command::new("python3")
        .args(["-m", "unittest", "discover", "-p", "test_*.py"])
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
