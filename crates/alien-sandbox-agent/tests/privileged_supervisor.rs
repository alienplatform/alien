//! Actual netfilter and command behavior in a separate Docker network namespace.
use std::{path::Path, process::Command};

#[test]
#[ignore = "requires Linux Docker with NET_ADMIN and internet access; run explicitly for qualification"]
fn privileged_supervisor_enforces_ipv4_and_dns_and_preserves_image_identity() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/test-privileged-sandbox-agent.sh");
    let status = Command::new(script)
        .arg(env!("CARGO_BIN_EXE_alien-sandbox-agent"))
        .status()
        .expect("run Docker qualification");
    assert!(status.success(), "Docker qualification failed: {status}");
}
