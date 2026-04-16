//!
//! Defines structures for managing the runtime state of deployed resources.
//! This includes platform-specific internal states, overall stack status,
//! resource outputs, error tracking, and pending user actions.

use crate::{
    Platform, Resource, ResourceLifecycle, ResourceOutputs, ResourceOutputsDefinition, ResourceRef,
    ResourceStatus, ResourceType,
};

use alien_error::AlienError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;
use uuid::Uuid;

use crate::{ErrorData, Result};

/// Represents the overall status of a stack based on its resource states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum StackStatus {
    /// Stack is initializing with no resources yet created
    Pending,
    /// Stack has resources that are currently being provisioned, updated, or deleted
    InProgress,
    /// All resources are successfully running and the stack is operational
    Running,
    /// All resources have been successfully deleted and the stack is removed
    Deleted,
    /// One or more resources have failed during provisioning, updating, or deleting
    Failure,
}

/// Represents the collective state of all resources in a stack, including platform and pending actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StackState {
    /// The target platform for this stack state.
    pub platform: Platform,
    /// The state of individual resources, keyed by resource ID.
    pub resources: HashMap<String, StackResourceState>,
    /// A prefix used for resource naming to ensure uniqueness across deployments.
    pub resource_prefix: String,
}

impl StackState {
    /// Creates a new, empty StackState for a given platform with a generated resource prefix.
    pub fn new(platform: Platform) -> Self {
        // Generate a resource prefix that matches [a-zA-Z][a-zA-Z\d\-]*[a-zA-Z\d] pattern
        // (e.g., "k44e9b72", "m8a3f1d5")
        let letters = "abcdefghijklmnopqrstuvwxyz";
        let first_char = letters
            .chars()
            .nth(Uuid::new_v4().as_bytes()[0] as usize % 26)
            .unwrap();
        let uuid_part = Uuid::new_v4().simple().to_string()[..7].to_string();
        let prefix = format!("{}{}", first_char, uuid_part);

        StackState {
            platform,
            resources: HashMap::new(),
            resource_prefix: prefix,
        }
    }

    /// Returns a reference to the state of a specific resource if it exists.
    pub fn resource(&self, id: &str) -> Option<&StackResourceState> {
        self.resources.get(id)
    }

    /// Computes the stack status from the current resource statuses.
    /// This is the main function that implements the logic from the TypeScript version.
    pub fn compute_stack_status(&self) -> Result<StackStatus> {
        let resource_statuses: Vec<ResourceStatus> = self
            .resources
            .values()
            .map(|resource| resource.status)
            .collect();

        Self::compute_stack_status_from_resources(&resource_statuses)
    }

    /// Static method to compute stack status from a list of resource statuses.
    /// This method contains the core logic and can be tested independently.
    pub fn compute_stack_status_from_resources(
        resource_statuses: &[ResourceStatus],
    ) -> Result<StackStatus> {
        // If there are no resources, it's pending (initializing a completely new stack state)
        if resource_statuses.is_empty() {
            return Ok(StackStatus::Pending);
        }

        // Check for any failure states
        if resource_statuses.iter().any(|status| {
            matches!(
                status,
                ResourceStatus::ProvisionFailed
                    | ResourceStatus::UpdateFailed
                    | ResourceStatus::DeleteFailed
                    | ResourceStatus::RefreshFailed
            )
        }) {
            return Ok(StackStatus::Failure);
        }

        // Check for any in-progress states
        if resource_statuses.iter().any(|status| {
            matches!(
                status,
                ResourceStatus::Pending
                    | ResourceStatus::Provisioning
                    | ResourceStatus::Updating
                    | ResourceStatus::Deleting
            )
        }) {
            return Ok(StackStatus::InProgress);
        }

        // Check for terminal states
        if resource_statuses
            .iter()
            .all(|status| matches!(status, ResourceStatus::Running))
        {
            return Ok(StackStatus::Running);
        }

        if resource_statuses
            .iter()
            .all(|status| matches!(status, ResourceStatus::Deleted))
        {
            return Ok(StackStatus::Deleted);
        }

        // Check for mixed Running + Deleted (deletion in progress)
        // This happens during dependency-ordered deletion when some resources are deleted
        // but others are still running while waiting for dependencies to clear
        let has_running = resource_statuses
            .iter()
            .any(|status| matches!(status, ResourceStatus::Running));
        let has_deleted = resource_statuses
            .iter()
            .any(|status| matches!(status, ResourceStatus::Deleted));
        let only_running_or_deleted = resource_statuses
            .iter()
            .all(|status| matches!(status, ResourceStatus::Running | ResourceStatus::Deleted));

        if has_running && has_deleted && only_running_or_deleted {
            return Ok(StackStatus::InProgress);
        }

        // Mixed terminal states or unexpected combinations
        let status_strings: Vec<String> = resource_statuses
            .iter()
            .map(|status| format!("{:?}", status).to_lowercase().replace('_', "-"))
            .collect();

        Err(AlienError::new(
            ErrorData::UnexpectedResourceStatusCombination {
                resource_statuses: status_strings,
                operation: "stack status computation".to_string(),
            },
        ))
    }

    /// Retrieves and downcasts the outputs of a resource from the stack state.
    ///
    /// # Arguments
    /// * `resource_id` - The ID of the resource to get outputs for
    ///
    /// # Returns
    /// * `Ok(T)` - The downcasted outputs if successful
    /// * `Err(Error)` - If the resource doesn't exist, has no outputs, or the outputs are not of the expected type
    ///
    /// # Example
    /// ```rust,ignore
    /// use alien_core::{StackState, Platform, FunctionOutputs};
    ///
    /// let stack_state = StackState::new(Platform::Aws);
    ///
    /// // Get function outputs with error handling
    /// let function_outputs = stack_state.get_resource_outputs::<FunctionOutputs>("my-function")?;
    /// if let Some(url) = &function_outputs.url {
    ///     println!("Function URL: {}", url);
    /// }
    /// ```
    pub fn get_resource_outputs<T: ResourceOutputsDefinition + 'static>(
        &self,
        resource_id: &str,
    ) -> Result<&T> {
        let resource_state = self.resources.get(resource_id).ok_or_else(|| {
            AlienError::new(ErrorData::ResourceNotFound {
                resource_id: resource_id.to_string(),
                available_resources: self.resources.keys().cloned().collect(),
            })
        })?;

        let outputs = resource_state.outputs.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceHasNoOutputs {
                resource_id: resource_id.to_string(),
            })
        })?;

        outputs.downcast_ref::<T>().ok_or_else(|| {
            AlienError::new(ErrorData::UnexpectedResourceType {
                resource_id: resource_id.to_string(),
                expected: ResourceType::from_static(std::any::type_name::<T>()),
                actual: resource_state.resource_type.clone().into(),
            })
        })
    }
}

/// Represents the state of a single resource within the stack for a specific platform.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StackResourceState {
    /// The high-level type of the resource (e.g., Function::RESOURCE_TYPE, Storage::RESOURCE_TYPE).
    #[serde(rename = "type")]
    pub resource_type: String,

    /// The platform-specific resource controller that manages this resource's lifecycle.
    /// This is None when the resource status is Pending.
    /// Stored as JSON to make the struct serializable and movable to alien-core.
    #[serde(rename = "_internal", skip_serializing_if = "Option::is_none")]
    pub internal_state: Option<serde_json::Value>,

    /// High-level status derived from the internal state.
    pub status: ResourceStatus,

    /// Outputs generated by the resource (e.g., ARN, URL, Bucket Name).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs: Option<ResourceOutputs>,

    /// The current resource configuration.
    pub config: Resource,

    /// The previous resource configuration during updates.
    /// This is set when an update is initiated and cleared when the update completes or fails.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_config: Option<Resource>,

    /// Tracks consecutive retry attempts for the current state transition.
    #[serde(default, skip_serializing_if = "is_zero")]
    #[builder(default)]
    pub retry_attempt: u32,

    /// Stores the last error encountered during a failed step transition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AlienError>,

    /// True if the resource was provisioned by an external system (e.g., CloudFormation).
    /// Defaults to false, indicating dynamic provisioning by the executor.
    #[serde(default, skip_serializing_if = "is_false")]
    #[builder(default)]
    pub is_externally_provisioned: bool,

    /// The lifecycle of the resource (Frozen or Live).
    /// Defaults to Live if not specified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<ResourceLifecycle>,

    /// Complete list of dependencies for this resource, including infrastructure dependencies.
    /// This preserves the full dependency information from the stack definition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[builder(default = vec![])]
    pub dependencies: Vec<ResourceRef>,

    /// Stores the controller state that failed, used for manual retry operations.
    /// This allows resuming from the exact point where the failure occurred.
    /// Stored as JSON to make the struct serializable and movable to alien-core.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_failed_state: Option<serde_json::Value>,

    /// Binding parameters for remote access.
    /// Only populated when the resource has `remote_access: true` in its ResourceEntry.
    /// This is the JSON serialization of the binding configuration (e.g., StorageBinding, VaultBinding).
    /// Populated by controllers during provisioning using get_binding_params().
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_binding_params: Option<serde_json::Value>,
}

impl StackResourceState {
    /// Creates a new pending StackResourceState for a resource that's about to be created
    pub fn new_pending(
        resource_type: String,
        config: Resource,
        lifecycle: Option<ResourceLifecycle>,
        dependencies: Vec<ResourceRef>,
    ) -> Self {
        Self {
            resource_type,
            internal_state: None,
            status: ResourceStatus::Pending,
            outputs: None,
            config,
            previous_config: None,
            retry_attempt: 0,
            error: None,
            is_externally_provisioned: false,
            lifecycle,
            dependencies,
            last_failed_state: None,
            remote_binding_params: None,
        }
    }

    /// Creates a new StackResourceState based on this one, with only the specified fields modified
    pub fn with_updates<F>(&self, update_fn: F) -> Self
    where
        F: FnOnce(&mut Self),
    {
        let mut new_state = self.clone();
        update_fn(&mut new_state);
        new_state
    }

    /// Creates a new StackResourceState with the status changed to a failure state and error set
    pub fn with_failure(&self, status: ResourceStatus, error: AlienError) -> Self {
        self.with_updates(|state| {
            state.status = status;
            state.error = Some(error);
            state.retry_attempt = 0;
        })
    }
}

// Helper function for skip_serializing_if on retry_attempt
fn is_zero(num: &u32) -> bool {
    *num == 0
}

// Helper function for skip_serializing_if on is_externally_provisioned
fn is_false(b: &bool) -> bool {
    !*b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Function, FunctionCode, FunctionOutputs, ResourceType, Storage, StorageOutputs};

    #[test]
    fn test_get_resource_outputs_success() {
        let mut stack_state = StackState::new(Platform::Aws);

        // Create a function with outputs
        let function_outputs = FunctionOutputs {
            function_name: "test-function".to_string(),
            url: Some("https://example.lambda-url.us-east-1.on.aws/".to_string()),
            identifier: Some(
                "arn:aws:lambda:us-east-1:123456789012:function:test-function".to_string(),
            ),
            load_balancer_endpoint: None,
            commands_push_target: None,
        };

        let test_function = Function::new("test-function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test-profile".to_string())
            .build();

        let resource_state = StackResourceState::new_pending(
            "function".to_string(),
            Resource::new(test_function),
            None,
            Vec::new(),
        )
        .with_updates(|state| {
            state.status = ResourceStatus::Running;
            state.outputs = Some(ResourceOutputs::new(function_outputs.clone()));
        });

        stack_state
            .resources
            .insert("test-function".to_string(), resource_state);

        // Test successful retrieval
        let retrieved_outputs = stack_state
            .get_resource_outputs::<FunctionOutputs>("test-function")
            .unwrap();
        assert_eq!(retrieved_outputs.function_name, "test-function");
        assert_eq!(
            retrieved_outputs.url,
            Some("https://example.lambda-url.us-east-1.on.aws/".to_string())
        );
        assert_eq!(
            retrieved_outputs.identifier,
            Some("arn:aws:lambda:us-east-1:123456789012:function:test-function".to_string())
        );
    }

    #[test]
    fn test_get_resource_outputs_resource_not_found() {
        let stack_state = StackState::new(Platform::Aws);

        // Test resource not found
        let result = stack_state.get_resource_outputs::<FunctionOutputs>("nonexistent-function");
        assert!(result.is_err());
        let error = result.unwrap_err();

        // Assert on the specific error variant
        let error_data = &error.error;
        if let Some(ErrorData::ResourceNotFound {
            resource_id,
            available_resources,
        }) = error_data
        {
            assert_eq!(resource_id, "nonexistent-function");
            assert_eq!(available_resources, &Vec::<String>::new());
        } else {
            panic!("Expected ResourceNotFound error, got: {:?}", error_data);
        }

        // Also check the string representation
        let error_message = error.to_string();
        assert!(error_message.contains("Resource 'nonexistent-function' not found in stack state"));
        assert!(error_message.contains("Available resources: []"));
    }

    #[test]
    fn test_get_resource_outputs_no_outputs() {
        let mut stack_state = StackState::new(Platform::Aws);

        // Create a resource without outputs
        let test_function_2 = Function::new("test-function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test-profile".to_string())
            .build();

        let resource_state = StackResourceState::new_pending(
            "function".to_string(),
            Resource::new(test_function_2),
            None,
            Vec::new(),
        )
        .with_updates(|state| {
            state.status = ResourceStatus::Provisioning;
        });

        stack_state
            .resources
            .insert("test-function".to_string(), resource_state);

        // Test no outputs
        let result = stack_state.get_resource_outputs::<FunctionOutputs>("test-function");
        assert!(result.is_err());
        let error = result.unwrap_err();

        // Assert on the specific error variant
        let error_data = &error.error;
        if let Some(ErrorData::ResourceHasNoOutputs { resource_id, .. }) = error_data {
            assert_eq!(resource_id, "test-function");
        } else {
            panic!("Expected ResourceHasNoOutputs error, got: {:?}", error_data);
        }

        // Also check the string representation
        let error_message = error.to_string();
        assert!(error_message.contains("Resource 'test-function' has no outputs"));
    }

    #[test]
    fn test_get_resource_outputs_wrong_type() {
        let mut stack_state = StackState::new(Platform::Aws);

        // Create a storage resource with storage outputs
        let storage_outputs = StorageOutputs {
            bucket_name: "test-bucket".to_string(),
        };

        let test_storage = Storage::new("test-storage".to_string()).build();

        let resource_state = StackResourceState::new_pending(
            "storage".to_string(),
            Resource::new(test_storage),
            None,
            Vec::new(),
        )
        .with_updates(|state| {
            state.status = ResourceStatus::Running;
            state.outputs = Some(ResourceOutputs::new(storage_outputs));
        });

        stack_state
            .resources
            .insert("test-storage".to_string(), resource_state);

        // Try to get function outputs from a storage resource
        let result = stack_state.get_resource_outputs::<FunctionOutputs>("test-storage");
        assert!(result.is_err());
        let error = result.unwrap_err();

        // Assert on the specific error variant
        let error_data = &error.error;
        if let Some(ErrorData::UnexpectedResourceType {
            resource_id,
            expected,
            actual,
        }) = error_data
        {
            assert_eq!(resource_id, "test-storage");
            assert!(
                expected.0.contains("FunctionOutputs"),
                "expected should reference FunctionOutputs, got: {}",
                expected.0
            );
            assert_eq!(*actual, ResourceType::from_static("storage"));
        } else {
            panic!(
                "Expected UnexpectedResourceType error, got: {:?}",
                error_data
            );
        }
    }

    #[test]
    fn test_get_resource_outputs_usage_example() {
        let mut stack_state = StackState::new(Platform::Aws);

        // Create a function with outputs (similar to your original sketch)
        let function_outputs = FunctionOutputs {
            function_name: "test-alien-function".to_string(),
            url: Some("https://test.lambda-url.us-east-1.on.aws/".to_string()),
            identifier: Some(
                "arn:aws:lambda:us-east-1:123456789012:function:test-alien-function".to_string(),
            ),
            load_balancer_endpoint: None,
            commands_push_target: None,
        };

        let test_alien_function = Function::new("test-alien-function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test-profile".to_string())
            .build();

        let resource_state = StackResourceState {
            resource_type: "function".to_string(),
            internal_state: None,
            status: ResourceStatus::Running,
            outputs: Some(ResourceOutputs::new(function_outputs)),
            config: Resource::new(test_alien_function),
            previous_config: None,
            retry_attempt: 0,
            error: None,
            is_externally_provisioned: false,
            lifecycle: None,
            dependencies: Vec::new(),
            last_failed_state: None,
            remote_binding_params: None,
        };

        stack_state
            .resources
            .insert("test-alien-function".to_string(), resource_state);

        // Test the usage pattern from your original sketch
        let function_outputs = stack_state
            .get_resource_outputs::<FunctionOutputs>("test-alien-function")
            .unwrap();

        let function_url = function_outputs
            .url
            .as_ref()
            .ok_or_else(|| "Function URL not found in stack state")
            .unwrap();

        assert_eq!(function_url, "https://test.lambda-url.us-east-1.on.aws/");
    }

    // Tests for StackStatus computation - ported from TypeScript
    #[cfg(test)]
    mod stack_status_tests {
        use super::*;

        #[test]
        fn test_compute_stack_status_empty_resources() {
            let result = StackState::compute_stack_status_from_resources(&[]).unwrap();
            assert_eq!(result, StackStatus::Pending);
        }

        #[test]
        fn test_compute_stack_status_single_pending() {
            let result =
                StackState::compute_stack_status_from_resources(&[ResourceStatus::Pending])
                    .unwrap();
            assert_eq!(result, StackStatus::InProgress);
        }

        #[test]
        fn test_compute_stack_status_single_provisioning() {
            let result =
                StackState::compute_stack_status_from_resources(&[ResourceStatus::Provisioning])
                    .unwrap();
            assert_eq!(result, StackStatus::InProgress);
        }

        #[test]
        fn test_compute_stack_status_single_updating() {
            let result =
                StackState::compute_stack_status_from_resources(&[ResourceStatus::Updating])
                    .unwrap();
            assert_eq!(result, StackStatus::InProgress);
        }

        #[test]
        fn test_compute_stack_status_single_deleting() {
            let result =
                StackState::compute_stack_status_from_resources(&[ResourceStatus::Deleting])
                    .unwrap();
            assert_eq!(result, StackStatus::InProgress);
        }

        #[test]
        fn test_compute_stack_status_single_provision_failed() {
            let result =
                StackState::compute_stack_status_from_resources(&[ResourceStatus::ProvisionFailed])
                    .unwrap();
            assert_eq!(result, StackStatus::Failure);
        }

        #[test]
        fn test_compute_stack_status_single_update_failed() {
            let result =
                StackState::compute_stack_status_from_resources(&[ResourceStatus::UpdateFailed])
                    .unwrap();
            assert_eq!(result, StackStatus::Failure);
        }

        #[test]
        fn test_compute_stack_status_single_delete_failed() {
            let result =
                StackState::compute_stack_status_from_resources(&[ResourceStatus::DeleteFailed])
                    .unwrap();
            assert_eq!(result, StackStatus::Failure);
        }

        #[test]
        fn test_compute_stack_status_single_refresh_failed() {
            let result =
                StackState::compute_stack_status_from_resources(&[ResourceStatus::RefreshFailed])
                    .unwrap();
            assert_eq!(result, StackStatus::Failure);
        }

        #[test]
        fn test_compute_stack_status_single_running() {
            let result =
                StackState::compute_stack_status_from_resources(&[ResourceStatus::Running])
                    .unwrap();
            assert_eq!(result, StackStatus::Running);
        }

        #[test]
        fn test_compute_stack_status_single_deleted() {
            let result =
                StackState::compute_stack_status_from_resources(&[ResourceStatus::Deleted])
                    .unwrap();
            assert_eq!(result, StackStatus::Deleted);
        }

        #[test]
        fn test_compute_stack_status_all_running() {
            let statuses = vec![
                ResourceStatus::Running,
                ResourceStatus::Running,
                ResourceStatus::Running,
            ];
            let result = StackState::compute_stack_status_from_resources(&statuses).unwrap();
            assert_eq!(result, StackStatus::Running);
        }

        #[test]
        fn test_compute_stack_status_all_deleted() {
            let statuses = vec![
                ResourceStatus::Deleted,
                ResourceStatus::Deleted,
                ResourceStatus::Deleted,
            ];
            let result = StackState::compute_stack_status_from_resources(&statuses).unwrap();
            assert_eq!(result, StackStatus::Deleted);
        }

        #[test]
        fn test_compute_stack_status_all_pending() {
            let statuses = vec![
                ResourceStatus::Pending,
                ResourceStatus::Pending,
                ResourceStatus::Pending,
            ];
            let result = StackState::compute_stack_status_from_resources(&statuses).unwrap();
            assert_eq!(result, StackStatus::InProgress);
        }

        #[test]
        fn test_compute_stack_status_all_provisioning() {
            let statuses = vec![
                ResourceStatus::Provisioning,
                ResourceStatus::Provisioning,
                ResourceStatus::Provisioning,
            ];
            let result = StackState::compute_stack_status_from_resources(&statuses).unwrap();
            assert_eq!(result, StackStatus::InProgress);
        }

        #[test]
        fn test_compute_stack_status_all_provision_failed() {
            let statuses = vec![
                ResourceStatus::ProvisionFailed,
                ResourceStatus::ProvisionFailed,
                ResourceStatus::ProvisionFailed,
            ];
            let result = StackState::compute_stack_status_from_resources(&statuses).unwrap();
            assert_eq!(result, StackStatus::Failure);
        }

        #[test]
        fn test_compute_stack_status_mixed_with_failure() {
            let statuses = vec![
                ResourceStatus::Running,
                ResourceStatus::ProvisionFailed,
                ResourceStatus::Updating,
            ];
            let result = StackState::compute_stack_status_from_resources(&statuses).unwrap();
            assert_eq!(result, StackStatus::Failure);
        }

        #[test]
        fn test_compute_stack_status_failure_with_success() {
            let statuses = vec![
                ResourceStatus::Running,
                ResourceStatus::UpdateFailed,
                ResourceStatus::Running,
            ];
            let result = StackState::compute_stack_status_from_resources(&statuses).unwrap();
            assert_eq!(result, StackStatus::Failure);
        }

        #[test]
        fn test_compute_stack_status_failure_with_in_progress() {
            let statuses = vec![
                ResourceStatus::Provisioning,
                ResourceStatus::DeleteFailed,
                ResourceStatus::Deleting,
            ];
            let result = StackState::compute_stack_status_from_resources(&statuses).unwrap();
            assert_eq!(result, StackStatus::Failure);
        }

        #[test]
        fn test_compute_stack_status_any_in_progress() {
            let statuses = vec![
                ResourceStatus::Running,
                ResourceStatus::Updating,
                ResourceStatus::Running,
            ];
            let result = StackState::compute_stack_status_from_resources(&statuses).unwrap();
            assert_eq!(result, StackStatus::InProgress);
        }

        #[test]
        fn test_compute_stack_status_mixed_in_progress_states() {
            let statuses = vec![
                ResourceStatus::Pending,
                ResourceStatus::Provisioning,
                ResourceStatus::Updating,
                ResourceStatus::Deleting,
            ];
            let result = StackState::compute_stack_status_from_resources(&statuses).unwrap();
            assert_eq!(result, StackStatus::InProgress);
        }

        #[test]
        fn test_compute_stack_status_deletion_in_progress() {
            // During deletion, some resources are deleted while others are still running
            // (waiting for dependencies to clear). This should be InProgress, not an error.
            let statuses = vec![ResourceStatus::Running, ResourceStatus::Deleted];
            let result = StackState::compute_stack_status_from_resources(&statuses).unwrap();
            assert_eq!(result, StackStatus::InProgress);
        }

        #[test]
        fn test_compute_stack_status_deletion_in_progress_many_resources() {
            // Test with a more realistic scenario: 9 resources, 2 deleted, 7 still running
            let statuses = vec![
                ResourceStatus::Running,
                ResourceStatus::Running,
                ResourceStatus::Deleted,
                ResourceStatus::Deleted,
                ResourceStatus::Running,
                ResourceStatus::Running,
                ResourceStatus::Running,
                ResourceStatus::Running,
                ResourceStatus::Running,
            ];
            let result = StackState::compute_stack_status_from_resources(&statuses).unwrap();
            assert_eq!(result, StackStatus::InProgress);
        }

        #[test]
        fn test_compute_stack_status_mixed_terminal_with_in_progress() {
            let statuses = vec![
                ResourceStatus::Running,
                ResourceStatus::Deleted,
                ResourceStatus::Pending,
            ];
            let result = StackState::compute_stack_status_from_resources(&statuses).unwrap();
            assert_eq!(result, StackStatus::InProgress);
        }

        #[test]
        fn test_compute_stack_status_large_number_of_resources() {
            let statuses: Vec<ResourceStatus> = (0..100).map(|_| ResourceStatus::Running).collect();
            let result = StackState::compute_stack_status_from_resources(&statuses).unwrap();
            assert_eq!(result, StackStatus::Running);
        }

        #[test]
        fn test_compute_stack_status_single_failure_among_many() {
            let mut statuses: Vec<ResourceStatus> =
                (0..50).map(|_| ResourceStatus::Running).collect();
            statuses.push(ResourceStatus::ProvisionFailed);
            statuses.extend((0..49).map(|_| ResourceStatus::Provisioning));

            let result = StackState::compute_stack_status_from_resources(&statuses).unwrap();
            assert_eq!(result, StackStatus::Failure);
        }

        #[test]
        fn test_compute_stack_status_failure_priority_over_in_progress() {
            let statuses = vec![
                ResourceStatus::ProvisionFailed,
                ResourceStatus::UpdateFailed,
                ResourceStatus::DeleteFailed,
                ResourceStatus::Provisioning,
                ResourceStatus::Updating,
                ResourceStatus::Deleting,
            ];
            let result = StackState::compute_stack_status_from_resources(&statuses).unwrap();
            assert_eq!(result, StackStatus::Failure);
        }

        #[test]
        fn test_compute_stack_status_mixed_success_and_in_progress() {
            let statuses = vec![
                ResourceStatus::Running,
                ResourceStatus::Provisioning,
                ResourceStatus::Running,
            ];
            let result = StackState::compute_stack_status_from_resources(&statuses).unwrap();
            assert_eq!(result, StackStatus::InProgress);
        }

        #[test]
        fn test_stack_state_status_computation() {
            let mut stack_state = StackState::new(Platform::Aws);

            // Initially should be pending (no resources)
            assert_eq!(
                stack_state.compute_stack_status().unwrap(),
                StackStatus::Pending
            );

            // Add a running resource
            let test_function = Function::new("test-function".to_string())
                .code(FunctionCode::Image {
                    image: "test:latest".to_string(),
                })
                .permissions("test-profile".to_string())
                .build();

            let resource_state = StackResourceState::new_pending(
                "function".to_string(),
                Resource::new(test_function),
                None,
                Vec::new(),
            )
            .with_updates(|state| {
                state.status = ResourceStatus::Running;
            });

            stack_state
                .resources
                .insert("test-function".to_string(), resource_state);

            // Compute status
            assert_eq!(
                stack_state.compute_stack_status().unwrap(),
                StackStatus::Running
            );
        }

        /// Regression test: externally provisioned AzureContainerAppsEnvironment
        /// must survive a JSON serialization roundtrip (simulates push model's
        /// state transfer through the manager API and SQLite).
        #[test]
        fn test_external_container_env_survives_json_roundtrip() {
            use crate::resources::AzureContainerAppsEnvironmentOutputs;
            use crate::AzureContainerAppsEnvironment;

            let mut stack_state = StackState::new(Platform::Azure);

            // 1. Create the externally provisioned container env resource
            //    (mirrors what executor.step() does for external bindings)
            let env_config =
                AzureContainerAppsEnvironment::new("default-container-env".to_string()).build();
            let env_outputs = AzureContainerAppsEnvironmentOutputs {
                environment_name: "test-env".to_string(),
                resource_id: "/subscriptions/sub-id/resourceGroups/rg/providers/Microsoft.App/managedEnvironments/test-env".to_string(),
                resource_group_name: "shared-rg".to_string(),
                default_domain: "test-env.azurecontainerapps.io".to_string(),
                static_ip: Some("10.0.0.1".to_string()),
            };

            let env_state = StackResourceState::new_pending(
                AzureContainerAppsEnvironment::RESOURCE_TYPE.to_string(),
                Resource::new(env_config),
                Some(ResourceLifecycle::Frozen),
                Vec::new(),
            )
            .with_updates(|state| {
                state.status = ResourceStatus::Running;
                state.is_externally_provisioned = true;
                state.outputs = Some(ResourceOutputs::new(env_outputs.clone()));
            });

            stack_state
                .resources
                .insert("default-container-env".to_string(), env_state);

            // 2. Also add a function that depends on it (like the real stack)
            let test_function = Function::new("alien-rs-fn".to_string())
                .code(FunctionCode::Image {
                    image: "test:latest".to_string(),
                })
                .permissions("execution".to_string())
                .build();

            let fn_state = StackResourceState::new_pending(
                "function".to_string(),
                Resource::new(test_function),
                Some(ResourceLifecycle::Live),
                vec![crate::ResourceRef::new(
                    AzureContainerAppsEnvironment::RESOURCE_TYPE,
                    "default-container-env",
                )],
            )
            .with_updates(|state| {
                state.status = ResourceStatus::Running;
            });

            stack_state
                .resources
                .insert("alien-rs-fn".to_string(), fn_state);

            // 3. Verify before roundtrip
            assert!(
                stack_state.resources.contains_key("default-container-env"),
                "default-container-env should be in state before roundtrip"
            );
            assert_eq!(stack_state.resources.len(), 2);

            // 4. Simulate the push model roundtrip:
            //    push client: serde_json::to_value(state) → send to manager API
            //    manager API: serde_json::from_value(json) → DeploymentState
            //    manager store: serde_json::to_string(stack_state) → SQLite TEXT
            //    manager read: serde_json::from_str(text) → StackState

            // Step A: to_value (what ManagerApiTransport.reconcile_step does)
            let json_value = serde_json::to_value(&stack_state)
                .expect("StackState serialization to Value should not fail");

            // Step B: from_value (what the manager reconcile handler does)
            let deserialized_from_value: StackState = serde_json::from_value(json_value)
                .expect("StackState deserialization from Value should not fail");

            assert!(
                deserialized_from_value
                    .resources
                    .contains_key("default-container-env"),
                "default-container-env lost during to_value/from_value roundtrip! \
                 Available: {:?}",
                deserialized_from_value.resources.keys().collect::<Vec<_>>()
            );

            // Step C: to_string (what SQLite store does)
            let json_string = serde_json::to_string(&deserialized_from_value)
                .expect("StackState serialization to String should not fail");

            // Step D: from_str (what SQLite store does on read)
            let deserialized_from_str: StackState = serde_json::from_str(&json_string)
                .expect("StackState deserialization from String should not fail");

            assert!(
                deserialized_from_str
                    .resources
                    .contains_key("default-container-env"),
                "default-container-env lost during to_string/from_str roundtrip! \
                 Available: {:?}",
                deserialized_from_str.resources.keys().collect::<Vec<_>>()
            );

            // 5. Verify the outputs survived too
            let outputs = deserialized_from_str
                .get_resource_outputs::<AzureContainerAppsEnvironmentOutputs>(
                    "default-container-env",
                )
                .expect("Should be able to get container env outputs after roundtrip");
            assert_eq!(outputs.environment_name, "test-env");
            assert_eq!(outputs.resource_group_name, "shared-rg");
            assert_eq!(outputs.static_ip, Some("10.0.0.1".to_string()));

            // 6. Verify externally_provisioned flag survived
            let env_resource = deserialized_from_str
                .resources
                .get("default-container-env")
                .unwrap();
            assert!(
                env_resource.is_externally_provisioned,
                "is_externally_provisioned flag should survive roundtrip"
            );
            assert_eq!(env_resource.status, ResourceStatus::Running);
            assert_eq!(env_resource.lifecycle, Some(ResourceLifecycle::Frozen));
        }

        /// Test the full DeploymentState roundtrip (not just StackState),
        /// since the push model serializes the entire DeploymentState.
        #[test]
        fn test_deployment_state_roundtrip_preserves_external_binding() {
            use crate::resources::AzureContainerAppsEnvironmentOutputs;
            use crate::{AzureContainerAppsEnvironment, DeploymentState, DeploymentStatus};

            let mut stack_state = StackState::new(Platform::Azure);

            let env_config =
                AzureContainerAppsEnvironment::new("default-container-env".to_string()).build();
            let env_outputs = AzureContainerAppsEnvironmentOutputs {
                environment_name: "test-env".to_string(),
                resource_id: "/subscriptions/sub/rg/env".to_string(),
                resource_group_name: "shared-rg".to_string(),
                default_domain: "test.io".to_string(),
                static_ip: None,
            };

            let env_state = StackResourceState::new_pending(
                AzureContainerAppsEnvironment::RESOURCE_TYPE.to_string(),
                Resource::new(env_config),
                Some(ResourceLifecycle::Frozen),
                Vec::new(),
            )
            .with_updates(|state| {
                state.status = ResourceStatus::Running;
                state.is_externally_provisioned = true;
                state.outputs = Some(ResourceOutputs::new(env_outputs));
            });

            stack_state
                .resources
                .insert("default-container-env".to_string(), env_state);

            // Build DeploymentState (what final_reconcile serializes)
            let deployment_state = DeploymentState {
                status: DeploymentStatus::Provisioning,
                platform: Platform::Azure,
                current_release: None,
                target_release: None,
                stack_state: Some(stack_state),
                environment_info: None,
                runtime_metadata: None,
                retry_requested: false,
                protocol_version: 1,
            };

            // Roundtrip through serde_json::Value (what the push client does)
            let json_value =
                serde_json::to_value(&deployment_state).expect("DeploymentState to_value failed");

            let deserialized: DeploymentState =
                serde_json::from_value(json_value).expect("DeploymentState from_value failed");

            let ss = deserialized
                .stack_state
                .as_ref()
                .expect("stack_state should be present");

            assert!(
                ss.resources.contains_key("default-container-env"),
                "default-container-env lost in DeploymentState roundtrip! \
                 Available: {:?}",
                ss.resources.keys().collect::<Vec<_>>()
            );
        }
    }
}
