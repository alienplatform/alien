//! Real Docker coverage for repeated OCI tags and host-loopback registries,
//! with and without a deployment token.
//! cargo nextest run -p alien-local --test container_image_releases --run-ignored all

use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;

use alien_local::{ContainerConfig, LocalBindingsProvider, LocalContainerManager};
use container_registry::auth::{Anonymous, Permissions};
use dockdash::{Arch, ClientProtocol, Image, PushOptions};
use tempfile::TempDir;

fn docker(args: &[&str]) -> String {
    let output = Command::new("docker").args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
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
        proxy_token: Some("local-test".to_string()),
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn repeated_archive_tag_runs_new_content_and_loopback_pull_uses_host() {
    let temp = TempDir::new().unwrap();
    let manager = LocalContainerManager::new(temp.path().join("containers")).unwrap();
    let id = format!("image-releases-{}", std::process::id());
    let arch = if docker(&["info", "--format", "{{.Architecture}}"]) == "aarch64" {
        Arch::ARM64
    } else {
        Arch::Amd64
    };
    let mut images = Vec::new();
    for version in ["first", "second"] {
        let (image, _) = Image::builder()
            .from("alpine:3.20")
            .platform("linux", &arch)
            .cmd(vec![
                "sh".to_string(),
                "-c".to_string(),
                format!("echo {version} >/version; exec sleep 300"),
            ])
            .output_name_and_tag("local-release-test/default:latest")
            .output_to(temp.path().join(format!("{version}.oci.tar")))
            .build()
            .await
            .unwrap();
        images.push(image);
    }
    for (image, version) in images.iter().zip(["first", "second"]) {
        let info = manager
            .start_container(&id, config(image.path().to_str().unwrap().to_string()))
            .await
            .unwrap();
        assert_eq!(
            docker(&["exec", &info.docker_container_id, "cat", "/version"]),
            version
        );
        assert_eq!(
            docker(&[
                "inspect",
                "--format",
                "{{.Image}}",
                &info.docker_container_id
            ]),
            image.config_digest()
        );
        assert_eq!(
            docker(&[
                "inspect",
                "--format",
                "{{.HostConfig.RestartPolicy.Name}}",
                &info.docker_container_id
            ]),
            "unless-stopped"
        );
        manager.delete_container(&id).await.unwrap();
    }
    let provider = LocalBindingsProvider::new(&temp.path().join("provider")).unwrap();
    std::fs::create_dir_all(temp.path().join("registry")).unwrap();
    let registry_server = container_registry::ContainerRegistry::builder()
        .storage(temp.path().join("registry"))
        .auth_provider(std::sync::Arc::new(sec::Secret::new(
            "local-test".to_string(),
        )))
        .build_for_testing()
        .run_in_background();
    let registry = format!("localhost:{}", registry_server.bound_addr().port());
    let remote = format!("{registry}/images/release:test");
    images[1]
        .push(
            &remote,
            &PushOptions {
                auth: dockdash::RegistryAuth::Basic(
                    "deployment".to_string(),
                    "local-test".to_string(),
                ),
                protocol: ClientProtocol::Http,
                monolithic_push: dockdash::MonolithicPushPolicy::Always,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let info = tokio::time::timeout(
        Duration::from_secs(20),
        manager.start_container(&id, config(remote)),
    )
    .await
    .expect("loopback pull must avoid daemon-side timeouts")
    .unwrap();
    assert_eq!(
        docker(&["exec", &info.docker_container_id, "cat", "/version"]),
        "second"
    );
    manager.stop_container(&id).await.unwrap();
    assert!(!manager.is_running(&id).await);
    manager.delete_container(&id).await.unwrap();

    // A container without a deployment token still pulls from a public loopback registry.
    std::fs::create_dir_all(temp.path().join("public-registry")).unwrap();
    let public_registry_server = container_registry::ContainerRegistry::builder()
        .storage(temp.path().join("public-registry"))
        .auth_provider(std::sync::Arc::new(Anonymous::new(
            Permissions::ReadWrite,
            Permissions::NoAccess,
        )))
        .build_for_testing()
        .run_in_background();
    let public_remote = format!(
        "localhost:{}/images/release:public",
        public_registry_server.bound_addr().port()
    );
    images[1]
        .push(
            &public_remote,
            &PushOptions {
                auth: dockdash::RegistryAuth::Anonymous,
                protocol: ClientProtocol::Http,
                monolithic_push: dockdash::MonolithicPushPolicy::Always,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let tokenless = ContainerConfig {
        proxy_token: None,
        ..config(public_remote)
    };
    let info = tokio::time::timeout(
        Duration::from_secs(20),
        manager.start_container(&id, tokenless),
    )
    .await
    .expect("tokenless loopback pull must avoid daemon-side timeouts")
    .expect("public loopback image starts without a deployment token");
    assert_eq!(
        docker(&["exec", &info.docker_container_id, "cat", "/version"]),
        "second"
    );
    manager.delete_container(&id).await.unwrap();
    provider.shutdown().await;
    for image in images {
        docker(&["image", "rm", "--force", image.config_digest()]);
    }
}
