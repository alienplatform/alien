use alien_local::{ContainerConfig, LocalContainerManager};
use bollard::Docker;
use dockdash::{Arch, Image};
use futures::FutureExt;
use std::{collections::HashMap, panic::AssertUnwindSafe, sync::Arc};

#[tokio::test]
#[ignore = "requires Docker and registry access"]
async fn concurrent_shared_image_loads_keep_both_containers_inspectable() {
    let directory = tempfile::tempdir().unwrap();
    let arch = if cfg!(target_arch = "aarch64") {
        Arch::ARM64
    } else {
        Arch::Amd64
    };
    let (first_image, _) = Image::builder()
        .from("alpine:3.22")
        .platform("linux", &arch)
        .build()
        .await
        .unwrap();
    let (second_image, _) = Image::builder()
        .from("alpine:3.22")
        .platform("linux", &arch)
        .build()
        .await
        .unwrap();
    let manager = Arc::new(LocalContainerManager::new(directory.path().to_path_buf()).unwrap());
    let config = |image: String| ContainerConfig {
        image,
        command: Some(vec!["sleep".to_string(), "300".to_string()]),
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
    };
    let unique = std::process::id();
    let first_name = format!("identity-first-{unique}");
    let second_name = format!("identity-second-{unique}");
    let (first, second) = tokio::join!(
        manager.start_container(
            &first_name,
            config(first_image.path().to_string_lossy().into_owned())
        ),
        manager.start_container(
            &second_name,
            config(second_image.path().to_string_lossy().into_owned())
        )
    );
    let docker = Docker::connect_with_local_defaults().unwrap();
    let inspect = AssertUnwindSafe(async {
        let first = first.unwrap();
        let second = second.unwrap();
        let first = docker
            .inspect_container(&first.docker_container_id, None)
            .await
            .unwrap();
        let second = docker
            .inspect_container(&second.docker_container_id, None)
            .await
            .unwrap();
        assert!(first.state.unwrap().running.unwrap());
        assert!(second.state.unwrap().running.unwrap());
        let id = first.image.unwrap();
        assert_eq!(second.image.as_deref(), Some(id.as_str()));
        assert!(docker.inspect_image(&id).await.is_ok());
        let labels = first.config.unwrap().labels.unwrap();
        assert_eq!(labels["alien.dev/image-id"], id);
        assert_eq!(labels["alien.dev/resource"], first_name);
    })
    .catch_unwind()
    .await;
    manager.delete_container(&first_name).await.unwrap();
    manager.delete_container(&second_name).await.unwrap();
    if let Err(panic) = inspect {
        std::panic::resume_unwind(panic);
    }
}
