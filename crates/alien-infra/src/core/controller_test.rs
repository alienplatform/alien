//! # Infrastructure Controller Testing Guide
//!
//! This module provides utilities and guidance for testing infrastructure controllers.
//! When you're writing tests for a new controller, you'll want to cover three main scenarios.
//!
//! ## Testing Strategy for Infrastructure Controllers
//!
//! ### Volume Tests: Testing the Happy Path
//!
//! Start with volume tests that verify your controller works correctly across many different
//! resource configurations. These tests focus on the core lifecycle: creating resources,
//! updating them, and cleaning them up.
//!
//! For example, if you're testing a role controller, you might test it with:
//! - A basic role with no permissions
//! - A role with storage permissions
//! - A role with complex mixed permissions
//! - A management role with special settings
//!
//! These tests use basic mocks and run quickly, making them perfect for catching regressions
//! when you refactor controller logic. Use `#[rstest]` with multiple fixtures to test
//! many configurations at once.
//!
//! ### Resilience Tests: When Things Go Wrong
//!
//! Real cloud infrastructure is messy. Resources get deleted outside your control, APIs fail,
//! and cleanup operations need to handle missing resources gracefully. Test these scenarios
//! by mocking cloud APIs to return "not found" errors.
//!
//! For example, test what happens when you try to delete a role but:
//! - The IAM policy was already deleted
//! - The role itself is missing
//! - Both the policy and role are gone
//!
//! Your controller should handle these cases without failing, since the end goal
//! (the resource is gone) is achieved.
//!
//! ### Validation Tests: Getting the Details Right
//!
//! Finally, write focused tests that validate the exact API calls your controller makes.
//! These tests ensure you're sending the right data to cloud APIs - correct policy documents,
//! proper resource names, valid configurations.
//!
//! Use `.withf()` expectations to inspect the actual requests being made. For example,
//! verify that a management role generates the correct assume role policy, or that
//! storage permissions result in the expected S3 actions in the policy document.
//!
//! ## How to Write Controller Tests
//!
//! ### Setting Up Mocks
//!
//! All cloud client APIs in Alien come with built-in mock support using [MockAll](https://github.com/asomers/mockall).
//! This makes it easy to simulate cloud API responses:
//!
//! ```rust
//! use alien_aws_clients::iam::{MockIamApi, CreateRoleResponse, CreateRoleResult, Role};
//!
//! # fn create_successful_response() -> CreateRoleResponse {
//! #     CreateRoleResponse {
//! #         create_role_result: CreateRoleResult {
//! #             role: Role {
//! #                 path: "/".to_string(),
//! #                 role_name: "test-role".to_string(),
//! #                 role_id: "AROAEXAMPLE123".to_string(),
//! #                 arn: "arn:aws:iam::123456789012:role/test-role".to_string(),
//! #                 create_date: "2023-01-01T00:00:00Z".to_string(),
//! #                 assume_role_policy_document: None,
//! #                 description: None,
//! #                 max_session_duration: None,
//! #                 permissions_boundary: None,
//! #                 tags: None,
//! #                 role_last_used: None,
//! #             },
//! #         },
//! #     }
//! # }
//! let mut mock_iam = MockIamApi::new();
//! mock_iam
//!     .expect_create_role()
//!     .returning(|_| Ok(create_successful_response()));
//!
//! // For validation tests, inspect the actual requests:
//! mock_iam
//!     .expect_create_role()
//!     .withf(|request| {
//!         request.role_name == "test-role" &&
//!         request.assume_role_policy_document.contains("sts:AssumeRole")
//!     })
//!     .returning(|_| Ok(create_successful_response()));
//! ```
//!
//! ### Organizing Test Data with Fixtures
//!
//! Create a `fixtures.rs` module alongside your tests. Use `rstest::fixture` to define
//! different resource configurations you want to test:
//!
//! ```rust
//! # use alien_core::{Storage, Function, FunctionCode};
//! use rstest::fixture;
//!
//! #[fixture]
//! pub(crate) fn basic_storage() -> Storage {
//!     Storage::new("basic-storage".to_string()).build()
//! }
//!
//! #[fixture]
//! pub(crate) fn basic_function() -> Function {
//!     Function::new("basic-function".to_string())
//!         .code(FunctionCode::Image { image: "test:latest".to_string() })
//!         .permissions("execute".to_string())
//!         .build()
//! }
//! ```
//!
//! Create a separate section in your fixtures file for dependency resources that multiple
//! test configurations need:
//!
//! ```rust
//! # use alien_core::{Storage, Function, FunctionCode};
//! # use rstest::fixture;
//! // ─────────────── DEPENDENCY FIXTURES ───────────────────────────────
//!
//! #[fixture]
//! pub(crate) fn test_storage() -> Storage {
//!     Storage::new("test-storage".to_string()).build()
//! }
//!
//! #[fixture]
//! pub(crate) fn test_function() -> Function {
//!     Function::new("test-function".to_string())
//!         .code(FunctionCode::Image { image: "test:latest".to_string() })
//!         .permissions("execute".to_string())
//!         .build()
//! }
//! ```
//!
//! ### The SingleControllerExecutor
//!
//! This module provides a `SingleControllerExecutor` that lets you test a controller in isolation.
//! Set up your resource, mock dependencies, and run through the complete lifecycle:
//!
//! ```rust,no_run
//! # use alien_infra::controller_test::SingleControllerExecutor;
//! # use alien_infra::AwsStorageController;
//! # use alien_core::{Storage, Platform, ResourceStatus};
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let storage = Storage::new("test-storage".to_string()).build();
//! # let updated_storage = Storage::new("test-storage".to_string()).versioning(true).build();
//! let mut executor = SingleControllerExecutor::builder()
//!     .resource(storage)
//!     .controller(AwsStorageController::default())
//!     .platform(Platform::Aws)
//!     .build()
//!     .await
//!     .unwrap();
//!
//! // Create the resource
//! executor.run_until_terminal().await.unwrap();
//! assert_eq!(executor.status(), ResourceStatus::Running);
//!
//! // Update it
//! executor.update(updated_storage).unwrap();
//! executor.run_until_terminal().await.unwrap();
//!
//! // Clean it up
//! executor.delete().unwrap();
//! executor.run_until_terminal().await.unwrap();
//! assert_eq!(executor.status(), ResourceStatus::Deleted);
//! # Ok(())
//! # }
//! ```
//!
//! ## Putting It All Together
//!
//! Here's what a complete test file structure looks like:
//!
//! ```rust,ignore
//! use rstest::rstest;
//!
//! // Volume test: test many configurations with minimal setup
//! #[rstest]
//! #[case::basic(basic_role())]
//! #[case::with_permissions(role_with_storage_permissions())]
//! #[case::complex(role_complex_permissions())]
//! #[tokio::test]
//! async fn test_create_and_delete_flow_succeeds(#[case] role: Role) {
//!     let mock_provider = setup_happy_path_mocks(&role.id);
//!     
//!     let mut executor = SingleControllerExecutor::builder()
//!         .resource(role)
//!         .controller(AwsRoleController::default())
//!         .platform(Platform::Aws)
//!         .service_provider(mock_provider)
//!         .build()
//!         .await
//!         .unwrap();
//!     
//!     executor.run_until_terminal().await.unwrap();
//!     assert_eq!(executor.status(), ResourceStatus::Running);
//!     
//!     executor.delete().unwrap();
//!     executor.run_until_terminal().await.unwrap();
//!     assert_eq!(executor.status(), ResourceStatus::Deleted);
//! }
//!
//! // Resilience test: what happens when things are already gone?
//! #[tokio::test]
//! async fn test_deletion_when_policy_missing() {
//!     let mut mock_iam = MockIamApi::new();
//!     mock_iam
//!         .expect_delete_role_policy()
//!         .returning(|_, _| Err(not_found_error()));
//!     mock_iam
//!         .expect_delete_role()
//!         .returning(|_| Ok(()));
//!     
//!     // Should succeed even though policy deletion failed
//!     // ... rest of test
//! }
//!
//! // Validation test: check the actual API calls
//! #[tokio::test]
//! async fn test_management_role_gets_correct_policy() {
//!     let mut mock_iam = MockIamApi::new();
//!     mock_iam
//!         .expect_create_role()
//!         .withf(|request| {
//!             request.assume_role_policy_document.contains("123456789012") &&
//!             request.assume_role_policy_document.contains("sts:AssumeRole")
//!         })
//!         .returning(|_| Ok(success_response()));
//!     
//!     // ... rest of test
//! }
//! ```

use crate::core::{
    DefaultPlatformServiceProvider, PlatformServiceProvider, ResourceController,
    ResourceControllerContext, ResourceControllerStepResult, ResourceRegistry,
};
use crate::error::{ErrorData, Result};
use crate::function::{AwsFunctionController, AzureFunctionController, GcpFunctionController};
// Note: Role controllers removed - now using ServiceAccount and permission profiles
use crate::infra_requirements::AzureContainerAppsEnvironmentController;
use crate::infra_requirements::AzureResourceGroupController;
use crate::infra_requirements::AzureServiceBusNamespaceController;
use crate::infra_requirements::AzureStorageAccountController;
use crate::storage::{AwsStorageController, AzureStorageController, GcpStorageController};
use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt as _};
use alien_azure_clients::{AzureClientConfig, AzureClientConfigExt as _};
use alien_core::ClientConfig;
use alien_core::{
    AzureContainerAppsEnvironment, AzureResourceGroup, AzureServiceBusNamespace,
    AzureStorageAccount, ComputeBackend, DeploymentConfig, DomainMetadata,
    EnvironmentVariablesSnapshot, Function, FunctionCode, ManagementConfig, Platform, Resource,
    ResourceDefinition, ResourceEntry, ResourceLifecycle, ResourceOutputs, ResourceRef,
    ResourceStatus, Stack, StackResourceState, StackSettings, StackState, Storage,
};
use alien_error::{AlienError, Context};
use alien_gcp_clients::{GcpClientConfig, GcpClientConfigExt as _};
use alien_preflights::runner::PreflightRunner;
use indexmap::IndexMap;
use serde_json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info};

/// A simplified executor for testing a single controller.
///
/// This executor allows you to test a controller in isolation, including update and delete operations.
///
/// # Example
/// ```rust,no_run
/// use alien_infra::controller_test::SingleControllerExecutor;
/// use alien_infra::AwsStorageController;
/// use alien_core::{Storage, Platform, ResourceStatus};
///
/// #[tokio::test]
/// async fn test_storage_lifecycle() {
///     let storage = Storage::new("test-storage".to_string()).build();
///     let updated_storage = Storage::new("test-storage".to_string()).versioning(true).build();
///
///     let mut executor = SingleControllerExecutor::builder()
///         .resource(storage)
///         .controller(AwsStorageController::default())
///         .platform(Platform::Aws)
///         .build()
///         .await
///         .unwrap();
///
///     executor.run_until_terminal().await.unwrap();
///     assert_eq!(executor.status(), ResourceStatus::Running);
///
///     executor.update(updated_storage).unwrap();
///     executor.run_until_terminal().await.unwrap();
///     assert_eq!(executor.status(), ResourceStatus::Running);
///
///     executor.delete().unwrap();
///     executor.run_until_terminal().await.unwrap();
///     assert_eq!(executor.status(), ResourceStatus::Deleted);
/// }
/// ```
pub struct SingleControllerExecutor {
    // The controller being tested
    controller: Box<dyn ResourceController>,
    // The resource ID
    resource_id: String,
    // Platform and config
    platform: Platform,
    client_config: ClientConfig,
    // User-provided stack settings
    stack_settings: StackSettings,
    // Platform management configuration
    management_config: Option<ManagementConfig>,
    // Compute backend configuration for Horizon
    compute_backend: Option<ComputeBackend>,
    // Environment variables snapshot for deployment config
    environment_variables: EnvironmentVariablesSnapshot,
    // Domain metadata for public resources (certificates, DNS)
    domain_metadata: Option<DomainMetadata>,
    // Public URL overrides for testing (resource_id -> url)
    public_urls: Option<HashMap<String, String>>,
    // Stack and state
    desired_stack: Stack,
    stack_state: StackState,
    // Registry for environment providers
    registry: Arc<ResourceRegistry>,
    // Client provider
    service_provider: Arc<dyn PlatformServiceProvider>,
    // Resource prefix
    resource_prefix: String,
}

impl SingleControllerExecutor {
    /// Creates a new builder for the SingleControllerExecutor.
    pub fn builder() -> SingleControllerExecutorBuilder {
        SingleControllerExecutorBuilder::new()
    }

    /// Runs a single step of the controller.
    pub async fn step(&mut self) -> Result<ResourceControllerStepResult> {
        // Get the desired config from the desired stack if it exists, otherwise use the current config from stack state
        // This handles the deletion case where the resource is removed from the desired stack
        let desired_config = if let Some(entry) =
            self.desired_stack.resources.get(&self.resource_id)
        {
            // Resource is still in desired stack (create/update case)
            entry.config.clone()
        } else {
            // Resource is not in desired stack (deletion case) - use current config from stack state
            self.stack_state
                .resources
                .get(&self.resource_id)
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceNotFound {
                        resource_id: self.resource_id.clone(),
                        available_resources: self.stack_state.resources.keys().cloned().collect(),
                    })
                })?
                .config
                .clone()
        };

        let context = ResourceControllerContext {
            desired_config: &desired_config,
            platform: self.platform,
            client_config: self.client_config.clone(),
            state: &self.stack_state,
            resource_prefix: &self.resource_prefix,
            registry: &self.registry,
            desired_stack: &self.desired_stack,
            service_provider: &self.service_provider,
            deployment_config: &DeploymentConfig::builder()
                .stack_settings(self.stack_settings.clone())
                .maybe_management_config(self.management_config.clone())
                .maybe_compute_backend(self.compute_backend.clone())
                .environment_variables(self.environment_variables.clone())
                .external_bindings(alien_core::ExternalBindings::default())
                .allow_frozen_changes(false)
                .maybe_domain_metadata(self.domain_metadata.clone())
                .maybe_public_urls(self.public_urls.clone())
                .manager_url("https://test-manager.alien.dev".to_string())
                .deployment_token("test-deployment-token".to_string())
                .build(),
        };

        let step_result = self.controller.step(&context).await?;

        // Update the stack state with the new controller state
        if let Some(resource_state) = self.stack_state.resources.get_mut(&self.resource_id) {
            resource_state.status = self.controller.get_status();
            resource_state.outputs = self.controller.get_outputs();
            resource_state.internal_state = Some(
                crate::core::serialize_controller(&*self.controller)
                    .expect("controller serialization"),
            );
        }

        Ok(step_result)
    }

    /// Runs the controller until it reaches a "synced" state.
    ///
    /// For create/update operations, this means reaching `Running` status.
    /// For delete operations, this means reaching `Deleted` status.
    /// Also stops on failure states (`ProvisionFailed`, `UpdateFailed`, `DeleteFailed`, `RefreshFailed`).
    pub async fn run_until_terminal(&mut self) -> Result<()> {
        let max_steps = 100;
        let mut step_count = 0;

        while !self.is_synced() {
            if step_count >= max_steps {
                return Err(AlienError::new(ErrorData::ExecutionMaxStepsReached {
                    max_steps: max_steps as u64,
                    pending_resources: vec![self.resource_id.clone()],
                }));
            }

            debug!(
                "Step {}: Current status = {:?}",
                step_count,
                self.controller.get_status()
            );
            let step_result = self.step().await?;

            // Don't wait for suggested delays once we're synced - heartbeats suggest delays
            // but we want to stop as soon as we reach the desired state
            if !self.is_synced() {
                if let Some(delay) = step_result.suggested_delay {
                    debug!("Controller suggested delay of {:?}", delay);
                    #[cfg(not(target_arch = "wasm32"))]
                    tokio::time::sleep(delay).await;
                } else {
                    // Small delay to prevent tight loops
                    #[cfg(not(target_arch = "wasm32"))]
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }

            step_count += 1;
        }

        info!(
            "Controller reached synced state {:?} after {} steps",
            self.controller.get_status(),
            step_count
        );
        Ok(())
    }

    /// Checks if the controller has reached its desired state.
    ///
    /// Returns true when:
    /// - Status is `Running` (create/update succeeded)
    /// - Status is `Deleted` (delete succeeded)
    /// - Status is a failure state (`ProvisionFailed`, `UpdateFailed`, `DeleteFailed`, `RefreshFailed`)
    fn is_synced(&self) -> bool {
        matches!(
            self.controller.get_status(),
            ResourceStatus::Running
                | ResourceStatus::Deleted
                | ResourceStatus::ProvisionFailed
                | ResourceStatus::UpdateFailed
                | ResourceStatus::DeleteFailed
                | ResourceStatus::RefreshFailed
        )
    }

    /// Updates the resource configuration and transitions the controller to update state.
    pub fn update<R: ResourceDefinition>(&mut self, new_resource: R) -> Result<()> {
        let new_resource = Resource::new(new_resource);

        // Verify the resource ID matches
        if new_resource.id() != self.resource_id {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Resource ID mismatch: expected '{}', got '{}'",
                    self.resource_id,
                    new_resource.id()
                ),
                resource_id: Some(self.resource_id.clone()),
            }));
        }

        // Update the desired stack
        if let Some(entry) = self.desired_stack.resources.get_mut(&self.resource_id) {
            entry.config = new_resource.clone();
        }

        // Update the current state in stack_state to trigger update
        if let Some(resource_state) = self.stack_state.resources.get_mut(&self.resource_id) {
            // Store previous config
            resource_state.previous_config = Some(resource_state.config.clone());
            // Update to new config
            resource_state.config = new_resource;
        }

        // Transition the controller to update state
        self.controller
            .transition_to_update()
            .context(ErrorData::InfrastructureError {
                message: "Failed to transition controller to update state".to_string(),
                operation: Some("update_transition".to_string()),
                resource_id: Some(self.resource_id.clone()),
            })?;

        Ok(())
    }

    /// Deletes the resource by removing it from the desired stack and transitioning to delete state.
    pub fn delete(&mut self) -> Result<()> {
        // Remove from desired stack (making it effectively empty for this resource)
        self.desired_stack.resources.swap_remove(&self.resource_id);

        // Transition the controller to delete state
        self.controller
            .transition_to_delete_start()
            .context(ErrorData::InfrastructureError {
                message: "Failed to transition controller to delete state".to_string(),
                operation: Some("delete_transition".to_string()),
                resource_id: Some(self.resource_id.clone()),
            })?;

        Ok(())
    }

    /// Gets the current status of the controller.
    pub fn status(&self) -> ResourceStatus {
        self.controller.get_status()
    }

    /// Gets the current outputs of the controller.
    pub fn outputs(&self) -> Option<ResourceOutputs> {
        self.controller.get_outputs()
    }

    /// Gets a reference to the current stack state.
    pub fn stack_state(&self) -> &StackState {
        &self.stack_state
    }

    /// Gets a reference to the controller internal state.
    /// Attempts to downcast the controller to a specific type for typed access.
    /// Returns None if the controller is not of the expected type.
    pub fn internal_state<T: ResourceController + 'static>(&self) -> Option<&T> {
        self.controller.as_any().downcast_ref::<T>()
    }
}

/// Builder for SingleControllerExecutor
pub struct SingleControllerExecutorBuilder {
    resource: Option<Resource>,
    controller: Option<Box<dyn ResourceController>>,
    platform: Option<Platform>,
    stack_settings: StackSettings,
    management_config: Option<ManagementConfig>,
    compute_backend: Option<ComputeBackend>,
    environment_variables: EnvironmentVariablesSnapshot,
    domain_metadata: Option<DomainMetadata>,
    public_urls: Option<HashMap<String, String>>,
    dependencies: Vec<(ResourceRef, Resource, Box<dyn ResourceController>)>,
    service_provider: Option<Arc<dyn PlatformServiceProvider>>,
}

impl SingleControllerExecutorBuilder {
    fn new() -> Self {
        Self {
            resource: None,
            controller: None,
            platform: None,
            stack_settings: StackSettings::default(),
            management_config: None,
            compute_backend: None,
            environment_variables: EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            },
            domain_metadata: None,
            public_urls: None,
            dependencies: Vec::new(),
            service_provider: None,
        }
    }

    /// Sets the stack settings.
    pub fn stack_settings(mut self, settings: StackSettings) -> Self {
        self.stack_settings = settings;
        self
    }

    /// Sets the management configuration.
    pub fn management_config(mut self, config: ManagementConfig) -> Self {
        self.management_config = Some(config);
        self
    }

    /// Sets the compute backend configuration.
    pub fn compute_backend(mut self, backend: ComputeBackend) -> Self {
        self.compute_backend = Some(backend);
        self
    }

    /// Sets the environment variables snapshot.
    pub fn environment_variables(mut self, variables: EnvironmentVariablesSnapshot) -> Self {
        self.environment_variables = variables;
        self
    }

    /// Sets the domain metadata for public resources (certificates, DNS).
    pub fn domain_metadata(mut self, metadata: DomainMetadata) -> Self {
        self.domain_metadata = Some(metadata);
        self
    }

    /// Sets public URL overrides for testing (resource_id -> full URL).
    /// Overrides the FQDN-derived URL from domain_metadata, useful for pointing
    /// readiness probes at mock HTTP servers during tests.
    pub fn public_urls(mut self, urls: HashMap<String, String>) -> Self {
        self.public_urls = Some(urls);
        self
    }

    /// Sets the resource to test.
    pub fn resource<R: ResourceDefinition>(mut self, resource: R) -> Self {
        self.resource = Some(Resource::new(resource));
        self
    }

    /// Sets the controller to test.
    pub fn controller(mut self, controller: impl ResourceController + 'static) -> Self {
        self.controller = Some(Box::new(controller));
        self
    }

    /// Sets the platform.
    pub fn platform(mut self, platform: Platform) -> Self {
        self.platform = Some(platform);
        self
    }

    /// Adds a dependency resource with its controller.
    pub fn with_dependency<R: ResourceDefinition>(
        mut self,
        resource: R,
        controller: impl ResourceController + 'static,
    ) -> Self {
        let resource = Resource::new(resource);
        let resource_ref = ResourceRef::new(resource.resource_type(), resource.id());
        self.dependencies
            .push((resource_ref, resource, Box::new(controller)));
        self
    }

    /// Sets a custom cloud client provider.
    pub fn service_provider(mut self, provider: Arc<dyn PlatformServiceProvider>) -> Self {
        self.service_provider = Some(provider);
        self
    }

    /// Adds all standard test dependencies with ready mock controllers.
    /// This includes: test-storage-1, test-storage-2, test-function-1, test-function-2, test-role-1, test-role-2
    /// Plus Azure infrastructure: azure-resource-group, azure-container-apps-environment, azure-storage-account
    /// Uses the appropriate platform-specific controllers based on the platform setting.
    pub fn with_test_dependencies(mut self) -> Self {
        let platform = self.platform.unwrap_or(Platform::Aws);

        match platform {
            Platform::Aws => {
                self = self
                    .with_dependency(
                        test_storage_1(),
                        AwsStorageController::mock_ready("test-storage-1"),
                    )
                    .with_dependency(
                        test_storage_2(),
                        AwsStorageController::mock_ready("test-storage-2"),
                    )
                    .with_dependency(
                        test_function_1(),
                        AwsFunctionController::mock_ready("test-function-1"),
                    )
                    .with_dependency(
                        test_function_2(),
                        AwsFunctionController::mock_ready("test-function-2"),
                    );
            }
            Platform::Gcp => {
                self = self
                    .with_dependency(
                        test_storage_1(),
                        GcpStorageController::mock_ready("test-storage-1"),
                    )
                    .with_dependency(
                        test_storage_2(),
                        GcpStorageController::mock_ready("test-storage-2"),
                    )
                    .with_dependency(
                        test_function_1(),
                        GcpFunctionController::mock_ready("test-function-1"),
                    )
                    .with_dependency(
                        test_function_2(),
                        GcpFunctionController::mock_ready("test-function-2"),
                    );
            }
            Platform::Azure => {
                self = self
                    .with_dependency(
                        test_storage_1(),
                        AzureStorageController::mock_ready("test-storage-1"),
                    )
                    .with_dependency(
                        test_storage_2(),
                        AzureStorageController::mock_ready("test-storage-2"),
                    )
                    .with_dependency(
                        test_function_1(),
                        AzureFunctionController::mock_ready("test-function-1"),
                    )
                    .with_dependency(
                        test_function_2(),
                        AzureFunctionController::mock_ready("test-function-2"),
                    )
                    // Azure infrastructure dependencies
                    .with_dependency(
                        test_azure_resource_group(),
                        AzureResourceGroupController::mock_ready("default-resource-group"),
                    )
                    .with_dependency(
                        test_azure_container_apps_environment(),
                        AzureContainerAppsEnvironmentController::mock_ready(
                            "default-container-env",
                        ),
                    )
                    .with_dependency(
                        test_azure_storage_account(),
                        AzureStorageAccountController::mock_ready("default-storage-account"),
                    )
                    .with_dependency(
                        test_azure_service_bus_namespace(),
                        AzureServiceBusNamespaceController::mock_ready(
                            "default-service-bus-namespace",
                        ),
                    );
            }
            _ => {
                // Fallback to AWS for unsupported platforms
                self = self
                    .with_dependency(
                        test_storage_1(),
                        AwsStorageController::mock_ready("test-storage-1"),
                    )
                    .with_dependency(
                        test_storage_2(),
                        AwsStorageController::mock_ready("test-storage-2"),
                    )
                    .with_dependency(
                        test_function_1(),
                        AwsFunctionController::mock_ready("test-function-1"),
                    )
                    .with_dependency(
                        test_function_2(),
                        AwsFunctionController::mock_ready("test-function-2"),
                    );
            }
        }

        self
    }

    /// Builds the SingleControllerExecutor.
    pub async fn build(self) -> Result<SingleControllerExecutor> {
        let resource = self.resource.ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Resource not set".to_string(),
                resource_id: None,
            })
        })?;

        let controller = self.controller.ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Controller not set".to_string(),
                resource_id: Some(resource.id().to_string()),
            })
        })?;

        let platform = self.platform.ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Platform not set".to_string(),
                resource_id: Some(resource.id().to_string()),
            })
        })?;

        // Create platform config with mock values
        let client_config = match platform {
            Platform::Aws => ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
            Platform::Gcp => ClientConfig::Gcp(Box::new(GcpClientConfig::mock())),
            Platform::Azure => ClientConfig::Azure(Box::new(AzureClientConfig::mock())),
            Platform::Test => ClientConfig::Test,
            _ => panic!("Unsupported platform for testing: {:?}", platform),
        };

        // Build stack and state directly
        let mut stack_resources = IndexMap::new();
        let mut stack_state = StackState::new(platform);
        let resource_id = resource.id().to_string();

        // Add dependencies first
        for (_dep_ref, dep_resource, dep_controller) in &self.dependencies {
            let dep_id = dep_resource.id().to_string();
            let dep_status = dep_controller.get_status();
            let dep_outputs = dep_controller.get_outputs();

            // Add to Stack resources map
            stack_resources.insert(
                dep_id.clone(),
                ResourceEntry {
                    config: dep_resource.clone(),
                    lifecycle: ResourceLifecycle::Live,
                    dependencies: vec![],
                    remote_access: false,
                },
            );

            // Serialize controller state
            let internal_state = if dep_status == ResourceStatus::Pending {
                None
            } else {
                Some(
                    crate::core::serialize_controller(&**dep_controller)
                        .expect("controller serialization"),
                )
            };

            // Add to StackState
            let stack_resource_state = StackResourceState::builder()
                .resource_type(dep_resource.resource_type().to_string())
                .status(dep_status)
                .config(dep_resource.clone())
                .maybe_internal_state(internal_state)
                .maybe_outputs(dep_outputs)
                .lifecycle(ResourceLifecycle::Live)
                .dependencies(vec![])
                .build();

            stack_state.resources.insert(dep_id, stack_resource_state);
        }

        // Add the main resource
        let status = controller.get_status();
        let outputs = controller.get_outputs();

        // Add to Stack resources map
        stack_resources.insert(
            resource_id.clone(),
            ResourceEntry {
                config: resource.clone(),
                lifecycle: ResourceLifecycle::Live,
                dependencies: self
                    .dependencies
                    .iter()
                    .map(|(ref_obj, _, _)| ref_obj.clone())
                    .collect(),
                remote_access: false,
            },
        );

        // Serialize controller state
        let internal_state = if status == ResourceStatus::Pending {
            None
        } else {
            Some(crate::core::serialize_controller(&*controller).expect("controller serialization"))
        };

        // Add to StackState
        let stack_resource_state = StackResourceState::builder()
            .resource_type(resource.resource_type().to_string())
            .status(status)
            .config(resource.clone())
            .maybe_internal_state(internal_state)
            .maybe_outputs(outputs)
            .lifecycle(ResourceLifecycle::Live)
            .dependencies(
                self.dependencies
                    .iter()
                    .map(|(ref_obj, _, _)| ref_obj.clone())
                    .collect(),
            )
            .build();

        stack_state
            .resources
            .insert(resource_id.clone(), stack_resource_state);

        // Create a default permission profile for test functions
        let mut default_profile = alien_core::permissions::PermissionProfile::new();
        default_profile.0.insert(
            "*".to_string(),
            vec![alien_core::permissions::PermissionSetReference::from_name(
                "function/execute",
            )],
        );

        let mut permissions = IndexMap::new();
        permissions.insert("default-profile".to_string(), default_profile);

        let mut stack = Stack {
            id: "test-stack".to_string(),
            resources: stack_resources,
            permissions: alien_core::permissions::PermissionsConfig {
                profiles: permissions,
                management: alien_core::permissions::ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        // Set resource prefix in stack state
        stack_state.resource_prefix = "test".to_string();

        // Apply mutations only (skip compile-time checks) to process the stack
        let preflight_runner = PreflightRunner::new();
        let config = DeploymentConfig::builder()
            .stack_settings(self.stack_settings.clone())
            .maybe_management_config(self.management_config.clone())
            .maybe_compute_backend(self.compute_backend.clone())
            .environment_variables(self.environment_variables.clone())
            .external_bindings(alien_core::ExternalBindings::default())
            .allow_frozen_changes(false)
            .build();

        let processed_stack = preflight_runner
            .apply_mutations(stack, &stack_state, &config)
            .await
            .context(ErrorData::InfrastructureError {
                message: "Cannot apply stack mutations".to_string(),
                operation: Some("apply_mutations".to_string()),
                resource_id: None,
            })?;
        stack = processed_stack;

        // After stack processing, add any new resources created by the stack processor to the stack state
        for (new_resource_id, new_resource_entry) in stack.resources() {
            if !stack_state.resources.contains_key(new_resource_id) {
                // This is a new resource created by the stack processor (like ServiceAccount, infrastructure resources)
                // Create a mock ready controller for it based on the resource type and platform
                let mock_controller: Box<dyn ResourceController> = match (new_resource_entry.config.resource_type().0.as_ref(), platform) {
                    ("service-account", Platform::Aws) => {
                        Box::new(crate::service_account::AwsServiceAccountController::mock_ready(new_resource_id))
                    },
                    ("service-account", Platform::Gcp) => {
                        Box::new(crate::service_account::GcpServiceAccountController::mock_ready(new_resource_id))
                    },
                    ("service-account", Platform::Azure) => {
                        Box::new(crate::service_account::AzureServiceAccountController::mock_ready(new_resource_id))
                    },
                    ("azure-resource-group", Platform::Azure) => {
                        Box::new(crate::infra_requirements::AzureResourceGroupController::mock_ready(new_resource_id))
                    },
                    ("azure-container-apps-environment", Platform::Azure) => {
                        Box::new(crate::infra_requirements::AzureContainerAppsEnvironmentController::mock_ready(new_resource_id))
                    },
                    ("azure-storage-account", Platform::Azure) => {
                        Box::new(crate::infra_requirements::AzureStorageAccountController::mock_ready(new_resource_id))
                    },
                    // For unrecognized resource types, create a basic mock
                    _ => {
                        // Create a generic "ready" controller for unrecognized types
                        // This is a fallback - ideally all resource types should be handled above
                        Box::new(crate::storage::AwsStorageController::mock_ready(new_resource_id))
                    }
                };

                let mock_status = mock_controller.get_status();
                let mock_outputs = mock_controller.get_outputs();

                // Serialize controller state
                let internal_state = if mock_status == ResourceStatus::Pending {
                    None
                } else {
                    Some(
                        crate::core::serialize_controller(&*mock_controller)
                            .expect("controller serialization"),
                    )
                };

                // Add to StackState
                let stack_resource_state = StackResourceState::builder()
                    .resource_type(new_resource_entry.config.resource_type().to_string())
                    .status(mock_status)
                    .config(new_resource_entry.config.clone())
                    .maybe_internal_state(internal_state)
                    .maybe_outputs(mock_outputs)
                    .lifecycle(new_resource_entry.lifecycle)
                    .dependencies(new_resource_entry.dependencies.clone())
                    .build();

                stack_state
                    .resources
                    .insert(new_resource_id.clone(), stack_resource_state);
            }
        }

        Ok(SingleControllerExecutor {
            controller,
            resource_id,
            platform,
            client_config,
            stack_settings: self.stack_settings,
            management_config: self.management_config,
            compute_backend: self.compute_backend,
            environment_variables: self.environment_variables,
            domain_metadata: self.domain_metadata,
            public_urls: self.public_urls,
            desired_stack: stack,
            stack_state,
            registry: Arc::new(ResourceRegistry::with_built_ins()),
            service_provider: self
                .service_provider
                .unwrap_or_else(|| Arc::new(DefaultPlatformServiceProvider::default())),
            resource_prefix: "test".to_string(),
        })
    }
}

/// Creates the first standard test storage dependency
pub fn test_storage_1() -> Storage {
    Storage::new("test-storage-1".to_string()).build()
}

/// Creates the second standard test storage dependency
pub fn test_storage_2() -> Storage {
    Storage::new("test-storage-2".to_string()).build()
}

/// Creates the first standard test function dependency
pub fn test_function_1() -> Function {
    Function::new("test-function-1".to_string())
        .code(FunctionCode::Image {
            image: "test-image-1:latest".to_string(),
        })
        .permissions("default-profile".to_string())
        .build()
}

/// Creates the second standard test function dependency
pub fn test_function_2() -> Function {
    Function::new("test-function-2".to_string())
        .code(FunctionCode::Image {
            image: "test-image-2:latest".to_string(),
        })
        .permissions("default-profile".to_string())
        .build()
}

// Note: test_role_1() and test_role_2() methods removed - using ServiceAccount and permission profiles instead

/// Creates a standard test Azure Resource Group dependency
pub fn test_azure_resource_group() -> AzureResourceGroup {
    AzureResourceGroup::new("default-resource-group".to_string()).build()
}

/// Creates a standard test Azure Container Apps Environment dependency
pub fn test_azure_container_apps_environment() -> AzureContainerAppsEnvironment {
    AzureContainerAppsEnvironment::new("default-container-env".to_string()).build()
}

/// Creates a standard test Azure Storage Account dependency
pub fn test_azure_storage_account() -> AzureStorageAccount {
    AzureStorageAccount::new("default-storage-account".to_string()).build()
}

/// Creates a standard test Azure Service Bus Namespace dependency
pub fn test_azure_service_bus_namespace() -> AzureServiceBusNamespace {
    AzureServiceBusNamespace::new("default-service-bus-namespace".to_string()).build()
}
