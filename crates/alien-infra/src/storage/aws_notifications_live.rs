//! Opt-in qualification of notification controllers on pre-existing test resources.
//! The fixture owner creates a bucket and queue, installs export_permissions on a
//! fresh restricted role, runs reconcile_fixture for create/update/remove phases,
//! verifies delivery independently, and deletes all fixture resources.

use crate::{
    core::controller_test::SingleControllerExecutor,
    queue::aws::{AwsQueueController, AwsQueueState},
    storage::{AwsStorageController, AwsStorageState},
    DefaultPlatformServiceProvider,
};
use alien_core::{
    AwsClientConfig, ClientConfig, Platform, Queue, Resource, ResourceLifecycle, ResourceStatus,
    Storage, StorageNotification,
};
use alien_permissions::{
    generators::{AwsIamPolicy, AwsRuntimePermissionsGenerator},
    get_permission_set, BindingTarget, PermissionContext,
};
use serde::Deserialize;
use std::{fs, path::PathBuf, sync::Arc};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    prefix: String,
    bucket: String,
    queue_url: String,
    queue_name: String,
    config: AwsClientConfig,
    evidence_dir: PathBuf,
}

fn fixture() -> Fixture {
    let path = std::env::var("AWS_QUEUE_NOTIFICATIONS_FIXTURE")
        .expect("explicit isolated fixture required");
    let fixture: Fixture = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert!(fixture.prefix.starts_with("n2081-"));
    assert!(fixture.bucket.starts_with(&fixture.prefix));
    assert!(fixture.queue_name.starts_with(&fixture.prefix));
    assert!(fixture.config.service_overrides.is_none());
    fixture
}

#[test]
#[ignore = "exports permissions for a fresh restricted test identity"]
fn export_permissions() {
    let f = fixture();
    let context = PermissionContext::new()
        .with_stack_prefix(&f.prefix)
        .with_aws_region(&f.config.region)
        .with_aws_account_id(&f.config.account_id);
    let mut statements = Vec::new();
    for id in [
        "storage/provision",
        "queue/provision",
        "storage/queue-notifications",
    ] {
        let policy = AwsRuntimePermissionsGenerator::new()
            .generate_policy(
                get_permission_set(id).unwrap(),
                BindingTarget::Stack,
                &context,
            )
            .unwrap();
        statements.extend(policy.statement);
    }
    fs::write(
        f.evidence_dir.join("grants.json"),
        serde_json::to_vec_pretty(&AwsIamPolicy {
            version: "2012-10-17".into(),
            statement: statements,
        })
        .unwrap(),
    )
    .unwrap();
}

#[tokio::test]
#[ignore = "mutates isolated cloud fixture using restricted management credentials"]
async fn reconcile_fixture() {
    let f = fixture();
    let phase = std::env::var("AWS_QUEUE_NOTIFICATIONS_PHASE")
        .expect("create, update or remove phase required");
    assert!(["create", "update", "remove"].contains(&phase.as_str()));
    let queue = Queue::new("work".into()).build();
    let storage = Storage::new("uploads".into())
        .notifications(vec![
            StorageNotification::builder()
                .queue((&queue).into())
                .prefix(if phase == "create" { "in/" } else { "changed/" }.into())
                .suffix(".json".into())
                .build(),
            StorageNotification::builder()
                .queue((&queue).into())
                .prefix("other/".into())
                .suffix(".data".into())
                .build(),
        ])
        .build();
    let provider = Arc::new(DefaultPlatformServiceProvider::default());
    let config = ClientConfig::Aws(Box::new(f.config));
    let mut queue_run = SingleControllerExecutor::builder()
        .resource(queue.clone())
        .controller(AwsQueueController {
            state: AwsQueueState::Ready,
            queue_url: Some(f.queue_url),
            queue_name: Some(f.queue_name),
            storage_ingress_enabled: phase != "create",
            ..Default::default()
        })
        .platform(Platform::Aws)
        .resource_prefix(&f.prefix)
        .client_config(config.clone())
        .with_stack_resource(storage.clone(), ResourceLifecycle::Live)
        .service_provider(provider.clone())
        .build()
        .await
        .unwrap();
    if phase == "create" {
        assert!(queue_run.needs_update().unwrap());
        queue_run.update(queue.clone()).unwrap();
        queue_run.run_until_terminal().await.unwrap();
    }
    assert!(!queue_run.needs_update().unwrap());
    let mut run = SingleControllerExecutor::builder()
        .resource(storage)
        .controller(AwsStorageController {
            state: if phase == "create" {
                AwsStorageState::CreatingNotifications
            } else {
                AwsStorageState::UpdatingNotifications
            },
            bucket_name: Some(f.bucket),
            notifications_managed: true,
            ..Default::default()
        })
        .platform(Platform::Aws)
        .resource_prefix(&f.prefix)
        .client_config(config)
        .with_dependency(
            queue.clone(),
            queue_run
                .internal_state::<AwsQueueController>()
                .unwrap()
                .clone(),
        )
        .service_provider(provider)
        .build()
        .await
        .unwrap();
    if phase == "remove" {
        run.delete().unwrap();
        for _ in 0..40 {
            if run.internal_state::<AwsStorageController>().unwrap().state
                == AwsStorageState::DeleteStart
            {
                break;
            }
            run.step().await.unwrap();
        }
        assert_eq!(
            run.internal_state::<AwsStorageController>().unwrap().state,
            AwsStorageState::DeleteStart
        );
        *queue_run.desired_resource_config_mut("uploads").unwrap() =
            Resource::new(Storage::new("uploads".into()).build());
        assert!(queue_run.needs_update().unwrap());
        queue_run.update(queue).unwrap();
        queue_run.run_until_terminal().await.unwrap();
        assert!(!queue_run.needs_update().unwrap());
    } else {
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Running);
        assert!(!run.needs_update().unwrap());
    }
    fs::write(
        f.evidence_dir.join(format!("{phase}-controller.json")),
        serde_json::to_vec_pretty(run.internal_state::<AwsStorageController>().unwrap()).unwrap(),
    )
    .unwrap();
}
