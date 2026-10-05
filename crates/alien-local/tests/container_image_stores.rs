//! Loading local OCI archives on both Docker image stores. The classic store
//! identifies a loaded image by its config digest, the containerd store by the
//! manifest digest in the archive's `index.json`. Each store runs in its own
//! disposable `docker:28-dind` daemon, so the test does not depend on how the
//! host's Docker is configured.
//!
//! Needs a host Docker that can run privileged containers:
//! cargo nextest run -p alien-local --test container_image_stores --run-ignored all

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use alien_local::{ContainerConfig, LocalContainerManager};
use dockdash::{Arch, Image};
use tempfile::TempDir;
use tokio::net::{TcpStream, UnixListener};

const DIND_IMAGE: &str = "docker:28-dind";
const CONTAINERD_SNAPSHOTTER: &str = "io.containerd.snapshotter.v1";

/// Runs `docker` against `host`, or against the host's own daemon when `None`.
fn docker(host: Option<&str>, args: &[&str]) -> String {
    let mut command = Command::new("docker");
    command.args(args).env_remove("DOCKER_CONTEXT");
    match host {
        Some(host) => command.env("DOCKER_HOST", host),
        None => command.env_remove("DOCKER_HOST"),
    };
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "docker {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

/// A disposable Docker daemon, removed on drop (including on test failure).
struct Dind {
    container: String,
    host: String,
}

impl Dind {
    fn start(containerd_store: bool) -> Self {
        let mut args = vec![
            "run",
            "--detach",
            "--privileged",
            "--env",
            "DOCKER_TLS_CERTDIR=",
            "--publish",
            "127.0.0.1::2375",
            DIND_IMAGE,
            "--host=tcp://0.0.0.0:2375",
        ];
        if containerd_store {
            args.extend(["--feature", "containerd-snapshotter"]);
        }
        let container = docker(None, &args);
        let port = docker(None, &["port", &container, "2375/tcp"]);
        let port = port.lines().next().unwrap().rsplit(':').next().unwrap();
        let dind = Self {
            host: format!("tcp://127.0.0.1:{port}"),
            container,
        };

        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let ready = Command::new("docker")
                .args(["version", "--format", "{{.Server.Version}}"])
                .env("DOCKER_HOST", &dind.host)
                .env_remove("DOCKER_CONTEXT")
                .output()
                .unwrap()
                .status
                .success();
            if ready {
                break;
            }
            assert!(Instant::now() < deadline, "dind daemon did not start");
            std::thread::sleep(Duration::from_millis(500));
        }
        dind
    }
}

impl Drop for Dind {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "--force", "--volumes", &self.container])
            .env_remove("DOCKER_HOST")
            .env_remove("DOCKER_CONTEXT")
            .output();
    }
}

/// Serves the dind daemon's TCP API on a unix socket. The manager's Docker
/// client only honors `unix://` values of DOCKER_HOST.
fn forward_unix_socket(socket: &Path, tcp_host: &str) {
    let listener = UnixListener::bind(socket).unwrap();
    let addr = tcp_host.trim_start_matches("tcp://").to_string();
    tokio::spawn(async move {
        loop {
            let (mut client, _) = listener.accept().await.unwrap();
            let addr = addr.clone();
            tokio::spawn(async move {
                let mut daemon = TcpStream::connect(&addr).await.unwrap();
                // Either side closing ends the connection.
                let _ = tokio::io::copy_bidirectional(&mut client, &mut daemon).await;
            });
        }
    });
}

fn config(image: String) -> ContainerConfig {
    ContainerConfig {
        image,
        command: None,
        ports: vec![],
        public_endpoint: None,
        health_check_port: None,
        env_vars: HashMap::new(),
        stateful: false,
        ordinal: None,
        volume_mount: None,
        volume_size: None,
        bind_mounts: vec![],
        proxy_token: None,
    }
}

/// Two releases built under the same tag must each start, and the second
/// must run its own content rather than whatever the tag pointed at before.
async fn repeated_tag_runs_latest_release(containerd_store: bool) {
    let dind = Dind::start(containerd_store);
    let driver_status = docker(Some(&dind.host), &["info", "--format", "{{.DriverStatus}}"]);
    assert_eq!(
        driver_status.contains(CONTAINERD_SNAPSHOTTER),
        containerd_store,
        "unexpected image store: {driver_status}"
    );

    // The manager and its `docker load` read DOCKER_HOST, so point them at
    // the disposable daemon. This binary runs a single test, so nothing else
    // observes the change.
    let temp = TempDir::new().unwrap();
    let socket = temp.path().join("docker.sock");
    forward_unix_socket(&socket, &dind.host);
    std::env::set_var("DOCKER_HOST", format!("unix://{}", socket.display()));
    std::env::remove_var("DOCKER_CONTEXT");

    let manager = LocalContainerManager::new(temp.path().join("containers")).unwrap();
    let arch = match docker(Some(&dind.host), &["info", "--format", "{{.Architecture}}"]).as_str() {
        "aarch64" => Arch::ARM64,
        _ => Arch::Amd64,
    };

    let mut loaded_images = Vec::new();
    for version in ["first", "second"] {
        let (image, _) = Image::builder()
            .from("alpine:3.20")
            .platform("linux", &arch)
            .cmd(vec![
                "sh".to_string(),
                "-c".to_string(),
                format!("echo {version} >/version; exec sleep 300"),
            ])
            .output_name_and_tag("local-image-store-test/default:latest")
            .output_to(temp.path().join(format!("{version}.oci.tar")))
            .build()
            .await
            .unwrap();

        let id = format!("image-store-{version}");
        let info = manager
            .start_container(&id, config(image.path().to_str().unwrap().to_string()))
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "{version} release must start (containerd store: {containerd_store}): {error}"
                )
            });
        assert_eq!(
            docker(
                Some(&dind.host),
                &["exec", &info.docker_container_id, "cat", "/version"]
            ),
            version
        );
        loaded_images.push(docker(
            Some(&dind.host),
            &[
                "inspect",
                "--format",
                "{{.Image}}",
                &info.docker_container_id,
            ],
        ));
        manager.delete_container(&id).await.unwrap();
    }
    assert_ne!(
        loaded_images[0], loaded_images[1],
        "each release must run from its own immutable image"
    );

    std::env::remove_var("DOCKER_HOST");
}

// Multi-threaded: the blocking `docker` calls must not starve the socket forwarder.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Docker with privileged containers"]
async fn repeated_tag_runs_latest_release_on_both_image_stores() {
    repeated_tag_runs_latest_release(true).await;
    repeated_tag_runs_latest_release(false).await;
}
