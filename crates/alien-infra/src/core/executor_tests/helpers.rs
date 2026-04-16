//! Shared test utilities for executor tests.

use crate::core::StackExecutor;
use crate::error::Result;
use alien_core::{
    ClientConfig, Function, FunctionCode, Platform, Resource, ResourceDefinition, ResourceEntry,
    ResourceLifecycle, ResourceRef, ResourceStatus, Stack, StackResourceState, StackState, Storage,
};

/// Create a test function with default settings.
pub fn test_function(id: &str) -> Function {
    Function::new(id.to_string())
        .code(FunctionCode::Image {
            image: format!("test-image-{}", id),
        })
        .permissions("execution".to_string())
        .build()
}

/// Create a test function with a specific image.
pub fn test_function_with_image(id: &str, image: &str) -> Function {
    Function::new(id.to_string())
        .code(FunctionCode::Image {
            image: image.to_string(),
        })
        .permissions("execution".to_string())
        .build()
}

/// Create a test function linked to another resource.
pub fn test_function_linked<R: ResourceDefinition>(id: &str, linked_to: &R) -> Function {
    Function::new(id.to_string())
        .code(FunctionCode::Image {
            image: format!("test-image-{}", id),
        })
        .permissions("execution".to_string())
        .link(linked_to)
        .build()
}

/// Create a test storage with default settings.
pub fn test_storage(id: &str) -> Storage {
    Storage::new(id.to_string()).build()
}

/// Create a test storage with public read setting.
pub fn test_storage_with_public_read(id: &str, public: bool) -> Storage {
    Storage::new(id.to_string()).public_read(public).build()
}

/// Get resource status from state, returning None if not present.
pub fn get_status(state: &StackState, resource_id: &str) -> Option<ResourceStatus> {
    state.resources.get(resource_id).map(|s| s.status)
}

/// Assert all resources have reached Running status.
pub fn assert_all_running(state: &StackState, ids: &[&str]) {
    for id in ids {
        assert_eq!(
            get_status(state, id),
            Some(ResourceStatus::Running),
            "Resource '{}' should be Running",
            id
        );
    }
}

/// Assert specific resources are deleted.
pub fn assert_deleted(state: &StackState, ids: &[&str]) {
    for id in ids {
        assert_eq!(
            get_status(state, id),
            Some(ResourceStatus::Deleted),
            "Resource '{}' should be Deleted",
            id
        );
    }
}

/// Assert resource is not in state.
pub fn assert_not_in_state(state: &StackState, ids: &[&str]) {
    for id in ids {
        assert!(
            !state.resources.contains_key(*id),
            "Resource '{}' should not be in state",
            id
        );
    }
}

/// Create a new StackState for testing.
pub fn new_test_state() -> StackState {
    StackState::new(Platform::Test)
}

/// Create a new StackExecutor for a stack.
pub fn new_executor(stack: &Stack) -> Result<StackExecutor> {
    StackExecutor::new(stack, ClientConfig::Test, None)
}

/// Create a new StackExecutor with lifecycle filter.
pub fn new_executor_with_filter(
    stack: &Stack,
    filter: Vec<ResourceLifecycle>,
) -> Result<StackExecutor> {
    StackExecutor::new(stack, ClientConfig::Test, Some(filter))
}

fn default_deployment_config() -> alien_core::DeploymentConfig {
    alien_core::DeploymentConfig::builder()
        .stack_settings(alien_core::StackSettings::default())
        .environment_variables(alien_core::EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(alien_core::ExternalBindings::default())
        .allow_frozen_changes(false)
        .build()
}

/// Create a deletion executor.
pub fn new_deletion_executor() -> Result<StackExecutor> {
    let deployment_config = alien_core::DeploymentConfig::builder()
        .stack_settings(alien_core::StackSettings::default())
        .environment_variables(alien_core::EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(alien_core::ExternalBindings::default())
        .allow_frozen_changes(false)
        .build();
    StackExecutor::for_deletion(ClientConfig::Test, &deployment_config, None)
}

/// Create a deletion executor with lifecycle filter.
pub fn new_deletion_executor_with_filter(filter: Vec<ResourceLifecycle>) -> Result<StackExecutor> {
    let deployment_config = alien_core::DeploymentConfig::builder()
        .stack_settings(alien_core::StackSettings::default())
        .environment_variables(alien_core::EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(alien_core::ExternalBindings::default())
        .allow_frozen_changes(false)
        .build();
    StackExecutor::for_deletion(ClientConfig::Test, &deployment_config, Some(filter))
}

/// Run executor until synced and return the final state.
pub async fn run_to_synced(executor: &StackExecutor, state: StackState) -> Result<StackState> {
    executor.run_until_synced(state).await.into_result()
}

/// Run executor step by step with a maximum step limit.
pub async fn run_steps(
    executor: &StackExecutor,
    mut state: StackState,
    max_steps: usize,
) -> Result<StackState> {
    for _ in 0..max_steps {
        if executor.is_synced(&state) {
            break;
        }
        let step_result = executor.step(state).await?;
        state = step_result.next_state;
    }
    Ok(state)
}

/// Create a running function state for testing.
pub fn create_running_function_state(id: &str, image: &str) -> StackResourceState {
    let func = test_function_with_image(id, image);
    let mut state = StackResourceState::new_pending(
        Function::RESOURCE_TYPE.to_string(),
        Resource::new(func),
        Some(ResourceLifecycle::Live),
        vec![],
    );
    state.status = ResourceStatus::Running;
    state
}

/// Create a pending function state for testing.
pub fn create_pending_function_state(id: &str) -> StackResourceState {
    let func = test_function(id);
    StackResourceState::new_pending(
        Function::RESOURCE_TYPE.to_string(),
        Resource::new(func),
        Some(ResourceLifecycle::Live),
        vec![],
    )
}

/// Create a deleting function state for testing.
pub fn create_deleting_function_state(id: &str) -> StackResourceState {
    let func = test_function(id);
    let mut state = StackResourceState::new_pending(
        Function::RESOURCE_TYPE.to_string(),
        Resource::new(func),
        Some(ResourceLifecycle::Live),
        vec![],
    );
    state.status = ResourceStatus::Deleting;
    state
}

/// Create a deleted function state for testing.
pub fn create_deleted_function_state(id: &str) -> StackResourceState {
    let func = test_function(id);
    let mut state = StackResourceState::new_pending(
        Function::RESOURCE_TYPE.to_string(),
        Resource::new(func),
        Some(ResourceLifecycle::Live),
        vec![],
    );
    state.status = ResourceStatus::Deleted;
    state
}

/// Create a provisioning function state for testing.
pub fn create_provisioning_function_state(id: &str) -> StackResourceState {
    let func = test_function(id);
    let mut state = StackResourceState::new_pending(
        Function::RESOURCE_TYPE.to_string(),
        Resource::new(func),
        Some(ResourceLifecycle::Live),
        vec![],
    );
    state.status = ResourceStatus::Provisioning;
    state
}

/// Create a provision-failed function state for testing.
pub fn create_provision_failed_function_state(id: &str) -> StackResourceState {
    let func = test_function(id);
    let mut state = StackResourceState::new_pending(
        Function::RESOURCE_TYPE.to_string(),
        Resource::new(func),
        Some(ResourceLifecycle::Live),
        vec![],
    );
    state.status = ResourceStatus::ProvisionFailed;
    state
}

/// Create an update-failed function state for testing.
pub fn create_update_failed_function_state(id: &str) -> StackResourceState {
    let func = test_function(id);
    let mut state = StackResourceState::new_pending(
        Function::RESOURCE_TYPE.to_string(),
        Resource::new(func),
        Some(ResourceLifecycle::Live),
        vec![],
    );
    state.status = ResourceStatus::UpdateFailed;
    state
}

/// Create a state with specified resources already running.
pub fn create_state_with_running(resources: Vec<(&str, ResourceLifecycle)>) -> StackState {
    let mut state = new_test_state();
    for (id, lifecycle) in resources {
        let func = test_function(id);
        let mut resource_state = StackResourceState::new_pending(
            Function::RESOURCE_TYPE.to_string(),
            Resource::new(func),
            Some(lifecycle),
            vec![],
        );
        resource_state.status = ResourceStatus::Running;
        state.resources.insert(id.to_string(), resource_state);
    }
    state
}

/// Build a stack with explicit dependencies (bypasses builder link method).
pub fn build_stack_with_deps(
    name: &str,
    resources: Vec<(Resource, ResourceLifecycle, Vec<ResourceRef>)>,
) -> Stack {
    let mut stack = Stack::new(name.to_string()).build();
    for (resource, lifecycle, deps) in resources {
        stack.resources.insert(
            resource.id().to_string(),
            ResourceEntry {
                config: resource,
                lifecycle,
                dependencies: deps,
                remote_access: false,
            },
        );
    }
    stack
}
