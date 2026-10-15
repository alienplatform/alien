//! Real native Worker coverage. Requires Bun and installed SDK dependencies.
//! Run explicitly: cargo nextest run -p alien-local --test local_worker_recovery --run-ignored all

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use alien_build::settings::PushSettings;
use alien_core::{
    BinaryTarget, Platform, ResourceLifecycle, ResourceRef, Stack, Storage, Worker, WorkerCode,
    WorkerTrigger,
};
use alien_local::LocalBindingsProvider;
use dockdash::{Arch, ClientProtocol, Image, Layer, PushOptions};
use serde_json::Value;
use tempfile::TempDir;

async fn state(url: &str) -> Value {
    reqwest::get(url).await.unwrap().json().await.unwrap()
}

async fn wait_for(url: &str, predicate: impl Fn(&Value) -> bool) -> Value {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let value = state(url).await;
            if predicate(&value) {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("Worker must receive its event")
}

fn commit_object(storage: &Path, key: &str) {
    let path = storage.join(key);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let staged = storage.join(format!("{key}#1"));
    std::fs::write(&staged, b"committed").unwrap();
    std::fs::rename(staged, path).unwrap();
}

#[tokio::test]
#[ignore = "requires Bun and installed SDK dependencies"]
async fn workers_start_once_route_independently_and_restore_after_manager_restart() {
    tracing_subscriber::fmt::try_init().ok();
    let temp = TempDir::new().unwrap();
    let root = workspace_root::get_workspace_root();
    let binary = temp.path().join("app");
    let output = std::process::Command::new("bun")
        .args(["build", "--compile"])
        .arg(root.join("crates/alien-local/tests/fixtures/event-worker.ts"))
        .arg("--outfile")
        .arg(&binary)
        .output()
        .expect("Bun is required");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let host = BinaryTarget::current_os();
    let arch = if host.oci_arch() == "arm64" {
        Arch::ARM64
    } else {
        Arch::Amd64
    };
    let layer = Layer::builder()
        .unwrap()
        .file(&binary, "app", Some(0o755))
        .unwrap()
        .build()
        .await
        .unwrap();
    let artifacts = temp.path().join("artifacts");
    std::fs::create_dir_all(&artifacts).unwrap();
    let (_image, _) = Image::builder()
        .platform(host.oci_os(), &arch)
        .layer(layer)
        .working_dir("/")
        .entrypoint(vec!["./app".to_string()])
        .output_to(artifacts.join(format!("{}.oci.tar", host.runtime_platform_id())))
        .build()
        .await
        .unwrap();
    let state_dir = temp.path().join("state");
    let launches = temp.path().join("launches");
    let provider = LocalBindingsProvider::new(&state_dir).unwrap();
    let manager = provider.worker_manager();
    if host.oci_os() != "linux" {
        // A Linux sentinel alongside the executable makes wrong-platform pushes fail visibly.
        Image::builder()
            .platform("linux", &arch)
            .entrypoint(vec!["/not-a-native-worker".to_string()])
            .output_to(artifacts.join("linux-aarch64.oci.tar"))
            .build()
            .await
            .unwrap();
    }
    std::fs::create_dir_all(temp.path().join("registry")).unwrap();
    let registry_server = container_registry::ContainerRegistry::builder()
        .storage(temp.path().join("registry"))
        .auth_provider(std::sync::Arc::new(sec::Secret::new(
            "local-test".to_string(),
        )))
        .build_for_testing()
        .run_in_background();
    let registry = format!("localhost:{}", registry_server.bound_addr().port());
    let stack = Stack::new("recovery-test".to_string())
        .add(
            Worker::new("projector".to_string())
                .code(WorkerCode::Image {
                    image: artifacts.to_str().unwrap().to_string(),
                })
                .permissions("execution".to_string())
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let pushed = alien_build::push_stack(
        stack,
        Platform::Local,
        &PushSettings {
            repository: format!("{registry}/recovery-test/workers"),
            destination_label: None,
            options: PushOptions {
                auth: dockdash::RegistryAuth::Basic(
                    "deployment".to_string(),
                    "local-test".to_string(),
                ),
                protocol: ClientProtocol::Http,
                monolithic_push: dockdash::MonolithicPushPolicy::Always,
                ..Default::default()
            },
        },
    )
    .await
    .unwrap();
    let remote = pushed
        .resources()
        .find_map(|(_, entry)| entry.config.downcast_ref::<Worker>())
        .and_then(|worker| {
            if let WorkerCode::Image { image } = &worker.code {
                Some(image.clone())
            } else {
                None
            }
        })
        .unwrap();
    let storage = provider
        .storage_manager()
        .create_storage("data")
        .await
        .unwrap();
    for id in ["projector", "maintenance"] {
        manager
            .extract_image(id, &remote, Some("local-test"))
            .await
            .unwrap();
    }
    let env = |role: &str| {
        HashMap::from([
            ("ROLE".to_string(), role.to_string()),
            (
                "LAUNCH_RECORD".to_string(),
                launches.to_str().unwrap().to_string(),
            ),
        ])
    };
    let (first, second) = tokio::join!(
        manager.start_worker("projector", env("projector"), vec![], vec![]),
        manager.start_worker("projector", env("projector"), vec![], vec![])
    );
    let projector = first.unwrap();
    assert_eq!(projector, second.unwrap());
    let maintenance = manager
        .start_worker("maintenance", env("maintenance"), vec![], vec![])
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&launches).unwrap().lines().count(),
        2
    );
    let storage_trigger = || {
        vec![WorkerTrigger::Storage {
            storage: ResourceRef::new(Storage::RESOURCE_TYPE, "data"),
            events: vec!["created".to_string()],
        }]
    };
    manager
        .ensure_triggers("projector", storage_trigger(), provider.clone())
        .await
        .unwrap();
    manager
        .ensure_triggers(
            "maintenance",
            vec![WorkerTrigger::Schedule {
                cron: "* * * * * *".to_string(),
            }],
            provider.clone(),
        )
        .await
        .unwrap();
    commit_object(&storage, "nested/first");
    let before = wait_for(&projector, |v| {
        v["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e == "nested/first")
    })
    .await;
    wait_for(&maintenance, |v| v["cron"].as_u64().unwrap() > 0).await;
    assert_eq!(state(&maintenance).await["events"], serde_json::json!([]));
    assert_eq!(state(&projector).await["cron"], 0);
    manager.stop_worker("projector").await.unwrap();
    let projector = manager
        .start_worker("projector", env("projector"), vec![], vec![])
        .await
        .unwrap();
    manager
        .ensure_triggers("projector", storage_trigger(), provider.clone())
        .await
        .unwrap();
    commit_object(&storage, "new/second");
    let after = wait_for(&projector, |v| {
        v["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e == "new/second")
    })
    .await;
    assert_ne!(before["pid"], after["pid"]);
    provider.shutdown().await;
    drop(manager);
    let provider = LocalBindingsProvider::new(&state_dir).unwrap();
    let manager = provider.worker_manager();
    // Recovery must wait for the controller's desired environment rather than metadata autostart.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!manager.is_running("projector").await);
    let projector = manager
        .start_worker("projector", env("recovered"), vec![], vec![])
        .await
        .unwrap();
    manager
        .ensure_triggers("projector", storage_trigger(), provider.clone())
        .await
        .unwrap();
    let recovered = wait_for(&projector, |v| v["events"].as_array().unwrap().len() == 2).await;
    assert_eq!(recovered["role"], "recovered");
    assert_eq!(
        std::fs::read_to_string(&launches).unwrap().lines().count(),
        4
    );
    provider.shutdown().await;
}
