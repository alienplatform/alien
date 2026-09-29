//! A disk image whose build ended `Failed` fails the resource once, and the retry of the failed
//! resource builds afresh even though the group still lists that image until `Ready` reaps it.

#![cfg(all(feature = "azure", feature = "test-utils"))]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use alien_azure_clients::azure::sandbox_data_plane::{
    DiskImage, DiskImageStatus, MockSandboxDataPlaneApi, SandboxDataPlaneApi,
};
use alien_azure_clients::azure::sandbox_groups::MockSandboxGroupsApi;
use alien_core::{
    azure_disk_image_label, Platform, ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress,
    SandboxLifecyclePolicy, AZURE_DISK_IMAGE_LABEL,
};
use alien_infra::controller_test::SingleControllerExecutor;
use alien_infra::{AzureSandboxController, MockPlatformServiceProvider};

const PYTHON: &str = "docker.io/library/python:3.14-slim";

fn failed_image() -> DiskImage {
    DiskImage {
        id: "failed-1".to_string(),
        labels: [(
            AZURE_DISK_IMAGE_LABEL.to_string(),
            azure_disk_image_label(PYTHON),
        )]
        .into(),
        status: Some(DiskImageStatus {
            state: Some("Failed".to_string()),
            error_message: Some("registry timed out".to_string()),
        }),
    }
}

#[tokio::test]
async fn a_failed_build_is_rebuilt_on_retry() {
    let creates = Arc::new(AtomicUsize::new(0));
    let seen = creates.clone();
    let mut client = MockSandboxDataPlaneApi::new();
    // Nothing deletes the Failed image between attempts, as on the live service while the
    // sandbox stays in EnsureDiskImage.
    client
        .expect_list_disk_images()
        .returning(|_| Ok(vec![failed_image()]));
    let deletes = Arc::new(AtomicUsize::new(0));
    let deleted = deletes.clone();
    client
        .expect_delete_disk_image()
        .withf(|_, id| id == "failed-1")
        .returning(move |_, _| {
            deleted.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
    client.expect_create_disk_image().returning(move |_, _| {
        seen.fetch_add(1, Ordering::SeqCst);
        Ok(DiskImage {
            id: "rebuilt".to_string(),
            labels: [(
                AZURE_DISK_IMAGE_LABEL.to_string(),
                azure_disk_image_label(PYTHON),
            )]
            .into(),
            status: Some(DiskImageStatus {
                state: Some("Ready".to_string()),
                error_message: None,
            }),
        })
    });
    let client: Arc<dyn SandboxDataPlaneApi> = Arc::new(client);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_azure_sandbox_data_plane_client()
        .returning(move |_, _, _| Ok(client.clone()));
    // The rebuilt sandbox reaches `Ready`, whose heartbeat reads the group over ARM.
    provider
        .expect_get_azure_sandbox_groups_client()
        .returning(|_| {
            let mut arm = MockSandboxGroupsApi::new();
            arm.expect_get_sandbox_group().returning(|_, name| {
                Err(alien_error::AlienError::new(
                    alien_client_core::ErrorData::RemoteResourceNotFound {
                        resource_type: "SandboxGroup".to_string(),
                        resource_name: name.to_string(),
                    },
                ))
            });
            Ok(Arc::new(arm))
        });

    let controller: AzureSandboxController = serde_json::from_value(serde_json::json!({
        "state": "ensureDiskImage",
        "sandboxGroup": "sbg",
        "region": "westus2",
        "resourceGroup": "rg",
        "diskImage": null,
        "egress": { "mode": "allow" },
    }))
    .unwrap();
    let mut executor = SingleControllerExecutor::builder()
        .resource(
            Sandbox::new("agents".to_string())
                .code(SandboxCode::Image {
                    image: PYTHON.to_string(),
                })
                .egress(SandboxEgress::Allow)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .build(),
        )
        .controller(controller)
        .platform(Platform::Azure)
        .resource_lifecycle(ResourceLifecycle::Frozen)
        .service_provider(Arc::new(provider))
        .build()
        .await
        .unwrap();

    let failure = executor
        .step()
        .await
        .expect_err("the Failed build is reported");
    assert!(
        !failure.retryable,
        "the executor must not rebuild on its own: {failure}"
    );

    // A retry of the failed resource resumes the same state with the controller as persisted.
    executor
        .step()
        .await
        .expect("the retry builds afresh and adopts the new image");
    executor
        .step()
        .await
        .expect("the Ready tick reaps the Failed image");
    assert_eq!(
        creates.load(Ordering::SeqCst),
        1,
        "a retry after a Failed build must build afresh rather than re-report the same failure"
    );
    assert_eq!(
        deletes.load(Ordering::SeqCst),
        1,
        "the Failed image is reaped once the rebuilt sandbox is Ready"
    );
}
