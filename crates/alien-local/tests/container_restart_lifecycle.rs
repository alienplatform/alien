//! Real lifecycle coverage. Requires an isolated Docker daemon and an executable
//! ALIEN_TEST_DOCKER_RESTART that restarts only that daemon. Never use a shared daemon.

use std::collections::HashMap;
use std::process::Command;
use std::time::{Duration, Instant};

use alien_local::{ContainerConfig, LocalContainerManager};
use tempfile::TempDir;

fn docker(args: &[&str]) -> String {
    let result = Command::new("docker").args(args).output().expect("docker");
    assert!(result.status.success(), "{result:?}");
    String::from_utf8(result.stdout)
        .expect("UTF-8")
        .trim()
        .to_string()
}

fn config() -> ContainerConfig {
    ContainerConfig {
        image: "alpine:3.20".to_string(),
        command: Some(vec![
            "sh".into(),
            "-c".into(),
            "while [ ! -f /data/crash ]; do sleep 0.1; done; rm /data/crash; exit 1".into(),
        ]),
        ports: vec![],
        public_endpoint: None,
        health_check_port: None,
        env_vars: HashMap::new(),
        stateful: true,
        ordinal: Some(0),
        volume_mount: Some("/data".into()),
        volume_size: None,
        bind_mounts: vec![],
        proxy_token: None,
    }
}

#[tokio::test]
#[ignore = "requires an explicitly configured disposable Docker daemon restart executable"]
async fn crash_stop_daemon_restart_resume_update_and_destroy_preserve_data() {
    let host = std::env::var("DOCKER_HOST").expect("set DOCKER_HOST to an isolated daemon");
    assert!(host.starts_with("unix://"));
    assert_ne!(
        host, "unix:///var/run/docker.sock",
        "never restart the shared daemon"
    );
    let restart = std::env::var("ALIEN_TEST_DOCKER_RESTART")
        .expect("set ALIEN_TEST_DOCKER_RESTART to the isolated daemon restart executable");
    let state = TempDir::new().unwrap();
    let manager = LocalContainerManager::new(state.path().to_path_buf()).unwrap();
    let id = format!("restart-test-{}", std::process::id());
    let name = format!("alien-{id}");
    manager.start_container(&id, config()).await.unwrap();
    docker(&[
        "exec",
        &name,
        "sh",
        "-c",
        "printf sentinel > /data/value; touch /data/crash",
    ]);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let count = docker(&["inspect", "--format", "{{.RestartCount}}", &name]);
        if count.parse::<u64>().unwrap() > 0
            && docker(&["inspect", "--format", "{{.State.Running}}", &name]) == "true"
        {
            break;
        }
        assert!(Instant::now() < deadline, "crashed container must restart");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    manager.stop_container(&id).await.unwrap();
    assert_eq!(
        docker(&["inspect", "--format", "{{.State.Running}}", &name]),
        "false"
    );
    drop(manager);
    let output = Command::new(restart)
        .output()
        .expect("restart isolated daemon");
    assert!(output.status.success(), "{output:?}");
    let manager = LocalContainerManager::new(state.path().to_path_buf()).unwrap();
    assert_eq!(
        docker(&["inspect", "--format", "{{.State.Running}}", &name]),
        "false"
    );
    assert_eq!(
        docker(&[
            "run",
            "--rm",
            "--network",
            "none",
            "--volumes-from",
            &name,
            "alpine:3.20",
            "cat",
            "/data/value"
        ]),
        "sentinel"
    );
    assert!(manager
        .load_metadata()
        .await
        .unwrap()
        .iter()
        .any(|m| m.container_id == id));
    manager.resume_container(&id).await.unwrap();
    assert_eq!(docker(&["exec", &name, "cat", "/data/value"]), "sentinel");
    manager.delete_container(&id).await.unwrap();
    manager.start_container(&id, config()).await.unwrap();
    assert_eq!(docker(&["exec", &name, "cat", "/data/value"]), "sentinel");
    manager.delete_container_and_storage(&id).await.unwrap();
    let volume = format!("alien-{id}-data");
    let result = Command::new("docker")
        .args(["volume", "inspect", &volume])
        .output()
        .unwrap();
    assert!(
        !result.status.success(),
        "destroy must remove the named volume"
    );
}
