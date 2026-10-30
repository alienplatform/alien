//! Opt-in qualification with generated restricted management permissions.
//! The fixture owner seeds an account, container, namespace, queue and independent
//! subscription, installs export_permissions on a fresh identity, then runs
//! reconcile_fixture for create/update/remove/retain. The provision phase also
//! creates the Queue and Storage on seeded shared account/namespace parents.
//! Set AZURE_QUEUE_NOTIFICATIONS_LIFECYCLE=frozen for direct setup authority.
//! Independently verify the
//! matching delivery cohorts and foreign subscription after each phase, then
//! delete every fixture resource. Reconciliation uses only the restricted identity.

use crate::{
    controller_test::SingleControllerExecutor,
    queue::azure::{AzureQueueController, AzureQueueState},
    AzureResourceGroupController, AzureServiceBusNamespaceController,
    AzureStorageAccountController, AzureStorageController, AzureStorageState,
    DefaultPlatformServiceProvider,
};
use alien_bindings::{providers::queue::AzureServiceBusQueue, traits::Queue as QueueBinding};
use alien_core::{
    AzureClientConfig, AzureResourceGroup, AzureServiceBusNamespace, AzureStorageAccount,
    ClientConfig, ExternalBindings, InitialSetupAuthority, Platform, Queue, ResourceLifecycle,
    ResourceStatus, Storage, StorageEvent, StorageEventType, StorageNotification,
};
use alien_permissions::{
    generators::AzureRuntimePermissionsGenerator, get_permission_set, BindingTarget,
    PermissionContext,
};
use serde::Deserialize;
use std::{fs, path::PathBuf, sync::Arc, time::Duration};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    prefix: String,
    group: String,
    account: String,
    namespace: String,
    container: String,
    queue: String,
    config: AzureClientConfig,
    evidence_dir: PathBuf,
}

fn fixture() -> Fixture {
    let path = std::env::var("AZURE_QUEUE_NOTIFICATIONS_FIXTURE")
        .expect("explicit isolated test fixture required");
    let f: Fixture = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert!(f.prefix.starts_with("n2081-"));
    for name in [&f.group, &f.namespace, &f.container, &f.queue] {
        assert!(name.starts_with(&f.prefix));
    }
    assert!(f.account.starts_with(&f.prefix.replace('-', "")));
    assert!(f.config.service_overrides.is_none());
    f
}

#[test]
#[ignore = "exports exact grants for a fresh isolated management identity"]
fn export_permissions() {
    let f = fixture();
    let context = PermissionContext::new()
        .with_stack_prefix(&f.prefix)
        .with_subscription_id(&f.config.subscription_id)
        .with_resource_group(&f.group)
        .with_storage_account_name(&f.account);
    let plans: Vec<_> = [
        "storage/provision",
        "queue/provision",
        "storage/trigger-management",
        // Default management profiles also observe shared lifecycle policy before
        // retaining a container. This Reader grant carries no data-plane access.
        "storage/heartbeat",
    ]
    .into_iter()
    .map(|id| {
        AzureRuntimePermissionsGenerator::new()
            .generate_grant_plan(
                get_permission_set(id).unwrap(),
                BindingTarget::Stack,
                &context,
            )
            .unwrap()
    })
    .collect();
    fs::write(
        f.evidence_dir.join("grants.json"),
        serde_json::to_vec_pretty(&plans).unwrap(),
    )
    .unwrap();
}

#[tokio::test]
#[ignore = "reconciles real subscriptions with a restricted management identity"]
async fn reconcile_fixture() {
    let f = fixture();
    let phase = std::env::var("AZURE_QUEUE_NOTIFICATIONS_PHASE")
        .expect("provision, create, update, remove or retain phase required");
    assert!(["provision", "create", "update", "remove", "retain"].contains(&phase.as_str()));
    let lifecycle = match std::env::var("AZURE_QUEUE_NOTIFICATIONS_LIFECYCLE").as_deref() {
        Ok("frozen") => ResourceLifecycle::Frozen,
        Ok("live") | Err(_) => ResourceLifecycle::Live,
        Ok(other) => panic!("unsupported qualification lifecycle {other}"),
    };
    let queue = Queue::new("work".into()).build();
    let storage = Storage::new("uploads".into())
        .notifications(vec![
            StorageNotification::builder()
                .queue((&queue).into())
                .prefix(
                    if matches!(phase.as_str(), "create" | "provision") {
                        "in/"
                    } else {
                        "changed/"
                    }
                    .into(),
                )
                .suffix(".json".into())
                .build(),
            StorageNotification::builder()
                .queue((&queue).into())
                .prefix("other/".into())
                .suffix(".data".into())
                .build(),
        ])
        .build();
    let mut group = AzureResourceGroupController::mock_ready(&f.group);
    group.resource_group_name = Some(f.group.clone());
    group.resource_id = Some(format!(
        "/subscriptions/{}/resourceGroups/{}",
        f.config.subscription_id, f.group
    ));
    let mut account = AzureStorageAccountController::mock_ready(&f.account);
    account.account_name = Some(f.account.clone());
    account.resource_id = Some(format!(
        "{}/providers/Microsoft.Storage/storageAccounts/{}",
        group.resource_id.as_ref().unwrap(),
        f.account
    ));
    let config = ClientConfig::Azure(Box::new(f.config));
    let provider = Arc::new(DefaultPlatformServiceProvider::default());
    let queue_controller = AzureQueueController {
        state: AzureQueueState::Ready,
        namespace_name: Some(f.namespace.clone()),
        queue_name: Some(f.queue.clone()),
        ..Default::default()
    };
    let mut namespace = AzureServiceBusNamespaceController::mock_ready(&f.namespace);
    namespace.resource_group_name = Some(f.group.clone());
    namespace.resource_id = Some(format!(
        "{}/providers/Microsoft.ServiceBus/namespaces/{}",
        group.resource_id.as_ref().unwrap(),
        f.namespace,
    ));
    let mut queue_run = SingleControllerExecutor::builder()
        .resource(queue.clone())
        .controller(if phase == "provision" {
            AzureQueueController::default()
        } else {
            queue_controller
        })
        .platform(Platform::Azure)
        .resource_prefix(&f.prefix)
        .resource_lifecycle(lifecycle)
        .initial_setup_authority(InitialSetupAuthority::DirectSetup)
        .client_config(config.clone())
        .service_provider(provider.clone())
        .real_delays()
        .with_stack_resource(storage.clone(), lifecycle)
        .with_dependency(
            AzureResourceGroup::new("default-resource-group".into()).build(),
            group.clone(),
        )
        .with_dependency(
            AzureServiceBusNamespace::new("default-service-bus-namespace".into()).build(),
            namespace,
        )
        .build()
        .await
        .unwrap();
    if phase == "provision" {
        queue_run.run_until_terminal().await.unwrap();
        assert_eq!(queue_run.status(), ResourceStatus::Running);
    }
    assert!(!queue_run.needs_update().unwrap());
    let mut run = SingleControllerExecutor::builder()
        .resource(storage)
        .controller(AzureStorageController {
            state: if phase == "provision" {
                AzureStorageState::CreateStart
            } else if phase == "create" {
                AzureStorageState::CreatingNotifications
            } else {
                AzureStorageState::UpdatingNotifications
            },
            container_name: Some(f.container),
            storage_account_name: Some(f.account),
            notifications_managed: true,
            ..Default::default()
        })
        .platform(Platform::Azure)
        .resource_prefix(&f.prefix)
        .resource_lifecycle(lifecycle)
        .initial_setup_authority(InitialSetupAuthority::DirectSetup)
        .client_config(config.clone())
        .service_provider(provider.clone())
        .real_delays()
        .with_dependency(
            AzureResourceGroup::new("default-resource-group".into()).build(),
            group,
        )
        .with_dependency(
            AzureStorageAccount::new("default-storage-account".into()).build(),
            account,
        )
        .with_dependency(
            queue,
            queue_run
                .internal_state::<AzureQueueController>()
                .unwrap()
                .clone(),
        )
        .dependency_lifecycle("work", lifecycle)
        .build()
        .await
        .unwrap();
    if phase == "retain" {
        // Retention deliberately returns retryable pending after each durable
        // deletion. The fixture runner, like the manager, owns that retry policy.
        tokio::time::timeout(Duration::from_secs(180), async {
            loop {
                match crate::protect_retained_storage(
                    run.stack_state(),
                    &["uploads".into()],
                    &config,
                    provider.as_ref(),
                    &ExternalBindings::default(),
                    &[lifecycle],
                )
                .await
                {
                    Ok(()) => break,
                    Err(error) if error.code == "STORAGE_NOTIFICATION_CLEANUP_PENDING" => {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                    Err(error) => panic!("retained storage protection failed: {error:?}"),
                }
            }
        })
        .await
        .expect("retention must converge before deadline");
    } else if phase == "remove" {
        run.delete().unwrap();
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Deleted);
        queue_run.delete().unwrap();
        queue_run.run_until_terminal().await.unwrap();
        assert_eq!(queue_run.status(), ResourceStatus::Deleted);
    } else {
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Running);
        assert!(!run.needs_update().unwrap());
    }
    fs::write(
        f.evidence_dir.join(format!("{phase}-controller.json")),
        serde_json::to_vec_pretty(run.internal_state::<AzureStorageController>().unwrap()).unwrap(),
    )
    .unwrap();
}

/// Run after the fixture owner uploads a fresh cohort. Consumption uses an
/// explicitly separate observer identity, never the restricted controller.
#[tokio::test]
#[ignore = "receives and acknowledges the exact uploaded real-cloud cohort"]
async fn receive_cohort() {
    let f = fixture();
    let path = std::env::var("AZURE_QUEUE_NOTIFICATIONS_OBSERVER")
        .expect("separate observer client configuration required");
    let config: AzureClientConfig = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let expected: Vec<String> = serde_json::from_str(
        &std::env::var("AZURE_QUEUE_NOTIFICATIONS_EXPECTED").expect("expected keys required"),
    )
    .unwrap();
    assert!(!expected.is_empty());
    let binding = AzureServiceBusQueue::new(f.namespace, f.queue, config)
        .await
        .unwrap();
    let mut events = Vec::new();
    tokio::time::timeout(Duration::from_secs(180), async {
        while events.len() < expected.len() {
            // One receipt per pull also exercises the feature independently of
            // partial-batch lock handling. Worker batching is qualified separately.
            for message in binding.receive("work", 1).await.unwrap() {
                let event: StorageEvent = serde_json::from_value(
                    serde_json::to_value(&message).unwrap()["payload"].clone(),
                )
                .unwrap();
                assert_eq!(event.event_type, StorageEventType::Created);
                assert_eq!(event.bucket_name, f.container);
                assert_eq!(event.size, Some(7));
                assert!(event.etag.as_ref().is_some_and(|etag| !etag.is_empty()));
                assert!(event.timestamp.timestamp() > 0);
                binding.ack("work", &message.receipt_handle).await.unwrap();
                events.push(event);
            }
        }
    })
    .await
    .expect("cohort must arrive before deadline");
    let mut actual: Vec<_> = events
        .iter()
        .map(|event| event.object_key.clone())
        .collect();
    let mut expected = expected;
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected);
    for _ in 0..2 {
        assert!(
            binding.receive("work", 1).await.unwrap().is_empty(),
            "unexpected extra delivery"
        );
    }
    fs::write(
        f.evidence_dir.join("cohort.json"),
        serde_json::to_vec_pretty(&events).unwrap(),
    )
    .unwrap();
}
