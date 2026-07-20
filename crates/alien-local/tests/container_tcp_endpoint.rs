//! Real Docker coverage for a Local TCP public endpoint.
//!
//! Run on Linux with:
//! `cargo nextest run -p alien-local --test container_tcp_endpoint --run-ignored all`

#![cfg(target_os = "linux")]

use std::collections::HashMap;
use std::io::Read;
use std::net::{SocketAddr, TcpStream};
use std::process::Command;
use std::time::{Duration, Instant};

use alien_core::ExposeProtocol;
use alien_local::{ContainerConfig, LocalContainerManager, LocalPublicEndpoint};
use tempfile::TempDir;

const IMAGE: &str = "alpine:3.20";
const BACKEND_PORT: u16 = 5432;

fn connect_when_ready(address: SocketAddr) -> TcpStream {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match TcpStream::connect_timeout(&address, Duration::from_millis(250)) {
            Ok(stream) => return stream,
            Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => panic!("TCP endpoint {address} did not become ready: {error}"),
        }
    }
}

#[tokio::test]
#[ignore = "needs a Linux Docker daemon; run explicitly in Local E2E"]
async fn publishes_declared_tcp_backend_and_reports_tcp_binding() {
    let pull = Command::new("docker")
        .args(["pull", IMAGE])
        .output()
        .expect("docker pull should run");
    assert!(
        pull.status.success(),
        "docker pull failed: {}",
        String::from_utf8_lossy(&pull.stderr)
    );

    let temp_dir = TempDir::new().expect("state temp dir");
    let manager =
        LocalContainerManager::new(temp_dir.path().to_path_buf()).expect("container manager");
    let container_id = format!("tcp-endpoint-e2e-{}", std::process::id());
    manager
        .delete_container_and_storage(&container_id)
        .await
        .expect("pre-test cleanup should be idempotent");

    let info = manager
        .start_container(
            &container_id,
            ContainerConfig {
                image: IMAGE.to_string(),
                command: Some(vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    format!("while true; do printf tcp-response | nc -l -p {BACKEND_PORT}; done"),
                ]),
                // The endpoint is deliberately not the first declared port.
                // This proves publication follows endpoint config rather than
                // relying on list order.
                ports: vec![8080, BACKEND_PORT],
                public_endpoint: Some(LocalPublicEndpoint {
                    port: BACKEND_PORT,
                    protocol: ExposeProtocol::Tcp,
                    names: vec!["database".to_string()],
                }),
                env_vars: HashMap::new(),
                stateful: false,
                ordinal: None,
                volume_mount: None,
                volume_size: None,
                bind_mounts: vec![],
                proxy_token: None,
            },
        )
        .await
        .expect("start TCP container");

    let host_port = info
        .host_port
        .expect("TCP endpoint should have a host port");
    assert_eq!(
        info.public_endpoint.as_ref().map(|endpoint| endpoint.port),
        Some(BACKEND_PORT)
    );
    let mut stream = connect_when_ready(SocketAddr::from(([127, 0, 0, 1], host_port)));
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("read TCP response");
    assert_eq!(response, "tcp-response");

    let binding = manager
        .get_binding(&container_id)
        .await
        .expect("read Local container binding");
    let encoded = serde_json::to_value(binding).expect("serialize binding");
    assert_eq!(encoded["service"], "local");
    assert_eq!(
        encoded["internalUrl"],
        format!("tcp://{container_id}.svc:{BACKEND_PORT}")
    );
    assert_eq!(encoded["publicUrl"], format!("tcp://localhost:{host_port}"));

    manager
        .delete_container_and_storage(&container_id)
        .await
        .expect("delete TCP container");
}
