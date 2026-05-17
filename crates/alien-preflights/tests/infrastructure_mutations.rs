use alien_core::{
    AwsManagementConfig, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings,
    ManagementConfig, PermissionsConfig, Platform, RemoteStackManagement, ResourceLifecycle, Stack,
    StackSettings, StackState, Storage, Worker, WorkerCode,
};
use alien_preflights::{runner::PreflightRunner, PreflightRegistry};

fn empty_env_snapshot() -> EnvironmentVariablesSnapshot {
    EnvironmentVariablesSnapshot {
        variables: Vec::new(),
        hash: String::new(),
        created_at: "2024-01-01T00:00:00Z".to_string(),
    }
}

#[tokio::test]
async fn test_remote_management_dependency_is_wired_after_all_mutations() {
    let function = Worker::new("test-function".to_string())
        .code(WorkerCode::Image {
            image: "test-image:latest".to_string(),
        })
        .permissions("test-permissions".to_string())
        .build();

    let storage = Storage::new("test-storage".to_string()).build();

    let stack = Stack::new("test-stack".to_string())
        .add(function, ResourceLifecycle::Live)
        .add(storage, ResourceLifecycle::Frozen)
        .permissions(PermissionsConfig::new())
        .build();

    let stack_state = StackState::new(Platform::Aws);
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .management_config(ManagementConfig::Aws(AwsManagementConfig {
            managing_role_arn: "arn:aws:iam::111122223333:role/alien-management".to_string(),
        }))
        .environment_variables(empty_env_snapshot())
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();

    let result = PreflightRunner::new()
        .apply_mutations(stack, &stack_state, &config)
        .await
        .unwrap();

    let remote_management = alien_core::ResourceRef::new(
        RemoteStackManagement::RESOURCE_TYPE,
        "remote-stack-management",
    );

    assert!(
        result.resources.contains_key("remote-stack-management"),
        "remote management should be added for managed AWS deployments"
    );

    for (resource_id, entry) in &result.resources {
        if resource_id == "remote-stack-management" {
            assert!(
                !entry.dependencies.contains(&remote_management),
                "remote management should not depend on itself"
            );
            continue;
        }

        assert!(
            entry.dependencies.contains(&remote_management),
            "{resource_id} should depend on remote management"
        );
    }
}

#[tokio::test]
async fn test_azure_infrastructure_mutations() {
    // Create a test stack with a function and storage resource
    let function = Worker::new("test-function".to_string())
        .code(WorkerCode::Image {
            image: "test-image:latest".to_string(),
        })
        .permissions("test-permissions".to_string())
        .build();

    let storage = Storage::new("test-storage".to_string()).build();

    let stack = Stack::new("test-stack".to_string())
        .add(function, ResourceLifecycle::Live)
        .add(storage, ResourceLifecycle::Frozen)
        .permissions(PermissionsConfig::new())
        .build();

    // Apply mutations for Azure platform
    let stack_state = StackState::new(Platform::Azure);
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(empty_env_snapshot())
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    let registry = PreflightRegistry::with_built_ins();
    let mutations = registry.get_mutations(&stack, &stack_state, &config);

    // Apply each mutation
    let mut current_stack = stack;
    for mutation in mutations {
        current_stack = mutation
            .mutate(current_stack, &stack_state, &config)
            .await
            .unwrap();
    }

    // Verify that infrastructure resources were added
    assert!(
        current_stack
            .resources
            .contains_key("default-resource-group"),
        "Azure Resource Group should be added"
    );
    assert!(
        current_stack.resources.contains_key("enable-app"),
        "Microsoft.App service activation should be added"
    );
    assert!(
        current_stack.resources.contains_key("enable-storage"),
        "Microsoft.Storage service activation should be added"
    );
    assert!(
        current_stack
            .resources
            .contains_key("default-container-env"),
        "Container Apps Environment should be added"
    );
    assert!(
        current_stack
            .resources
            .contains_key("default-storage-account"),
        "Storage Account should be added"
    );

    // Verify dependencies were added correctly
    let function_entry = current_stack.resources.get("test-function").unwrap();
    assert!(
        function_entry
            .dependencies
            .iter()
            .any(|dep| dep.id() == "default-resource-group"),
        "Worker should depend on resource group"
    );
    assert!(
        function_entry
            .dependencies
            .iter()
            .any(|dep| dep.id() == "enable-app"),
        "Worker should depend on Microsoft.App service"
    );
    assert!(
        function_entry
            .dependencies
            .iter()
            .any(|dep| dep.id() == "default-container-env"),
        "Worker should depend on container environment"
    );

    let storage_entry = current_stack.resources.get("test-storage").unwrap();
    assert!(
        storage_entry
            .dependencies
            .iter()
            .any(|dep| dep.id() == "default-resource-group"),
        "Storage should depend on resource group"
    );
    assert!(
        storage_entry
            .dependencies
            .iter()
            .any(|dep| dep.id() == "enable-storage"),
        "Storage should depend on Microsoft.Storage service"
    );
    assert!(
        storage_entry
            .dependencies
            .iter()
            .any(|dep| dep.id() == "default-storage-account"),
        "Storage should depend on storage account"
    );
}

#[tokio::test]
async fn test_gcp_infrastructure_mutations() {
    // Create a test stack with a function resource
    let function = Worker::new("test-function".to_string())
        .code(WorkerCode::Image {
            image: "test-image:latest".to_string(),
        })
        .permissions("test-permissions".to_string())
        .build();

    let stack = Stack::new("test-stack".to_string())
        .add(function, ResourceLifecycle::Live)
        .permissions(PermissionsConfig::new())
        .build();

    // Apply mutations for GCP platform
    let stack_state = StackState::new(Platform::Gcp);
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(empty_env_snapshot())
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    let registry = PreflightRegistry::with_built_ins();
    let mutations = registry.get_mutations(&stack, &stack_state, &config);

    // Apply each mutation
    let mut current_stack = stack;
    for mutation in mutations {
        current_stack = mutation
            .mutate(current_stack, &stack_state, &config)
            .await
            .unwrap();
    }

    // Verify that service activation was added
    assert!(
        current_stack.resources.contains_key("enable-cloud-run"),
        "Cloud Run API should be enabled"
    );

    // Verify dependencies were added correctly
    let function_entry = current_stack.resources.get("test-function").unwrap();
    assert!(
        function_entry
            .dependencies
            .iter()
            .any(|dep| dep.id() == "enable-cloud-run"),
        "Worker should depend on Cloud Run API"
    );
}

#[tokio::test]
async fn test_kubernetes_infrastructure_mutations() {
    // Create a test stack with a function resource
    let function = Worker::new("test-function".to_string())
        .code(WorkerCode::Image {
            image: "test-image:latest".to_string(),
        })
        .permissions("test-permissions".to_string())
        .build();

    let stack = Stack::new("test-stack".to_string())
        .add(function, ResourceLifecycle::Live)
        .permissions(PermissionsConfig::new())
        .build();

    // Apply mutations for Kubernetes platform
    let stack_state = StackState::new(Platform::Kubernetes);
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(empty_env_snapshot())
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    let registry = PreflightRegistry::with_built_ins();
    let mutations = registry.get_mutations(&stack, &stack_state, &config);

    // Apply each mutation
    let mut current_stack = stack;
    for mutation in mutations {
        current_stack = mutation
            .mutate(current_stack, &stack_state, &config)
            .await
            .unwrap();
    }

    // Kubernetes namespace is created by Helm; mutation should not add a namespace resource.
    assert!(
        !current_stack.resources.contains_key("ns"),
        "Kubernetes namespace should not be added by preflights"
    );

    // Verify dependencies were not added for a namespace that doesn't exist
    let function_entry = current_stack.resources.get("test-function").unwrap();
    assert!(
        !function_entry
            .dependencies
            .iter()
            .any(|dep| dep.id() == "ns"),
        "Worker should not depend on a namespace resource"
    );
}

#[tokio::test]
async fn test_no_mutations_for_aws() {
    // Create a test stack with a function resource
    let function = Worker::new("test-function".to_string())
        .code(WorkerCode::Image {
            image: "test-image:latest".to_string(),
        })
        .permissions("test-permissions".to_string())
        .build();

    let stack = Stack::new("test-stack".to_string())
        .add(function, ResourceLifecycle::Live)
        .permissions(PermissionsConfig::new())
        .build();

    // Apply mutations for AWS platform
    let stack_state = StackState::new(Platform::Aws);
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(empty_env_snapshot())
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    let registry = PreflightRegistry::with_built_ins();
    let mutations = registry.get_mutations(&stack, &stack_state, &config);

    // Apply each mutation
    let mut current_stack = stack;
    for mutation in mutations {
        current_stack = mutation
            .mutate(current_stack, &stack_state, &config)
            .await
            .unwrap();
    }

    // AWS should not have any infrastructure mutations (except for management/service accounts)
    // but infrastructure-specific resources like resource groups, namespaces should not be added
    assert!(
        !current_stack
            .resources
            .contains_key("default-resource-group"),
        "AWS should not have Azure Resource Group"
    );
    assert!(
        !current_stack.resources.contains_key("ns"),
        "AWS should not have Kubernetes namespace"
    );
    assert!(
        !current_stack.resources.contains_key("enable-cloud-run"),
        "AWS should not have GCP service activations"
    );
}

#[tokio::test]
async fn test_empty_stack_no_mutations() {
    // Create an empty stack
    let stack = Stack::new("empty-stack".to_string())
        .permissions(PermissionsConfig::new())
        .build();

    // Apply mutations for Azure platform
    let stack_state = StackState::new(Platform::Azure);
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(empty_env_snapshot())
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    let registry = PreflightRegistry::with_built_ins();
    let mutations = registry.get_mutations(&stack, &stack_state, &config);

    // Apply each mutation
    let mut current_stack = stack;
    for mutation in mutations {
        current_stack = mutation
            .mutate(current_stack, &stack_state, &config)
            .await
            .unwrap();
    }

    // Empty stack should only get management-related resources, not infrastructure
    assert!(
        !current_stack
            .resources
            .contains_key("default-resource-group"),
        "Empty stack should not get resource group"
    );
    assert!(
        !current_stack.resources.contains_key("enable-app"),
        "Empty stack should not get service activations"
    );
}

#[tokio::test]
async fn test_mutation_ordering() {
    // Create a test stack with a function resource
    let function = Worker::new("test-function".to_string())
        .code(WorkerCode::Image {
            image: "test-image:latest".to_string(),
        })
        .permissions("test-permissions".to_string())
        .build();

    let stack = Stack::new("test-stack".to_string())
        .add(function, ResourceLifecycle::Live)
        .permissions(PermissionsConfig::new())
        .build();

    // Get mutations for Azure platform
    let stack_state = StackState::new(Platform::Azure);
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(empty_env_snapshot())
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    let registry = PreflightRegistry::with_built_ins();
    let mutations = registry.get_mutations(&stack, &stack_state, &config);

    // Verify that mutations are returned in the correct order
    let mutation_descriptions: Vec<_> = mutations.iter().map(|m| m.description()).collect();

    // Infrastructure dependencies should be last
    let infra_deps_position = mutation_descriptions
        .iter()
        .position(|&desc| {
            desc == "Add dependencies from user resources to infrastructure resources"
        })
        .expect("Infrastructure dependencies mutation should be present");

    // Verify it's the last mutation
    assert_eq!(
        infra_deps_position,
        mutation_descriptions.len() - 1,
        "Infrastructure dependencies mutation should be last"
    );

    // Verify that resource group comes before service activations
    let resource_group_pos = mutation_descriptions
        .iter()
        .position(|&desc| desc == "Add Azure Resource Group required by all Azure resources");
    let service_activation_pos = mutation_descriptions
        .iter()
        .position(|&desc| desc == "Enable required Azure service providers for resources");

    if let (Some(rg_pos), Some(sa_pos)) = (resource_group_pos, service_activation_pos) {
        assert!(
            rg_pos < sa_pos,
            "Resource group should come before service activations"
        );
    }
}
