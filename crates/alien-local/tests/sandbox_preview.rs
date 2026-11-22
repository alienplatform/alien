//! Real Docker coverage for previews behind the gateway: a session with a declared preview port
//! is reachable from the host through its relay, and still has no route out except through the
//! sandbox's gateway.
//!
//! Needs a gateway image that has the relay mode, named by `ALIEN_TEST_EGRESS_GATEWAY_IMAGE`:
//!
//! `ALIEN_TEST_EGRESS_GATEWAY_IMAGE=<image> cargo test -p alien-local --test sandbox_preview -- --ignored --test-threads=1`

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;

use alien_local::{LocalSandboxManager, SandboxEgressMode, SandboxOutput, SandboxSessionConfig};
use tempfile::TempDir;

const IMAGE: &str = "python:3.12-alpine";
const SANDBOX: &str = "preview-a";

fn config() -> SandboxSessionConfig {
    SandboxSessionConfig {
        image: IMAGE.to_string(),
        cpu_cores: 0.5,
        memory_bytes: 256 * 1024 * 1024,
        pids_limit: Some(64),
        scratch_bytes: 16 * 1024 * 1024,
        egress: SandboxEgressMode::Allow,
        preview_ports: vec![8080],
        env: HashMap::new(),
        links: Vec::new(),
    }
}

async fn run(manager: &LocalSandboxManager, container: &str, command: &str) -> (i64, String) {
    let result = manager
        .exec(
            container,
            &[
                "/bin/sh".to_string(),
                "-c".to_string(),
                format!("{command} 2>&1"),
            ],
        )
        .await
        .expect("exec");
    let text = result
        .output
        .iter()
        .map(|frame| match frame {
            SandboxOutput::Stdout(bytes) | SandboxOutput::Stderr(bytes) => {
                String::from_utf8_lossy(bytes).to_string()
            }
        })
        .collect();
    (result.exit_code, text)
}

#[tokio::test]
#[ignore = "requires a real Docker daemon and a gateway image with the relay mode"]
async fn a_preview_works_behind_the_gateway_and_the_session_has_no_other_way_out() {
    let image = std::env::var("ALIEN_TEST_EGRESS_GATEWAY_IMAGE")
        .expect("ALIEN_TEST_EGRESS_GATEWAY_IMAGE names the gateway image to test");
    let dir = TempDir::new().unwrap();
    let manager = LocalSandboxManager::new(dir.path().to_path_buf())
        .expect("Docker must be reachable")
        .with_gateway_image(image);
    manager.reap(SANDBOX).await.expect("clean slate");

    let session = manager
        .create_session(SANDBOX, "s1", &config())
        .await
        .expect("a session with a preview port and no links starts");
    let id = session.container_id.clone();

    // A server inside the session, on the declared port, for as long as the probe runs. An exec
    // ends its background children, so the server is an exec of its own.
    let server = run(
        &manager,
        &id,
        "cd /sandbox && echo preview-ok > index.html && timeout 12 python3 -m http.server 8080",
    );
    let probe = async {
        let address = manager
            .preview_address(&id, 8080)
            .await
            .expect("the declared port has a preview address");
        let host = address.trim_start_matches("http://").to_string();
        let mut body = String::new();
        for _ in 0..30 {
            if let Ok(mut stream) = TcpStream::connect(&host) {
                stream
                    .write_all(b"GET /index.html HTTP/1.0\r\nHost: preview\r\n\r\n")
                    .unwrap();
                body.clear();
                let _ = stream.read_to_string(&mut body);
                if body.contains("preview-ok") {
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
        body
    };
    let (_, body) = tokio::join!(server, probe);
    assert!(
        body.contains("preview-ok"),
        "preview through the relay: {body}"
    );

    // No route out except the gateway: a direct connection (no proxy) fails, and the session's
    // only proxy is its gateway.
    let (code, out) = run(
        &manager,
        &id,
        "python3 -c \"import socket; socket.create_connection(('1.1.1.1', 443), timeout=5)\"",
    )
    .await;
    assert_ne!(code, 0, "a direct connection must fail: {out}");
    let (_, env) = run(&manager, &id, "env").await;
    assert!(env.contains("HTTPS_PROXY=http://alien:"), "{env}");

    // Terminating the session removes its relay.
    manager.terminate(SANDBOX, "s1").await.expect("terminates");
    let relays = std::process::Command::new("docker")
        .args([
            "ps",
            "-a",
            "--filter",
            "label=dev.alien.sandbox.preview-relay",
            "-q",
        ])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&relays.stdout).trim().is_empty(),
        "the relay is removed with its session"
    );
    manager.reap(SANDBOX).await.expect("cleanup");
}
