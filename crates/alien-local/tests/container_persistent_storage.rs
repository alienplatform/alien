//! Real Docker coverage for Local container persistent storage.
//!
//! Run on Linux with:
//! `cargo nextest run -p alien-local --test container_persistent_storage --run-ignored all`

#![cfg(target_os = "linux")]

use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Output};

use alien_local::{BindMount, ContainerConfig, LocalContainerManager};
use tempfile::TempDir;

const IMAGE: &str = "alpine:3.20";

fn docker(args: &[&str]) -> Output {
    Command::new("docker")
        .args(args)
        .output()
        .expect("docker command should run")
}

fn assert_docker_success(output: Output, operation: &str) -> String {
    assert!(
        output.status.success(),
        "{operation} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("docker output should be UTF-8")
}

fn config(shared_dir: &Path) -> ContainerConfig {
    ContainerConfig {
        image: IMAGE.to_string(),
        command: Some(vec!["sleep".to_string(), "300".to_string()]),
        ports: vec![],
        public_endpoint: None,
        env_vars: HashMap::new(),
        stateful: true,
        ordinal: Some(0),
        volume_mount: Some("/data".to_string()),
        volume_size: Some("1Gi".to_string()),
        bind_mounts: vec![BindMount {
            host_path: shared_dir.to_path_buf(),
            container_path: "/shared".to_string(),
            resource_id: "shared-data".to_string(),
            shared_with_host_workloads: true,
        }],
        proxy_token: None,
    }
}

#[tokio::test]
#[ignore = "needs a Linux Docker daemon; run explicitly in Local E2E"]
async fn non_root_container_writes_persists_and_deletes_storage() {
    // This is specifically the Linux host-identity path. Running it as root
    // would exercise different product behavior and produce a false green.
    // SAFETY: geteuid/getegid are side-effect-free process identity queries.
    let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
    assert_ne!(uid, 0, "the test must run as a non-root operator");

    assert_docker_success(docker(&["pull", IMAGE]), "pull test image");

    let temp_dir = TempDir::new().expect("state temp dir");
    let shared_dir = temp_dir.path().join("shared");
    std::fs::create_dir(&shared_dir).expect("create shared bind directory");
    let manager =
        LocalContainerManager::new(temp_dir.path().to_path_buf()).expect("container manager");
    let container_id = format!("persistent-storage-e2e-{}", std::process::id());
    let docker_name = format!("alien-{container_id}");
    let old_volume_name = format!("alien-{container_id}-data");
    let storage_dir = temp_dir
        .path()
        .join("container-volumes")
        .join(&container_id);

    manager
        .delete_container_and_storage(&container_id)
        .await
        .expect("pre-test cleanup should be idempotent");

    manager
        .start_container(&container_id, config(&shared_dir))
        .await
        .expect("start non-root container with persistent storage");

    let configured_user = assert_docker_success(
        docker(&["inspect", "--format", "{{.Config.User}}", &docker_name]),
        "inspect container user",
    );
    assert_eq!(configured_user.trim(), format!("{uid}:{gid}"));

    assert_docker_success(
        docker(&[
            "exec",
            &docker_name,
            "sh",
            "-c",
            "printf persistent-value > /data/value",
        ]),
        "write persistent data as non-root",
    );
    assert_eq!(
        std::fs::read_to_string(storage_dir.join("value")).expect("host should read written data"),
        "persistent-value"
    );

    assert_docker_success(docker(&["restart", &docker_name]), "restart container");
    let after_restart = assert_docker_success(
        docker(&["exec", &docker_name, "cat", "/data/value"]),
        "read data after restart",
    );
    assert_eq!(after_restart, "persistent-value");

    // Updates replace the Docker container but retain resource-owned data.
    manager
        .delete_container(&container_id)
        .await
        .expect("remove container while preserving storage");
    manager
        .start_container(&container_id, config(&shared_dir))
        .await
        .expect("recreate container against existing storage");
    let after_replacement = assert_docker_success(
        docker(&["exec", &docker_name, "cat", "/data/value"]),
        "read data after replacement",
    );
    assert_eq!(after_replacement, "persistent-value");

    // Deleting the resource owns cleanup. Both the current directory backend
    // and the previous named-volume backend must be absent afterward.
    manager
        .delete_container_and_storage(&container_id)
        .await
        .expect("delete container and persistent storage");
    assert!(
        !storage_dir.exists(),
        "persistent directory should be deleted"
    );
    assert!(
        !docker(&["volume", "inspect", &old_volume_name])
            .status
            .success(),
        "old-format named volume should not remain"
    );
    manager
        .delete_container_and_storage(&container_id)
        .await
        .expect("repeated delete should remain idempotent");
}
