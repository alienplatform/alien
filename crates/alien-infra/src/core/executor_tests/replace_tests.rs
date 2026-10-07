//! Tests for replacing a failed create whose controller state records what it already made.
//!
//! A config change on such a resource must delete those cloud resources, against the config
//! they were created with, before creating it again. A fresh create would drop the saved IDs
//! and leak everything the failed create made. Resources that are not safe to delete that way
//! (user data, live replicas or instances) are the exception: they are created again in place.

use std::collections::HashMap;

use super::helpers::*;
use crate::core::state_utils::StackResourceStateExt;
use crate::core::{allow_denied_replaces_to_retry, StackExecutor};
use crate::error::Result;
use crate::storage::{
    test_storage_deletes_issued, TestStorageController, TestStorageState,
    SIMULATE_STORAGE_CREATE_FAILURE_ORIGIN,
};
use crate::worker::{
    allow_test_worker_deletes, deny_test_worker_deletes, test_worker_deletes_denied,
    test_worker_deletes_issued, TestWorkerController, TestWorkerState,
};
use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt as _};
use alien_core::{
    ClientConfig, InitialSetupAuthority, Resource, ResourceLifecycle, ResourceRef, ResourceStatus,
    Stack, StackResourceState, StackState, StackStatus, Storage, Worker, WorkerCode,
};

const CREATE_WORKER_FAILURE: (&str, &str) = ("SIMULATE_CREATE_WORKER_FAILURE", "true");

fn worker(id: &str, image: &str, environment: &[(&str, &str)]) -> Worker {
    Worker::new(id.to_string())
        .code(WorkerCode::Image {
            image: image.to_string(),
        })
        .environment(
            environment
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect::<HashMap<_, _>>(),
        )
        .permissions("execution".to_string())
        .build()
}

fn single_worker_stack(worker: Worker, lifecycle: ResourceLifecycle) -> Stack {
    Stack::new("replace-test".to_owned())
        .add(worker, lifecycle)
        .build()
}

fn identifier(id: &str) -> String {
    format!("test:worker:{id}")
}

fn image(state: &StackState, id: &str) -> String {
    match &state.resources[id]
        .config
        .downcast_ref::<Worker>()
        .expect("worker config")
        .code
    {
        WorkerCode::Image { image } => image.clone(),
        other => panic!("expected an image worker, got {other:?}"),
    }
}

fn images(configs: &[Worker]) -> Vec<String> {
    configs
        .iter()
        .map(|config| match &config.code {
            WorkerCode::Image { image } => image.clone(),
            other => panic!("expected an image worker, got {other:?}"),
        })
        .collect()
}

/// Steps until `id` reaches `status`, failing the test if it never does.
async fn step_until(
    executor: &StackExecutor,
    mut state: StackState,
    id: &str,
    status: ResourceStatus,
) -> Result<StackState> {
    for _ in 0..40 {
        if get_status(&state, id) == Some(status) {
            return Ok(state);
        }
        state = executor.step(state).await?.next_state;
    }
    panic!(
        "'{id}' never reached {status:?}; last status {:?}",
        get_status(&state, id)
    );
}

/// Runs `stack` until `id` fails in CreateWorker, after CreateStart recorded its identifier.
async fn failed_after_first_mutation(stack: &Stack, id: &str) -> Result<StackState> {
    let executor = new_executor(stack)?;
    let state = step_until(
        &executor,
        new_test_state(),
        id,
        ResourceStatus::ProvisionFailed,
    )
    .await?;

    let failed = &state.resources[id];
    let controller = failed.get_internal_controller_typed::<TestWorkerController>()?;
    assert_eq!(controller.state, TestWorkerState::CreateFailed);
    assert_eq!(controller.identifier, Some(identifier(id)));
    let mut checkpoint = failed
        .get_last_failed_controller()?
        .expect("failed create keeps its checkpoint");
    assert!(
        checkpoint.transition_to_update().is_err(),
        "CreateWorker must not be a checkpoint the update flow can start from"
    );
    assert!(test_worker_deletes_issued(&identifier(id)).is_empty());
    Ok(state)
}

/// The failed create is deleted against its old config, then created with the new one.
#[tokio::test]
async fn failed_create_with_saved_ids_is_deleted_then_created_with_new_config() -> Result<()> {
    let id = "replace-saved-ids";
    let v1 = worker(id, "image-v1", &[CREATE_WORKER_FAILURE]);
    let state =
        failed_after_first_mutation(&single_worker_stack(v1, ResourceLifecycle::Live), id).await?;

    let v2 = worker(id, "image-v2", &[]);
    let executor = new_executor(&single_worker_stack(v2.clone(), ResourceLifecycle::Live))?;
    let plan = executor.plan(&state)?;
    assert_eq!(plan.replaces, vec![id.to_string()]);
    assert!(plan.creates.is_empty(), "{plan:?}");
    assert!(plan.updates.is_empty(), "{plan:?}");
    assert!(plan.deletes.is_empty(), "{plan:?}");

    let state = executor.step(state).await?.next_state;
    let deleting = &state.resources[id];
    assert_eq!(deleting.status, ResourceStatus::Deleting);
    assert_eq!(
        image(&state, id),
        "image-v1",
        "the delete keeps the created config"
    );
    assert!(
        deleting.last_failed_state.is_some(),
        "the failed create checkpoint is kept until a delete step removes something"
    );
    assert!(deleting.error.is_none());
    assert_eq!(
        deleting
            .get_internal_controller_typed::<TestWorkerController>()?
            .identifier,
        Some(identifier(id))
    );

    let state = run_to_synced(&executor, state).await?;
    let issued = test_worker_deletes_issued(&identifier(id));
    assert_eq!(images(&issued), vec!["image-v1"]);
    assert_eq!(
        issued[0].environment.get(CREATE_WORKER_FAILURE.0),
        Some(&"true".to_string()),
        "the delete handler sees the config the resource was created with"
    );

    let replaced = &state.resources[id];
    assert_eq!(replaced.status, ResourceStatus::Running);
    assert_eq!(replaced.config, Resource::new(v2));
    assert!(replaced.last_failed_state.is_none());
    let controller = replaced.get_internal_controller_typed::<TestWorkerController>()?;
    assert_eq!(controller.state, TestWorkerState::Ready);
    assert_eq!(controller.identifier, Some(identifier(id)));
    Ok(())
}

/// With no controller state nothing remote can exist, so the create simply restarts.
#[tokio::test]
async fn failed_create_without_controller_state_restarts_create() -> Result<()> {
    let id = "replace-no-state";
    let mut failed = create_provision_failed_function_state(id);
    failed.config = Resource::new(worker(id, "image-v1", &[]));
    let mut state = new_test_state();
    state.resources.insert(id.to_string(), failed);

    let executor = new_executor(&single_worker_stack(
        worker(id, "image-v2", &[]),
        ResourceLifecycle::Live,
    ))?;
    let plan = executor.plan(&state)?;
    assert_eq!(plan.creates, vec![id.to_string()]);
    assert!(plan.replaces.is_empty(), "{plan:?}");

    let state = run_to_synced(&executor, state).await?;
    assert_eq!(get_status(&state, id), Some(ResourceStatus::Running));
    assert_eq!(image(&state, id), "image-v2");
    assert!(test_worker_deletes_issued(&identifier(id)).is_empty());
    Ok(())
}

/// A replace whose delete keeps failing ends DeleteFailed with its IDs, never in a create.
/// The next attempt restarts the delete and the create follows it.
#[tokio::test]
async fn failed_replace_delete_is_retried_and_never_falls_back_to_create() -> Result<()> {
    let id = "replace-delete-fails";
    // Fails every delete attempt the executor makes before giving up.
    let v1 = worker(
        id,
        "image-v1",
        &[
            CREATE_WORKER_FAILURE,
            ("SIMULATE_DELETE_FAILURE_COUNT", "10"),
        ],
    );
    let mut state =
        failed_after_first_mutation(&single_worker_stack(v1, ResourceLifecycle::Live), id).await?;

    let executor = new_executor(&single_worker_stack(
        worker(id, "image-v2", &[]),
        ResourceLifecycle::Live,
    ))?;
    for _ in 0..40 {
        if get_status(&state, id) == Some(ResourceStatus::DeleteFailed) {
            break;
        }
        state = executor.step(state).await?.next_state;
        assert!(
            matches!(
                get_status(&state, id),
                Some(ResourceStatus::Deleting | ResourceStatus::DeleteFailed)
            ),
            "a failing replace delete must not reach {:?}",
            get_status(&state, id)
        );
    }

    let failed = &state.resources[id];
    assert_eq!(failed.status, ResourceStatus::DeleteFailed);
    assert_eq!(
        failed.error.as_ref().map(|error| error.code.as_str()),
        Some("EXECUTION_STEP_FAILED")
    );
    assert_eq!(image(&state, id), "image-v1");
    let controller = failed.get_internal_controller_typed::<TestWorkerController>()?;
    assert_eq!(controller.identifier, Some(identifier(id)));
    assert_eq!(controller.delete_failures_count, 10);
    assert_eq!(
        state.compute_stack_status().expect("stack status"),
        StackStatus::Failure
    );
    assert!(test_worker_deletes_issued(&identifier(id)).is_empty());

    let plan = executor.plan(&state)?;
    assert!(plan.creates.is_empty(), "{plan:?}");
    assert_eq!(plan.replaces, vec![id.to_string()]);

    let state = executor.step(state).await?.next_state;
    assert_eq!(get_status(&state, id), Some(ResourceStatus::Deleting));
    let state = run_to_synced(&executor, state).await?;
    assert_eq!(
        images(&test_worker_deletes_issued(&identifier(id))),
        vec!["image-v1"]
    );
    assert_eq!(get_status(&state, id), Some(ResourceStatus::Running));
    assert_eq!(image(&state, id), "image-v2");
    Ok(())
}

/// A replace delete that failed after its first step resumes at the step that failed. It
/// does not delete the worker again, whose not-found would end the whole delete as
/// best-effort and skip the steps still left.
#[tokio::test]
async fn failed_replace_delete_resumes_at_the_failed_step() -> Result<()> {
    let id = "replace-delete-resumes";
    let v1 = worker(
        id,
        "image-v1",
        &[
            CREATE_WORKER_FAILURE,
            ("SIMULATE_DELETED_WORKER_NOT_FOUND", "true"),
            ("SIMULATE_DELETE_POLL_FAILURE_COUNT", "10"),
        ],
    );
    let state =
        failed_after_first_mutation(&single_worker_stack(v1, ResourceLifecycle::Live), id).await?;

    let executor = new_executor(&single_worker_stack(
        worker(id, "image-v2", &[]),
        ResourceLifecycle::Live,
    ))?;
    let state = step_until(&executor, state, id, ResourceStatus::DeleteFailed).await?;
    let failed = &state.resources[id];
    let checkpoint: TestWorkerController = serde_json::from_value(
        failed
            .last_failed_state
            .clone()
            .expect("the failed delete keeps its checkpoint"),
    )
    .expect("checkpoint deserializes");
    assert_eq!(checkpoint.state, TestWorkerState::DeleteWorkerPolling);
    assert!(checkpoint.worker_delete_issued);
    assert_eq!(
        images(&test_worker_deletes_issued(&identifier(id))),
        vec!["image-v1"]
    );
    assert_eq!(executor.plan(&state)?.replaces, vec![id.to_string()]);

    let state = executor.step(state).await?.next_state;
    let resumed = &state.resources[id];
    assert_eq!(resumed.status, ResourceStatus::Deleting);
    assert!(resumed.error.is_none());
    assert_eq!(
        resumed
            .get_internal_controller_typed::<TestWorkerController>()?
            .state,
        TestWorkerState::DeleteWorkerPolling,
        "the delete resumes at the step that failed"
    );

    let state = step_until(&executor, state, id, ResourceStatus::Deleted).await?;
    let deleted = &state.resources[id];
    assert_eq!(
        deleted
            .get_internal_controller_typed::<TestWorkerController>()?
            .state,
        TestWorkerState::Deleted,
        "the delete finished its own steps instead of ending on a best-effort not-found"
    );
    assert_eq!(
        test_worker_deletes_issued(&identifier(id)).len(),
        1,
        "the worker is not deleted again"
    );

    let state = run_to_synced(&executor, state).await?;
    assert_eq!(get_status(&state, id), Some(ResourceStatus::Running));
    assert_eq!(image(&state, id), "image-v2");
    Ok(())
}

/// A not-found answer ends the delete as best-effort, and the create follows.
#[tokio::test]
async fn replace_delete_ended_by_not_found_creates_with_new_config() -> Result<()> {
    let id = "replace-delete-not-found";
    let v1 = worker(
        id,
        "image-v1",
        &[CREATE_WORKER_FAILURE, ("SIMULATE_DELETE_NOT_FOUND", "true")],
    );
    let state =
        failed_after_first_mutation(&single_worker_stack(v1, ResourceLifecycle::Live), id).await?;

    let executor = new_executor(&single_worker_stack(
        worker(id, "image-v2", &[]),
        ResourceLifecycle::Live,
    ))?;
    let state = step_until(&executor, state, id, ResourceStatus::Deleted).await?;
    assert!(
        !state.resources[id].has_internal_state(),
        "a best-effort delete drops the controller"
    );
    assert_eq!(
        images(&test_worker_deletes_issued(&identifier(id))),
        vec!["image-v1"]
    );

    let state = run_to_synced(&executor, state).await?;
    assert_eq!(get_status(&state, id), Some(ResourceStatus::Running));
    assert_eq!(image(&state, id), "image-v2");
    Ok(())
}

/// Access denied does not end a replace delete. The resource is still desired, so taking the
/// denial as "deleted" would create a second one next to the first. The delete fails with the
/// error and keeps the IDs; nothing is created. (A delete of a resource the stack no longer
/// declares still treats access denied as best-effort.)
#[tokio::test]
async fn replace_delete_denied_access_fails_and_does_not_create() -> Result<()> {
    let id = "replace-delete-denied";
    let v1 = worker(
        id,
        "image-v1",
        &[
            CREATE_WORKER_FAILURE,
            ("SIMULATE_DELETE_ACCESS_DENIED", "true"),
        ],
    );
    let mut state =
        failed_after_first_mutation(&single_worker_stack(v1, ResourceLifecycle::Live), id).await?;

    let executor = new_executor(&single_worker_stack(
        worker(id, "image-v2", &[]),
        ResourceLifecycle::Live,
    ))?;
    for _ in 0..40 {
        if get_status(&state, id) == Some(ResourceStatus::DeleteFailed) {
            break;
        }
        state = executor.step(state).await?.next_state;
        assert!(
            matches!(
                get_status(&state, id),
                Some(ResourceStatus::Deleting | ResourceStatus::DeleteFailed)
            ),
            "a denied replace delete must not reach {:?}",
            get_status(&state, id)
        );
    }

    let failed = &state.resources[id];
    assert_eq!(failed.status, ResourceStatus::DeleteFailed);
    let error = failed.error.as_ref().expect("the denial is recorded");
    let mut codes = vec![error.code.clone()];
    let mut source = error.source.as_deref();
    while let Some(inner) = source {
        codes.push(inner.code.clone());
        source = inner.source.as_deref();
    }
    assert_eq!(codes[0], "REPLACE_DELETE_DENIED", "{codes:?}");
    assert!(
        error.message.contains("may already be deleted")
            && !error.message.contains("revert the configuration"),
        "a denial after the worker was deleted cannot offer a revert: {}",
        error.message
    );
    assert!(
        codes.iter().any(|code| code == "REMOTE_ACCESS_DENIED"),
        "{codes:?}"
    );
    let plan = executor.plan(&state)?;
    assert!(
        plan.replaces.is_empty(),
        "a denied replace waits for an explicit retry: {plan:?}"
    );
    assert_eq!(image(&state, id), "image-v1");
    assert_eq!(
        failed
            .get_internal_controller_typed::<TestWorkerController>()?
            .identifier,
        Some(identifier(id)),
        "the IDs stay for the next delete attempt"
    );
    assert_eq!(
        images(&test_worker_deletes_issued(&identifier(id))),
        vec!["image-v1"]
    );
    Ok(())
}

/// A dependent that never started cannot be using the failed resource, so it does not hold
/// up the replace, and is created once the replacement is Running.
#[tokio::test]
async fn pending_dependent_does_not_block_replace() -> Result<()> {
    let id = "replace-with-pending-dependent";
    let dependent = test_function("pending-dependent-of-replace");
    let depends_on_replaced = vec![ResourceRef::new(Worker::RESOURCE_TYPE, id)];
    let stack = |image: &str, environment: &[(&str, &str)]| {
        Stack::new("replace-dependents".to_owned())
            .add(worker(id, image, environment), ResourceLifecycle::Live)
            .add_with_dependencies(
                dependent.clone(),
                ResourceLifecycle::Live,
                depends_on_replaced.clone(),
            )
            .build()
    };
    let state =
        failed_after_first_mutation(&stack("image-v1", &[CREATE_WORKER_FAILURE]), id).await?;
    assert_eq!(
        get_status(&state, "pending-dependent-of-replace"),
        Some(ResourceStatus::Pending)
    );
    assert!(!state.resources["pending-dependent-of-replace"].has_internal_state());

    let executor = new_executor(&stack("image-v2", &[]))?;
    let state = executor.step(state).await?.next_state;
    assert_eq!(get_status(&state, id), Some(ResourceStatus::Deleting));

    // Once the failed resource is Deleted, the dependent must keep waiting for its
    // replacement instead of treating the deleted dependency as settled.
    let state = step_until(&executor, state, id, ResourceStatus::Deleted).await?;
    assert_eq!(
        get_status(&state, "pending-dependent-of-replace"),
        Some(ResourceStatus::Pending)
    );
    let state = executor.step(state).await?.next_state;
    assert_eq!(
        get_status(&state, "pending-dependent-of-replace"),
        Some(ResourceStatus::Pending),
        "the dependent waits while the replacement provisions"
    );

    let state = run_to_synced(&executor, state).await?;
    assert_all_running(&state, &[id, "pending-dependent-of-replace"]);
    assert_eq!(image(&state, id), "image-v2");
    assert_eq!(
        images(&test_worker_deletes_issued(&identifier(id))),
        vec!["image-v1"]
    );
    Ok(())
}

/// A dependent whose controller has started may hold the failed resource, so it blocks the
/// replace delete.
#[tokio::test]
async fn dependent_with_controller_state_blocks_replace() -> Result<()> {
    let id = "replace-with-active-dependent";
    let dependent_id = "active-dependent-of-replace";
    let dependent = test_function(dependent_id);
    let v1 = Stack::new("replace-dependents".to_owned())
        .add(
            worker(id, "image-v1", &[CREATE_WORKER_FAILURE]),
            ResourceLifecycle::Live,
        )
        .add(dependent.clone(), ResourceLifecycle::Live)
        .build();
    let v1_executor = new_executor(&v1)?;
    let mut state = step_until(
        &v1_executor,
        new_test_state(),
        id,
        ResourceStatus::ProvisionFailed,
    )
    .await?;
    state = step_until(&v1_executor, state, dependent_id, ResourceStatus::Running).await?;
    // The dependent's recorded dependencies say it holds the failed resource.
    let depends_on_replaced = vec![ResourceRef::new(Worker::RESOURCE_TYPE, id)];
    state.resources.get_mut(dependent_id).unwrap().dependencies = depends_on_replaced.clone();

    let v2 = Stack::new("replace-dependents".to_owned())
        .add(worker(id, "image-v2", &[]), ResourceLifecycle::Live)
        .add_with_dependencies(dependent, ResourceLifecycle::Live, depends_on_replaced)
        .build();
    let executor = new_executor(&v2)?;
    assert_eq!(executor.plan(&state)?.replaces, vec![id.to_string()]);

    for _ in 0..3 {
        state = executor.step(state).await?.next_state;
        assert_eq!(
            get_status(&state, id),
            Some(ResourceStatus::ProvisionFailed)
        );
        assert_eq!(
            get_status(&state, dependent_id),
            Some(ResourceStatus::Running)
        );
    }
    assert!(test_worker_deletes_issued(&identifier(id)).is_empty());
    Ok(())
}

/// Runtime credentials never delete a setup-owned resource: the failed create stays as it is,
/// with an error that says setup must run, until an executor running as setup replaces it.
#[tokio::test]
async fn setup_owned_failed_create_is_replaced_only_by_setup() -> Result<()> {
    let id = "replace-frozen";
    let v1 = worker(id, "image-v1", &[CREATE_WORKER_FAILURE]);
    let mut state =
        failed_after_first_mutation(&single_worker_stack(v1, ResourceLifecycle::Frozen), id)
            .await?;
    let create_error = state.resources[id]
        .error
        .clone()
        .expect("failed create records its error");

    let v2 = single_worker_stack(worker(id, "image-v2", &[]), ResourceLifecycle::Frozen);
    let runtime = new_executor(&v2)?;
    let plan = runtime.plan(&state)?;
    assert_eq!(plan.setup_required, vec![id.to_string()]);
    assert!(plan.replaces.is_empty(), "{plan:?}");
    assert!(plan.creates.is_empty(), "{plan:?}");
    assert!(plan.updates.is_empty(), "{plan:?}");

    for _ in 0..3 {
        state = runtime.step(state).await?.next_state;
        let failed = &state.resources[id];
        assert_eq!(failed.status, ResourceStatus::ProvisionFailed);
        assert_eq!(image(&state, id), "image-v1");
        assert_eq!(
            failed
                .get_internal_controller_typed::<TestWorkerController>()?
                .identifier,
            Some(identifier(id))
        );
        let error = failed.error.as_ref().expect("setup-required error");
        assert_eq!(error.code, "RESOURCE_CONFIG_INVALID");
        assert!(error.message.contains("rerun setup"), "{}", error.message);
        let cause = error
            .source
            .as_deref()
            .expect("the create error is the cause");
        assert_eq!(cause.message, create_error.message);
        assert!(
            cause.source.is_none(),
            "planning again must not wrap the error again"
        );
    }
    assert!(test_worker_deletes_issued(&identifier(id)).is_empty());

    let setup = StackExecutor::builder(&v2, ClientConfig::Test)
        .deployment_config(&default_deployment_config())
        .initial_setup_authority(InitialSetupAuthority::DirectSetup)
        .build()?;
    assert_eq!(setup.plan(&state)?.replaces, vec![id.to_string()]);
    let state = run_to_synced(&setup, state).await?;
    assert_eq!(get_status(&state, id), Some(ResourceStatus::Running));
    assert_eq!(image(&state, id), "image-v2");
    assert_eq!(
        images(&test_worker_deletes_issued(&identifier(id))),
        vec!["image-v1"]
    );
    Ok(())
}

/// A data-holding resource is never deleted to be replaced: its create may have adopted a
/// bucket that already holds data. With saved controller state and a config change it is
/// created again in place, while a worker failed the same way in the same stack is still
/// deleted and recreated.
#[tokio::test]
async fn failed_storage_create_is_created_again_in_place_instead_of_deleted() -> Result<()> {
    let store_id = "replace-data-store";
    let worker_id = "replace-data-worker";
    let stack = |storage: Storage, worker: Worker| {
        Stack::new("replace-test".to_owned())
            .add(storage, ResourceLifecycle::Live)
            .add(worker, ResourceLifecycle::Live)
            .build()
    };
    let failing_store = Storage::new(store_id.to_string())
        .cors_allowed_origins(vec![SIMULATE_STORAGE_CREATE_FAILURE_ORIGIN.to_string()])
        .build();
    let v1 = stack(
        failing_store,
        worker(worker_id, "image-v1", &[CREATE_WORKER_FAILURE]),
    );

    let executor = new_executor(&v1)?;
    let mut state = new_test_state();
    for _ in 0..40 {
        if get_status(&state, store_id) == Some(ResourceStatus::ProvisionFailed)
            && get_status(&state, worker_id) == Some(ResourceStatus::ProvisionFailed)
        {
            break;
        }
        state = executor.step(state).await?.next_state;
    }
    let failed_store = &state.resources[store_id];
    assert_eq!(failed_store.status, ResourceStatus::ProvisionFailed);
    let bucket = failed_store
        .get_internal_controller_typed::<TestStorageController>()?
        .bucket_name
        .clone()
        .expect("the failed create recorded its bucket");
    assert!(failed_store.last_failed_state.is_some());
    assert_eq!(
        get_status(&state, worker_id),
        Some(ResourceStatus::ProvisionFailed)
    );

    let fixed_store = Storage::new(store_id.to_string()).versioning(true).build();
    let v2 = stack(fixed_store.clone(), worker(worker_id, "image-v2", &[]));
    let executor = new_executor(&v2)?;
    let plan = executor.plan(&state)?;
    assert_eq!(plan.creates, vec![store_id.to_string()], "{plan:?}");
    assert_eq!(plan.replaces, vec![worker_id.to_string()], "{plan:?}");
    assert!(plan.deletes.is_empty(), "{plan:?}");

    let state = executor.step(state).await?.next_state;
    assert_eq!(
        get_status(&state, store_id),
        Some(ResourceStatus::Provisioning),
        "the store goes straight to its create, not to a delete"
    );
    assert_eq!(
        get_status(&state, worker_id),
        Some(ResourceStatus::Deleting)
    );

    let state = run_to_synced(&executor, state).await?;
    assert!(
        test_storage_deletes_issued(store_id).is_empty(),
        "the store's bucket must never be deleted"
    );
    let store = &state.resources[store_id];
    assert_eq!(store.status, ResourceStatus::Running);
    assert_eq!(store.config, Resource::new(fixed_store));
    assert_eq!(
        store
            .get_internal_controller_typed::<TestStorageController>()?
            .bucket_name,
        Some(bucket),
        "the new create finds the same bucket by its name"
    );

    assert_eq!(
        images(&test_worker_deletes_issued(&identifier(worker_id))),
        vec!["image-v1"]
    );
    assert_eq!(get_status(&state, worker_id), Some(ResourceStatus::Running));
    assert_eq!(image(&state, worker_id), "image-v2");
    Ok(())
}

/// A data-holding resource left DeleteFailed while still desired with a changed config is
/// not handed to the replace path, which would finish deleting it.
#[tokio::test]
async fn delete_failed_storage_with_a_changed_config_is_not_replaced() -> Result<()> {
    let store_id = "replace-data-delete-failed";
    let mut state = new_test_state();
    let mut failed = StackResourceState::new_pending(
        Storage::RESOURCE_TYPE.to_string(),
        Resource::new(Storage::new(store_id.to_string()).build()),
        Some(ResourceLifecycle::Live),
        vec![],
    );
    failed.status = ResourceStatus::DeleteFailed;
    failed.set_internal_controller(Some(Box::new(TestStorageController {
        state: TestStorageState::DeleteFailed,
        bucket_name: Some(format!("{}-{store_id}", state.resource_prefix)),
        ready_checks: 0,
        convergence_reconciliation: false,
        _internal_stay_count: None,
    })))?;
    state.resources.insert(store_id.to_string(), failed);

    let executor = new_executor(
        &Stack::new("replace-test".to_owned())
            .add(
                Storage::new(store_id.to_string()).versioning(true).build(),
                ResourceLifecycle::Live,
            )
            .build(),
    )?;
    let plan = executor.plan(&state)?;
    assert!(plan.replaces.is_empty(), "{plan:?}");
    assert!(plan.creates.is_empty(), "{plan:?}");
    assert!(plan.deletes.is_empty(), "{plan:?}");

    let state = executor.step(state).await?.next_state;
    assert_eq!(
        get_status(&state, store_id),
        Some(ResourceStatus::DeleteFailed)
    );
    assert!(test_storage_deletes_issued(store_id).is_empty());
    Ok(())
}

fn capacity_group(max_size: u32) -> alien_core::CapacityGroup {
    alien_core::CapacityGroup {
        group_id: "general".to_string(),
        instance_type: None,
        profile: None,
        min_size: 1,
        max_size,
        scale_policy: None,
        nested_virtualization: None,
    }
}

/// A failed create recorded with its controller state, as the executor leaves it.
fn failed_create(
    config: Resource,
    controller: Box<dyn crate::core::ResourceController>,
) -> Result<StackResourceState> {
    let mut failed = StackResourceState::new_pending(
        config.resource_type().to_string(),
        config,
        Some(ResourceLifecycle::Live),
        vec![],
    );
    failed.status = ResourceStatus::ProvisionFailed;
    failed.set_internal_controller(Some(controller.clone()))?;
    failed.set_last_failed_controller(Some(controller))?;
    Ok(failed)
}

/// Daemons and compute clusters are created again in place instead of replaced: a create
/// that failed late may already run replicas or instances, which a delete would stop.
#[tokio::test]
async fn failed_daemon_and_compute_cluster_creates_are_created_again_not_replaced() -> Result<()> {
    let daemon = |image: &str| {
        alien_core::Daemon::new("agent".to_string())
            .code(alien_core::DaemonCode::Image {
                image: image.to_string(),
            })
            .permissions("execution".to_string())
            .build()
    };
    let cluster = |max_size: u32| {
        alien_core::ComputeCluster::new("compute".to_string())
            .capacity_group(capacity_group(max_size))
            .build()
    };

    let mut state = StackState::new(alien_core::Platform::Kubernetes);
    state.resources.insert(
        "agent".to_string(),
        failed_create(
            Resource::new(daemon("agent:v1")),
            Box::new(crate::daemon::KubernetesDaemonController {
                state: crate::daemon::KubernetesDaemonState::CreateFailed,
                ..Default::default()
            }),
        )?,
    );
    state.resources.insert(
        "compute".to_string(),
        failed_create(
            Resource::new(cluster(2)),
            Box::new(crate::compute_cluster::KubernetesComputeClusterController {
                state: crate::compute_cluster::KubernetesComputeClusterState::ProvisionFailed,
                ..Default::default()
            }),
        )?,
    );

    let stack = Stack::new("replace-test".to_owned())
        .add(daemon("agent:v2"), ResourceLifecycle::Live)
        .add(cluster(3), ResourceLifecycle::Live)
        .build();
    let executor = StackExecutor::builder(
        &stack,
        ClientConfig::Kubernetes(Box::new(alien_core::KubernetesClientConfig::InCluster {
            namespace: Some("application".to_string()),
            additional_headers: None,
        })),
    )
    .deployment_config(&default_deployment_config())
    .build()?;
    let plan = executor.plan(&state)?;
    let mut creates = plan.creates.clone();
    creates.sort();
    assert_eq!(
        creates,
        vec!["agent".to_string(), "compute".to_string()],
        "{plan:?}"
    );
    assert!(plan.replaces.is_empty(), "{plan:?}");
    assert!(plan.deletes.is_empty(), "{plan:?}");
    Ok(())
}

/// A failed create whose replace delete was denied at its second step (the one that deletes the
/// worker), after a first step that deleted nothing. Returns the state after the denial, with
/// the deletes still denied.
async fn replace_denied_before_anything_was_deleted(id: &str) -> Result<StackState> {
    let v1 = worker(id, "image-v1", &[CREATE_WORKER_FAILURE]);
    let mut state =
        failed_after_first_mutation(&single_worker_stack(v1, ResourceLifecycle::Live), id).await?;
    deny_test_worker_deletes(&identifier(id));

    let executor = new_executor(&single_worker_stack(
        worker(id, "image-v2", &[]),
        ResourceLifecycle::Live,
    ))?;
    for _ in 0..40 {
        if state.resources[id]
            .error
            .as_ref()
            .is_some_and(|error| error.code == "REPLACE_DELETE_DENIED")
        {
            break;
        }
        state = executor.step(state).await?.next_state;
        assert!(
            matches!(
                get_status(&state, id),
                Some(ResourceStatus::Deleting | ResourceStatus::ProvisionFailed)
            ),
            "{:?}",
            get_status(&state, id)
        );
    }

    let failed = &state.resources[id];
    assert_eq!(failed.status, ResourceStatus::ProvisionFailed);
    let error = failed.error.as_ref().expect("the denial is recorded");
    assert_eq!(error.code, "REPLACE_DELETE_DENIED");
    assert!(
        error.message.contains(&identifier(id))
            && error.message.contains("Nothing was deleted")
            && error.message.contains("revert the configuration"),
        "{}",
        error.message
    );
    assert_eq!(image(&state, id), "image-v1");
    let restored = failed.get_internal_controller_typed::<TestWorkerController>()?;
    assert_eq!(restored.state, TestWorkerState::CreateFailed);
    assert_eq!(restored.identifier, Some(identifier(id)));
    let checkpoint = failed
        .get_last_failed_controller()?
        .expect("the failed create checkpoint is restored");
    let checkpoint = checkpoint
        .as_any()
        .downcast_ref::<TestWorkerController>()
        .expect("a worker checkpoint");
    assert_eq!(checkpoint.state, TestWorkerState::CreateWorker);
    assert!(test_worker_deletes_issued(&identifier(id)).is_empty());
    assert_eq!(test_worker_deletes_denied(&identifier(id)), 1);

    // The denied delete is not repeated on every step.
    let mut state = state;
    for _ in 0..3 {
        state = executor.step(state).await?.next_state;
    }
    assert_eq!(
        get_status(&state, id),
        Some(ResourceStatus::ProvisionFailed)
    );
    assert_eq!(test_worker_deletes_denied(&identifier(id)), 1);
    Ok(state)
}

/// A replace delete denied before anything was deleted aborts the replace and puts the failed
/// create back; it is not tried again until an explicit retry.
#[tokio::test]
async fn denied_replace_delete_restores_the_failed_create() -> Result<()> {
    let id = "replace-denied-restores";
    replace_denied_before_anything_was_deleted(id).await?;
    allow_test_worker_deletes(&identifier(id));
    Ok(())
}

/// Reverting the config to what the failed create used resumes that create at its saved step.
#[tokio::test]
async fn reverting_after_a_denied_replace_resumes_the_failed_create() -> Result<()> {
    let id = "replace-denied-reverted";
    let state = replace_denied_before_anything_was_deleted(id).await?;

    let reverted = new_update_executor(&single_worker_stack(
        worker(id, "image-v1", &[CREATE_WORKER_FAILURE]),
        ResourceLifecycle::Live,
    ))?;
    let plan = reverted.plan(&state)?;
    assert!(
        plan.replaces.is_empty() && plan.creates.is_empty(),
        "{plan:?}"
    );
    let state = reverted.step(state).await?.next_state;
    // The create resumed at CreateWorker, which fails again for this config: the recorded
    // error is that create failure now, not the denial.
    let resumed = &state.resources[id];
    let error = resumed.error.as_ref().expect("the create failed again");
    assert_ne!(error.code, "REPLACE_DELETE_DENIED");
    assert!(
        error.message.contains("Simulated CreateWorker failure"),
        "{}",
        error.message
    );
    let checkpoint = resumed
        .get_last_failed_controller()?
        .expect("the create checkpoint");
    let checkpoint = checkpoint
        .as_any()
        .downcast_ref::<TestWorkerController>()
        .expect("a worker checkpoint");
    assert_eq!(checkpoint.state, TestWorkerState::CreateWorker);
    assert_eq!(checkpoint.identifier, Some(identifier(id)));
    assert_eq!(test_worker_deletes_denied(&identifier(id)), 1);
    allow_test_worker_deletes(&identifier(id));
    Ok(())
}

/// Removing the resource from the stack deletes it, with the usual best-effort rule: a denied
/// delete of a resource nobody wants any more counts as deleted.
#[tokio::test]
async fn removing_a_resource_after_a_denied_replace_deletes_it() -> Result<()> {
    let id = "replace-denied-removed";
    let state = replace_denied_before_anything_was_deleted(id).await?;

    let without = new_executor(&Stack::new("replace-test".to_owned()).build())?;
    assert_eq!(without.plan(&state)?.deletes, vec![id.to_string()]);
    let state = run_to_synced(&without, state).await?;
    assert_eq!(get_status(&state, id), Some(ResourceStatus::Deleted));
    assert_eq!(test_worker_deletes_denied(&identifier(id)), 2);
    allow_test_worker_deletes(&identifier(id));
    Ok(())
}

/// Once the permission is granted, an explicit retry completes the replace.
#[tokio::test]
async fn retrying_a_denied_replace_after_the_permission_is_granted_completes_it() -> Result<()> {
    let id = "replace-denied-granted";
    let mut state = replace_denied_before_anything_was_deleted(id).await?;
    assert_eq!(allow_test_worker_deletes(&identifier(id)), 1);

    assert_eq!(
        allow_denied_replaces_to_retry(&mut state),
        vec![id.to_string()]
    );
    let executor = new_executor(&single_worker_stack(
        worker(id, "image-v2", &[]),
        ResourceLifecycle::Live,
    ))?;
    assert_eq!(executor.plan(&state)?.replaces, vec![id.to_string()]);
    let state = run_to_synced(&executor, state).await?;
    assert_eq!(get_status(&state, id), Some(ResourceStatus::Running));
    assert_eq!(image(&state, id), "image-v2");
    assert_eq!(
        images(&test_worker_deletes_issued(&identifier(id))),
        vec!["image-v1"]
    );
    Ok(())
}

/// A replace denied after a delete step succeeded stays DeleteFailed; removing the resource
/// from the stack then resumes and finishes that delete.
#[tokio::test]
async fn removing_a_resource_whose_replace_failed_mid_delete_finishes_the_delete() -> Result<()> {
    let id = "replace-denied-mid-delete-removed";
    let v1 = worker(
        id,
        "image-v1",
        &[
            CREATE_WORKER_FAILURE,
            ("SIMULATE_DELETE_ACCESS_DENIED", "true"),
        ],
    );
    let state =
        failed_after_first_mutation(&single_worker_stack(v1, ResourceLifecycle::Live), id).await?;
    let executor = new_executor(&single_worker_stack(
        worker(id, "image-v2", &[]),
        ResourceLifecycle::Live,
    ))?;
    let state = step_until(&executor, state, id, ResourceStatus::DeleteFailed).await?;
    assert!(
        !state.resources[id].has_last_failed_state()
            || state.resources[id]
                .get_last_failed_controller()?
                .is_some_and(|checkpoint| checkpoint.get_status() == ResourceStatus::Deleting),
        "the failed create checkpoint is gone once a delete step succeeded"
    );

    let without = new_update_executor(&Stack::new("replace-test".to_owned()).build())?;
    assert_eq!(without.plan(&state)?.deletes, vec![id.to_string()]);
    let state = step_until(&without, state, id, ResourceStatus::Deleted).await?;
    assert!(state.resources[id].error.is_none());
    Ok(())
}

/// An artifact registry and a sandbox are created again in place instead of replaced: their
/// creates adopt an existing repository or image by name, which a delete would remove with
/// images this deployment never proved it owns.
#[tokio::test]
async fn failed_artifact_registry_and_sandbox_creates_are_created_again_not_replaced() -> Result<()>
{
    let registry = |replication: bool| {
        let mut registry = alien_core::ArtifactRegistry::new("images".to_string()).build();
        if replication {
            registry.replication_regions = vec!["us-west-2".to_string()];
        }
        registry
    };
    let sandbox = |bundle: &str| {
        alien_core::Sandbox::new("agents".to_string())
            .code(alien_core::SandboxCode::Image {
                image: bundle.to_string(),
            })
            .egress(alien_core::SandboxEgress::Deny)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: Some(1800),
                idle_pause_seconds: Some(600),
            })
            .build()
    };

    let mut state = StackState::new(alien_core::Platform::Aws);
    state.resources.insert(
        "images".to_string(),
        failed_create(
            Resource::new(registry(false)),
            Box::new(crate::artifact_registry::AwsArtifactRegistryController {
                state: crate::artifact_registry::AwsArtifactRegistryState::CreateFailed,
                ..Default::default()
            }),
        )?,
    );
    state.resources.insert(
        "agents".to_string(),
        failed_create(
            Resource::new(sandbox("s3://bundles/v1.zip")),
            Box::new(crate::sandbox::AwsSandboxController {
                state: crate::sandbox::AwsSandboxState::ProvisionFailed,
                ..Default::default()
            }),
        )?,
    );

    let stack = Stack::new("replace-test".to_owned())
        .add(registry(true), ResourceLifecycle::Live)
        .add(sandbox("s3://bundles/v2.zip"), ResourceLifecycle::Live)
        .build();
    let executor =
        StackExecutor::builder(&stack, ClientConfig::Aws(Box::new(AwsClientConfig::mock())))
            .deployment_config(&default_deployment_config())
            .build()?;
    let plan = executor.plan(&state)?;
    let mut creates = plan.creates.clone();
    creates.sort();
    assert_eq!(
        creates,
        vec!["agents".to_string(), "images".to_string()],
        "{plan:?}"
    );
    assert!(plan.replaces.is_empty(), "{plan:?}");
    Ok(())
}

/// A replace whose delete failed after it deleted the worker cannot resume the failed create.
/// Reverting the config to what that create used finishes the delete and creates the worker
/// again, rather than leaving the half-deleted resource as it is.
#[tokio::test]
async fn reverting_after_a_replace_failed_mid_delete_finishes_the_delete_and_creates_again(
) -> Result<()> {
    let id = "replace-mid-delete-reverted";
    let v1 = worker(
        id,
        "image-v1",
        &[
            CREATE_WORKER_FAILURE,
            ("SIMULATE_DELETE_POLL_FAILURE_COUNT", "10"),
        ],
    );
    let state = failed_after_first_mutation(
        &single_worker_stack(v1.clone(), ResourceLifecycle::Live),
        id,
    )
    .await?;
    let replace = new_executor(&single_worker_stack(
        worker(id, "image-v2", &[]),
        ResourceLifecycle::Live,
    ))?;
    let state = step_until(&replace, state, id, ResourceStatus::DeleteFailed).await?;
    assert_eq!(
        images(&test_worker_deletes_issued(&identifier(id))),
        vec!["image-v1"],
        "the worker itself was deleted before the delete failed"
    );

    let reverted = new_executor(&single_worker_stack(v1, ResourceLifecycle::Live))?;
    assert_eq!(reverted.plan(&state)?.replaces, vec![id.to_string()]);
    let state = step_until(&reverted, state, id, ResourceStatus::Deleted).await?;
    assert!(state.resources[id].error.is_none());
    let state = reverted.step(state).await?.next_state;
    assert_eq!(
        get_status(&state, id),
        Some(ResourceStatus::Provisioning),
        "the worker is created again"
    );
    Ok(())
}
