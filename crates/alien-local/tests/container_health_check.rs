//! Real Docker coverage for a Local container health-check port.
//!
//! Run with a Docker daemon using:
//! `cargo nextest run -p alien-local --test container_health_check --run-ignored all`

use std::collections::HashMap;
use std::process::Command;
use std::time::{Duration, Instant};

use alien_local::{ContainerConfig, LocalContainerManager};
use tempfile::TempDir;

const IMAGE: &str = "nginx:1.27-alpine";
const HEALTH_PORT: u16 = 80;

#[tokio::test]
#[ignore = "needs a Docker daemon; run explicitly in Local E2E"]
async fn probes_an_internal_only_health_port_through_loopback() {
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
    let container_id = format!("health-check-e2e-{}", std::process::id());
    manager
        .delete_container_and_storage(&container_id)
        .await
        .expect("pre-test cleanup should be idempotent");

    let info = manager
        .start_container(
            &container_id,
            ContainerConfig {
                image: IMAGE.to_string(),
                command: None,
                ports: vec![HEALTH_PORT],
                public_endpoint: None,
                health_check_port: Some(HEALTH_PORT),
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
        .expect("start HTTP container");
    assert!(
        info.host_port.is_none(),
        "health port must not become public"
    );
    assert!(
        info.health_host_port.is_some(),
        "health port must be probeable"
    );

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match manager
            .check_health(
                &container_id,
                Some("HEAD"),
                Some("/"),
                Duration::from_secs(1),
            )
            .await
        {
            Ok(restart_count) => {
                assert_eq!(restart_count, 0);
                break;
            }
            Err(_) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(100)).await
            }
            Err(error) => panic!("container did not become healthy: {error}"),
        }
    }

    assert!(manager
        .check_health(
            &container_id,
            Some("GET"),
            Some("/missing"),
            Duration::from_secs(1),
        )
        .await
        .is_err());

    manager
        .delete_container_and_storage(&container_id)
        .await
        .expect("delete HTTP container");
}
