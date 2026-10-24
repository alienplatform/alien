use super::helpers::*;
use crate::core::{ResourceController, StackResourceStateExt};
use crate::worker::{TestWorkerController, TestWorkerState};
use alien_core::{ResourceLifecycle, ResourceRef, ResourceStatus, Stack, Storage};

#[test]
fn generated_progress_follows_handler_and_clears_for_unannotated_states() {
    let mut controller = TestWorkerController::default();
    assert_eq!(controller.get_progress(), None);
    controller.state = TestWorkerState::CreateWorkerPolling;
    assert_eq!(
        controller.get_progress(),
        Some("Waiting for the worker to become available.")
    );
    controller.state = TestWorkerState::Ready;
    assert_eq!(controller.get_progress(), None);
    controller.state = TestWorkerState::CreateFailed;
    assert_eq!(controller.get_progress(), None);
}

#[tokio::test]
async fn progress_survives_resume_and_clears_after_dependency_and_handler_waits() {
    let stack = Stack::new("progress".to_owned())
        .add(test_storage("storage"), ResourceLifecycle::Live)
        .add_with_dependencies(
            test_function("worker"),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "storage")],
        )
        .build();
    let executor = new_executor(&stack).unwrap();
    let mut state = executor.step(new_test_state()).await.unwrap().next_state;
    assert_eq!(
        state.resources["worker"].progress.as_deref(),
        Some("Waiting for storage.")
    );
    let mut saw_handler_wait = false;
    for _ in 0..100 {
        // Every step resumes from the serialized document, as a manager does.
        state = serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
        state = executor.step(state).await.unwrap().next_state;
        let worker = &state.resources["worker"];
        if worker.progress.as_deref() == Some("Waiting for the worker to become available.") {
            saw_handler_wait = true;
            assert_eq!(worker.status, ResourceStatus::Provisioning);
            assert_eq!(worker.retry_attempt, 0);
        }
        if worker.status == ResourceStatus::Running {
            assert!(worker.progress.is_none());
            assert!(saw_handler_wait);
            assert!(state.resources["storage"].progress.is_none());
            return;
        }
    }
    panic!("worker did not become running");
}

#[test]
fn persisted_progress_clears_on_failure_and_retry_replaces_old_attempt_text() {
    let mut resource = create_pending_function_state("worker");
    let mut controller = TestWorkerController::default();
    controller.state = TestWorkerState::CreateWorkerPolling;
    resource
        .set_internal_controller(Some(Box::new(controller.clone())))
        .unwrap();
    assert!(resource.progress.is_some());
    resource
        .set_last_failed_controller(Some(Box::new(controller.clone())))
        .unwrap();
    controller.transition_to_failure();
    resource.status = controller.get_status();
    resource
        .set_internal_controller(Some(Box::new(controller)))
        .unwrap();
    assert!(resource.progress.is_none());
    resource.progress = Some("Old attempt".to_owned());
    assert!(resource.retry_failed().unwrap());
    assert_eq!(resource.status, ResourceStatus::Provisioning);
    assert_eq!(
        resource.progress.as_deref(),
        Some("Waiting for the worker to become available.")
    );
}

#[tokio::test]
async fn deleting_resource_reports_its_active_dependent_then_clears() {
    let stack = Stack::new("delete-progress".to_owned())
        .add(test_storage("storage"), ResourceLifecycle::Live)
        .add_with_dependencies(
            test_function("worker"),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "storage")],
        )
        .build();
    let executor = new_executor(&stack).unwrap();
    let mut state = run_to_synced(&executor, new_test_state()).await.unwrap();
    // Resume a delete already in flight while its dependent is still present.
    let storage = state.resources.get_mut("storage").unwrap();
    let mut controller = storage.get_internal_controller().unwrap().unwrap();
    controller.transition_to_delete_start().unwrap();
    storage.status = controller.get_status();
    storage.set_internal_controller(Some(controller)).unwrap();
    let deletion = new_deletion_executor().unwrap();
    state = deletion.step(state).await.unwrap().next_state;
    assert_eq!(
        state.resources["storage"].progress.as_deref(),
        Some("Waiting for worker.")
    );
    assert_eq!(state.resources["storage"].retry_attempt, 0);
    state = run_to_synced(&deletion, state).await.unwrap();
    assert_deleted(&state, &["storage", "worker"]);
    assert!(state
        .resources
        .values()
        .all(|resource| resource.progress.is_none()));
}

#[tokio::test]
async fn planned_delete_explains_wait_before_the_delete_controller_starts() {
    let stack = Stack::new("delete-wait".to_owned())
        .add(test_storage("storage"), ResourceLifecycle::Live)
        .add_with_dependencies(
            test_function("worker"),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "storage")],
        )
        .build();
    let executor = new_executor(&stack).unwrap();
    let running = run_to_synced(&executor, new_test_state()).await.unwrap();
    let deletion = new_deletion_executor().unwrap();
    let waiting = deletion.step(running).await.unwrap().next_state;
    assert_eq!(waiting.resources["storage"].status, ResourceStatus::Running);
    assert_eq!(
        waiting.resources["storage"].progress.as_deref(),
        Some("Waiting for worker.")
    );
    let restored = executor.step(waiting.clone()).await.unwrap().next_state;
    assert!(restored.resources["storage"].progress.is_none());
    let deleted = run_to_synced(&deletion, waiting).await.unwrap();
    assert_deleted(&deleted, &["storage", "worker"]);
    assert!(deleted
        .resources
        .values()
        .all(|resource| resource.progress.is_none()));
}

#[tokio::test]
async fn metadata_only_release_updates_presentation_without_a_resource_operation() {
    let mut stack = Stack::new("metadata".to_owned())
        .add_entry(
            alien_core::ResourceEntry::new(test_function("worker"), ResourceLifecycle::Live)
                .display_name("Document processor")
                .description("Processes uploaded documents."),
        )
        .build();
    let state = run_to_synced(&new_executor(&stack).unwrap(), new_test_state())
        .await
        .unwrap();
    assert_eq!(
        state.resources["worker"].display_name.as_deref(),
        Some("Document processor")
    );
    let before = state.resources["worker"].clone();
    stack.resources.get_mut("worker").unwrap().description =
        Some("Processes documents and results.".to_owned());
    let executor = new_executor(&stack).unwrap();
    let plan = executor.plan(&state).unwrap();
    assert!(
        plan.creates.is_empty()
            && plan.updates.is_empty()
            && plan.deletes.is_empty()
            && plan.replaces.is_empty()
            && plan.setup_required.is_empty()
    );
    let result = executor.step(state).await.unwrap();
    let after = &result.next_state.resources["worker"];
    assert_eq!(after.status, ResourceStatus::Running);
    assert_eq!(after.internal_state, before.internal_state);
    assert_eq!(after.outputs, before.outputs);
    assert_eq!(after.config, before.config);
    assert_eq!(
        after.description.as_deref(),
        Some("Processes documents and results.")
    );
    stack.resources.get_mut("worker").unwrap().display_name = None;
    stack.resources.get_mut("worker").unwrap().description = None;
    let cleared = new_executor(&stack)
        .unwrap()
        .step(result.next_state)
        .await
        .unwrap();
    assert!(cleared.next_state.resources["worker"]
        .display_name
        .is_none());
    assert!(cleared.next_state.resources["worker"].description.is_none());
}
