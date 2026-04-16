//! Stress tests for large scale dependency graphs.
//!
//! These tests verify the executor can handle complex scenarios with many resources.

use std::time::Duration;

use super::helpers::*;
use crate::error::{ErrorData, Result};
use alien_core::{
    Function, Resource, ResourceLifecycle, ResourceRef, ResourceStatus, Stack, Storage,
};
use alien_error::AlienError;
use rand::{seq::IndexedRandom, Rng};

/// Tests a complex dependency graph with 100+ resources.
#[tokio::test]
async fn test_complex_large_dependency_graph() -> Result<()> {
    const NUM_STORAGE: usize = 50;
    const NUM_FUNCTIONS: usize = 50;
    const TOTAL_RESOURCES: usize = NUM_STORAGE + NUM_FUNCTIONS;

    let mut stack_builder = Stack::new("complex-stack".to_owned());

    // Create storages (no dependencies)
    let mut storages = Vec::with_capacity(NUM_STORAGE);
    for i in 0..NUM_STORAGE {
        let storage = test_storage(&format!("store-{}", i));
        storages.push(storage.clone());
        stack_builder = stack_builder.add(storage, ResourceLifecycle::Frozen);
    }

    // Create functions with random dependencies
    let mut functions = Vec::with_capacity(NUM_FUNCTIONS);
    let mut rng = rand::rng();

    for i in 0..NUM_FUNCTIONS {
        let func_name = format!("func-{}", i);
        let func = test_function(&func_name);

        // Functions can depend on any storage or any *previous* function
        let mut potential_deps: Vec<Resource> =
            storages.iter().cloned().map(Resource::new).collect();
        potential_deps.extend(functions.iter().cloned().map(Resource::new));

        let mut deps = Vec::new();
        if !potential_deps.is_empty() {
            let num_deps = rng.random_range(1..=3.min(potential_deps.len()));
            let chosen_deps: Vec<&Resource> = potential_deps
                .as_slice()
                .choose_multiple(&mut rng, num_deps)
                .collect();

            for dep_ref in chosen_deps {
                deps.push(ResourceRef::new(dep_ref.resource_type(), dep_ref.id()));
            }
        }

        functions.push(func.clone());
        stack_builder = stack_builder.add_with_dependencies(func, ResourceLifecycle::Live, deps);
    }

    let stack = stack_builder.build();

    let executor = new_executor(&stack)?;
    let mut state = new_test_state();

    // Run with step limit
    let max_steps = TOTAL_RESOURCES * 10;
    let mut step_counter = 0;

    while !executor.is_synced(&state) {
        if step_counter >= max_steps {
            return Err(AlienError::new(ErrorData::ExecutionMaxStepsReached {
                max_steps: max_steps as u64,
                pending_resources: vec!["timeout".to_string()],
            }));
        }

        let step_result = executor.step(state).await?;
        state = step_result.next_state;

        // Verify dependencies are respected
        for (_id, resource) in stack.resources() {
            let resource_id = resource.config.id();
            if let Some(current_resource_state) = state.resources.get(resource_id) {
                let dependencies = resource.config.get_dependencies();
                if !dependencies.is_empty()
                    && current_resource_state.status != ResourceStatus::Pending
                {
                    for dep_ref in &dependencies {
                        let dep_status = get_status(&state, dep_ref.id());
                        assert!(
                            matches!(
                                dep_status,
                                Some(ResourceStatus::Running) | Some(ResourceStatus::Deleted)
                            ),
                            "Dependency {} should be Running/Deleted before {} processes",
                            dep_ref.id(),
                            resource_id
                        );
                    }
                }
            }
        }

        std::thread::sleep(Duration::from_millis(1));
        step_counter += 1;
    }

    // Verify all resources reached Running state
    for i in 0..NUM_STORAGE {
        let id = format!("store-{}", i);
        assert_eq!(
            get_status(&state, &id),
            Some(ResourceStatus::Running),
            "Storage {} should be Running",
            id
        );
    }
    for i in 0..NUM_FUNCTIONS {
        let id = format!("func-{}", i);
        assert_eq!(
            get_status(&state, &id),
            Some(ResourceStatus::Running),
            "Function {} should be Running",
            id
        );
    }

    Ok(())
}

/// Tests a deep dependency chain (20+ levels).
#[tokio::test]
async fn test_deep_dependency_chain() -> Result<()> {
    const CHAIN_DEPTH: usize = 20;

    let mut stack_builder = Stack::new("deep-chain-stack".to_owned());

    // Create the initial storage resource
    let base_storage = test_storage("store-base");
    stack_builder = stack_builder.add(base_storage.clone(), ResourceLifecycle::Frozen);

    // Create functions in a chain
    let mut previous_resource = Resource::new(base_storage);
    let mut all_func_ids = Vec::with_capacity(CHAIN_DEPTH);

    for i in 0..CHAIN_DEPTH {
        let func_name = format!("func-chain-{}", i);
        let func = test_function(&func_name);

        let dep_ref = ResourceRef::new(previous_resource.resource_type(), previous_resource.id());

        all_func_ids.push(func_name.clone());
        stack_builder = stack_builder.add_with_dependencies(
            func.clone(),
            ResourceLifecycle::Live,
            vec![dep_ref],
        );
        previous_resource = Resource::new(func);
    }

    let stack = stack_builder.build();
    let total_resources = CHAIN_DEPTH + 1;

    let executor = new_executor(&stack)?;
    let mut state = new_test_state();

    // Run step-by-step
    let max_steps = total_resources * 10;
    let mut step_counter = 0;

    while !executor.is_synced(&state) {
        if step_counter >= max_steps {
            return Err(AlienError::new(ErrorData::ExecutionMaxStepsReached {
                max_steps: max_steps as u64,
                pending_resources: vec!["timeout".to_string()],
            }));
        }

        let step_result = executor.step(state).await?;
        state = step_result.next_state;

        // Verify dependencies
        for (_id, resource) in stack.resources() {
            let resource_id = resource.config.id();
            if let Some(current_resource_state) = state.resources.get(resource_id) {
                let dependencies = resource.config.get_dependencies();
                if !dependencies.is_empty()
                    && current_resource_state.status != ResourceStatus::Pending
                {
                    for dep_ref in &dependencies {
                        let dep_status = get_status(&state, dep_ref.id());
                        assert!(
                            matches!(
                                dep_status,
                                Some(ResourceStatus::Running) | Some(ResourceStatus::Deleted)
                            ),
                            "Dependency {} should be Running/Deleted before {} processes",
                            dep_ref.id(),
                            resource_id
                        );
                    }
                }
            }
        }

        std::thread::sleep(Duration::from_millis(1));
        step_counter += 1;
    }

    // Verify final state
    assert_eq!(
        get_status(&state, "store-base"),
        Some(ResourceStatus::Running),
        "Base storage failed"
    );
    for func_id in &all_func_ids {
        assert_eq!(
            get_status(&state, func_id),
            Some(ResourceStatus::Running),
            "Function {} failed",
            func_id
        );
    }

    Ok(())
}

/// Tests wide dependency graph (many resources depending on same base).
#[tokio::test]
async fn test_wide_dependency_graph() -> Result<()> {
    const NUM_DEPENDENTS: usize = 30;

    let base_storage = test_storage("base-storage");

    let mut stack_builder = Stack::new("wide-graph-stack".to_owned())
        .add(base_storage.clone(), ResourceLifecycle::Frozen);

    // Create many functions all depending on the same storage
    for i in 0..NUM_DEPENDENTS {
        let func = test_function(&format!("func-{}", i));
        stack_builder = stack_builder.add_with_dependencies(
            func,
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "base-storage")],
        );
    }

    let stack = stack_builder.build();
    let executor = new_executor(&stack)?;
    let state = new_test_state();

    let final_state = run_to_synced(&executor, state).await?;

    assert_eq!(
        get_status(&final_state, "base-storage"),
        Some(ResourceStatus::Running)
    );
    for i in 0..NUM_DEPENDENTS {
        assert_eq!(
            get_status(&final_state, &format!("func-{}", i)),
            Some(ResourceStatus::Running)
        );
    }

    Ok(())
}

/// Tests diamond pattern at scale.
#[tokio::test]
async fn test_diamond_pattern_at_scale() -> Result<()> {
    // Pattern:
    //       A
    //      /|\
    //     B C D
    //      \|/
    //       E
    // Repeated multiple times with different bases

    const NUM_DIAMONDS: usize = 10;

    let mut stack_builder = Stack::new("diamond-scale-stack".to_owned());

    for d in 0..NUM_DIAMONDS {
        let base = test_storage(&format!("base-{}", d));
        let left = test_function(&format!("left-{}", d));
        let middle = test_function(&format!("middle-{}", d));
        let right = test_function(&format!("right-{}", d));
        let tip = test_function(&format!("tip-{}", d));

        stack_builder = stack_builder
            .add(base.clone(), ResourceLifecycle::Frozen)
            .add_with_dependencies(
                left.clone(),
                ResourceLifecycle::Live,
                vec![ResourceRef::new(
                    Storage::RESOURCE_TYPE,
                    &format!("base-{}", d),
                )],
            )
            .add_with_dependencies(
                middle.clone(),
                ResourceLifecycle::Live,
                vec![ResourceRef::new(
                    Storage::RESOURCE_TYPE,
                    &format!("base-{}", d),
                )],
            )
            .add_with_dependencies(
                right.clone(),
                ResourceLifecycle::Live,
                vec![ResourceRef::new(
                    Storage::RESOURCE_TYPE,
                    &format!("base-{}", d),
                )],
            )
            .add_with_dependencies(
                tip,
                ResourceLifecycle::Live,
                vec![
                    ResourceRef::new(Function::RESOURCE_TYPE, &format!("left-{}", d)),
                    ResourceRef::new(Function::RESOURCE_TYPE, &format!("middle-{}", d)),
                    ResourceRef::new(Function::RESOURCE_TYPE, &format!("right-{}", d)),
                ],
            );
    }

    let stack = stack_builder.build();
    let executor = new_executor(&stack)?;
    let state = new_test_state();

    let final_state = run_to_synced(&executor, state).await?;

    // Verify all resources
    for d in 0..NUM_DIAMONDS {
        assert_eq!(
            get_status(&final_state, &format!("base-{}", d)),
            Some(ResourceStatus::Running)
        );
        assert_eq!(
            get_status(&final_state, &format!("left-{}", d)),
            Some(ResourceStatus::Running)
        );
        assert_eq!(
            get_status(&final_state, &format!("middle-{}", d)),
            Some(ResourceStatus::Running)
        );
        assert_eq!(
            get_status(&final_state, &format!("right-{}", d)),
            Some(ResourceStatus::Running)
        );
        assert_eq!(
            get_status(&final_state, &format!("tip-{}", d)),
            Some(ResourceStatus::Running)
        );
    }

    Ok(())
}
