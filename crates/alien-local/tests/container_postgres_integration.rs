//! Integration test: a local Docker container reaching the embedded local Postgres.
//!
//! Boots pg through `LocalPostgresManager` (which now listens on loopback + the docker bridge), starts
//! a real `postgres:16-alpine` container on the shared `deployment-network`, and proves the two things
//! the design rests on: (1) a same-stack container CAN connect to pg over `host.docker.internal`, and
//! (2) pg is still NOT reachable from outside the stack (its port is refused on the host's LAN
//! interface, and a wrong password is rejected — scram, not trust).
//!
//! `#[ignore]`d: it needs Docker AND a reachable pgvector host (`ALIEN_PGVECTOR_RELEASES_URL`), and it
//! must actually connect — so it can never report a green pass without doing the real thing. Run it in
//! the e2e job on Linux:
//!   ALIEN_PGVECTOR_RELEASES_URL=https://releases.alien.dev/pgvector \
//!     cargo test -p alien-local --test container_postgres_integration -- --ignored --nocapture

use std::collections::HashMap;
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::process::Command;
use std::time::Duration;

use alien_core::bindings::{BindingValue, PostgresBinding};
use alien_local::{ContainerConfig, LocalContainerManager, LocalPostgresManager};
use tempfile::TempDir;

const PG_ID: &str = "e2e-container-pg";
const CLIENT_ID: &str = "e2e-pgclient";
const CLIENT_IMAGE: &str = "postgres:16-alpine";

/// Extracts the concrete connection fields from a Local Postgres binding, then rebuilds the URL against
/// `host.docker.internal` (the address a container reaches the host at) instead of the binding's
/// loopback host. Returns `(url, port)`.
fn container_connection_url(
    binding: &PostgresBinding,
    password_override: Option<&str>,
) -> (String, u16) {
    let PostgresBinding::Local(local) = binding else {
        panic!("expected a Local Postgres binding");
    };
    let BindingValue::Value(port) = &local.port else {
        panic!("port is a concrete value");
    };
    let BindingValue::Value(database) = &local.database else {
        panic!("database is a concrete value");
    };
    let BindingValue::Value(username) = &local.username else {
        panic!("username is a concrete value");
    };
    let password = password_override.unwrap_or(&local.password);
    let url = format!(
        "postgresql://{username}:{password}@host.docker.internal:{port}/{database}?sslmode=disable"
    );
    (url, *port)
}

/// The host's routable (non-loopback) IP, via the default-route trick — no packets are sent.
fn host_lan_ip() -> std::net::IpAddr {
    let udp = UdpSocket::bind("0.0.0.0:0").expect("bind udp");
    udp.connect("8.8.8.8:80").expect("pick default route");
    udp.local_addr().expect("local addr").ip()
}

#[tokio::test]
#[ignore = "needs Docker + a reachable ALIEN_PGVECTOR_RELEASES_URL; run explicitly in the e2e job"]
async fn local_container_reaches_local_postgres_but_not_the_outside() {
    // Reaching here means the test was invoked explicitly (it is `#[ignore]`d), so assert its
    // preconditions loudly rather than skipping.
    std::env::var("ALIEN_PGVECTOR_RELEASES_URL")
        .expect("ALIEN_PGVECTOR_RELEASES_URL must point at a host serving pgvector_compiled.zip");

    let temp_dir = TempDir::new().expect("temp dir");
    let (_shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);
    let (pg_manager, _monitor) =
        LocalPostgresManager::new_with_shutdown(temp_dir.path().to_path_buf(), shutdown_rx);

    pg_manager
        .start_postgres(PG_ID, "16")
        .await
        .expect("start_postgres should boot pg listening on loopback + the docker bridge");
    let binding = pg_manager
        .get_binding(PG_ID)
        .expect("get_binding should read the persisted metadata");

    // Pull the client image explicitly — the local container manager does not pull registry images.
    let pull = Command::new("docker")
        .args(["pull", CLIENT_IMAGE])
        .output()
        .expect("docker pull should run");
    assert!(
        pull.status.success(),
        "docker pull {CLIENT_IMAGE} failed: {}",
        String::from_utf8_lossy(&pull.stderr)
    );

    let container_manager =
        LocalContainerManager::new(temp_dir.path().to_path_buf()).expect("container manager");
    // A long-lived container on `deployment-network` we can `docker exec` into.
    container_manager
        .start_container(
            CLIENT_ID,
            ContainerConfig {
                image: CLIENT_IMAGE.to_string(),
                command: Some(vec!["sleep".to_string(), "300".to_string()]),
                ports: vec![],
                expose_public: false,
                env_vars: HashMap::new(),
                stateful: false,
                ordinal: None,
                volume_mount: None,
                volume_size: None,
                bind_mounts: vec![],
            },
        )
        .await
        .expect("start_container should launch the client on deployment-network");

    let docker_name = format!("alien-{CLIENT_ID}");
    let (good_url, pg_port) = container_connection_url(&binding, None);

    // (1) POSITIVE — a same-stack container connects to pg over host.docker.internal.
    let ok = Command::new("docker")
        .args(["exec", &docker_name, "psql", &good_url, "-tAc", "SELECT 1"])
        .output()
        .expect("docker exec psql should run");
    assert!(
        ok.status.success(),
        "container should connect to pg; stderr: {}",
        String::from_utf8_lossy(&ok.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&ok.stdout).trim(),
        "1",
        "SELECT 1 should return 1 from inside the container"
    );

    // (2) NEGATIVE-AUTH — wrong password is refused (proves scram auth, not trust).
    let (bad_url, _) = container_connection_url(&binding, Some("definitely-not-the-password"));
    let bad = Command::new("docker")
        .args(["exec", &docker_name, "psql", &bad_url, "-tAc", "SELECT 1"])
        .output()
        .expect("docker exec psql (bad pw) should run");
    assert!(!bad.status.success(), "wrong password must be rejected");
    let bad_err = String::from_utf8_lossy(&bad.stderr).to_lowercase();
    assert!(
        bad_err.contains("authentication") || bad_err.contains("password"),
        "rejection should be an auth failure, got: {bad_err}"
    );

    // (3) NEGATIVE-NETWORK — pg is NOT reachable on the host's LAN interface (only loopback + docker0).
    let lan_ip = host_lan_ip();
    assert!(
        !lan_ip.is_loopback(),
        "need a routable interface to prove LAN is refused; got {lan_ip}"
    );
    let lan_refused =
        TcpStream::connect_timeout(&SocketAddr::new(lan_ip, pg_port), Duration::from_secs(2))
            .is_err();
    assert!(
        lan_refused,
        "pg must not be reachable on the LAN interface {lan_ip}:{pg_port}"
    );

    // Cleanup.
    container_manager
        .stop_container(CLIENT_ID)
        .await
        .expect("stop_container should tear down the client");
    pg_manager
        .delete_postgres(PG_ID)
        .await
        .expect("delete_postgres should tear pg down cleanly");
}
