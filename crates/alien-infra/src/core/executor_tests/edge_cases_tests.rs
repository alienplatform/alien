//! Tests for edge cases and special state handling.

use super::helpers::*;
use crate::core::StackExecutor;
use crate::error::Result;
use alien_core::{
    bindings::ExternalAiBinding, Ai, ClientConfig, DeploymentConfig, EnvironmentVariablesSnapshot,
    ExternalBinding, ExternalBindings, Resource, ResourceLifecycle, ResourceRef, ResourceStatus,
    Stack, StackSettings, Storage, StorageBinding,
};
use std::collections::HashSet;

fn external_storage_config(resource_id: &str) -> DeploymentConfig {
    let mut external_bindings = ExternalBindings::new();
    external_bindings.insert(
        resource_id,
        ExternalBinding::Storage(StorageBinding::s3("external-test-bucket")),
    );

    DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(external_bindings)
        .allow_frozen_changes(false)
        .build()
}

fn external_ai_config(resource_id: &str) -> DeploymentConfig {
    let mut external_bindings = ExternalBindings::new();
    external_bindings.insert(
        resource_id,
        ExternalBinding::Ai(ExternalAiBinding {
            provider: "openai".to_string(),
            api_key: "sk-test".into(),
        }),
    );

    DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(external_bindings)
        .allow_frozen_changes(false)
        .build()
}

async fn assert_best_effort_delete_error_marks_deleted(flag: &str) -> Result<()> {
    let mut func1 = test_function("func1");
    func1
        .environment
        .insert(flag.to_string(), "true".to_string());

    let stack = Stack::new(format!("best-effort-delete-{}", flag))
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
    let state_after_delete = run_to_synced(&deletion_executor, state_after_create).await?;

    assert_deleted(&state_after_delete, &["func1"]);
    assert!(
        state_after_delete
            .resources
            .get("func1")
            .and_then(|state| state.internal_state.as_ref())
            .is_none(),
        "Best-effort delete should clean up controller state"
    );

    Ok(())
}

/// Tests recreating a deleted resource.
#[tokio::test]
async fn test_recreate_deleted_resource() -> Result<()> {
    let func1 = test_function("func1");

    let stack = Stack::new("recreate-test".to_owned())
        .add(func1.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor, state).await?;

    assert_eq!(
        get_status(&state_after_create, "func1"),
        Some(ResourceStatus::Running)
    );

    // Delete the resource
    let deletion_executor = new_deletion_executor()?;
    let state_after_delete = run_to_synced(&deletion_executor, state_after_create).await?;

    assert_deleted(&state_after_delete, &["func1"]);

    // Recreate using the same stack
    let recreate_executor = new_executor(&stack)?;
    let final_state = run_to_synced(&recreate_executor, state_after_delete).await?;

    assert_eq!(
        get_status(&final_state, "func1"),
        Some(ResourceStatus::Running),
        "Resource should be recreated after deletion"
    );

    Ok(())
}

/// Tests best-effort delete cleanup when the remote resource is already gone.
#[tokio::test]
async fn test_delete_not_found_marks_deleted() -> Result<()> {
    assert_best_effort_delete_error_marks_deleted("SIMULATE_DELETE_NOT_FOUND").await
}

/// Tests best-effort delete cleanup when imported-resource credentials can no longer access it.
#[tokio::test]
async fn test_delete_access_denied_marks_deleted() -> Result<()> {
    assert_best_effort_delete_error_marks_deleted("SIMULATE_DELETE_ACCESS_DENIED").await
}

/// Tests handling resources that are externally provisioned.
#[tokio::test]
async fn test_absent_resource_is_planned_for_delete() -> Result<()> {
    let func1 = test_function("func1");

    let stack = Stack::new("external-test".to_owned())
        .add(func1.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor, state).await?;

    assert_eq!(
        get_status(&state_after_create, "func1"),
        Some(ResourceStatus::Running)
    );

    // Try to delete by removing the resource from the desired stack.
    let empty_stack = Stack::new("external-test".to_owned()).build();
    let delete_executor = new_executor(&empty_stack)?;

    let plan = delete_executor.plan(&state_after_create)?;

    // Resources that are absent from the desired stack should be deleted.
    assert!(
        plan.deletes.contains(&"func1".to_string()),
        "Absent resource should be marked for deletion"
    );

    Ok(())
}

/// Tests handling resources in various states during batch operations.
#[tokio::test]
async fn test_resources_in_various_states() -> Result<()> {
    let mut state = new_test_state();

    // Add resources in different states
    state.resources.insert(
        "running-func".to_string(),
        create_running_function_state("running-func", "image"),
    );
    state.resources.insert(
        "pending-func".to_string(),
        create_pending_function_state("pending-func"),
    );
    state.resources.insert(
        "deleted-func".to_string(),
        create_deleted_function_state("deleted-func"),
    );

    // Create stack with only running-func (should delete pending-func, skip deleted-func)
    let func = test_function("running-func");
    let stack = Stack::new("various-states-test".to_owned())
        .add(func, ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let plan = executor.plan(&state)?;

    // running-func: no changes (already running with same config)
    // pending-func: should be deleted (not in desired stack)
    // deleted-func: already deleted, no action

    assert!(
        plan.deletes.contains(&"pending-func".to_string()),
        "pending-func should be deleted"
    );
    assert!(
        !plan.deletes.contains(&"deleted-func".to_string()),
        "deleted-func already deleted, no action"
    );

    Ok(())
}

/// Tests handling pending-only resources during deletion.
/// Pending resources are removed directly from state (not marked as Deleted).
#[tokio::test]
async fn test_delete_only_pending() -> Result<()> {
    let mut state = new_test_state();

    // Add only pending resources
    state.resources.insert(
        "pending1".to_string(),
        create_pending_function_state("pending1"),
    );
    state.resources.insert(
        "pending2".to_string(),
        create_pending_function_state("pending2"),
    );

    let deletion_executor = new_deletion_executor()?;
    let final_state = run_to_synced(&deletion_executor, state).await?;

    // Pending resources are removed directly from state (not marked as Deleted)
    assert_not_in_state(&final_state, &["pending1", "pending2"]);

    Ok(())
}

/// Tests idempotent updates (no change when config is same).
#[tokio::test]
async fn test_idempotent_update() -> Result<()> {
    let func1 = test_function("func1");

    let stack = Stack::new("idempotent-test".to_owned())
        .add(func1.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_v1 = run_to_synced(&executor, state).await?;

    // Run again with same config
    let state_v2 = run_to_synced(&executor, state_v1.clone()).await?;

    // Should be unchanged
    assert_eq!(
        get_status(&state_v2, "func1"),
        Some(ResourceStatus::Running)
    );

    // Check that config remained the same
    let config_v1 = &state_v1.resources.get("func1").unwrap().config;
    let config_v2 = &state_v2.resources.get("func1").unwrap().config;
    assert_eq!(config_v1, config_v2, "Config should remain unchanged");

    Ok(())
}

/// Tests step-by-step execution with no suggested delay.
#[tokio::test]
async fn test_step_without_delay() -> Result<()> {
    let store1 = test_storage("store1");

    let stack = Stack::new("no-delay-test".to_owned())
        .add(store1.clone(), ResourceLifecycle::Frozen)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();

    // Storage typically doesn't require polling delay
    let step_result = executor.step(state).await?;

    // First step should progress immediately
    assert!(
        matches!(
            get_status(&step_result.next_state, "store1"),
            Some(ResourceStatus::Provisioning) | Some(ResourceStatus::Running)
        ),
        "Storage should progress immediately"
    );

    Ok(())
}

/// Tests handling config change triggers update.
#[tokio::test]
async fn test_config_change_triggers_update() -> Result<()> {
    let func_v1 = test_function_with_image("func1", "image-v1");

    let stack_v1 = Stack::new("config-change-test".to_owned())
        .add(func_v1.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let state = new_test_state();
    let state_v1 = run_to_synced(&executor_v1, state).await?;

    let original_config = state_v1.resources.get("func1").unwrap().config.clone();

    // Change config
    let func_v2 = test_function_with_image("func1", "image-v2");

    let stack_v2 = Stack::new("config-change-test".to_owned())
        .add(func_v2.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v2 = new_executor(&stack_v2)?;
    let final_state = run_to_synced(&executor_v2, state_v1).await?;

    let new_config = &final_state.resources.get("func1").unwrap().config;
    assert_ne!(
        &original_config, new_config,
        "Config should change after update"
    );

    Ok(())
}

/// Tests empty stack operations.
#[tokio::test]
async fn test_empty_stack() -> Result<()> {
    let empty_stack = Stack::new("empty-test".to_owned()).build();

    let executor = new_executor(&empty_stack)?;
    let state = new_test_state();

    // Should be immediately synced
    assert!(executor.is_synced(&state), "Empty stack should be synced");

    let plan = executor.plan(&state)?;
    assert!(plan.creates.is_empty());
    assert!(plan.updates.is_empty());
    assert!(plan.deletes.is_empty());

    Ok(())
}

/// Tests that dependencies are preserved through state transitions.
#[tokio::test]
async fn test_dependencies_preserved_in_state() -> Result<()> {
    let store1 = test_storage("store1");
    let func1 = test_function("func1");

    let stack = Stack::new("deps-preserved-test".to_owned())
        .add(store1.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            func1.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "store1")],
        )
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let final_state = run_to_synced(&executor, state).await?;

    // Check func1's dependencies are stored in state
    let func_state = final_state.resources.get("func1").unwrap();
    assert!(
        !func_state.dependencies.is_empty(),
        "Dependencies should be preserved in state"
    );
    assert!(
        func_state.dependencies.iter().any(|d| d.id() == "store1"),
        "Should depend on store1"
    );

    Ok(())
}

/// Tests lifecycle is preserved in state.
#[tokio::test]
async fn test_lifecycle_preserved_in_state() -> Result<()> {
    let frozen_store = test_storage("frozen-store");
    let live_func = test_function("live-func");

    let stack = Stack::new("lifecycle-preserved-test".to_owned())
        .add(frozen_store.clone(), ResourceLifecycle::Frozen)
        .add(live_func.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let final_state = run_to_synced(&executor, state).await?;

    assert_eq!(
        final_state.resources.get("frozen-store").unwrap().lifecycle,
        Some(ResourceLifecycle::Frozen)
    );
    assert_eq!(
        final_state.resources.get("live-func").unwrap().lifecycle,
        Some(ResourceLifecycle::Live)
    );

    Ok(())
}

// ─── External binding + lifecycle filter regression tests ───────────────
//
// These tests cover the interaction between externally provisioned resources
// and lifecycle filters. The push model runs InitialSetup locally (no filter,
// external binding injected) then hands off to the manager for Provisioning
// (Live filter only). The Frozen external binding must survive in state across
// multiple Provisioning steps — if removed, Live resources that depend on it
// fail with DEPENDENCY_NOT_READY.

/// Regression test: externally provisioned Frozen resource must NOT be removed
/// from state when the executor runs with a Live lifecycle filter.
///
/// This is the exact bug that caused push model Azure e2e failures:
/// step() cleanup code removed external resources not in self.resources,
/// but lifecycle-filtered resources were also absent from self.resources.
#[tokio::test]
async fn test_external_binding_preserved_with_lifecycle_filter() -> Result<()> {
    let frozen_store = test_storage("infra-resource");
    let live_func = test_function("live-func");

    // Stack has both Frozen and Live resources
    let stack = Stack::new("external-filter-test".to_owned())
        .add(frozen_store.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            live_func.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "infra-resource")],
        )
        .build();

    // Simulate state after InitialSetup: both resources Running.
    let mut state = new_test_state();
    let mut infra_state = create_running_function_state("infra-resource", "n/a");
    infra_state.resource_type = Storage::RESOURCE_TYPE.to_string();
    infra_state.config = Resource::new(frozen_store.clone());
    infra_state.lifecycle = Some(ResourceLifecycle::Frozen);
    state
        .resources
        .insert("infra-resource".to_string(), infra_state);

    let mut func_state = create_running_function_state("live-func", "test-image-live-func");
    func_state.dependencies = vec![ResourceRef::new(Storage::RESOURCE_TYPE, "infra-resource")];
    state.resources.insert("live-func".to_string(), func_state);

    // Create executor with Live filter (simulates Provisioning phase)
    let executor = new_executor_with_filter(&stack, vec![ResourceLifecycle::Live])?;

    // Run a step — the external resource must survive
    let step_result = executor.step(state).await?;

    assert!(
        step_result
            .next_state
            .resources
            .contains_key("infra-resource"),
        "Frozen resource must NOT be removed when Live filter is active. \
         Available: {:?}",
        step_result.next_state.resources.keys().collect::<Vec<_>>()
    );

    Ok(())
}

/// Multi-step regression test: simulates the push model's Provisioning phase
/// where the manager runs multiple steps. The frozen resource must survive
/// across ALL steps, not just the first one.
#[tokio::test]
async fn test_external_binding_survives_multiple_provisioning_steps() -> Result<()> {
    let frozen_store = test_storage("infra-resource");
    let live_func = test_function("live-func");

    let stack = Stack::new("multi-step-external-test".to_owned())
        .add(frozen_store.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            live_func.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "infra-resource")],
        )
        .build();

    // State: infra-resource is Running, live-func is Running
    let mut state = new_test_state();
    let mut infra_state = create_running_function_state("infra-resource", "n/a");
    infra_state.resource_type = Storage::RESOURCE_TYPE.to_string();
    infra_state.config = Resource::new(frozen_store.clone());
    infra_state.lifecycle = Some(ResourceLifecycle::Frozen);
    state
        .resources
        .insert("infra-resource".to_string(), infra_state);

    let mut func_state = create_running_function_state("live-func", "test-image-live-func");
    func_state.dependencies = vec![ResourceRef::new(Storage::RESOURCE_TYPE, "infra-resource")];
    state.resources.insert("live-func".to_string(), func_state);

    let executor = new_executor_with_filter(&stack, vec![ResourceLifecycle::Live])?;

    // Run multiple steps (simulates manager's multi-step Provisioning)
    let state_after = run_steps(&executor, state, 5).await?;

    assert!(
        state_after.resources.contains_key("infra-resource"),
        "Frozen resource must survive multiple steps. \
         Available: {:?}",
        state_after.resources.keys().collect::<Vec<_>>()
    );

    // live-func should still be Running (no changes needed)
    assert_eq!(
        get_status(&state_after, "live-func"),
        Some(ResourceStatus::Running)
    );

    Ok(())
}

/// The cleanup code SHOULD still delete resources that are genuinely absent
/// from the desired stack (not just filtered).
/// This prevents over-correction from the lifecycle filter fix.
#[tokio::test]
async fn test_absent_resource_deleted_when_genuinely_absent_from_stack() -> Result<()> {
    // Current state has both resources running from an earlier stack version.
    let live_func = test_function("live-func");
    let orphan_func = test_function("orphan-resource");
    let initial_stack = Stack::new("genuine-removal-test".to_owned())
        .add(orphan_func.clone(), ResourceLifecycle::Frozen)
        .add(live_func.clone(), ResourceLifecycle::Live)
        .build();
    let initial_executor = new_executor(&initial_stack)?;
    let state = run_to_synced(&initial_executor, new_test_state()).await?;

    // Desired stack has only live-func, so orphan-resource should enter delete.
    let stack = Stack::new("genuine-removal-test".to_owned())
        .add(live_func.clone(), ResourceLifecycle::Live)
        .build();

    // No lifecycle filter — executor sees the full stack
    let executor = new_executor(&stack)?;
    let step_result = executor.step(state).await?;

    assert!(
        matches!(
            get_status(&step_result.next_state, "orphan-resource"),
            Some(ResourceStatus::Deleting | ResourceStatus::Deleted)
        ),
        "Genuinely absent resource should enter delete flow"
    );

    Ok(())
}

/// External bindings are state-only resources. They are Running while desired
/// even though they do not have controller state.
#[tokio::test]
async fn test_external_binding_resource_syncs_without_controller() -> Result<()> {
    let storage = test_storage("external-store");
    let stack = Stack::new("external-binding-sync-test".to_owned())
        .add(storage.clone(), ResourceLifecycle::Frozen)
        .build();
    let deployment_config = external_storage_config("external-store");
    let executor = StackExecutor::builder(&stack, ClientConfig::Test)
        .deployment_config(&deployment_config)
        .build()?;

    let state = run_to_synced(&executor, new_test_state()).await?;
    let external_state = state.resources.get("external-store").unwrap();

    assert_eq!(external_state.status, ResourceStatus::Running);
    assert!(
        external_state.internal_state.is_none(),
        "External binding resources should not require controller state"
    );
    assert!(
        executor.is_synced(&state),
        "Desired external binding should be considered synced while Running"
    );

    Ok(())
}

/// Naming a resource in the bindings does not hand it over: one Alien provisioned keeps its
/// controller, which rewrites its own outputs every update. Planning against the binding
/// instead would schedule an update the controller undoes, forever, re-mutating a live
/// resource each pass.
#[tokio::test]
async fn test_a_binding_on_a_provisioned_resource_does_not_replan_forever() -> Result<()> {
    let stack = Stack::new("binding-over-provisioned-test".to_owned())
        .add(test_storage("store"), ResourceLifecycle::Live)
        .build();

    // Provisioned by Alien first, so it carries controller state and controller outputs.
    let state = run_to_synced(&new_executor(&stack)?, new_test_state()).await?;
    assert!(
        state.resources.get("store").unwrap().internal_state.is_some(),
        "fixture must own a controller for this to mean anything"
    );

    // A binding is added for the same id afterwards.
    let bound = external_storage_config("store");
    let executor = StackExecutor::builder(&stack, ClientConfig::Test)
        .deployment_config(&bound)
        .build()?;

    assert!(
        !executor.plan(&state)?.updates.contains_key("store"),
        "a resource with a controller must not be planned against its binding"
    );

    Ok(())
}

/// A Container Apps Environment's declared config is only its id, so repointing its binding
/// changes nothing the planner compares. Dependents resolve the environment through
/// the outputs it derives, so the update has to be planned off the binding itself.
#[tokio::test]
async fn test_repointing_an_external_binding_refreshes_its_outputs() -> Result<()> {
    fn config_for(environment: &str) -> DeploymentConfig {
        let mut bindings = ExternalBindings::new();
        bindings.insert(
            "env",
            ExternalBinding::ContainerAppsEnvironment(
                alien_core::ContainerAppsEnvironmentBinding::new(
                    environment.to_string(),
                    format!("/subscriptions/s/resourceGroups/rg/providers/Microsoft.App/managedEnvironments/{environment}"),
                    "rg".to_string(),
                    format!("{environment}.example.com"),
                ),
            ),
        );
        DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(bindings)
            .allow_frozen_changes(false)
            .build()
    }

    let stack = Stack::new("binding-repoint-test".to_owned())
        .add(
            alien_core::AzureContainerAppsEnvironment::new("env".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let first = config_for("old-environment");
    let state = run_to_synced(
        &StackExecutor::builder(&stack, ClientConfig::Test)
            .deployment_config(&first)
            .build()?,
        new_test_state(),
    )
    .await?;
    let outputs = format!("{:?}", state.resources.get("env").unwrap().outputs);
    assert!(
        outputs.contains("old-environment"),
        "the create should adopt the binding's outputs, got {outputs}"
    );

    // Same stack, same declared config — only the binding moved.
    let second = config_for("new-environment");
    let executor = StackExecutor::builder(&stack, ClientConfig::Test)
        .deployment_config(&second)
        .build()?;
    assert!(
        executor.plan(&state)?.updates.contains_key("env"),
        "a repointed binding must schedule the update that refreshes its outputs"
    );

    let stepped = executor.step(state).await?.next_state;
    let refreshed = format!("{:?}", stepped.resources.get("env").unwrap().outputs);
    assert!(
        refreshed.contains("new-environment") && !refreshed.contains("old-environment"),
        "the outputs must follow the binding, got {refreshed}"
    );

    Ok(())
}

/// A config change on an external binding has no cloud work behind it: the update
/// must adopt the new config directly, or the planner re-plans the same update on
/// every step and the stack never reaches synced.
#[tokio::test]
async fn test_external_binding_resource_adopts_config_changes() -> Result<()> {
    let storage = test_storage_with_public_read("external-store", false);
    let stack = Stack::new("external-binding-update-test".to_owned())
        .add(storage.clone(), ResourceLifecycle::Frozen)
        .build();
    let deployment_config = external_storage_config("external-store");
    let executor = StackExecutor::builder(&stack, ClientConfig::Test)
        .deployment_config(&deployment_config)
        .build()?;
    let state = run_to_synced(&executor, new_test_state()).await?;

    let updated_storage = test_storage_with_public_read("external-store", true);
    let updated_stack = Stack::new("external-binding-update-test".to_owned())
        .add(updated_storage.clone(), ResourceLifecycle::Frozen)
        .build();
    let update_executor = StackExecutor::builder(&updated_stack, ClientConfig::Test)
        .deployment_config(&deployment_config)
        .build()?;

    let state = run_steps(&update_executor, state, 3).await?;
    let external_state = state.resources.get("external-store").unwrap();

    assert_eq!(external_state.status, ResourceStatus::Running);
    assert_eq!(
        external_state.config,
        Resource::new(updated_storage),
        "the external binding must adopt the new declared config"
    );
    assert!(
        external_state.internal_state.is_none(),
        "adopting a config change must not synthesize controller state"
    );
    assert!(
        update_executor.is_synced(&state),
        "a config change on an external binding must reach synced"
    );

    Ok(())
}

/// Only an external binding may reach an update with no controller. Anything else has lost
/// the state its update would resume from, so the resource fails loudly rather than having
/// the update silently skipped and the deployment reporting success.
#[tokio::test]
async fn test_a_controllerless_resource_fails_its_update() -> Result<()> {
    let stack = Stack::new("controllerless-update-test".to_owned())
        .add(test_storage_with_public_read("store", true), ResourceLifecycle::Live)
        .build();

    // Running, config differs from desired, and no controller state to update from.
    let mut state = new_test_state();
    let mut entry = create_running_function_state("store", "n/a");
    entry.resource_type = Storage::RESOURCE_TYPE.to_string();
    entry.config = Resource::new(test_storage_with_public_read("store", false));
    entry.internal_state = None;
    state.resources.insert("store".to_string(), entry);

    let executor = new_executor(&stack)?;
    let step_result = executor.step(state).await?;
    let stepped = step_result.next_state.resources.get("store").unwrap();

    assert_eq!(stepped.status, ResourceStatus::UpdateFailed);
    let error = stepped
        .error
        .as_ref()
        .expect("a failed update must record why");
    assert!(
        format!("{error:?}").contains("no controller state"),
        "the error must name the missing controller state, got {error:?}"
    );

    Ok(())
}

/// A resource dropped from the desired stack is owed a deletion even while a consumer still
/// records a dependency on it — the executor defers that delete for a step, and a caller
/// judging completion has to see the debt or it declares success one step early.
#[tokio::test]
async fn test_pending_deletions_reports_a_deferred_delete() -> Result<()> {
    // Deploy both for real, so the survivor carries the controller state its update resumes
    // from, then record the dependency that defers the dropped resource's deletion.
    let deployed = Stack::new("pending-deletions-test".to_owned())
        .add(test_function("agent"), ResourceLifecycle::Live)
        .add(test_function("dropped"), ResourceLifecycle::Live)
        .build();
    let mut state = run_to_synced(
        &new_executor_with_filter(&deployed, vec![ResourceLifecycle::Live])?,
        new_test_state(),
    )
    .await?;
    state.resources.get_mut("agent").unwrap().dependencies =
        vec![ResourceRef::new(alien_core::Worker::RESOURCE_TYPE, "dropped")];

    // The release drops `dropped` and changes the survivor, which is what a scrubbed link
    // looks like: an update is planned, and it is what releases the deferred delete.
    let stack = Stack::new("pending-deletions-test".to_owned())
        .add(
            test_function_with_image("agent", "new-image"),
            ResourceLifecycle::Live,
        )
        .build();

    let executor = new_executor_with_filter(&stack, vec![ResourceLifecycle::Live])?;
    assert_eq!(
        executor.pending_deletions(&state)?,
        vec!["dropped"],
        "a dropped resource still deployed is a deletion this executor owes"
    );

    // The debt has to be payable: stepping must shed the dependency that defers it, or
    // holding the update open for this deletion would never end.
    let stepped = executor.step(state).await?.next_state;
    let agent_dependencies = &stepped.resources.get("agent").unwrap().dependencies;
    assert!(
        !agent_dependencies.iter().any(|d| d.id() == "dropped"),
        "the agent's update must drop the dependency deferring the delete, got {agent_dependencies:?}"
    );

    Ok(())
}

/// The deletion scope is the whole answer: a resource this executor would never delete must
/// not be reported as owed, or a caller waiting on it waits forever.
#[tokio::test]
async fn test_pending_deletions_excludes_resources_outside_the_deletion_scope() -> Result<()> {
    let agent = test_function("agent");
    let stack = Stack::new("pending-deletions-scope-test".to_owned())
        .add(agent, ResourceLifecycle::Live)
        .build();

    let mut state = new_test_state();
    state.resources.insert(
        "agent".to_string(),
        create_running_function_state("agent", "test-image-agent"),
    );
    let mut frozen_state = create_running_function_state("frozen-leftover", "n/a");
    frozen_state.lifecycle = Some(ResourceLifecycle::Frozen);
    state
        .resources
        .insert("frozen-leftover".to_string(), frozen_state);

    let executor = new_executor_with_filter(&stack, vec![ResourceLifecycle::Live])?;
    assert!(
        executor.pending_deletions(&state)?.is_empty(),
        "a frozen leftover is outside a Live-filtered executor's deletion scope"
    );

    Ok(())
}

/// `tracked_resource_ids` is the executor's post-filter view: a caller judging
/// convergence must see exactly the resources this executor reconciles.
#[tokio::test]
async fn test_tracked_resource_ids_follow_the_lifecycle_filter() -> Result<()> {
    let frozen_store = test_storage("frozen-store");
    let live_func = test_function("live-func");
    let stack = Stack::new("tracked-ids-test".to_owned())
        .add(frozen_store, ResourceLifecycle::Frozen)
        .add(live_func, ResourceLifecycle::Live)
        .build();

    let filtered = new_executor_with_filter(&stack, vec![ResourceLifecycle::Live])?;
    assert_eq!(
        filtered.tracked_resource_ids(),
        HashSet::from(["live-func"])
    );

    let unfiltered = new_executor(&stack)?;
    assert_eq!(
        unfiltered.tracked_resource_ids(),
        HashSet::from(["frozen-store", "live-func"])
    );

    Ok(())
}

/// An external (BYO-key) AI resource must be treated as an external binding:
/// the cloud AI controller (Bedrock/Vertex/Foundry) is SKIPPED and the resource
/// syncs Running with no controller state. Without this, an external OpenAI
/// resource would silently provision and route to Bedrock.
#[tokio::test]
async fn test_external_ai_binding_skips_cloud_controller() -> Result<()> {
    let ai = Ai::new("llm".to_string()).build();
    let stack = Stack::new("external-ai-sync-test".to_owned())
        .add(ai.clone(), ResourceLifecycle::Frozen)
        .build();
    let deployment_config = external_ai_config("llm");
    let executor = StackExecutor::builder(&stack, ClientConfig::Test)
        .deployment_config(&deployment_config)
        .build()?;

    let state = run_to_synced(&executor, new_test_state()).await?;
    let external_state = state.resources.get("llm").unwrap();

    assert_eq!(external_state.status, ResourceStatus::Running);
    assert!(
        external_state.internal_state.is_none(),
        "External AI binding must not run the cloud controller (no controller state)"
    );
    assert!(
        executor.is_synced(&state),
        "Desired external AI binding should be considered synced while Running"
    );

    Ok(())
}

/// Full-stack deletion should mark controllerless external bindings deleted
/// instead of looping forever waiting for a controller that intentionally
/// does not exist.
#[tokio::test]
async fn test_external_binding_resource_deletes_without_controller() -> Result<()> {
    let storage = test_storage("external-store");
    let stack = Stack::new("external-binding-delete-test".to_owned())
        .add(storage.clone(), ResourceLifecycle::Frozen)
        .build();
    let deployment_config = external_storage_config("external-store");
    let executor = StackExecutor::builder(&stack, ClientConfig::Test)
        .deployment_config(&deployment_config)
        .build()?;
    let state = run_to_synced(&executor, new_test_state()).await?;

    let deletion_executor =
        StackExecutor::for_deletion(ClientConfig::Test, &deployment_config, None)?;
    let deleted_state = run_to_synced(&deletion_executor, state).await?;
    let external_state = deleted_state.resources.get("external-store").unwrap();

    assert_eq!(external_state.status, ResourceStatus::Deleted);
    assert!(
        external_state.internal_state.is_none(),
        "Deleting an external binding should not synthesize controller state"
    );
    assert!(
        deletion_executor.is_synced(&deleted_state),
        "Deleted external binding should be considered synced"
    );

    Ok(())
}
