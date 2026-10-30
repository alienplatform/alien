//! Tests for resource deletion flows and ordering.

use std::sync::{Arc, Mutex};

use super::helpers::*;
use crate::{
    error::Result,
    remote_bindings::{AwsRemoteBindingsController, AwsRemoteBindingsState},
    service_account::AwsServiceAccountController,
    state_utils::{StackResourceStateExt, StackStateExt},
    storage::{test_storage_deletes_issued, SIMULATE_STORAGE_CREATE_FAILURE_ORIGIN},
    worker::{AwsWorkerController, AwsWorkerState},
    MockPlatformServiceProvider, StackExecutor,
};
use alien_aws_clients::cloudwatch_logs::MockCloudWatchLogsApi;
use alien_aws_clients::iam::{
    ListAttachedRolePoliciesResponse, ListAttachedRolePoliciesResult, ListRolePoliciesResponse,
    ListRolePoliciesResult, MockIamApi, PolicyNames,
};
use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt as _};
use alien_core::{
    ClientConfig, RemoteBindings, RemoteStackManagement, Resource, ResourceLifecycle, ResourceRef,
    ResourceStatus, ServiceAccount, SetupManagement, Stack, Storage, Vault,
};

/// Tests basic deletion of a single resource.
#[tokio::test]
async fn test_delete_single_resource() -> Result<()> {
    let func1 = test_function("func1");

    let stack = Stack::new("delete-test".to_owned())
        .add(func1.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor, state).await?;

    assert_eq!(
        get_status(&state_after_create, "func1"),
        Some(ResourceStatus::Running)
    );

    let deletion_executor = new_deletion_executor()?;
    let final_state = run_to_synced(&deletion_executor, state_after_create).await?;

    assert_deleted(&final_state, &["func1"]);
    Ok(())
}

/// Tests deletion order is reverse of creation order.
#[tokio::test]
async fn test_deletion_respects_reverse_dependency_order() -> Result<()> {
    let store1 = test_storage("store1");
    let func1 = test_function("func1");

    let stack = Stack::new("reverse-delete-test".to_owned())
        .add(store1.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            func1.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "store1")],
        )
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor, state).await?;

    assert_all_running(&state_after_create, &["store1", "func1"]);

    let deletion_executor = new_deletion_executor()?;

    // Track deletion order
    let mut deletion_order = Vec::new();
    let mut current_state = state_after_create.clone();
    let max_steps = 20;

    for _ in 0..max_steps {
        if deletion_executor.is_synced(&current_state) {
            break;
        }
        let step_result = deletion_executor.step(current_state).await?;
        current_state = step_result.next_state;

        for (resource_id, resource_state) in &current_state.resources {
            if resource_state.status == ResourceStatus::Deleting
                && !deletion_order.contains(resource_id)
            {
                deletion_order.push(resource_id.clone());
            }
        }
    }

    // Finish deletion
    let final_state = run_to_synced(&deletion_executor, current_state).await?;
    assert_deleted(&final_state, &["store1", "func1"]);

    // Dependent (func1) should be deleted before dependency (store1)
    let func_index = deletion_order
        .iter()
        .position(|id| id == "func1")
        .expect("func1 should be in deletion order");
    let store_index = deletion_order
        .iter()
        .position(|id| id == "store1")
        .expect("store1 should be in deletion order");

    assert!(
        func_index < store_index,
        "func1 should be deleted before store1 (dependent before dependency)"
    );

    Ok(())
}

/// Tests dependencies are not stepped while their dependents are still deleting.
///
/// This matters for providers whose delete handlers still read dependency
/// outputs while polling, such as Azure resources reading the resource group.
#[tokio::test]
async fn test_dependency_waits_until_dependent_delete_completes() -> Result<()> {
    let store1 = test_storage("store1");
    let func1 = test_function("func1");

    let stack = Stack::new("delete-waits-test".to_owned())
        .add(store1.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            func1.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "store1")],
        )
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor, state).await?;
    assert_all_running(&state_after_create, &["store1", "func1"]);

    let deletion_executor = new_deletion_executor()?;

    let step_result = deletion_executor.step(state_after_create).await?;
    let state = step_result.next_state;
    assert_eq!(get_status(&state, "func1"), Some(ResourceStatus::Deleting));
    assert_eq!(get_status(&state, "store1"), Some(ResourceStatus::Running));

    let step_result = deletion_executor.step(state).await?;
    let state = step_result.next_state;
    assert_eq!(get_status(&state, "func1"), Some(ResourceStatus::Deleting));
    assert_eq!(get_status(&state, "store1"), Some(ResourceStatus::Running));

    let final_state = run_to_synced(&deletion_executor, state).await?;
    assert_deleted(&final_state, &["store1", "func1"]);

    Ok(())
}

/// Tests partial deletion using lifecycle filter.
#[tokio::test]
async fn test_partial_deletion_with_lifecycle_filter() -> Result<()> {
    let store1 = test_storage("store1");
    let func1 = test_function("func1");

    let stack = Stack::new("partial-delete-test".to_owned())
        .add(store1.clone(), ResourceLifecycle::Frozen)
        .add(func1.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor, state).await?;

    assert_all_running(&state_after_create, &["store1", "func1"]);

    // Delete only Live resources
    let live_deletion = new_deletion_executor_with_filter(vec![ResourceLifecycle::Live])?;
    let final_state = run_to_synced(&live_deletion, state_after_create).await?;

    assert_eq!(
        get_status(&final_state, "store1"),
        Some(ResourceStatus::Running)
    );
    assert_eq!(
        get_status(&final_state, "func1"),
        Some(ResourceStatus::Deleted)
    );
    Ok(())
}

/// Tests delete requests interrupt in-flight updates instead of waiting for
/// update stabilization.
#[tokio::test]
async fn test_deletion_interrupts_updating_resource() -> Result<()> {
    let func = test_function_with_image("func1", "image:v1");

    let stack = Stack::new("delete-updating-test".to_owned())
        .add(func.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state = run_to_synced(&executor, state).await?;

    let updated_func = test_function_with_image("func1", "image:v2");
    let updated_stack = Stack::new("delete-updating-test".to_owned())
        .add(updated_func.clone(), ResourceLifecycle::Live)
        .build();
    let update_executor = new_executor(&updated_stack)?;
    let step_result = update_executor.step(state).await?;
    let state = step_result.next_state;

    assert_eq!(get_status(&state, "func1"), Some(ResourceStatus::Updating));

    let deletion_executor = new_deletion_executor()?;
    let step_result = deletion_executor.step(state).await?;

    assert_eq!(
        get_status(&step_result.next_state, "func1"),
        Some(ResourceStatus::Deleting)
    );

    Ok(())
}

/// Tests lifecycle-filtered deletion does not continue deleting resources
/// outside its scope.
///
/// This protects imported setup flows: if a prior full cleanup attempt already
/// moved a frozen resource into Deleting, a later live-only cleanup must leave
/// that frozen resource to the setup tool that owns it.
#[tokio::test]
async fn test_lifecycle_filter_skips_out_of_scope_deleting_resources() -> Result<()> {
    let mut state = new_test_state();

    let mut frozen_func = create_deleting_function_state("frozen-func");
    frozen_func.lifecycle = Some(ResourceLifecycle::Frozen);
    state
        .resources
        .insert("frozen-func".to_string(), frozen_func);

    let mut live_func = create_deleted_function_state("live-func");
    live_func.lifecycle = Some(ResourceLifecycle::Live);
    state.resources.insert("live-func".to_string(), live_func);

    let live_deletion = new_deletion_executor_with_filter(vec![ResourceLifecycle::Live])?;
    let step_result = live_deletion.step(state).await?;

    assert_eq!(
        get_status(&step_result.next_state, "frozen-func"),
        Some(ResourceStatus::Deleting)
    );
    assert!(
        live_deletion.is_synced(&step_result.next_state),
        "live-only deletion should be synced without stepping frozen resources"
    );

    Ok(())
}

/// Tests live-only deletion does not step non-terminal Frozen resources.
///
/// This covers imported setup-owned resources that may still be Provisioning
/// for read-only discovery while live resource deletion runs.
#[tokio::test]
async fn test_lifecycle_filter_skips_out_of_scope_provisioning_resources() -> Result<()> {
    let mut state = new_test_state();

    let mut frozen_func = create_provisioning_function_state("frozen-func");
    frozen_func.lifecycle = Some(ResourceLifecycle::Frozen);
    state
        .resources
        .insert("frozen-func".to_string(), frozen_func);

    let live_deletion = new_deletion_executor_with_filter(vec![ResourceLifecycle::Live])?;
    let step_result = live_deletion.step(state).await?;

    assert_eq!(
        get_status(&step_result.next_state, "frozen-func"),
        Some(ResourceStatus::Provisioning)
    );
    assert!(
        live_deletion.is_synced(&step_result.next_state),
        "live-only deletion should be synced without stepping frozen provisioning resources"
    );

    Ok(())
}

/// Tests deleting multiple independent resources.
#[tokio::test]
async fn test_delete_multiple_independent_resources() -> Result<()> {
    let func1 = test_function("func1");
    let func2 = test_function("func2");

    let stack = Stack::new("multi-delete-test".to_owned())
        .add(func1.clone(), ResourceLifecycle::Live)
        .add(func2.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor, state).await?;

    assert_all_running(&state_after_create, &["func1", "func2"]);

    let deletion_executor = new_deletion_executor()?;
    let final_state = run_to_synced(&deletion_executor, state_after_create).await?;

    assert_deleted(&final_state, &["func1", "func2"]);
    Ok(())
}

/// Tests deletion from empty state.
#[tokio::test]
async fn test_delete_empty_state() -> Result<()> {
    let deletion_executor = new_deletion_executor()?;
    let state = new_test_state();

    // Should complete immediately with no changes
    assert!(
        deletion_executor.is_synced(&state),
        "Empty state should be synced"
    );

    let final_state = run_to_synced(&deletion_executor, state).await?;
    assert!(final_state.resources.is_empty());

    Ok(())
}

/// Tests deletion of resources in mixed states.
#[tokio::test]
async fn test_delete_mixed_states() -> Result<()> {
    let running_func = test_function("running-func");
    let pending_func = test_function("pending-func");

    let stack = Stack::new("mixed-states-test".to_owned())
        .add(running_func.clone(), ResourceLifecycle::Live)
        .add(pending_func.clone(), ResourceLifecycle::Live)
        .build();

    // Create only one resource
    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_partial = run_to_synced(&executor, state).await?;

    // Both should be running after full sync
    assert_all_running(&state_partial, &["running-func", "pending-func"]);

    // Now delete everything
    let deletion_executor = new_deletion_executor()?;
    let final_state = run_to_synced(&deletion_executor, state_partial).await?;

    assert_deleted(&final_state, &["running-func", "pending-func"]);

    Ok(())
}

/// Tests deletion handles already deleted resources.
#[tokio::test]
async fn test_delete_already_deleted() -> Result<()> {
    // First create and run a resource
    let func1 = test_function("func1");
    let stack = Stack::new("delete-test".to_owned())
        .add(func1.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor, state).await?;

    // Delete it
    let deletion_executor = new_deletion_executor()?;
    let state_after_delete = run_to_synced(&deletion_executor, state_after_create).await?;

    assert_deleted(&state_after_delete, &["func1"]);

    // Try to delete again - should be a no-op (already synced)
    assert!(
        deletion_executor.is_synced(&state_after_delete),
        "Already deleted state should be synced"
    );

    Ok(())
}

/// Tests deletion of dependent resources in correct order.
#[tokio::test]
async fn test_delete_dependent_running() -> Result<()> {
    let base_storage = test_storage("base-storage");
    let middle_func = test_function("middle-func");
    let leaf_func = test_function("leaf-func");

    let stack = Stack::new("dependent-delete-test".to_owned())
        .add(base_storage.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            middle_func.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "base-storage")],
        )
        .add_with_dependencies(
            leaf_func.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(
                alien_core::Worker::RESOURCE_TYPE,
                "middle-func",
            )],
        )
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor, state).await?;

    assert_all_running(
        &state_after_create,
        &["base-storage", "middle-func", "leaf-func"],
    );

    // Delete all
    let deletion_executor = new_deletion_executor()?;

    // Track deletion order
    let mut deletion_order = Vec::new();
    let mut current_state = state_after_create;
    let max_steps = 30;

    for _ in 0..max_steps {
        if deletion_executor.is_synced(&current_state) {
            break;
        }
        let step_result = deletion_executor.step(current_state).await?;
        current_state = step_result.next_state;

        for (resource_id, resource_state) in &current_state.resources {
            if resource_state.status == ResourceStatus::Deleting
                && !deletion_order.contains(resource_id)
            {
                deletion_order.push(resource_id.clone());
            }
        }
    }

    // Verify order: leaf first, then middle, then base
    let leaf_idx = deletion_order
        .iter()
        .position(|id| id == "leaf-func")
        .expect("leaf should be in order");
    let middle_idx = deletion_order
        .iter()
        .position(|id| id == "middle-func")
        .expect("middle should be in order");
    let base_idx = deletion_order
        .iter()
        .position(|id| id == "base-storage")
        .expect("base should be in order");

    assert!(
        leaf_idx < middle_idx,
        "leaf should be deleted before middle"
    );
    assert!(
        middle_idx < base_idx,
        "middle should be deleted before base"
    );

    Ok(())
}

/// Tests deletion terminal state tracking with filters.
#[tokio::test]
async fn test_deletion_terminal_state_with_filters() -> Result<()> {
    let frozen_store = test_storage("frozen-store");
    let live_store = test_storage("live-store");

    let stack = Stack::new("deletion-terminal-test".to_owned())
        .add(frozen_store.clone(), ResourceLifecycle::Frozen)
        .add(live_store.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor, state).await?;

    assert_all_running(&state_after_create, &["frozen-store", "live-store"]);

    // Delete only Live resources
    let live_deletion = new_deletion_executor_with_filter(vec![ResourceLifecycle::Live])?;

    // Run a single step
    let step_result = live_deletion.step(state_after_create.clone()).await?;
    let mut state = step_result.next_state;

    // After one step, live_store should be Deleting but not synced
    assert_eq!(
        get_status(&state, "frozen-store"),
        Some(ResourceStatus::Running)
    );
    assert_eq!(
        get_status(&state, "live-store"),
        Some(ResourceStatus::Deleting)
    );
    assert!(
        !live_deletion.is_synced(&state),
        "Stack should not be synced during deletion"
    );

    // Run to completion
    state = run_to_synced(&live_deletion, state).await?;

    // live_store deleted, frozen_store still Running
    assert_eq!(
        get_status(&state, "frozen-store"),
        Some(ResourceStatus::Running)
    );
    assert_eq!(
        get_status(&state, "live-store"),
        Some(ResourceStatus::Deleted)
    );

    // Stack should be synced for the deletion executor
    assert!(
        live_deletion.is_synced(&state),
        "Stack should be synced after filtered deletion completes"
    );

    Ok(())
}

/// Tests deletion with dependency in different lifecycle filter.
#[tokio::test]
async fn test_deletion_with_filtered_dependencies() -> Result<()> {
    let base_store = test_storage("base-store");
    let dependent_func = test_function("dependent-func");

    let stack = Stack::new("dep-deletion-test".to_owned())
        .add(base_store.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            dependent_func.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "base-store")],
        )
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor, state).await?;

    assert_all_running(&state_after_create, &["base-store", "dependent-func"]);

    // Try to delete only Frozen (base store) while keeping dependent
    // This should not delete base_store because dependent_func depends on it
    let frozen_deletion = new_deletion_executor_with_filter(vec![ResourceLifecycle::Frozen])?;
    let final_state = run_to_synced(&frozen_deletion, state_after_create).await?;

    // base_store should not be deleted because dependent_func depends on it
    assert_eq!(
        get_status(&final_state, "base-store"),
        Some(ResourceStatus::Running),
        "Base store should not be deleted - has Live dependent"
    );
    assert_eq!(
        get_status(&final_state, "dependent-func"),
        Some(ResourceStatus::Running),
        "Dependent func should remain"
    );

    Ok(())
}

/// Preservation must freeze even an already-prepared delete checkpoint. A caller
/// can resume a legacy deletion after upgrading without advancing its data delete.
#[tokio::test]
async fn preserved_checkpoints_survive_reload_while_workloads_are_deleted() -> Result<()> {
    for (lifecycle, checkpoint) in [ResourceLifecycle::Live, ResourceLifecycle::Frozen]
        .into_iter()
        .flat_map(|lifecycle| {
            [
                ResourceStatus::Running,
                ResourceStatus::Deleting,
                ResourceStatus::UpdateFailed,
            ]
            .into_iter()
            .map(move |checkpoint| (lifecycle, checkpoint))
        })
    {
        let stack = Stack::new("preserve-data".to_owned())
            .add(test_storage("dependency"), ResourceLifecycle::Frozen)
            .add_with_dependencies(
                test_storage("data"),
                lifecycle,
                vec![ResourceRef::new(Storage::RESOURCE_TYPE, "dependency")],
            )
            .add_with_dependencies(
                test_function("worker"),
                ResourceLifecycle::Live,
                vec![ResourceRef::new(Storage::RESOURCE_TYPE, "data")],
            )
            .build();
        let mut state = run_to_synced(&new_executor(&stack)?, new_test_state()).await?;
        if checkpoint == ResourceStatus::Deleting {
            state.prepare_for_destroy()?;
        } else if checkpoint == ResourceStatus::UpdateFailed {
            let data = state.resources.get_mut("data").unwrap();
            data.status = checkpoint;
            data.last_failed_state = data.internal_state.clone();
        }
        let data = serde_json::to_value(&state.resources["data"]).unwrap();
        let dependency = serde_json::to_value(&state.resources["dependency"]).unwrap();
        for _ in 0..20 {
            // Rebuild both the executor and its installed state, as a manager
            // does after a checkpoint/restart, rather than reuse an in-memory scope.
            state = serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
            let executor =
                new_deletion_executor()?.preserving_resources(&state, ["data".to_string()])?;
            let plan = executor.plan(&state)?;
            assert!(!plan.deletes.contains(&"data".to_string()));
            assert!(!plan.deletes.contains(&"dependency".to_string()));
            state = executor.step(state).await?.next_state;
            assert_eq!(
                serde_json::to_value(&state.resources["data"]).unwrap(),
                data
            );
            assert_eq!(
                serde_json::to_value(&state.resources["dependency"]).unwrap(),
                dependency
            );
            if executor.is_synced(&state) {
                break;
            }
        }
        assert_deleted(&state, &["worker"]);
        let executor =
            new_deletion_executor()?.preserving_resources(&state, ["data".to_string()])?;
        assert!(executor.is_synced(&state));
        assert!(executor.pending_deletions(&state)?.is_empty());
        let before = serde_json::to_value(&state).unwrap();
        state = executor.step(state).await?.next_state;
        assert_eq!(serde_json::to_value(&state).unwrap(), before);

        // A separate explicit erase can still finish from the original retained
        // IDs and controller checkpoints, without creating another resource.
        let erase = new_deletion_executor()?;
        erase.prepare_for_deletion(&mut state)?;
        state = run_to_synced(&erase, state).await?;
        assert_deleted(&state, &["data", "dependency", "worker"]);
    }
    Ok(())
}

#[tokio::test]
async fn preservation_refuses_missing_inventory_and_non_deletion_executor() -> Result<()> {
    let stack = Stack::new("preserve-validation".to_owned())
        .add(test_storage("data"), ResourceLifecycle::Live)
        .build();
    let state = run_to_synced(&new_executor(&stack)?, new_test_state()).await?;
    assert!(new_executor(&stack)?
        .preserving_resources(&state, ["data".to_string()])
        .is_err());
    assert!(new_deletion_executor()?
        .preserving_resources(&state, ["missing".to_string()])
        .is_err());
    let mut incomplete = state;
    incomplete
        .resources
        .get_mut("data")
        .unwrap()
        .dependencies
        .push(ResourceRef::new(
            Storage::RESOURCE_TYPE,
            "missing-dependency",
        ));
    assert!(new_deletion_executor()?
        .preserving_resources(&incomplete, ["data".to_string()])
        .is_err());
    Ok(())
}

#[tokio::test]
async fn preservation_keeps_failed_create_identity_until_explicit_erase() -> Result<()> {
    let id = "preservation-failed-create";
    let storage = Storage::new(id.to_string())
        .cors_allowed_origins(vec![SIMULATE_STORAGE_CREATE_FAILURE_ORIGIN.to_string()])
        .build();
    let stack = Stack::new("failed-retention".to_string())
        .add(storage, ResourceLifecycle::Live)
        .build();
    let created = new_executor(&stack)?
        .run_until_synced(new_test_state())
        .await;
    assert!(!created.success);
    let mut state = created.final_state;
    assert_eq!(state.resources[id].status, ResourceStatus::ProvisionFailed);
    assert!(state.resources[id]
        .get_internal_controller_typed::<crate::storage::TestStorageController>()?
        .bucket_name
        .is_some());
    let checkpoint = serde_json::to_value(&state.resources[id]).unwrap();
    for _ in 0..3 {
        state = serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
        let executor = new_deletion_executor()?.preserving_resources(&state, [id.to_string()])?;
        assert!(executor.prepare_for_deletion(&mut state)?.is_empty());
        state = executor.step(state).await?.next_state;
        assert_eq!(
            serde_json::to_value(&state.resources[id]).unwrap(),
            checkpoint
        );
        assert!(test_storage_deletes_issued(id).is_empty());
        assert!(executor.is_synced(&state));
    }
    let erase = new_deletion_executor()?;
    erase.prepare_for_deletion(&mut state)?;
    state = run_to_synced(&erase, state).await?;
    assert_deleted(&state, &[id]);
    assert_eq!(test_storage_deletes_issued(id).len(), 1);
    Ok(())
}

#[tokio::test]
async fn preservation_follows_previous_config_dependencies_during_interrupted_update() -> Result<()>
{
    let storage = test_storage("previous-data");
    let old_worker = test_function_linked("retained-dependent", &storage);
    let stack = Stack::new("previous-dependency".to_string())
        .add(storage, ResourceLifecycle::Live)
        .add(old_worker.clone(), ResourceLifecycle::Live)
        .build();
    let mut state = run_to_synced(&new_executor(&stack)?, new_test_state()).await?;
    let dependent = state.resources.get_mut("retained-dependent").unwrap();
    dependent.previous_config = Some(alien_core::Resource::new(old_worker));
    dependent.config = alien_core::Resource::new(test_function("retained-dependent"));
    dependent.dependencies.clear();
    dependent.status = ResourceStatus::Updating;
    let before = serde_json::to_value(&state).unwrap();
    let executor = new_deletion_executor()?
        .preserving_resources(&state, ["retained-dependent".to_string()])?;
    assert!(executor.prepare_for_deletion(&mut state)?.is_empty());
    assert!(executor.plan(&state)?.deletes.is_empty());
    state = executor.step(state).await?.next_state;
    assert_eq!(serde_json::to_value(&state).unwrap(), before);
    assert!(executor.is_synced(&state));
    Ok(())
}

#[tokio::test]
async fn retained_data_releases_grantees_after_workload_cleanup() -> Result<()> {
    for (legacy_deleting, runtime_first) in
        [(false, false), (false, true), (true, false), (true, true)]
    {
        let stack = Stack::new("retain-with-grants".to_string())
            .add(
                ServiceAccount::new("reader".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                RemoteStackManagement::new("management".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                test_storage("encryption-dependency"),
                ResourceLifecycle::Frozen,
            )
            .add_with_dependencies(
                test_storage("retained"),
                ResourceLifecycle::Live,
                vec![
                    ResourceRef::new(ServiceAccount::RESOURCE_TYPE, "reader"),
                    ResourceRef::new(RemoteStackManagement::RESOURCE_TYPE, "management"),
                    ResourceRef::new(Storage::RESOURCE_TYPE, "encryption-dependency"),
                ],
            )
            .add_with_dependencies(
                test_function("consumer"),
                ResourceLifecycle::Live,
                vec![
                    ResourceRef::new(ServiceAccount::RESOURCE_TYPE, "reader"),
                    ResourceRef::new(RemoteStackManagement::RESOURCE_TYPE, "management"),
                    ResourceRef::new(Storage::RESOURCE_TYPE, "retained"),
                ],
            )
            .build();
        let mut state = run_to_synced(&new_executor(&stack)?, new_test_state()).await?;
        if legacy_deleting {
            state.prepare_for_destroy()?;
        }
        let data = serde_json::to_value(&state.resources["retained"]).unwrap();
        let dependency = serde_json::to_value(&state.resources["encryption-dependency"]).unwrap();
        // Runtime cannot delete the setup-owned identity, even when retained
        // data no longer holds it. Its recorded identity survives the handoff.
        if runtime_first {
            let runtime = new_deletion_executor_with_filter(vec![ResourceLifecycle::Live])?
                .preserving_resources(&state, ["retained".to_string()])?;
            runtime.prepare_for_deletion(&mut state)?;
            state = run_to_synced(&runtime, state).await?;
            assert_deleted(&state, &["consumer"]);
            assert_ne!(get_status(&state, "reader"), Some(ResourceStatus::Deleted));
            assert_ne!(
                get_status(&state, "management"),
                Some(ResourceStatus::Deleted)
            );
        }
        for _ in 0..30 {
            state = serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
            let executor =
                new_deletion_executor()?.preserving_resources(&state, ["retained".to_string()])?;
            assert!(!executor.preserves_resource("reader"));
            assert!(!executor.preserves_resource("management"));
            executor.prepare_for_deletion(&mut state)?;
            let waiting_for_workload =
                get_status(&state, "consumer") != Some(ResourceStatus::Deleted);
            let mut identity_before = serde_json::to_value(&state.resources["reader"]).unwrap();
            let mut management_before = serde_json::to_value(&state.resources["management"]).unwrap();
            // The controller checkpoint stays unchanged while deletion reports
            // the workload that still needs this identity.
            if waiting_for_workload {
                identity_before["progress"] = serde_json::json!("Waiting for consumer.");
                management_before["progress"] = serde_json::json!("Waiting for consumer.");
            }
            state = executor.step(state).await?.next_state;
            if waiting_for_workload {
                assert_eq!(
                    serde_json::to_value(&state.resources["reader"]).unwrap(),
                    identity_before
                );
                assert_eq!(
                    serde_json::to_value(&state.resources["management"]).unwrap(),
                    management_before
                );
            }
            assert_eq!(
                serde_json::to_value(&state.resources["retained"]).unwrap(),
                data
            );
            assert_eq!(
                serde_json::to_value(&state.resources["encryption-dependency"]).unwrap(),
                dependency
            );
            if executor.is_synced(&state) {
                break;
            }
        }
        assert_deleted(&state, &["consumer", "reader", "management"]);
        let executor =
            new_deletion_executor()?.preserving_resources(&state, ["retained".to_string()])?;
        assert!(executor.is_synced(&state));
        assert!(executor.pending_deletions(&state)?.is_empty());
        let before = serde_json::to_value(&state).unwrap();
        state = executor.step(state).await?.next_state;
        assert_eq!(serde_json::to_value(&state).unwrap(), before);
        let erase = new_deletion_executor()?;
        erase.prepare_for_deletion(&mut state)?;
        state = run_to_synced(&erase, state).await?;
        assert_deleted(
            &state,
            &[
                "retained",
                "encryption-dependency",
                "consumer",
                "reader",
                "management",
            ],
        );
    }
    Ok(())
}

#[tokio::test]
async fn retention_keeps_workload_identity_dependencies() -> Result<()> {
    let stack = Stack::new("retain-workload".to_string())
        .add(
            ServiceAccount::new("identity".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_with_dependencies(
            test_function("workload"),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(ServiceAccount::RESOURCE_TYPE, "identity")],
        )
        .build();
    let mut state = run_to_synced(&new_executor(&stack)?, new_test_state()).await?;
    let before = serde_json::to_value(&state).unwrap();
    let executor =
        new_deletion_executor()?.preserving_resources(&state, ["workload".to_string()])?;
    assert!(executor.preserves_resource("identity"));
    assert!(executor.prepare_for_deletion(&mut state)?.is_empty());
    assert!(executor.plan(&state)?.deletes.is_empty());
    state = executor.step(state).await?.next_state;
    assert_eq!(serde_json::to_value(&state).unwrap(), before);
    Ok(())
}

#[tokio::test]
async fn retained_data_keeps_an_identity_used_by_previous_configuration() -> Result<()> {
    let identity = ServiceAccount::new("previous-identity".to_string()).build();
    let stack = Stack::new("retained-previous-identity".to_string())
        .add(identity.clone(), ResourceLifecycle::Frozen)
        .add(test_storage("data"), ResourceLifecycle::Live)
        .build();
    let mut state = run_to_synced(&new_executor(&stack)?, new_test_state()).await?;
    let data = state.resources.get_mut("data").unwrap();
    data.previous_config = Some(alien_core::Resource::new(test_function_linked(
        "data", &identity,
    )));
    data.status = ResourceStatus::Updating;
    data.dependencies.push(ResourceRef::new(
        ServiceAccount::RESOURCE_TYPE,
        "previous-identity",
    ));
    state = serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
    let before = serde_json::to_value(&state).unwrap();
    let executor = new_deletion_executor()?.preserving_resources(&state, ["data".to_string()])?;
    assert!(executor.preserves_resource("previous-identity"));
    assert!(executor.prepare_for_deletion(&mut state)?.is_empty());
    assert!(executor.plan(&state)?.deletes.is_empty());
    state = executor.step(state).await?.next_state;
    assert_eq!(serde_json::to_value(&state).unwrap(), before);
    Ok(())
}

#[tokio::test]
async fn retained_data_releases_published_access_only_during_setup_and_retries_denial() -> Result<()>
{
    let stack = Stack::new("retained-published-data".to_string())
        .add(
            ServiceAccount::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_with_dependencies(
            test_storage("data"),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(RemoteBindings::RESOURCE_TYPE, "access")],
        )
        .build();
    let mut state = run_to_synced(&new_executor(&stack)?, new_test_state()).await?;
    // Install the real AWS controller checkpoint. Data is modeled by the test
    // storage controller; any unexpected call to it still changes its checkpoint.
    let access = state.resources.get_mut("access").unwrap();
    access.config = Resource::new(RemoteBindings::new("access".to_string()).build());
    access.controller_platform = Some(alien_core::Platform::Aws);
    access.resource_type = RemoteBindings::RESOURCE_TYPE.to_string();
    access.outputs = Some(alien_core::ResourceOutputs::new(
        alien_core::RemoteBindingsOutputs {
            resource_id: "arn:aws:iam::123456789012:role/owned-access".to_string(),
            access_configuration: "arn:aws:iam::123456789012:role/owned-access".to_string(),
        },
    ));
    access.set_internal_controller(Some(Box::new(AwsRemoteBindingsController {
        state: AwsRemoteBindingsState::Ready,
        role_name: Some("owned-access".to_string()),
        role_arn: Some("arn:aws:iam::123456789012:role/owned-access".to_string()),
        ..Default::default()
    })))?;
    let data = serde_json::to_value(&state.resources["data"]).unwrap();
    let access_before = serde_json::to_value(&state.resources["access"]).unwrap();
    let runtime = new_deletion_executor_with_filter(vec![ResourceLifecycle::Live])?
        .preserving_resources(&state, ["data".to_string()])?;
    runtime.prepare_for_deletion(&mut state)?;
    state = run_to_synced(&runtime, state).await?;
    assert_eq!(
        serde_json::to_value(&state.resources["access"]).unwrap(),
        access_before
    );
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut iam = MockIamApi::new();
    iam.expect_list_role_policies()
        .withf(|name| name == "owned-access")
        .returning(|_| {
            Ok(ListRolePoliciesResponse {
                list_role_policies_result: ListRolePoliciesResult {
                    policy_names: Some(PolicyNames {
                        member: vec!["published-data".to_string()],
                    }),
                    is_truncated: Some(false),
                    marker: None,
                },
            })
        });
    let seen = calls.clone();
    iam.expect_delete_role_policy()
        .returning(move |role, policy| {
            assert_eq!((role, policy), ("owned-access", "published-data"));
            let mut calls = seen.lock().unwrap();
            calls.push("policy");
            if calls.len() == 1 {
                return Err(alien_error::AlienError::new(
                    alien_client_core::ErrorData::RemoteAccessDenied {
                        resource_type: "role policy".to_string(),
                        resource_name: policy.to_string(),
                    },
                ));
            }
            Ok(())
        });
    let seen = calls.clone();
    iam.expect_delete_role()
        .withf(|role| role == "owned-access")
        .times(1)
        .returning(move |_| {
            seen.lock().unwrap().push("role");
            Ok(())
        });
    let iam = Arc::new(iam);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_aws_iam_client()
        .returning(move |_| Ok(iam.clone()));
    let provider = Arc::new(provider);
    for attempt in 0..2 {
        state = serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
        if attempt == 1 {
            assert!(state.resources.get_mut("access").unwrap().retry_failed()?);
        }
        let setup = StackExecutor::for_deletion_with_service_provider(
            ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
            &default_deployment_config(),
            provider.clone(),
            Some(vec![ResourceLifecycle::Frozen]),
        )?
        .preserving_resources(&state, ["data".to_string()])?;
        assert!(!setup.preserves_resource("access"));
        setup.prepare_for_deletion(&mut state)?;
        if attempt == 0 {
            // Resume the final automatic attempt so this exercises the durable
            // failure/retry boundary without waiting through exponential backoff.
            let access = state.resources.get_mut("access").unwrap();
            let mut controller = access.get_internal_controller()?.unwrap();
            controller.transition_to_delete_start()?;
            access.status = controller.get_status();
            access.set_internal_controller(Some(controller))?;
            access.retry_attempt = 9;
        }
        for _ in 0..10 {
            state = setup.step(state).await?.next_state;
            if matches!(
                get_status(&state, "access"),
                Some(ResourceStatus::Deleted | ResourceStatus::DeleteFailed)
            ) {
                break;
            }
        }
        assert_eq!(
            serde_json::to_value(&state.resources["data"]).unwrap(),
            data
        );
        if attempt == 0 {
            assert_eq!(
                get_status(&state, "access"),
                Some(ResourceStatus::DeleteFailed)
            );
            assert_eq!(*calls.lock().unwrap(), ["policy"]);
            // Synced includes terminal failures; the status above must remain failed.
        } else {
            assert_deleted(&state, &["access"]);
            assert!(setup.is_synced(&state));
            assert_eq!(*calls.lock().unwrap(), ["policy", "policy", "role"]);
        }
    }
    Ok(())
}

#[tokio::test]
async fn automatic_cleanup_deletes_identity_after_workload_but_preserves_setup_vault() -> Result<()>
{
    for (management, runtime_created, missing, setup_teardown, tag_owner) in [
        (SetupManagement::Manual, false, false, false, "setup"),
        (SetupManagement::Automatic, false, false, false, "setup"),
        (SetupManagement::Automatic, false, false, false, "runtime"),
        (
            SetupManagement::Automatic,
            false,
            false,
            false,
            "other-deployment",
        ),
        (SetupManagement::Automatic, false, false, false, "untagged"),
        (
            SetupManagement::Automatic,
            false,
            false,
            false,
            "read-error",
        ),
        (SetupManagement::Automatic, false, true, false, "runtime"),
        (SetupManagement::Automatic, true, false, false, "runtime"),
        (SetupManagement::Automatic, true, true, false, "runtime"),
        (SetupManagement::Automatic, false, false, true, "setup"),
    ] {
        let stack = Stack::new("automatic-cleanup".to_string())
            .add(
                ServiceAccount::new("execution-sa".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add_with_dependencies(
                test_storage("secrets"),
                ResourceLifecycle::Frozen,
                vec![ResourceRef::new(
                    ServiceAccount::RESOURCE_TYPE,
                    "execution-sa",
                )],
            )
            .add_with_dependencies(
                test_function("consumer"),
                ResourceLifecycle::Live,
                vec![ResourceRef::new(
                    ServiceAccount::RESOURCE_TYPE,
                    "execution-sa",
                )],
            )
            .build();
        let mut state = run_to_synced(&new_executor(&stack)?, new_test_state()).await?;
        // The IAM role and final worker delete use real controllers; the vault is outside
        // runtime cleanup and must keep its exact checkpoint without any call.
        let vault = state.resources.get_mut("secrets").unwrap();
        vault.config = Resource::new(Vault::new("secrets".to_string()).build());
        vault.resource_type = Vault::RESOURCE_TYPE.to_string();
        let vault_before = serde_json::to_value(&*vault).unwrap();
        let account = state.resources.get_mut("execution-sa").unwrap();
        account.controller_platform = Some(alien_core::Platform::Aws);
        let mut checkpoint = serde_json::json!({
            "state": "ready",
            "roleName": "owned-execution-sa",
            "stackPermissionsApplied": true,
            "runtimeCreated": runtime_created,
            "roleArn": "arn:aws:iam::123456789012:role/owned-execution-sa"
        });
        if !runtime_created {
            checkpoint.as_object_mut().unwrap().remove("runtimeCreated");
        }
        let controller: AwsServiceAccountController = serde_json::from_value(checkpoint).unwrap();
        account.set_internal_controller(Some(Box::new(controller)))?;
        let identity_before = serde_json::to_value(&*account).unwrap();
        if management == SetupManagement::Automatic {
            let mut controller = account.get_internal_controller()?.unwrap();
            controller.transition_to_delete_start()?;
            account.status = controller.get_status();
            account.set_internal_controller(Some(controller))?;
        }
        let worker = state.resources.get_mut("consumer").unwrap();
        worker.controller_platform = Some(alien_core::Platform::Aws);
        worker.status = ResourceStatus::Deleting;
        worker.set_internal_controller(Some(Box::new(AwsWorkerController {
            state: AwsWorkerState::DeletingLogGroup,
            ..Default::default()
        })))?;
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = calls.clone();
        let mut logs = MockCloudWatchLogsApi::new();
        logs.expect_delete_log_group().times(1).returning(move |_| {
            seen.lock().unwrap().push("workload");
            Ok(())
        });
        let logs = Arc::new(logs);
        let expected = usize::from(
            management == SetupManagement::Automatic
                && (runtime_created || setup_teardown || (tag_owner == "runtime" && !missing)),
        );
        let mut iam = MockIamApi::new();
        let prefix = state.resource_prefix.clone();
        let check_tags =
            management == SetupManagement::Automatic && !runtime_created && !setup_teardown;
        iam.expect_get_role()
            .withf(|role| role == "owned-execution-sa")
            .times(if check_tags { 1 + expected } else { 0 })
            .returning(move |_| {
                if tag_owner == "read-error" {
                    return Err(alien_error::AlienError::new(
                        alien_client_core::ErrorData::RemoteAccessDenied {
                            resource_type: "IAM role".to_string(),
                            resource_name: "owned-execution-sa".to_string(),
                        },
                    ));
                }
                if missing {
                    return Err(alien_error::AlienError::new(
                        alien_client_core::ErrorData::RemoteResourceNotFound {
                            resource_type: "IAM role".to_string(),
                            resource_name: "owned-execution-sa".to_string(),
                        },
                    ));
                }
                let tags = if tag_owner == "untagged" { vec![] } else { vec![
                    serde_json::json!({"Key": "managed-by", "Value": if tag_owner == "other-deployment" { "runtime" } else { tag_owner }}),
                    serde_json::json!({"Key": "deployment", "Value": if tag_owner == "other-deployment" { "different" } else { &prefix }}),
                    serde_json::json!({"Key": "resource", "Value": "execution-sa"}),
                ] };
                Ok(serde_json::from_value(serde_json::json!({"GetRoleResult": {"Role": {
                    "Path": "/", "RoleName": "owned-execution-sa", "RoleId": "existing-role",
                    "Arn": "arn:aws:iam::123456789012:role/owned-execution-sa",
                    "CreateDate": "2026-10-30T00:00:00Z", "Tags": {"member": tags}
                }}})).unwrap())
            });
        iam.expect_list_attached_role_policies()
            .withf(|role| role == "owned-execution-sa")
            .times(expected)
            .returning(move |_| {
                if missing {
                    return Err(alien_error::AlienError::new(
                        alien_client_core::ErrorData::RemoteResourceNotFound {
                            resource_type: "IAM role".to_string(),
                            resource_name: "owned-execution-sa".to_string(),
                        },
                    ));
                }
                Ok(ListAttachedRolePoliciesResponse {
                    list_attached_role_policies_result: ListAttachedRolePoliciesResult {
                        attached_policies: None,
                        is_truncated: Some(false),
                        marker: None,
                    },
                })
            });
        iam.expect_list_role_policies()
            .withf(|role| role == "owned-execution-sa")
            .times(expected)
            .returning(move |_| {
                if missing {
                    return Err(alien_error::AlienError::new(
                        alien_client_core::ErrorData::RemoteResourceNotFound {
                            resource_type: "IAM role".to_string(),
                            resource_name: "owned-execution-sa".to_string(),
                        },
                    ));
                }
                Ok(ListRolePoliciesResponse {
                    list_role_policies_result: ListRolePoliciesResult {
                        policy_names: Some(PolicyNames {
                            member: vec!["execution".to_string()],
                        }),
                        is_truncated: Some(false),
                        marker: None,
                    },
                })
            });
        let seen = calls.clone();
        iam.expect_delete_role_policy()
            .withf(|role, policy| role == "owned-execution-sa" && policy == "execution")
            .times(if missing { 0 } else { expected })
            .returning(move |_, _| {
                seen.lock().unwrap().push("policy");
                Ok(())
            });
        let seen = calls.clone();
        iam.expect_delete_role()
            .withf(|role| role == "owned-execution-sa")
            .times(expected)
            .returning(move |_| {
                seen.lock().unwrap().push("role");
                if missing {
                    return Err(alien_error::AlienError::new(
                        alien_client_core::ErrorData::RemoteResourceNotFound {
                            resource_type: "IAM role".to_string(),
                            resource_name: "owned-execution-sa".to_string(),
                        },
                    ));
                }
                Ok(())
            });
        let iam = Arc::new(iam);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(iam.clone()));
        provider
            .expect_get_aws_logs_client()
            .returning(move |_| Ok(logs.clone()));
        let provider = Arc::new(provider);
        let mut config = default_deployment_config();
        config.stack_settings.setup_management = management;
        for _ in 0..20 {
            state = serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
            let client = ClientConfig::Aws(Box::new(AwsClientConfig::mock()));
            let executor = if setup_teardown {
                StackExecutor::for_deletion_with_service_provider(
                    client,
                    &config,
                    provider.clone(),
                    None,
                )?
                .preserving_resources(&state, ["secrets".to_string()])?
            } else {
                StackExecutor::for_runtime_cleanup_deletion_with_service_provider(
                    client,
                    &config,
                    provider.clone(),
                )?
            };
            executor.prepare_for_deletion(&mut state)?;
            let step = executor.step(state).await?;
            state = step.next_state;
            if get_status(&state, "consumer") != Some(ResourceStatus::Deleted) {
                assert!(
                    calls.lock().unwrap().is_empty(),
                    "workload still needs its identity"
                );
            }
            assert_eq!(
                serde_json::to_value(&state.resources["secrets"]).unwrap(),
                vault_before
            );
            if executor.is_synced(&state)
                || state.resources["execution-sa"].error.is_some()
            {
                break;
            }
        }
        assert_deleted(&state, &["consumer"]);
        if tag_owner == "read-error" {
            assert_eq!(
                get_status(&state, "execution-sa"),
                Some(ResourceStatus::Deleting)
            );
            assert_eq!(state.resources["execution-sa"].retry_attempt, 1);
            assert!(state.resources["execution-sa"]
                .error
                .as_ref()
                .unwrap()
                .to_string()
                .contains("ownership"));
            assert_eq!(*calls.lock().unwrap(), ["workload"]);
        } else if management == SetupManagement::Automatic && expected == 0 {
            assert_deleted(&state, &["execution-sa"]);
            assert!(state.resources["execution-sa"].error.is_none());
            assert_eq!(*calls.lock().unwrap(), ["workload"]);
        } else if management == SetupManagement::Automatic {
            assert_deleted(&state, &["execution-sa"]);
            assert_eq!(
                *calls.lock().unwrap(),
                if missing {
                    vec!["workload", "role"]
                } else {
                    vec!["workload", "policy", "role"]
                }
            );
        } else {
            assert_eq!(
                serde_json::to_value(&state.resources["execution-sa"]).unwrap(),
                identity_before
            );
            assert_eq!(*calls.lock().unwrap(), ["workload"]);
        }
    }
    Ok(())
}
