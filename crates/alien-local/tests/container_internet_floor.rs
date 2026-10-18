//! Actual Linux bridge traffic, host rules, process isolation and restart cleanup.
//! Run explicitly as a user with host CAP_NET_ADMIN and access to Docker.

use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::process::Command;

use alien_core::{ContainerEgress, ContainerSecurity, ContainerSecurityProfile};
use alien_local::{ContainerConfig, LocalContainerManager};
use futures::FutureExt;
use serde_json::{json, Value};
use tokio::net::TcpListener;

fn docker(args: &[&str]) -> String {
    let result = Command::new("docker").args(args).output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap().trim().into()
}

fn config() -> ContainerConfig {
    ContainerConfig {
        image: "python:3.13-slim".into(),
        command: Some(vec![
            "python".into(),
            "-m".into(),
            "http.server".into(),
            "8000".into(),
        ]),
        ports: vec![8000],
        public_endpoint: None,
        health_check_port: Some(8000),
        env_vars: HashMap::new(),
        stateful: false,
        ordinal: None,
        volume_mount: None,
        volume_size: None,
        bind_mounts: vec![],
        proxy_token: None,
        egress: None,
        security: None,
        egress_denied_ranges: vec![],
        memory_bytes: Some(128 * 1024 * 1024),
    }
}

fn connect(container: &str, host: &str, port: u16) -> bool {
    let target = json!([host, port]).to_string();
    let script = format!("import socket,json\ns=socket.socket();s.settimeout(2)\ntry:\n s.connect(tuple(json.loads({target:?})));print('yes')\nexcept OSError:\n print('no')");
    docker(&["exec", "--user", "0:0", container, "python", "-c", &script]) == "yes"
}

fn raw_frames_allowed(container: &str) -> bool {
    let script = "import socket\ntry:\n socket.socket(socket.AF_PACKET,socket.SOCK_RAW);print('yes')\nexcept PermissionError:\n print('no')";
    docker(&["exec", "--user", "0:0", container, "python", "-c", script]) == "yes"
}

fn inspect(container: &str) -> Value {
    serde_json::from_str::<Value>(&docker(&["inspect", container])).unwrap()[0].clone()
}

#[tokio::test]
#[ignore = "requires same-host Linux Docker, nftables and host CAP_NET_ADMIN"]
async fn internet_floor_precedes_start_survives_replacement_and_is_removed_after_docker() {
    let state = tempfile::tempdir().unwrap();
    let manager = LocalContainerManager::new(state.path().into()).unwrap();
    let prefix = format!("internet-floor-e2e-{}", std::process::id());
    let subject = format!("{prefix}-subject");
    let peer = format!("{prefix}-peer");
    let network_existed = Command::new("docker")
        .args(["network", "inspect", "deployment-network"])
        .output()
        .unwrap()
        .status
        .success();
    let listener = TcpListener::bind("0.0.0.0:0").await.unwrap();
    let host_port = listener.local_addr().unwrap().port();
    let host_server = tokio::spawn(async move {
        loop {
            let _connection = listener.accept().await.unwrap();
        }
    });
    let result = AssertUnwindSafe(async {
        let peer_info = manager.start_container(&peer, config()).await.unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            match manager
                .check_health(
                    &peer,
                    Some("GET"),
                    Some("/"),
                    std::time::Duration::from_secs(1),
                )
                .await
            {
                Ok(_) => break,
                Err(_) if std::time::Instant::now() < deadline => {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await
                }
                Err(error) => panic!("peer did not become ready: {error}"),
            }
        }
        let peer_address = inspect(&peer_info.docker_container_id)["NetworkSettings"]["Networks"]
            ["deployment-network"]["IPAddress"]
            .as_str()
            .unwrap()
            .to_owned();
        let initial = manager.start_container(&subject, config()).await.unwrap();
        let details = inspect(&initial.docker_container_id);
        let gateway = details["NetworkSettings"]["Networks"]["deployment-network"]["Gateway"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            connect(&initial.docker_container_id, &gateway, host_port),
            "positive host fixture"
        );
        assert!(
            connect(&initial.docker_container_id, &peer_address, 8000),
            "positive peer fixture"
        );
        assert!(
            connect(&initial.docker_container_id, "1.1.1.1", 443),
            "positive public control"
        );

        assert!(
            raw_frames_allowed(&initial.docker_container_id),
            "positive raw-frame control"
        );

        let mut restricted = config();
        restricted.egress = Some(ContainerEgress::Internet);
        restricted.security = Some(ContainerSecurity {
            profile: ContainerSecurityProfile::Restricted,
            run_as_user: 1000,
            run_as_group: 1000,
            read_only_root_filesystem: true,
        });
        let after = manager
            .start_container(&subject, restricted.clone())
            .await
            .unwrap();
        assert!(
            !connect(&after.docker_container_id, &gateway, host_port),
            "host is denied even to guest root"
        );
        assert!(
            !connect(&after.docker_container_id, &peer_address, 8000),
            "peer is denied even to guest root"
        );
        assert!(
            !connect(&after.docker_container_id, "169.254.169.254", 80),
            "metadata is denied"
        );
        assert!(
            connect(&after.docker_container_id, "1.1.1.1", 443),
            "public control remains available"
        );
        assert!(
            !raw_frames_allowed(&after.docker_container_id),
            "guest root cannot forge a different source MAC"
        );
        let details = inspect(&after.docker_container_id);
        assert_eq!(details["Config"]["User"], "1000:1000");
        assert_eq!(details["HostConfig"]["Memory"], 128 * 1024 * 1024);
        assert_eq!(details["HostConfig"]["ReadonlyRootfs"], true);
        assert_eq!(details["HostConfig"]["CapDrop"], json!(["ALL"]));
        assert_eq!(details["HostConfig"]["RestartPolicy"]["Name"], "no");
        assert_eq!(
            docker(&[
                "exec",
                &after.docker_container_id,
                "cat",
                "/proc/sys/net/ipv6/conf/all/disable_ipv6"
            ]),
            "1"
        );

        assert_eq!(
            docker(&[
                "exec",
                &after.docker_container_id,
                "cat",
                "/proc/sys/net/ipv6/conf/default/disable_ipv6"
            ]),
            "1"
        );
        let metadata_path = state
            .path()
            .join("containers")
            .join(&subject)
            .join("metadata.json");
        let initial_metadata: Value =
            serde_json::from_slice(&std::fs::read(&metadata_path).unwrap()).unwrap();
        let alias = state.path().join("alias");
        #[cfg(unix)]
        std::os::unix::fs::symlink(state.path(), &alias).unwrap();
        #[cfg(not(unix))]
        let alias = state.path().join(".");
        let restarted = LocalContainerManager::new(alias).unwrap();
        restricted.egress_denied_ranges.push("1.1.1.1/32".into());
        let replaced = restarted
            .start_container(&subject, restricted.clone())
            .await
            .unwrap();
        assert!(
            !connect(&replaced.docker_container_id, "1.1.1.1", 443),
            "a public deployment range is also denied after replacement"
        );
        let metadata: Value = serde_json::from_slice(
            &std::fs::read(
                state
                    .path()
                    .join("containers")
                    .join(&subject)
                    .join("metadata.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let table = metadata["internetFloor"]["table"].as_str().unwrap();
        assert_eq!(
            metadata["internetFloor"]["table"], initial_metadata["internetFloor"]["table"],
            "restarting through a state-directory alias retains firewall ownership"
        );
        let unrestricted = restarted.start_container(&subject, config()).await.unwrap();
        assert!(connect(
            &unrestricted.docker_container_id,
            &gateway,
            host_port
        ));
        assert!(connect(
            &unrestricted.docker_container_id,
            &peer_address,
            8000
        ));
        assert!(connect(&unrestricted.docker_container_id, "1.1.1.1", 443));
        assert!(
            !Command::new("nft")
                .args(["list", "table", "inet", table])
                .output()
                .unwrap()
                .status
                .success(),
            "removing the restriction retires its owned firewall"
        );
        restarted
            .start_container(&subject, restricted)
            .await
            .unwrap();
        restarted.delete_container(&subject).await.unwrap();
        assert!(
            !Command::new("nft")
                .args(["list", "table", "inet", table])
                .output()
                .unwrap()
                .status
                .success(),
            "owned firewall must be gone after Docker deletion"
        );
        restarted.delete_container(&subject).await.unwrap();
    })
    .catch_unwind()
    .await;
    // Cleanup also runs after a failed traffic assertion.
    let recovered = LocalContainerManager::new(state.path().into()).unwrap();
    let subject_cleanup = recovered.delete_container(&subject).await;
    let peer_cleanup = recovered.delete_container(&peer).await;
    host_server.abort();
    if !network_existed {
        docker(&["network", "rm", "deployment-network"]);
    }
    subject_cleanup.unwrap();
    peer_cleanup.unwrap();
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
