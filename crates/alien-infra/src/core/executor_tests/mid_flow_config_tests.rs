//! Tests for a desired config that changes while a create or update is in flight.
//!
//! The flow finishes with the config it started with, and that config stays recorded, so the
//! record matches what the cloud holds. Once the resource is Running the planner sees the
//! newer desired config and updates it. Without this, the record would say the new config
//! while the resource ran the old one, and no update would ever be planned.

use std::collections::HashMap;

use super::helpers::*;
use crate::core::state_utils::StackResourceStateExt;
use crate::core::StackExecutor;
use crate::error::Result;
use crate::worker::{
    test_worker_configs_deployed, test_worker_deletes_issued, TestWorkerController, TestWorkerState,
};
use alien_core::{ResourceLifecycle, ResourceStatus, Stack, StackState, Worker, WorkerCode};

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

fn stack_of(worker: Worker) -> Stack {
    Stack::new("mid-flow-config-test".to_owned())
        .add(worker, ResourceLifecycle::Live)
        .build()
}

fn identifier(id: &str) -> String {
    format!("test:worker:{id}")
}

fn image_of(config: &Worker) -> String {
    match &config.code {
        WorkerCode::Image { image } => image.clone(),
        other => panic!("expected an image worker, got {other:?}"),
    }
}

fn recorded_image(state: &StackState, id: &str) -> String {
    image_of(
        state.resources[id]
            .config
            .downcast_ref::<Worker>()
            .expect("worker config"),
    )
}

fn previous_image(state: &StackState, id: &str) -> Option<String> {
    state.resources[id]
        .previous_config
        .as_ref()
        .map(|config| image_of(config.downcast_ref::<Worker>().expect("worker config")))
}

fn deployed_images(id: &str) -> Vec<String> {
    test_worker_configs_deployed(&identifier(id))
        .iter()
        .map(image_of)
        .collect()
}

fn controller_state(state: &StackState, id: &str) -> TestWorkerState {
    state.resources[id]
        .get_internal_controller_typed::<TestWorkerController>()
        .expect("controller state deserializes")
        .state
}

/// Steps until the controller of `id` is in `target`, failing the test if it never gets there.
async fn step_until_controller(
    executor: &StackExecutor,
    mut state: StackState,
    id: &str,
    target: TestWorkerState,
) -> Result<StackState> {
    for _ in 0..40 {
        if state.resources.get(id).is_some_and(|resource| {
            resource.internal_state.is_some() && controller_state(&state, id) == target
        }) {
            return Ok(state);
        }
        state = executor.step(state).await?.next_state;
    }
    panic!("'{id}' never reached controller state {target:?}");
}

async fn step_until_status(
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

fn assert_nothing_planned(executor: &StackExecutor, state: &StackState) -> Result<()> {
    let plan = executor.plan(state)?;
    assert!(
        plan.creates.is_empty()
            && plan.updates.is_empty()
            && plan.deletes.is_empty()
            && plan.replaces.is_empty()
            && plan.setup_required.is_empty(),
        "nothing should be left to plan, got {plan:?}"
    );
    Ok(())
}

/// The desired image changes after the create already deployed the old one. The record keeps
/// the image the worker got until the create is Running, then an update deploys the new one.
#[tokio::test]
async fn create_that_deployed_the_old_config_is_updated_once_running() -> Result<()> {
    let id = "mid-create-change";
    let executor_v1 = new_executor(&stack_of(worker(id, "image-v1", &[])))?;
    let state = step_until_controller(
        &executor_v1,
        new_test_state(),
        id,
        TestWorkerState::CreateWorkerPolling,
    )
    .await?;
    assert_eq!(deployed_images(id), vec!["image-v1"]);

    let executor_v2 = new_executor(&stack_of(worker(id, "image-v2", &[])))?;
    let plan = executor_v2.plan(&state)?;
    assert!(
        plan.updates.is_empty() && plan.creates.is_empty() && plan.replaces.is_empty(),
        "the create in flight is not interrupted, got {plan:?}"
    );

    // The create finishes with the config it started with, and says so.
    let state = step_until_status(&executor_v2, state, id, ResourceStatus::Running).await?;
    assert_eq!(deployed_images(id), vec!["image-v1"]);
    assert_eq!(
        recorded_image(&state, id),
        "image-v1",
        "the record must say what the worker runs"
    );
    let plan = executor_v2.plan(&state)?;
    assert_eq!(
        plan.updates.keys().collect::<Vec<_>>(),
        vec![id],
        "the change that arrived mid-create is planned as an update"
    );

    let state = run_to_synced(&executor_v2, state).await?;
    assert_eq!(get_status(&state, id), Some(ResourceStatus::Running));
    assert_eq!(controller_state(&state, id), TestWorkerState::Ready);
    assert_eq!(deployed_images(id), vec!["image-v1", "image-v2"]);
    assert_eq!(recorded_image(&state, id), "image-v2");
    assert_eq!(previous_image(&state, id).as_deref(), Some("image-v1"));
    assert_nothing_planned(&executor_v2, &state)
}

/// The desired image changes after CreateStart but before the handler that deploys the code.
/// The rest of the create still uses the config it started with: a create never mixes two
/// configs. The update then deploys the new one.
#[tokio::test]
async fn create_that_has_not_deployed_yet_still_finishes_with_its_config() -> Result<()> {
    let id = "mid-create-change-before-deploy";
    let executor_v1 = new_executor(&stack_of(worker(id, "image-v1", &[])))?;
    let state = executor_v1.step(new_test_state()).await?.next_state;
    assert_eq!(controller_state(&state, id), TestWorkerState::CreateWorker);
    assert!(deployed_images(id).is_empty());

    let executor_v2 = new_executor(&stack_of(worker(id, "image-v2", &[])))?;
    let state = run_to_synced(&executor_v2, state).await?;

    assert_eq!(get_status(&state, id), Some(ResourceStatus::Running));
    assert_eq!(deployed_images(id), vec!["image-v1", "image-v2"]);
    assert_eq!(recorded_image(&state, id), "image-v2");
    assert_eq!(previous_image(&state, id).as_deref(), Some("image-v1"));
    assert_nothing_planned(&executor_v2, &state)
}

/// The desired image changes again while an update is in flight. The update finishes with
/// the image it started with, then a second update deploys the newest one.
#[tokio::test]
async fn update_in_flight_finishes_then_the_newer_config_is_applied() -> Result<()> {
    let id = "mid-update-change";
    let state = run_to_synced(
        &new_executor(&stack_of(worker(id, "image-v1", &[])))?,
        new_test_state(),
    )
    .await?;
    assert_eq!(deployed_images(id), vec!["image-v1"]);

    let executor_v2 = new_executor(&stack_of(worker(id, "image-v2", &[])))?;
    let state =
        step_until_controller(&executor_v2, state, id, TestWorkerState::UpdateCodePolling).await?;
    assert_eq!(get_status(&state, id), Some(ResourceStatus::Updating));
    assert_eq!(deployed_images(id), vec!["image-v1", "image-v2"]);

    let executor_v3 = new_executor(&stack_of(worker(id, "image-v3", &[])))?;
    let state = step_until_status(&executor_v3, state, id, ResourceStatus::Running).await?;
    assert_eq!(
        recorded_image(&state, id),
        "image-v2",
        "the finished update records the image it deployed"
    );
    assert_eq!(deployed_images(id), vec!["image-v1", "image-v2"]);

    let state = run_to_synced(&executor_v3, state).await?;
    assert_eq!(
        deployed_images(id),
        vec!["image-v1", "image-v2", "image-v3"]
    );
    assert_eq!(recorded_image(&state, id), "image-v3");
    assert_eq!(previous_image(&state, id).as_deref(), Some("image-v2"));
    assert_nothing_planned(&executor_v3, &state)
}

/// With no config change, a create deploys once and nothing is planned afterwards, including
/// for a create resumed from a checkpoint persisted mid-flow (whose shape is unchanged).
#[tokio::test]
async fn unchanged_config_is_deployed_once_and_needs_no_update() -> Result<()> {
    let id = "unchanged-create";
    let executor = new_executor(&stack_of(worker(id, "image-v1", &[])))?;
    let state = step_until_controller(
        &executor,
        new_test_state(),
        id,
        TestWorkerState::CreateWorkerPolling,
    )
    .await?;

    let persisted = serde_json::to_string(&state).expect("state serializes");
    let restored: StackState = serde_json::from_str(&persisted).expect("state deserializes");
    let mut state = run_to_synced(
        &new_executor(&stack_of(worker(id, "image-v1", &[])))?,
        restored,
    )
    .await?;
    assert_eq!(get_status(&state, id), Some(ResourceStatus::Running));
    assert_nothing_planned(&executor, &state)?;

    // Ready steps on the Running worker deploy nothing either.
    for _ in 0..3 {
        state = executor.step(state).await?.next_state;
    }
    assert_eq!(get_status(&state, id), Some(ResourceStatus::Running));
    assert_eq!(deployed_images(id), vec!["image-v1"]);
    assert_eq!(recorded_image(&state, id), "image-v1");
    assert!(state.resources[id].previous_config.is_none());
    assert_nothing_planned(&executor, &state)
}

/// A create that started with v1 fails after the desired config changed to v2. The record
/// still says v1, so the planner replaces it, and the delete runs against v1: the config the
/// failed create made its resources with.
#[tokio::test]
async fn create_failing_after_a_mid_create_change_is_replaced_against_the_config_it_ran(
) -> Result<()> {
    let id = "mid-create-change-then-failure";
    let failing_v1 = worker(
        id,
        "image-v1",
        &[("SIMULATE_CREATE_WORKER_FAILURE", "true")],
    );
    let executor_v1 = new_executor(&stack_of(failing_v1))?;
    let state = executor_v1.step(new_test_state()).await?.next_state;
    assert_eq!(controller_state(&state, id), TestWorkerState::CreateWorker);

    let executor_v2 = new_executor(&stack_of(worker(id, "image-v2", &[])))?;
    let state = step_until_status(&executor_v2, state, id, ResourceStatus::ProvisionFailed).await?;
    assert_eq!(
        recorded_image(&state, id),
        "image-v1",
        "the failed create ran with v1"
    );
    let plan = executor_v2.plan(&state)?;
    assert_eq!(plan.replaces, vec![id.to_string()]);

    let state = run_to_synced(&executor_v2, state).await?;
    let deletes = test_worker_deletes_issued(&identifier(id));
    assert_eq!(
        deletes.len(),
        1,
        "what the failed create made is deleted once"
    );
    assert_eq!(image_of(&deletes[0]), "image-v1");
    assert_eq!(
        deletes[0]
            .environment
            .get("SIMULATE_CREATE_WORKER_FAILURE")
            .map(String::as_str),
        Some("true"),
        "the delete sees the config the failed create ran with"
    );
    assert_eq!(get_status(&state, id), Some(ResourceStatus::Running));
    assert_eq!(deployed_images(id), vec!["image-v2"]);
    assert_eq!(recorded_image(&state, id), "image-v2");
    assert_nothing_planned(&executor_v2, &state)
}
