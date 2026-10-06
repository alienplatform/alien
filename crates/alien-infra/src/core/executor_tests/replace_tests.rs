//! Tests for replacing a failed create whose controller state records what it already made.
//!
//! A config change on such a resource must delete those cloud resources, against the config
//! they were created with, before creating it again. A fresh create would drop the saved IDs
//! and leak everything the failed create made.

use std::collections::HashMap;

use super::helpers::*;
use crate::core::state_utils::StackResourceStateExt;
use crate::core::StackExecutor;
use crate::error::Result;
use crate::worker::{test_worker_deletes_issued, TestWorkerController, TestWorkerState};
use alien_core::{
    ClientConfig, InitialSetupAuthority, Resource, ResourceLifecycle, ResourceRef, ResourceStatus,
    Stack, StackState, StackStatus, Worker, WorkerCode,
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
    assert!(deleting.last_failed_state.is_none());
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

/// A best-effort not-found or access-denied answer ends the delete, and the create follows.
#[tokio::test]
async fn replace_delete_ended_by_best_effort_error_creates_with_new_config() -> Result<()> {
    for (id, flag) in [
        ("replace-delete-not-found", "SIMULATE_DELETE_NOT_FOUND"),
        ("replace-delete-denied", "SIMULATE_DELETE_ACCESS_DENIED"),
    ] {
        let v1 = worker(id, "image-v1", &[CREATE_WORKER_FAILURE, (flag, "true")]);
        let state =
            failed_after_first_mutation(&single_worker_stack(v1, ResourceLifecycle::Live), id)
                .await?;

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
        assert_eq!(
            get_status(&state, id),
            Some(ResourceStatus::Running),
            "{flag}"
        );
        assert_eq!(image(&state, id), "image-v2", "{flag}");
    }
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
