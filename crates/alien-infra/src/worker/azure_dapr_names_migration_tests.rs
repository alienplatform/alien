use std::time::Duration;

use alien_azure_clients::container_apps::MockContainerAppsApi;
use alien_azure_clients::long_running_operation::{LongRunningOperation, OperationResult};
use alien_azure_clients::models::managed_environments_dapr_components::{DaprComponent, Secret};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{Queue, ResourceRef, Storage, Worker, WorkerCode, WorkerTrigger};
use alien_error::AlienError;

use super::*;
use crate::worker::azure_dapr_components::{
    dapr_component_matches, delete_dapr_component_if_owned, delete_owned_legacy_dapr_components,
    service_bus_dapr_component, validate_dapr_component_write_ownership,
    DaprComponentDeleteOperation, LegacyDaprComponentCleanupStep,
};
use crate::worker::azure_names::{
    get_azure_blob_trigger_dapr_component_name, get_azure_dapr_component_name,
    get_azure_internal_commands_dapr_component_name, get_azure_queue_trigger_dapr_component_name,
    get_legacy_azure_internal_commands_dapr_component_names,
    get_legacy_azure_queue_trigger_dapr_component_names,
};
use crate::worker::AzureWorkerController;

#[test]
fn deletion_plan_covers_queue_storage_and_cron_without_touching_cron_naming() {
    let worker = worker_with_triggers(vec![
        WorkerTrigger::Queue {
            queue: ResourceRef::new(Queue::RESOURCE_TYPE, "events"),
        },
        WorkerTrigger::Storage {
            storage: ResourceRef::new(Storage::RESOURCE_TYPE, "archive"),
            events: vec!["created".to_string()],
        },
        WorkerTrigger::schedule("0 * * * *"),
    ]);

    let names = dapr_component_deletion_candidates(&worker, "worker-app", &[], None);

    assert!(names.contains(&get_azure_queue_trigger_dapr_component_name(
        "worker-app",
        "events"
    )));
    assert!(names.contains(&get_azure_blob_trigger_dapr_component_name(
        "worker-app",
        "archive"
    )));
    assert!(names.contains(&get_azure_dapr_component_name("cron-worker-app-0")));
}

#[test]
fn deletion_plan_deduplicates_shared_commands_and_queue_legacy_alias() {
    let worker = worker_with_triggers(vec![WorkerTrigger::Queue {
        queue: ResourceRef::new(Queue::RESOURCE_TYPE, "internal-commands"),
    }]);
    let command_legacy = get_legacy_azure_internal_commands_dapr_component_names("worker-app");
    let queue_legacy =
        get_legacy_azure_queue_trigger_dapr_component_names("worker-app", "internal-commands");
    let shared_name = command_legacy
        .iter()
        .find(|name| queue_legacy.contains(name))
        .expect("historical commands and queue names should overlap");

    let names = dapr_component_deletion_candidates(&worker, "worker-app", &[], None);

    assert_eq!(names.iter().filter(|name| *name == shared_name).count(), 1);
}

#[test]
fn imported_empty_tracking_is_repaired_once_before_delete() {
    let worker = worker_with_triggers(vec![WorkerTrigger::Storage {
        storage: ResourceRef::new(Storage::RESOURCE_TYPE, "archive"),
        events: vec!["created".to_string()],
    }]);
    let mut controller = AzureWorkerController::mock_ready("worker-app");
    controller.dapr_component_naming_version = 0;
    controller.dapr_components.clear();
    controller.commands_dapr_component = None;

    assert!(controller.initialize_dapr_component_deletion_candidates(&worker, "worker-app"));
    assert!(!controller.dapr_components.is_empty());
    assert!(controller
        .dapr_components
        .contains(&get_azure_internal_commands_dapr_component_name(
            "worker-app"
        )));
    assert!(controller
        .dapr_components
        .contains(&get_azure_blob_trigger_dapr_component_name(
            "worker-app",
            "archive"
        )));
    assert_eq!(controller.dapr_component_naming_version, 0);

    let persisted_names = controller.dapr_components.clone();
    assert!(!controller.initialize_dapr_component_deletion_candidates(&worker, "worker-app"));
    assert_eq!(controller.dapr_components, persisted_names);
}

#[test]
fn pending_delete_completion_does_not_advance_migration_version() {
    let mut controller = AzureWorkerController::mock_ready("worker-app");
    controller.dapr_component_naming_version = 0;
    controller.dapr_components = vec!["legacy-component".to_string()];
    controller.pending_dapr_component_deletion_name = Some("legacy-component".to_string());

    controller.complete_pending_dapr_component_deletion();

    assert!(controller.dapr_components.is_empty());
    assert_eq!(controller.dapr_component_naming_version, 0);
}

#[test]
fn service_bus_component_validation_detects_all_mutable_property_drift() {
    let desired = service_bus_dapr_component(
        "component".to_string(),
        "worker-app",
        "namespace",
        "queue".to_string(),
        "client-id",
    );
    assert!(dapr_component_matches(&desired, &desired));

    let mut metadata_drift = desired.clone();
    metadata_drift
        .properties
        .as_mut()
        .unwrap()
        .metadata
        .iter_mut()
        .find(|metadata| metadata.name.as_deref() == Some("queueName"))
        .unwrap()
        .value = Some("wrong-queue".to_string());
    assert!(!dapr_component_matches(&metadata_drift, &desired));

    let mut ignore_errors_drift = desired.clone();
    ignore_errors_drift
        .properties
        .as_mut()
        .unwrap()
        .ignore_errors = true;
    assert!(!dapr_component_matches(&ignore_errors_drift, &desired));

    let mut init_timeout_drift = desired.clone();
    init_timeout_drift.properties.as_mut().unwrap().init_timeout = Some("30s".to_string());
    assert!(!dapr_component_matches(&init_timeout_drift, &desired));

    let mut secret_store_drift = desired.clone();
    secret_store_drift
        .properties
        .as_mut()
        .unwrap()
        .secret_store_component = Some("secret-store".to_string());
    assert!(!dapr_component_matches(&secret_store_drift, &desired));

    let mut secrets_drift = desired.clone();
    secrets_drift.properties.as_mut().unwrap().secrets = vec![Secret {
        identity: None,
        key_vault_url: None,
        name: Some("token".to_string()),
        value: Some("secret".to_string()),
    }];
    assert!(!dapr_component_matches(&secrets_drift, &desired));
}

#[tokio::test]
async fn legacy_cleanup_deletes_only_component_owned_by_worker() {
    let mut client = MockContainerAppsApi::new();
    client
        .expect_get_dapr_component()
        .returning(|_, _, _| Ok(component_with_scopes(&["worker-app"])));
    client
        .expect_delete_dapr_component()
        .returning(|_, _, _| Ok(OperationResult::Completed(())));

    let operation = delete_owned_legacy_dapr_components(
        &client,
        "environment-rg",
        "environment",
        "worker-app",
        "structured-component",
        &["legacy-component".to_string()],
        "worker",
    )
    .await
    .unwrap();

    assert!(matches!(operation, LegacyDaprComponentCleanupStep::Mutated));
}

#[tokio::test]
async fn legacy_cleanup_preserves_foreign_collision() {
    let mut client = MockContainerAppsApi::new();
    client
        .expect_get_dapr_component()
        .returning(|_, _, _| Ok(component_with_scopes(&["other-worker-app"])));
    client.expect_delete_dapr_component().times(0);

    let operation = delete_owned_legacy_dapr_components(
        &client,
        "environment-rg",
        "environment",
        "worker-app",
        "structured-component",
        &["legacy-component".to_string()],
        "worker",
    )
    .await
    .unwrap();

    assert!(matches!(
        operation,
        LegacyDaprComponentCleanupStep::Complete
    ));
}

#[tokio::test]
async fn foreign_desired_name_is_rejected_before_write() {
    let mut client = MockContainerAppsApi::new();
    client
        .expect_get_dapr_component()
        .returning(|_, _, _| Ok(component_with_scopes(&["other-worker-app"])));

    let error = validate_dapr_component_write_ownership(
        &client,
        "environment-rg",
        "environment",
        "worker-app",
        "structured-component",
        "worker",
    )
    .await
    .unwrap_err();

    assert!(matches!(error.error, Some(ErrorData::ResourceDrift { .. })));
}

#[tokio::test]
async fn legacy_cleanup_returns_delete_lro_to_state_handler() {
    let mut client = MockContainerAppsApi::new();
    client
        .expect_get_dapr_component()
        .returning(|_, _, _| Ok(component_with_scopes(&["worker-app"])));
    client.expect_delete_dapr_component().returning(|_, _, _| {
        Ok(OperationResult::LongRunning(LongRunningOperation {
            url: "https://management.azure.com/operations/delete-legacy".to_string(),
            retry_after: Some(Duration::from_secs(2)),
            location_url: None,
        }))
    });

    let operation = match delete_owned_legacy_dapr_components(
        &client,
        "environment-rg",
        "environment",
        "worker-app",
        "structured-component",
        &["legacy-component".to_string()],
        "worker",
    )
    .await
    .unwrap()
    {
        LegacyDaprComponentCleanupStep::LongRunning(operation) => operation,
        _ => panic!("delete should be awaited before creating the replacement"),
    };

    assert_eq!(
        operation.url,
        "https://management.azure.com/operations/delete-legacy"
    );
}

#[tokio::test]
async fn delete_404_after_owned_get_is_idempotent_success() {
    let mut client = MockContainerAppsApi::new();
    client
        .expect_get_dapr_component()
        .returning(|_, _, _| Ok(component_with_scopes(&["worker-app"])));
    client.expect_delete_dapr_component().returning(|_, _, _| {
        Err(AlienError::new(
            CloudClientErrorData::RemoteResourceNotFound {
                resource_type: "Dapr component".to_string(),
                resource_name: "legacy-component".to_string(),
            },
        ))
    });

    let operation = delete_dapr_component_if_owned(
        &client,
        "environment-rg",
        "environment",
        "worker-app",
        "legacy-component",
        "worker",
    )
    .await
    .unwrap();

    assert!(matches!(operation, DaprComponentDeleteOperation::NotFound));
}

fn component_with_scopes(scopes: &[&str]) -> DaprComponent {
    let mut component = service_bus_dapr_component(
        "legacy-component".to_string(),
        "worker-app",
        "namespace",
        "queue".to_string(),
        "client-id",
    );
    component.properties.as_mut().unwrap().scopes =
        scopes.iter().map(|scope| (*scope).to_string()).collect();
    component
}

fn worker_with_triggers(triggers: Vec<WorkerTrigger>) -> Worker {
    let mut builder = Worker::new("worker".to_string())
        .code(WorkerCode::Image {
            image: "registry.invalid/worker:latest".to_string(),
        })
        .permissions("default-profile".to_string());
    for trigger in triggers {
        builder = builder.trigger(trigger);
    }
    builder.build()
}
