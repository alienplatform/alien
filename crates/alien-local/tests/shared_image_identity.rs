use alien_local::{ContainerConfig, LocalContainerManager};
use dockdash::{Arch, Image};
use futures::FutureExt;
use std::{collections::HashMap, panic::AssertUnwindSafe, sync::Arc};
use uuid::Uuid;

fn config(image: String) -> ContainerConfig {
    ContainerConfig {
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
    }
}

#[tokio::test]
#[ignore = "requires Docker and registry access"]
async fn concurrent_shared_image_loads_keep_both_containers_inspectable() {
    let directory = tempfile::tempdir().unwrap();
    let arch = if cfg!(target_arch = "aarch64") {
        Arch::ARM64
    } else {
        Arch::Amd64
    };
    let test_identity = Uuid::new_v4().to_string();
    let (first_image, _) = Image::builder()
        .from("alpine:3.22")
        .env("ALIEN_IDENTITY_TEST", &test_identity)
        .platform("linux", &arch)
        .build()
        .await
        .unwrap();
    let (second_image, _) = Image::builder()
        .from("alpine:3.22")
        .env("ALIEN_IDENTITY_TEST", &test_identity)
        .platform("linux", &arch)
        .build()
        .await
        .unwrap();
    assert_eq!(first_image.config_digest(), second_image.config_digest());
    let docker = alien_local::connect_docker().unwrap();
    let mut archive = tar::Archive::new(std::fs::File::open(first_image.path()).unwrap());
    let index = archive
        .entries_with_seek()
        .unwrap()
        .find_map(|entry| {
            let entry = entry.unwrap();
            let path = entry.path().unwrap().into_owned();
            (path.strip_prefix(".").unwrap_or(&path) == std::path::Path::new("index.json"))
                .then(|| serde_json::from_reader::<_, serde_json::Value>(entry).unwrap())
        })
        .expect("OCI index");
    let candidates = [
        index["manifests"][0]["digest"]
            .as_str()
            .unwrap()
            .to_string(),
        first_image.config_digest().to_string(),
    ];
    for candidate in &candidates {
        assert!(
            matches!(
                docker.inspect_image(candidate).await,
                Err(bollard::errors::Error::DockerResponseServerError {
                    status_code: 404,
                    ..
                })
            ),
            "test image must be absent before concurrent imports"
        );
    }
    let manager = Arc::new(LocalContainerManager::new(directory.path().to_path_buf()).unwrap());
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
    let mut removed = false;
    for candidate in &candidates {
        match docker.remove_image(candidate, None, None).await {
            Ok(_) => {
                removed = true;
                break;
            }
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => {}
            Err(error) => panic!("image cleanup failed: {error}"),
        }
    }
    assert!(removed, "the concurrently imported image must be removed");
    for candidate in &candidates {
        assert!(matches!(
            docker.inspect_image(candidate).await,
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404,
                ..
            })
        ));
    }
    if let Err(panic) = inspect {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
#[ignore = "requires Docker and registry access"]
async fn registry_container_uses_and_labels_its_immutable_image_id() {
    let directory = tempfile::tempdir().unwrap();
    let manager = LocalContainerManager::new(directory.path().to_path_buf()).unwrap();
    let name = format!("registry-identity-{}", Uuid::new_v4());
    let docker = alien_local::connect_docker().unwrap();
    let result = manager
        .start_container(&name, config("alpine:3.22".to_string()))
        .await;
    let inspection = AssertUnwindSafe(async {
        let container = result.unwrap();
        let container = docker
            .inspect_container(&container.docker_container_id, None)
            .await
            .unwrap();
        let id = container.image.unwrap();
        assert!(id.starts_with("sha256:"));
        let config = container.config.unwrap();
        assert_eq!(config.image.as_deref(), Some(id.as_str()));
        assert_eq!(config.labels.unwrap()["alien.dev/image-id"], id);
        assert!(docker.inspect_image(&id).await.is_ok());
    })
    .catch_unwind()
    .await;
    manager.delete_container(&name).await.unwrap();
    if let Err(panic) = inspection {
        std::panic::resume_unwind(panic);
    }
}
