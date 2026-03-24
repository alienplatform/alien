//! # Resource Controller Guidelines
//!
//! Resource controllers manage the lifecycle of cloud resources (create, update, delete) through state machines.
//! Each controller handles one resource type on one platform (e.g., `AwsFunctionController`, `GcpRoleController`).
//!
//! **Key Principles:**
//! - **Fail Fast on Conflict for Create/Update**: Issue the cloud API call directly and propagate all errors (including `RemoteResourceConflict`). The executor handles retries automatically. Deletion flows still tolerate `RemoteResourceNotFound`.
//! - **Always Retry from Start**: When operations fail, always retry from the beginning of that flow (Start state)
//! - **Strictly Linear State Flow**: Each flow follows the exact same sequence every time - NO conditional branching or optimization anywhere
//! - **One Logical Operation Per State**: Each state performs exactly one logical operation (command or query)
//! - **Predictability Over Efficiency**: Always go through all states in order, even if some are no-ops
//!
//! **Examples:**
//! - `AwsFunctionController`: Manages Lambda functions, IAM roles, and deployment packages
//! - `GcpStorageController`: Manages Cloud Storage buckets, permissions, and lifecycle policies  
//! - `AzureServiceController`: Manages Container Instances, networking, and monitoring
//!
//! ## State Design Principles
//!
//! ### Linear State Flow with Start States
//! Resource controllers must follow a **strictly linear state flow** where each flow has a dedicated Start state
//! that performs the first operation. This ensures predictability and simplicity.
//!
//! **Required flow pattern:**
//! ```ignore
//! enum MyResourceStatus {
//!     // Create flow - always linear
//!     CreateStart,        // Performs first operation (e.g., create service account)
//!     CreatingRole,       // Performs second operation
//!     BindingRoleToAccount, // Performs third operation
//!     Ready,              // Terminal state
//!     CreateFailed,       // Failure state
//!
//!     // Update flow - always linear
//!     UpdateStart,        // Performs update operation
//!     UpdateFailed,       // Failure state
//!
//!     // Delete flow - always linear
//!     DeleteStart,        // Performs first delete operation (e.g., unbind role)
//!     DeletingRole,       // Performs second delete operation
//!     DeletingServiceAccount, // Performs third delete operation
//!     Deleted,            // Terminal state
//!     DeleteFailed,       // Failure state
//! }
//! ```
//!
//! ### One Logical Operation Per State
//! Each state should perform exactly one logical cloud operation. This includes both **command operations**
//! (that change remote state) and **query operations** (that check remote state). Start states are **not**
//! transition states - they must perform actual operations.
//!
//! **Command vs Query Operations:**
//! - **Command Operations**: Create, update, delete resources (e.g., `create_function`, `update_function_code`)
//! - **Query Operations**: Check status, poll for completion (e.g., `get_function_configuration`, `describe_operation`)
//!
//! **Reading Before Commands is Allowed:**
//! It's perfectly fine to read/query cloud state as part of preparing for a command operation within the same state.
//! For example, checking if a resource exists before creating it, or reading current configuration before updating.
//!
//! **Why Separate Wait States Are Necessary:**
//! Many cloud operations are asynchronous and require separate command and wait phases:
//! 1. **Command Phase**: Issue the create/update/delete request (may include preparatory reads)
//! 2. **Wait Phase**: Poll until the operation completes
//!
//! Combining these phases would break proper retry behavior - if waiting fails, you want to retry just
//! the waiting, not re-issue the command.
//!
//! **Good - Separate Command and Wait:**
//! ```ignore
//! async fn create_start(&self, s: &MyState, ctx: &Context) -> Result<MyState> {
//!     // OK to read before command: check if resource already exists
//!     if let Ok(_existing) = client.get_function(&function_name).await {
//!         return Ok(MyState { status: CreateWaitForActive, ..s.clone() });
//!     }
//!     
//!     // Command operation: create the resource
//!     let response = client.create_function(&request).await?;
//!     Ok(MyState {
//!         status: CreateWaitForActive,
//!         function_arn: response.arn,
//!         ..s.clone()
//!     })
//! }
//!
//! async fn create_wait_for_active(&self, s: &MyState, ctx: &Context) -> Result<(MyState, Option<Duration>)> {
//!     // Wait operation: poll until resource is ready
//!     let status = client.get_function_configuration(&s.function_name).await?;
//!     if status.state == "Active" {
//!         Ok((MyState { status: Ready, ..s.clone() }, None))
//!     } else {
//!         Ok((s.clone(), Some(Duration::from_secs(3)))) // Retry just the waiting
//!     }
//! }
//! ```
//!
//! **Rule of thumb:** Start states should contain the logic and potential operation for their flow.
//! They can conditionally perform operations based on internal state (e.g., `if s.url.is_some() { delete_url() }`),
//! but should not be pure "decision" states that only choose the next state without any substantive logic.
//!
//! **Avoid - Pure Decision States:**
//! ```ignore
//! async fn delete_start(&self, s: &MyState, ctx: &Context) -> Result<MyState> {
//!     // Don't just decide what to do next without performing any operation
//!     if s.url.is_some() {
//!         Ok(MyState { status: DeletingUrl, ..s.clone() })
//!     } else {
//!         Ok(MyState { status: DeletingFunction, ..s.clone() })
//!     }
//! }
//! ```
//!
//! ### Strictly Linear Flow - No Conditional Optimization
//! **CRITICAL PRINCIPLE**: Linear flow means **NO conditional branching or optimization anywhere**
//! in the state machine, not just in Start states. Each state should always transition to the
//! same next state, regardless of whether the operation was needed or successful.
//!
//! **Why Predictability Over Efficiency:**
//! - Makes debugging much easier - you always know the exact path taken
//! - Simplifies retry logic - failed operations always restart from a predictable point
//! - Reduces state machine complexity and edge cases
//! - Makes the flow testable with a single path through each resource type
//!
//! **The Complete Linear Pattern:**
//! ```ignore
//! // Update flow - always follows this exact sequence
//! UpdateStart → UpdateCodeWaitForActive → UpdateConfigStart → UpdateConfigWaitForActive → Ready
//! ```
//!
//! **Good - Always Linear (Even with No-Ops):**
//! ```ignore
//! async fn update_code_wait(&self, s: &MyState, ctx: &Context) -> Result<(MyState, Option<Duration>)> {
//!     let status = client.get_function_configuration(&s.function_name).await?;
//!     if status.state == "Active" {
//!         // Always go to UpdateConfigStart next, even if no config changes needed
//!         Ok((MyState { status: UpdateConfigStart, ..s.clone() }, None))
//!     } else {
//!         Ok((s.clone(), Some(Duration::from_secs(3))))
//!     }
//! }
//!
//! async fn update_config_start(&self, s: &MyState, ctx: &Context) -> Result<(MyState, Option<Duration>)> {
//!     let current = ctx.desired_resource_config::<Function>()?;
//!     let previous = ctx.previous_resource_config::<Function>()?;
//!     let config_changed = current.memory_mb != previous.memory_mb; // ... other checks
//!     
//!     // Only perform update if needed, but always transition to wait state
//!     if config_changed {
//!         client.update_function_configuration(&request).await?;
//!     }
//!     // Always go to wait state (even if no update was performed)
//!     Ok((MyState { status: UpdateConfigWaitForActive, ..s.clone() }, Some(Duration::from_secs(3))))
//! }
//! ```
//!
//! **Avoid - Conditional Optimization in Wait States:**
//! ```ignore
//! async fn update_code_wait(&self, s: &MyState, ctx: &Context) -> Result<(MyState, Option<Duration>)> {
//!     let status = client.get_function_configuration(&s.function_name).await?;
//!     if status.state == "Active" {
//!         let config_changed = check_if_config_changed();
//!         if config_changed {
//!             // DON'T optimize by skipping states
//!             Ok((MyState { status: UpdateConfigStart, ..s.clone() }, None))
//!         } else {
//!             // DON'T jump directly to Ready - breaks linear flow
//!             Ok((MyState { status: Ready, ..s.clone() }, None))
//!         }
//!     } else {
//!         Ok((s.clone(), Some(Duration::from_secs(3))))
//!     }
//! }
//! ```
//!
//! **Multi-Step Operations:**
//! For resources that need multiple types of updates (code, configuration, networking, etc.),
//! **always** go through all phases in the same order, even if some phases are no-ops:
//!
//! ```ignore
//! enum FunctionStatus {
//!     UpdateStart,              // Updates code if needed
//!     UpdateCodeWaitForActive,  // Waits for code update (even if no-op)
//!     UpdateConfigStart,        // Updates config if needed  
//!     UpdateConfigWaitForActive,// Waits for config update (even if no-op)
//!     UpdateNetworkingStart,    // Updates networking if needed
//!     UpdateNetworkingWait,     // Waits for networking update (even if no-op)
//!     Ready,
//! }
//! ```
//!
//! This makes every update follow the exact same predictable path:
//! Start → CodeWait → ConfigStart → ConfigWait → NetworkingStart → NetworkingWait → Ready
//!
//! ### Track Created Resources
//! Store identifiers for created resources to enable proper cleanup and idempotent operations.
//!
//! ```ignore
//! #[derive(Debug, Serialize, Deserialize, Clone)]
//! struct MyResourceState {
//!     status: MyResourceStatus,
//!     service_account_id: Option<String>,    // Track created SA
//!     role_name: Option<String>,             // Track created role  
//!     role_bound: bool,                      // Track if binding succeeded
//! }
//! ```
//!
//! ## Idempotency and Retry Guidelines
//!
//! ### Automatic Retry and Manual Intervention
//! The executor automatically handles retries with exponential backoff. When a step fails, it will:
//! 1. Retry up to `MAX_RETRIES` times with exponential backoff delays
//! 2. If max retries are reached, store the failed state in `last_failed_state` and transition to `*Failed`
//! 3. Manual retry operations resume from the exact state that failed (stored in `last_failed_state`)
//!
//! **Controllers should simply bubble up errors - no special retry logic needed:**
//! ```ignore
//! async fn create_start(&self, _s: &MyState, ctx: &Context) -> Result<(MyState, Option<Duration>)> {
//!     let resp = client.create_service_account(&req).await?; // Just propagate any error
//!     Ok((MyState { status: CreatingRole, id: Some(resp.id) }, None))
//! }
//! ```
//!
//! **Benefits of this approach:**
//! - Controllers stay simple - no need to implement `transition_to_retry()`
//! - Manual retry resumes exactly where the failure occurred
//! - No complex logic to determine where to restart from
//! - User has full control over retry timing after manual intervention
//!
//! ### Create and Update Operations – Fail Fast
//! Issue the cloud API call once. If *any* error (including `RemoteResourceConflict`) occurs, simply return it.
//! The executor will back-off and retry up to `MAX_RETRIES`; after that the resource remains in `*Failed` for
//! manual intervention.
//!
//! ```ignore
//! async fn create_start(&self, _s: &MyState, ctx: &Context) -> Result<(MyState, Option<Duration>)> {
//!     let resp = client.create_service_account(&req).await?; // Propagate conflict as error
//!     Ok((MyState { status: CreatingRole, id: Some(resp.id) }, None))
//! }
//! ```
//!
//! ### Delete Operations – Ignore NotFound
//! Delete states should treat `RemoteResourceNotFound` as success and continue, making the delete flow idempotent.
//! ```ignore
//! match client.delete_service_account(&name).await {
//!     Ok(_) => next_state_deleting_role(),
//!     Err(e) if matches!(e.error, Some(CloudClientErrorData::RemoteResourceNotFound { .. })) => next_state_deleting_role(),
//!     Err(e) => return Err(e),
//! }
//! ```
//!
//! ## Error Handling and State Transitions
//!
//! ### Failure State Design
//! Use specific failure states without storing error details in the state itself.
//! Errors are stored globally in the new architecture.
//!
//! ```ignore
//! #[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
//! enum MyResourceStatus {
//!     CreateStart,
//!     CreatingRole,
//!     Ready,
//!     CreateFailed,        // No error field - stored globally
//!     UpdateStart,
//!     UpdateFailed,        // No error field - stored globally
//!     DeleteStart,
//!     DeleteFailed,        // No error field - stored globally
//!     Deleted,
//! }
//! ```
//!
//! ### Transition Methods
//! Implement state transition methods with simplified signatures:
//!
//! ```ignore
//! impl ResourceControllerState for MyResourceState {
//!     fn transition_to_failure(&self) -> Box<dyn ResourceControllerState> {
//!         // No error parameter - errors stored globally
//!         let failed_status = match &self.status {
//!             CreateStart | CreatingRole => CreateFailed,
//!             UpdateStart => UpdateFailed,
//!             DeleteStart | DeletingRole => DeleteFailed,
//!             _ => self.status.clone(),
//!         };
//!         Box::new(MyResourceState { status: failed_status, ..self.clone() })
//!     }
//!
//!     fn transition_to_delete_start(&self) -> Option<Box<dyn ResourceControllerState>> {
//!         // Only allow deletion from stable states
//!         match &self.status {
//!             Ready | CreateFailed | UpdateFailed | DeleteFailed => {
//!                 Some(Box::new(MyResourceState { status: DeleteStart, ..self.clone() }))
//!             }
//!             _ => None,
//!         }
//!     }
//!
//!     fn transition_to_update(&self) -> Option<Box<dyn ResourceControllerState>> {
//!         // No config parameter - config accessed from context
//!         if matches!(self.status, Ready | UpdateFailed) {
//!             Some(Box::new(MyResourceState { status: UpdateStart, ..self.clone() }))
//!         } else {
//!             None
//!         }
//!     }
//! }
//! ```
//!
//! **Note:** The `transition_to_retry()` method is no longer needed. Manual retries now use
//! the stored `last_failed_state` to resume exactly where the failure occurred.
//!
//! ### Resource Configuration Access
//! Use context methods to access configuration instead of storing it in state:
//!
//! ```ignore
//! async fn create_start(&self, s: &MyState, ctx: &Context) -> Result<MyState> {
//!     // Access config from context, not from state
//!     let role_config = ctx.desired_resource_config::<Role>()?;
//!     
//!     let account = client.create_service_account(&role_config.name).await?;
//!     Ok(MyState {
//!         status: CreatingRole,
//!         service_account_id: Some(account.id),
//!         ..s.clone()
//!     })
//! }
//! ```
//!
//! ## Dependency Management
//!
//! Controllers often need to access state from other resources they depend on (e.g., a Function needs its Role's ARN).
//! Use the provided helper method to safely access dependency internal state.
//!
//! ```ignore
//! async fn get_role_arn(
//!     &self,
//!     ctx: &ResourceControllerContext<'_>,
//!     role_ref: &ResourceRef,
//! ) -> Result<String> {
//!     let role_state = ctx.require_dependency::<AwsRoleState>(role_ref)?;
//!
//!     role_state.arn.as_deref()
//!         .ok_or_else(|| AlienError::new(ErrorData::DependencyNotReady {
//!             resource_id: ctx.desired_config.id().to_string(),
//!             dependency_id: role_ref.id().to_string(),
//!         }))
//!         .map(|s| s.to_string())
//! }
//! ```
//!
//! - Always validate dependency resource types before accessing state
//! - Check if dependency outputs are available before proceeding  
//! - Provide clear error messages when dependencies aren't ready
//!
//! ## Async Operation Polling
//!
//! For long-running cloud operations, controllers should check status and schedule retries rather than blocking.
//! Return a suggested delay to let the executor handle scheduling.
//!
//! ```ignore
//! async fn wait_for_operation(
//!     &self,
//!     s: &MyState,
//!     client: &Client,
//!     operation_name: &str,
//! ) -> Result<(MyState, Option<Duration>)> {
//!     let operation = client.get_operation(operation_name).await?;
//!     
//!     if operation.done {
//!         if let Some(error) = operation.error {
//!             return Err(Error::Generic { message: error.message, source: None });
//!         }
//!         Ok((MyState { status: NextStatus, ..s.clone() }, None))
//!     } else {
//!         // Don't block - schedule next check with delay
//!         Ok((s.clone(), Some(Duration::from_secs(5))))
//!     }
//! }
//! ```
//!
//! - Use exponential backoff with reasonable maximum delays
//! - Handle operation errors and completion states
//! - Never block the controller thread - always return quickly with a delay
//!
//! ## Testing Guidelines
//!
//! ### Use Fixture Functions for Test Data
//! Create fixture functions for different resource configurations to ensure consistent test data.
//! Use `rstest::fixture` for reusable test data that can be composed in different ways.
//!
//! ```ignore
//! #[fixture]
//! pub fn basic_function() -> Function {
//!     Function::new("test-func".to_string())
//!         .code(FunctionCode::Image { image: "test:latest".to_string() })
//!         .build()
//! }
//!
//! #[fixture]
//! pub fn function_with_env_vars() -> Function {
//!     let mut env = HashMap::new();
//!     env.insert("APP_ENV".to_string(), "test".to_string());
//!     basic_function().environment(env).build()
//! }
//! ```
//!
//! ### Mock Cloud Provider APIs
//! Use `httpmock::MockServer` to mock cloud provider APIs and control responses precisely.
//! Create different mock scenarios for success, failure, and edge cases.
//!
//! ```ignore
//! #[tokio::test]
//! async fn test_create_resource() {
//!     let server = MockServer::start();
//!     
//!     // Mock successful creation
//!     let create_mock = server.mock(|when, then| {
//!         when.method(POST).path("/api/resources");
//!         then.status(200).json_body(json!({"id": "resource-123"}));
//!     });
//!     
//!     // Execute test...
//!     create_mock.assert();
//! }
//! ```
//!
//! ### Test Complete Lifecycles
//! Write integration tests that validate the full resource lifecycle: create → ready → update → delete.
//! Use parameterized tests with `rstest` to test different configurations through the same lifecycle.
//!
//! ```ignore
//! #[rstest]
//! #[case::basic(basic_function())]
//! #[case::with_env(function_with_env_vars())]
//! #[tokio::test]
//! async fn test_full_lifecycle(#[case] function: Function) {
//!     // Test creation flow until Ready
//!     // Test update flow  
//!     // Test deletion flow until Deleted
//! }
//! ```
//!
//! ### Validate State Transitions
//! Test all state transition methods to ensure they follow linear flow principles.
//!
//! ```ignore
//! #[tokio::test]
//! async fn test_state_transitions() {
//!     let state = create_test_state_in_creating_role();
//!     
//!     // Test failure transition (no error parameter)
//!     let failed = state.transition_to_failure();
//!     assert!(matches!(failed.status, Status::CreateFailed));
//!     
//!     // Test manual retry resumes from last failed state
//!     // Simulate executor storing the failed state, then retry
//!     let mut stack_state = StackState::new(Platform::Test);
//!     stack_state.resources.insert("my-resource".into(), StackResourceState {
//!         resource_type: "myResource".into(),
//!         internal_state: Some(failed.box_clone()),
//!         status: failed.get_status(),
//!         outputs: failed.get_outputs(),
//!         config: Resource::new(MyResource::default()),
//!         previous_config: None,
//!         retry_attempt: 3,
//!         error: Some(AlienError::new(ErrorData::GenericError { message: "boom".into() })),
//!         is_externally_provisioned: false,
//!         lifecycle: None,
//!         dependencies: Vec::new(),
//!         last_failed_state: Some(failed.box_clone()),
//!     });
//!     stack_state.retry_failed().unwrap();
//!     let resumed = stack_state.resources.get("my-resource").unwrap();
//!     assert_eq!(resumed.retry_attempt, 0);
//! }
//! ```
//!
//! ### Test Error Scenarios
//! Create tests for various error conditions (API failures, timeouts, resource conflicts)
//! and verify that controllers handle them gracefully with appropriate retries and logging.
//!
//! ```ignore
//! #[tokio::test]
//! async fn test_api_failure_recovery() {
//!     let server = MockServer::start();
//!     
//!     // Mock API failure
//!     let error_mock = server.mock(|when, then| {
//!         when.method(POST).path("/api/resources");
//!         then.status(500).json_body(json!({"error": "Internal error"}));
//!     });
//!     
//!     let result = controller.step(state, &ctx).await;
//!     assert!(result.is_err());
//!     error_mock.assert();
//! }
//! ```
//!
//! ### Test Request Body Validation
//! Validate that controllers send correct request bodies to cloud APIs, especially for
//! complex configurations with environment variables, dependencies, and custom settings.
//!
//! ```ignore
//! #[tokio::test]
//! async fn test_create_request_body() {
//!     let mock = server.mock(|when, then| {
//!         when.method(POST)
//!             .path("/api/functions")
//!             .json_body_partial(r#"{"MemorySize": 512}"#)
//!             .json_body_partial(r#"{"Timeout": 120}"#);
//!         then.status(200);
//!     });
//!     
//!     // Execute controller step...
//!     mock.assert();
//! }
//! ```
//!
//! ### Use Helper Functions for Test Setup
//! Create helper functions to reduce boilerplate in test setup, especially for creating
//! contexts, stack states, and platform configurations.
//!
//! ```ignore
//! fn create_test_context(
//!     resource: &MyResource,
//!     dependencies: Vec<(&str, Box<dyn ResourceControllerState>)>,
//!     server_url: &str,
//! ) -> (StackState, ClientConfig, ResourceRegistry) {
//!     // Build stack state with dependencies
//!     // Create platform config with mock endpoints
//!     // Return complete test context
//! }
//! ```
//!
//! ### Test Dependency Interactions
//! Verify that controllers correctly access dependency state and handle cases where
//! dependencies are not ready or have incomplete data.
//!
//! ```ignore
//! #[tokio::test]
//! async fn test_dependency_access() {
//!     // Test with ready dependency
//!     let role_state = create_ready_role_state();
//!     add_dependency_to_stack(&mut stack_state, "role", role_state);
//!     
//!     // Test with missing dependency
//!     let result = controller.get_role_arn(&ctx, &role_ref).await;
//!     assert!(matches!(result, Err(Error::DependencyNotReady { .. })));
//! }
//! ```

use crate::{
    core::ResourceRegistry,
    error::{ErrorData, Result},
};
#[cfg(feature = "aws")]
use alien_aws_clients::AwsClientConfig;
#[cfg(feature = "azure")]
use alien_azure_clients::AzureClientConfig;
use alien_core::ClientConfig;
use alien_core::{
    AwsManagementConfig, AzureManagementConfig, GcpManagementConfig, KubernetesClientConfig,
    Platform, Resource, ResourceDefinition, ResourceOutputs, ResourceRef, ResourceStatus,
    StackState,
};
use alien_error::{AlienError, Context, IntoAlienError};
#[cfg(feature = "gcp")]
use alien_gcp_clients::GcpClientConfig;
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use std::sync::Arc;
use std::time::Duration;
use std::{any::Any, fmt::Debug};

use crate::core::PlatformServiceProvider;

/// Context passed to ResourceController methods.
#[derive(Clone)]
pub struct ResourceControllerContext<'a> {
    /// The desired resource configuration (what we want to achieve).
    /// During create/update: the target configuration to deploy.
    /// During delete: the current configuration being deleted.
    pub desired_config: &'a Resource,
    /// The target platform identifier.
    pub platform: Platform,
    /// The platform-specific configuration (e.g., SDK clients).
    pub client_config: ClientConfig,
    /// The current state of the entire stack.
    pub state: &'a StackState,
    /// The prefix assigned to this stack deployment for resource naming.
    pub resource_prefix: &'a str,
    /// Resource registry for accessing resource type controllers and providers.
    pub registry: &'a Arc<ResourceRegistry>,
    /// The desired stack configuration (target state).
    pub desired_stack: &'a alien_core::Stack,
    /// Provider for platform services - enables dependency injection for testing.
    /// For cloud platforms: provides API clients (S3, Lambda, GCS, etc.)
    /// For local platform: provides service managers (FunctionManager, StorageManager, etc.)
    pub service_provider: &'a Arc<dyn PlatformServiceProvider>,
    /// Deployment configuration containing stack settings, management config, and deployment-time settings.
    pub deployment_config: &'a alien_core::DeploymentConfig,
}

/// Represents the outcome of a single step in the resource state machine.
#[derive(Debug)]
pub struct ResourceControllerStepResult {
    /// An optional suggested duration to wait before executing the next step for this resource.
    pub suggested_delay: Option<Duration>,
}

/// Trait implemented by platform-specific controller structs (e.g., AwsFunctionController).
/// Defines the logic for managing a specific resource type on a specific platform.
// Make Send + Sync + Debug for potential multi-threading and easier debugging
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait ResourceController: Send + Sync + Debug {
    /// Returns the controller type name, used as a discriminator for polymorphic serialization
    fn controller_type(&self) -> &'static str;

    /// Serializes this controller's fields to a JSON Value (without the "type" tag)
    fn to_json_value(&self) -> serde_json::Result<serde_json::Value>;

    /// Attempts to advance the resource's state by one step.
    /// Returns optional suggested delay before the next step for this resource.
    async fn step(
        &mut self,
        context: &ResourceControllerContext,
    ) -> Result<ResourceControllerStepResult>;

    /// Transitions the resource to its specific terminal failure state for the current operation context.
    /// This method should be called on the state *before* the step that failed.
    /// Returns the new failure state as a trait object.
    fn transition_to_failure(&mut self);

    /// Returns the state required to begin the deletion process, if applicable from the current state.
    /// Returns the new state as a trait object.
    fn transition_to_delete_start(&mut self) -> Result<()>;

    /// Transitions the resource state to begin an update process, if possible from the current state.
    /// This is typically called when the executor detects a config change for a resource in a `Running` or `UpdateFailed` status.
    /// It should update the internal state to the appropriate 'UpdateStart' status.
    ///
    /// # Returns
    /// Returns the new state boxed as a trait object if the transition is valid, otherwise None.
    fn transition_to_update(&mut self) -> Result<()>;

    /// Derives the high-level ResourceStatus from the internal state.
    fn get_status(&self) -> ResourceStatus;

    /// Derives the resource outputs (like ARN, URL) from the internal state, if available.
    fn get_outputs(&self) -> Option<ResourceOutputs>;

    /// Resets the internal Stay polling counter to `None`.
    /// Called by `retry_failed()` so a retried resource gets a full fresh polling window.
    fn reset_stay_count(&mut self);

    /// Helper for cloning the boxed trait object.
    fn box_clone(&self) -> Box<dyn ResourceController>;

    /// Helper for downcasting the trait object.
    fn as_any(&self) -> &dyn Any;

    /// Gets binding parameters for this resource when it's used as a dependency.
    /// This method is called when other resources need to access this resource.
    ///
    /// # Important: Pure Function Design
    ///
    /// This is a **pure function** that derives binding parameters solely from the controller's internal state.
    /// All necessary data (infrastructure identifiers, computed environment variables) should be stored in the
    /// controller state during resource creation/updates.
    ///
    /// **Why this approach**:
    /// - Eliminates context confusion (when Function depends on Build, context was for Function, not Build)
    /// - Makes the method predictable and testable
    /// - Ensures all needed data is computed and stored during resource lifecycle operations
    ///
    /// **Data Storage**: Controllers should compute and store all binding-related data during
    /// `create_start` and `update_start` operations, not during `get_binding_params`.
    ///
    /// # Returns
    /// A serde_json::Value containing the binding parameters struct, or None if not ready
    fn get_binding_params(&self) -> Option<serde_json::Value> {
        // Default implementation returns None for resources that don't expose binding parameters
        None
    }
}

impl Clone for Box<dyn ResourceController> {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

/// Serializes a controller to a JSON Value, injecting its type tag.
pub fn serialize_controller(controller: &dyn ResourceController) -> serde_json::Result<serde_json::Value> {
    let mut v = controller.to_json_value()?;
    v.as_object_mut()
        .ok_or_else(|| serde::ser::Error::custom("controller must serialize as object"))?
        .insert("type".into(), serde_json::Value::String(controller.controller_type().into()));
    Ok(v)
}

/// Deserializes a JSON value into a boxed ResourceController by reading the "type" tag
/// and dispatching to the correct concrete type.
pub fn deserialize_controller(value: serde_json::Value) -> std::result::Result<Box<dyn ResourceController>, serde_json::Error> {
    use serde::de::Error as _;
    let mut value = value;
    let type_tag = value.get("type").and_then(|v| v.as_str())
        .ok_or_else(|| serde_json::Error::custom("missing 'type' field in controller state"))?
        .to_string();

    // Remove the "type" tag before passing to concrete deserializer
    if let Some(obj) = value.as_object_mut() {
        obj.remove("type");
    }

    deserialize_controller_by_tag(&type_tag, value)
}

fn deserialize_controller_by_tag(type_tag: &str, value: serde_json::Value) -> std::result::Result<Box<dyn ResourceController>, serde_json::Error> {
    use serde::de::Error as _;

    // This macro reduces boilerplate for each controller type
    macro_rules! deser {
        ($t:ty) => {
            Ok(Box::new(serde_json::from_value::<$t>(value)?))
        };
    }

    match type_tag {
        // Function controllers
        #[cfg(feature = "aws")]
        "AwsFunctionController" => deser!(crate::function::AwsFunctionController),
        #[cfg(feature = "gcp")]
        "GcpFunctionController" => deser!(crate::function::GcpFunctionController),
        #[cfg(feature = "azure")]
        "AzureFunctionController" => deser!(crate::function::AzureFunctionController),
        #[cfg(feature = "kubernetes")]
        "KubernetesFunctionController" => deser!(crate::function::KubernetesFunctionController),
        #[cfg(feature = "local")]
        "LocalFunctionController" => deser!(crate::function::LocalFunctionController),
        #[cfg(feature = "test")]
        "TestFunctionController" => deser!(crate::function::TestFunctionController),

        // Container controllers
        #[cfg(feature = "aws")]
        "AwsContainerController" => deser!(crate::container::AwsContainerController),
        #[cfg(feature = "gcp")]
        "GcpContainerController" => deser!(crate::container::GcpContainerController),
        #[cfg(feature = "azure")]
        "AzureContainerController" => deser!(crate::container::AzureContainerController),
        #[cfg(feature = "kubernetes")]
        "KubernetesContainerController" => deser!(crate::container::KubernetesContainerController),
        #[cfg(feature = "local")]
        "LocalContainerController" => deser!(crate::container::LocalContainerController),

        // Container cluster controllers
        #[cfg(feature = "aws")]
        "AwsContainerClusterController" => deser!(crate::container_cluster::AwsContainerClusterController),
        #[cfg(feature = "gcp")]
        "GcpContainerClusterController" => deser!(crate::container_cluster::GcpContainerClusterController),
        #[cfg(feature = "azure")]
        "AzureContainerClusterController" => deser!(crate::container_cluster::AzureContainerClusterController),
        #[cfg(feature = "local")]
        "LocalContainerClusterController" => deser!(crate::container_cluster::LocalContainerClusterController),

        // Storage controllers
        #[cfg(feature = "aws")]
        "AwsStorageController" => deser!(crate::storage::AwsStorageController),
        #[cfg(feature = "gcp")]
        "GcpStorageController" => deser!(crate::storage::GcpStorageController),
        #[cfg(feature = "azure")]
        "AzureStorageController" => deser!(crate::storage::AzureStorageController),
        #[cfg(feature = "local")]
        "LocalStorageController" => deser!(crate::storage::LocalStorageController),
        #[cfg(feature = "test")]
        "TestStorageController" => deser!(crate::storage::TestStorageController),

        // Vault controllers
        #[cfg(feature = "aws")]
        "AwsVaultController" => deser!(crate::vault::AwsVaultController),
        #[cfg(feature = "gcp")]
        "GcpVaultController" => deser!(crate::vault::GcpVaultController),
        #[cfg(feature = "azure")]
        "AzureVaultController" => deser!(crate::vault::AzureVaultController),
        #[cfg(feature = "kubernetes")]
        "KubernetesVaultController" => deser!(crate::vault::KubernetesVaultController),
        #[cfg(feature = "local")]
        "LocalVaultController" => deser!(crate::vault::LocalVaultController),
        #[cfg(feature = "test")]
        "TestVaultController" => deser!(crate::vault::TestVaultController),

        // KV controllers
        #[cfg(feature = "aws")]
        "AwsKvController" => deser!(crate::kv::AwsKvController),
        #[cfg(feature = "gcp")]
        "GcpKvController" => deser!(crate::kv::GcpKvController),
        #[cfg(feature = "azure")]
        "AzureKvController" => deser!(crate::kv::AzureKvController),
        #[cfg(feature = "local")]
        "LocalKvController" => deser!(crate::kv::LocalKvController),

        // Queue controllers
        #[cfg(feature = "aws")]
        "AwsQueueController" => deser!(crate::queue::aws::AwsQueueController),
        #[cfg(feature = "gcp")]
        "GcpQueueController" => deser!(crate::queue::gcp::GcpQueueController),
        #[cfg(feature = "azure")]
        "AzureQueueController" => deser!(crate::queue::azure::AzureQueueController),

        // Network controllers
        #[cfg(feature = "aws")]
        "AwsNetworkController" => deser!(crate::network::AwsNetworkController),
        #[cfg(feature = "gcp")]
        "GcpNetworkController" => deser!(crate::network::GcpNetworkController),
        #[cfg(feature = "azure")]
        "AzureNetworkController" => deser!(crate::network::AzureNetworkController),

        // Build controllers
        #[cfg(feature = "aws")]
        "AwsBuildController" => deser!(crate::build::AwsBuildController),
        #[cfg(feature = "gcp")]
        "GcpBuildController" => deser!(crate::build::GcpBuildController),
        #[cfg(feature = "azure")]
        "AzureBuildController" => deser!(crate::build::AzureBuildController),
        #[cfg(feature = "kubernetes")]
        "KubernetesBuildController" => deser!(crate::build::KubernetesBuildController),

        // Service account controllers
        #[cfg(feature = "aws")]
        "AwsServiceAccountController" => deser!(crate::service_account::AwsServiceAccountController),
        #[cfg(feature = "gcp")]
        "GcpServiceAccountController" => deser!(crate::service_account::GcpServiceAccountController),
        #[cfg(feature = "azure")]
        "AzureServiceAccountController" => deser!(crate::service_account::AzureServiceAccountController),
        #[cfg(feature = "local")]
        "LocalServiceAccountController" => deser!(crate::service_account::LocalServiceAccountController),
        #[cfg(feature = "test")]
        "TestServiceAccountController" => deser!(crate::service_account::TestServiceAccountController),

        // Artifact registry controllers
        #[cfg(feature = "aws")]
        "AwsArtifactRegistryController" => deser!(crate::artifact_registry::AwsArtifactRegistryController),
        #[cfg(feature = "gcp")]
        "GcpArtifactRegistryController" => deser!(crate::artifact_registry::GcpArtifactRegistryController),
        #[cfg(feature = "azure")]
        "AzureArtifactRegistryController" => deser!(crate::artifact_registry::AzureArtifactRegistryController),
        #[cfg(feature = "local")]
        "LocalArtifactRegistryController" => deser!(crate::artifact_registry::LocalArtifactRegistryController),

        // Remote stack management controllers
        #[cfg(feature = "aws")]
        "AwsRemoteStackManagementController" => deser!(crate::remote_stack_management::AwsRemoteStackManagementController),
        #[cfg(feature = "gcp")]
        "GcpRemoteStackManagementController" => deser!(crate::remote_stack_management::GcpRemoteStackManagementController),
        #[cfg(feature = "azure")]
        "AzureRemoteStackManagementController" => deser!(crate::remote_stack_management::AzureRemoteStackManagementController),
        #[cfg(feature = "test")]
        "TestRemoteStackManagementController" => deser!(crate::remote_stack_management::TestRemoteStackManagementController),

        // Service activation controllers
        #[cfg(feature = "gcp")]
        "GcpServiceActivationController" => deser!(crate::service_activation::GcpServiceActivationController),
        #[cfg(feature = "azure")]
        "AzureServiceActivationController" => deser!(crate::service_activation::AzureServiceActivationController),

        // Azure infra requirement controllers
        #[cfg(feature = "azure")]
        "AzureResourceGroupController" => deser!(crate::infra_requirements::AzureResourceGroupController),
        #[cfg(feature = "azure")]
        "AzureStorageAccountController" => deser!(crate::infra_requirements::AzureStorageAccountController),
        #[cfg(feature = "azure")]
        "AzureContainerAppsEnvironmentController" => deser!(crate::infra_requirements::AzureContainerAppsEnvironmentController),
        #[cfg(feature = "azure")]
        "AzureServiceBusNamespaceController" => deser!(crate::infra_requirements::AzureServiceBusNamespaceController),

        other => Err(serde_json::Error::custom(format!("unknown controller type: {}", other))),
    }
}

// Add the new helper method implementation
impl ResourceControllerContext<'_> {
    /// Requires a dependency resource to be ready and returns its internal state.
    ///
    /// This method looks up a dependency resource in the stack state and attempts to
    /// deserialize its internal state to the specified type `T`. Unlike optional dependency
    /// access, this method treats missing or unready dependencies as errors.
    ///
    /// # Arguments
    /// * `dependency_ref` - Reference to the dependency resource
    ///
    /// # Returns
    /// * `Ok(state)` - The dependency's internal state if found and ready
    /// * `Err(DependencyNotFound)` - If the dependency is not defined in the stack
    /// * `Err(DependencyNotReady)` - If the dependency exists but is not ready
    /// * `Err(ControllerStateTypeMismatch)` - If the dependency state can't be deserialized to type T
    pub fn require_dependency<T: ResourceController + DeserializeOwned + 'static>(
        &self,
        dependency_ref: &ResourceRef,
    ) -> Result<T> {
        let dependency_id = dependency_ref.id();
        let dependent_id = self.desired_config.id();

        let dep_stack_state = self.state.resources.get(dependency_id).ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotFound {
                resource_id: dependent_id.to_string(),
                dependency_id: dependency_id.to_string(),
            })
        })?;

        let json_value = dep_stack_state.internal_state.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: dependent_id.to_string(),
                dependency_id: dependency_id.to_string(),
            })
        })?;

        serde_json::from_value::<T>(json_value.clone())
            .into_alien_error()
            .context(ErrorData::ControllerStateTypeMismatch {
                expected: std::any::type_name::<T>().to_string(),
                resource_id: dependency_id.to_string(),
            })
    }

    #[cfg(feature = "aws")]
    pub fn get_aws_config(&self) -> crate::Result<&AwsClientConfig> {
        match self.client_config {
            ClientConfig::Aws(ref config) => Ok(config.as_ref()),
            _ => Err(AlienError::new(ErrorData::ClientConfigMismatch {
                required_platform: alien_core::Platform::Aws,
                found_platform: self.client_config.platform(),
            })),
        }
    }

    #[cfg(feature = "gcp")]
    pub fn get_gcp_config(&self) -> crate::Result<&GcpClientConfig> {
        match self.client_config {
            ClientConfig::Gcp(ref config) => Ok(config.as_ref()),
            _ => Err(AlienError::new(ErrorData::ClientConfigMismatch {
                required_platform: alien_core::Platform::Gcp,
                found_platform: self.client_config.platform(),
            })),
        }
    }

    #[cfg(feature = "azure")]
    pub fn get_azure_config(&self) -> crate::Result<&AzureClientConfig> {
        match self.client_config {
            ClientConfig::Azure(ref config) => Ok(config.as_ref()),
            _ => Err(AlienError::new(ErrorData::ClientConfigMismatch {
                required_platform: alien_core::Platform::Azure,
                found_platform: self.client_config.platform(),
            })),
        }
    }

    pub fn get_kubernetes_config(&self) -> crate::Result<&KubernetesClientConfig> {
        match self.client_config {
            ClientConfig::Kubernetes(ref config) => Ok(config.as_ref()),
            _ => Err(AlienError::new(ErrorData::ClientConfigMismatch {
                required_platform: alien_core::Platform::Kubernetes,
                found_platform: self.client_config.platform(),
            })),
        }
    }

    /// Gets the AWS management configuration from context
    pub fn get_aws_management_config(&self) -> crate::Result<Option<AwsManagementConfig>> {
        match self.deployment_config.management_config.as_ref() {
            Some(alien_core::ManagementConfig::Aws(config)) => Ok(Some(config.clone())),
            Some(_) => Err(AlienError::new(ErrorData::InfrastructureError {
                message: "Management configuration is not for AWS platform".to_string(),
                operation: Some("get_aws_management_config".to_string()),
                resource_id: None,
            })),
            None => Ok(None),
        }
    }

    /// Gets the GCP management configuration from context
    pub fn get_gcp_management_config(&self) -> crate::Result<Option<GcpManagementConfig>> {
        match self.deployment_config.management_config.as_ref() {
            Some(alien_core::ManagementConfig::Gcp(config)) => Ok(Some(config.clone())),
            Some(_) => Err(AlienError::new(ErrorData::InfrastructureError {
                message: "Management configuration is not for GCP platform".to_string(),
                operation: Some("get_gcp_management_config".to_string()),
                resource_id: None,
            })),
            None => Ok(None),
        }
    }

    /// Gets the Azure management configuration from context
    pub fn get_azure_management_config(&self) -> crate::Result<Option<AzureManagementConfig>> {
        match self.deployment_config.management_config.as_ref() {
            Some(alien_core::ManagementConfig::Azure(config)) => Ok(Some(config.clone())),
            Some(_) => Err(AlienError::new(ErrorData::InfrastructureError {
                message: "Management configuration is not for Azure platform".to_string(),
                operation: Some("get_azure_management_config".to_string()),
                resource_id: None,
            })),
            None => Ok(None),
        }
    }

    /// Gets the previous resource configuration from the stack state during updates with type-safe downcasting.
    /// This is the configuration that was active before the current update started.
    /// Returns an error if this is not an update operation, if no previous config exists, or if the downcast fails.
    pub fn previous_resource_config<T>(&self) -> Result<&T>
    where
        T: ResourceDefinition + 'static,
    {
        let resource_state = self
            .state
            .resources
            .get(self.desired_config.id())
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceNotFound {
                    resource_id: self.desired_config.id().to_string(),
                    available_resources: self.state.resources.keys().cloned().collect(),
                })
            })?;

        let previous_config = resource_state.previous_config.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "No previous configuration available".to_string(),
                resource_id: Some(self.desired_config.id().to_string()),
            })
        })?;

        previous_config.downcast_ref::<T>().ok_or_else(|| {
            AlienError::new(ErrorData::ControllerResourceTypeMismatch {
                expected: alien_core::ResourceType::from_static(std::any::type_name::<T>()),
                actual: previous_config.resource_type(),
                resource_id: self.desired_config.id().to_string(),
            })
        })
    }

    /// Gets the desired resource configuration with type-safe downcasting.
    /// This is the target configuration we want to achieve.
    /// Returns an error if the downcast fails.
    pub fn desired_resource_config<T>(&self) -> Result<&T>
    where
        T: ResourceDefinition + 'static,
    {
        self.desired_config.downcast_ref::<T>().ok_or_else(|| {
            AlienError::new(ErrorData::ControllerResourceTypeMismatch {
                expected: alien_core::ResourceType::from_static(std::any::type_name::<T>()),
                actual: self.desired_config.resource_type(),
                resource_id: self.desired_config.id().to_string(),
            })
        })
    }

    /// Gets the last error from the stack state if the resource is in a failed state.
    pub fn get_resource_error(&self) -> Option<&AlienError> {
        self.state
            .resources
            .get(self.desired_config.id())
            .and_then(|resource_state| resource_state.error.as_ref())
    }
}
