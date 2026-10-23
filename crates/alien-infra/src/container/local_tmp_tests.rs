use super::*;
use std::{panic::AssertUnwindSafe, sync::Arc};

use crate::{core::controller_test::SingleControllerExecutor, ResourceController};
use futures::FutureExt;
use tokio::process::Command;

#[tokio::test]
#[ignore = "requires Docker CLI, OCI-capable Buildx, and registry access"]
async fn non_root_tmp_writes_survive_saved_health_and_ready_checkpoints() {
    let directory = tempfile::tempdir().unwrap();
    let id = format!("tmp-write-{}", std::process::id());
    let image = format!("alien-local-tmp-test:{}", std::process::id());
    tokio::fs::write(
        directory.path().join("Dockerfile"),
        "FROM busybox:1.37.0\nUSER 1001:1001\n",
    )
    .await
    .unwrap();
    let artifact = directory.path().join("image.tar");
    let output = format!("type=oci,dest={}", artifact.display());
    let build = Command::new("docker")
        .args([
            "buildx",
            "build",
            "--provenance=false",
            "--sbom=false",
            "--output",
            &output,
            "--tag",
            &image,
        ])
        .arg(directory.path())
        .output()
        .await
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let bindings = alien_local::LocalBindingsProvider::new(directory.path()).unwrap();
    let manager = bindings.container_manager().unwrap();
    let services = Arc::new(crate::DefaultPlatformServiceProvider::with_local_bindings(
        bindings,
    ));
    let tmp = manager.get_container_tmp_dir(&id);
    tokio::fs::create_dir_all(&tmp).await.unwrap();
    tokio::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o700))
        .await
        .unwrap();
    let config = Container::new(id.clone())
        .permissions("default-profile".to_string())
        .cpu(alien_core::ResourceSpec { min: "0.25".to_string(), desired: "0.25".to_string() })
        .memory(alien_core::ResourceSpec { min: "64Mi".to_string(), desired: "64Mi".to_string() })
        .code(ContainerCode::Image { image: artifact.to_string_lossy().into_owned() })
        .command(vec!["sh".to_string(), "-c".to_string(),
            "set -eu; test $(id -u) = 1001; mkdir -p /tmp/web; chmod 777 /tmp/web; echo owned >/tmp/owned; echo ready >/tmp/web/health; exec busybox httpd -f -p 8080 -h /tmp/web".to_string()])
        .health_check(alien_core::HealthCheck { path: "/health".to_string(), port: Some(8080), method: "GET".to_string(), timeout_seconds: 1, failure_threshold: 20 })
        .build();
    let client = alien_core::ClientConfig::Local {
        state_directory: directory.path().to_string_lossy().into_owned(),
    };
    let mut executor = SingleControllerExecutor::builder()
        .resource(config.clone())
        .controller(LocalContainerController::default())
        .platform(Platform::Local)
        .client_config(client.clone())
        .service_provider(services.clone())
        .real_delays()
        .build()
        .await
        .unwrap();
    let verification = AssertUnwindSafe(async {
        executor.step().await.unwrap();
        let controller = executor
            .internal_state::<LocalContainerController>()
            .unwrap();
        let docker_id = controller
            .container_info
            .as_ref()
            .unwrap()
            .docker_container_id
            .clone();
        let initial_write = Command::new("docker")
            .args([
                "exec",
                &docker_id,
                "sh",
                "-c",
                "p=$(mktemp /tmp/initial.XXXXXX) && rm $p",
            ])
            .output()
            .await
            .unwrap();
        assert!(
            initial_write.status.success(),
            "{}",
            String::from_utf8_lossy(&initial_write.stderr)
        );
        let checkpoint = serde_json::to_vec(controller).unwrap();

        // Old readiness checkpoints resume without running StartingContainer.
        tokio::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o700))
            .await
            .unwrap();
        executor = SingleControllerExecutor::builder()
            .resource(config)
            .controller(serde_json::from_slice::<LocalContainerController>(&checkpoint).unwrap())
            .platform(Platform::Local)
            .client_config(client)
            .service_provider(services)
            .real_delays()
            .build()
            .await
            .unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(
            executor
                .internal_state::<LocalContainerController>()
                .unwrap()
                .container_info
                .as_ref()
                .unwrap()
                .docker_container_id,
            docker_id
        );

        let peer_delete = Command::new("docker")
            .args([
                "exec",
                "--user",
                "1002:1002",
                &docker_id,
                "rm",
                "/tmp/owned",
            ])
            .output()
            .await
            .unwrap();
        assert!(
            !peer_delete.status.success(),
            "another uid must not remove a peer's temporary file"
        );
        let owner_read = Command::new("docker")
            .args(["exec", &docker_id, "cat", "/tmp/owned"])
            .output()
            .await
            .unwrap();
        assert!(owner_read.status.success());
        assert_eq!(owner_read.stdout, b"owned\n");

        // Already-running checkpoints must also repair directories from older managers.
        tokio::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o700))
            .await
            .unwrap();
        executor.step().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        let write = Command::new("docker")
            .args([
                "exec",
                &docker_id,
                "sh",
                "-c",
                "p=$(mktemp /tmp/check.XXXXXX) && echo verified >$p && cat $p && rm $p",
            ])
            .output()
            .await
            .unwrap();
        assert!(
            write.status.success(),
            "{}",
            String::from_utf8_lossy(&write.stderr)
        );
        assert_eq!(write.stdout, b"verified\n");
        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert!(!manager.container_exists(&id).await.unwrap());
        assert!(
            !tmp.exists(),
            "normal teardown must remove the temporary mount"
        );
    })
    .catch_unwind()
    .await;
    if manager.container_exists(&id).await.unwrap() {
        manager.delete_container_and_storage(&id).await.unwrap();
    }
    let removal = Command::new("docker")
        .args(["image", "rm", &image])
        .output()
        .await
        .unwrap();
    assert!(
        removal.status.success(),
        "{}",
        String::from_utf8_lossy(&removal.stderr)
    );
    if let Err(panic) = verification {
        std::panic::resume_unwind(panic);
    }
}
