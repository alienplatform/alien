use crate::events::{EventBus, EventHandle, EventState};
use crate::{Result, StackState};
use alien_error::{AlienError, AlienErrorData};
use serde::{Deserialize, Serialize};
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// Progress information for image push operations
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct PushProgress {
    /// Current operation being performed
    pub operation: String,
    /// Number of layers uploaded so far
    pub layers_uploaded: usize,
    /// Total number of layers to upload
    pub total_layers: usize,
    /// Bytes uploaded so far
    pub bytes_uploaded: u64,
    /// Total bytes to upload
    pub total_bytes: u64,
}

/// Represents all possible events in the Alien system
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(tag = "type")]
pub enum AlienEvent {
    // ============================================================================
    // General Events
    // ============================================================================
    /// Loading configuration file
    #[serde(rename_all = "camelCase")]
    LoadingConfiguration,

    /// Operation finished successfully
    #[serde(rename_all = "camelCase")]
    Finished,

    // ============================================================================
    // Alien Build Events
    // ============================================================================
    /// Stack packaging event
    #[serde(rename_all = "camelCase")]
    BuildingStack {
        /// Name of the stack being built
        stack: String,
    },

    /// Running build-time preflight checks and mutations
    #[serde(rename_all = "camelCase")]
    RunningPreflights {
        /// Name of the stack being checked
        stack: String,
        /// Platform being targeted
        platform: String,
    },

    /// Downloading alien runtime event
    #[serde(rename_all = "camelCase")]
    DownloadingAlienRuntime {
        /// Target triple for the runtime
        target_triple: String,
        /// URL being downloaded from
        url: String,
    },

    /// Resource build event (function, container, or worker)
    #[serde(rename_all = "camelCase")]
    BuildingResource {
        /// Name of the resource being built
        resource_name: String,
        /// Type of the resource: "function", "container", "worker"
        resource_type: String,
        /// All resource names sharing this build (for deduped container groups)
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        related_resources: Vec<String>,
    },

    /// Image build event
    #[serde(rename_all = "camelCase")]
    BuildingImage {
        /// Name of the image being built
        image: String,
    },

    /// Image push event
    #[serde(rename_all = "camelCase")]
    PushingImage {
        /// Name of the image being pushed
        image: String,
        /// Progress information for the push operation
        progress: Option<PushProgress>,
    },

    /// Pushing stack images to registry
    #[serde(rename_all = "camelCase")]
    PushingStack {
        /// Name of the stack being pushed
        stack: String,
        /// Target platform
        platform: String,
    },

    /// Pushing resource images to registry
    #[serde(rename_all = "camelCase")]
    PushingResource {
        /// Name of the resource being pushed
        resource_name: String,
        /// Type of the resource: "function", "container", "worker"
        resource_type: String,
    },

    /// Creating a release on the platform
    #[serde(rename_all = "camelCase")]
    CreatingRelease {
        /// Project name
        project: String,
    },

    /// Code compilation event (rust, typescript, etc.)
    #[serde(rename_all = "camelCase")]
    CompilingCode {
        /// Language being compiled (rust, typescript, etc.)
        language: String,
        /// Current progress/status line from the build output
        progress: Option<String>,
    },

    // ============================================================================
    // Alien Infra Events
    // ============================================================================
    #[serde(rename_all = "camelCase")]
    StackStep {
        /// The resulting state of the stack after the step.
        next_state: StackState,
        /// An suggested duration to wait before executing the next step.
        suggested_delay_ms: Option<u64>,
    },

    /// Generating CloudFormation template
    #[serde(rename_all = "camelCase")]
    GeneratingCloudFormationTemplate,

    /// Generating infrastructure template
    #[serde(rename_all = "camelCase")]
    GeneratingTemplate {
        /// Platform for which the template is being generated
        platform: String,
    },

    // ============================================================================
    // Agent Events
    // ============================================================================
    /// Provisioning a new agent
    #[serde(rename_all = "camelCase")]
    ProvisioningAgent {
        /// ID of the agent being provisioned
        agent_id: String,
        /// ID of the release being deployed to the agent
        release_id: String,
    },

    /// Updating an existing agent
    #[serde(rename_all = "camelCase")]
    UpdatingAgent {
        /// ID of the agent being updated
        agent_id: String,
        /// ID of the new release being deployed to the agent
        release_id: String,
    },

    /// Deleting an agent
    #[serde(rename_all = "camelCase")]
    DeletingAgent {
        /// ID of the agent being deleted
        agent_id: String,
        /// ID of the release that was running on the agent
        release_id: String,
    },

    /// Starting a debug session for an agent
    #[serde(rename_all = "camelCase")]
    DebuggingAgent {
        /// ID of the agent being debugged
        agent_id: String,
        /// ID of the debug session
        debug_session_id: String,
    },

    // ============================================================================
    // Alien Test Events (General)
    // ============================================================================
    /// Preparing environment for deployment
    #[serde(rename_all = "camelCase")]
    PreparingEnvironment {
        /// Name of the deployment strategy being used
        strategy_name: String,
    },

    /// Deploying stack with alien-infra
    #[serde(rename_all = "camelCase")]
    DeployingStack {
        /// Name of the stack being deployed
        stack_name: String,
    },

    /// Running test function after deployment
    #[serde(rename_all = "camelCase")]
    RunningTestFunction {
        /// Name of the stack being tested
        stack_name: String,
    },

    /// Cleaning up deployed stack resources
    #[serde(rename_all = "camelCase")]
    CleaningUpStack {
        /// Name of the stack being cleaned up
        stack_name: String,
        /// Name of the deployment strategy being used for cleanup
        strategy_name: String,
    },

    /// Cleaning up deployment environment
    #[serde(rename_all = "camelCase")]
    CleaningUpEnvironment {
        /// Name of the stack being cleaned up
        stack_name: String,
        /// Name of the deployment strategy being used for cleanup
        strategy_name: String,
    },

    /// Setting up platform context
    #[serde(rename_all = "camelCase")]
    SettingUpPlatformContext {
        /// Name of the platform (e.g., "AWS", "GCP")
        platform_name: String,
    },

    // ============================================================================
    // Alien Test Events (AWS-specific)
    // ============================================================================
    /// Ensuring docker repository exists
    #[serde(rename_all = "camelCase")]
    EnsuringDockerRepository {
        /// Name of the docker repository
        repository_name: String,
    },

    /// Deploying CloudFormation stack
    #[serde(rename_all = "camelCase")]
    DeployingCloudFormationStack {
        /// Name of the CloudFormation stack
        cfn_stack_name: String,
        /// Current stack status
        current_status: String,
    },

    /// Assuming AWS IAM role
    #[serde(rename_all = "camelCase")]
    AssumingRole {
        /// ARN of the role to assume
        role_arn: String,
    },

    /// Importing stack state from CloudFormation
    #[serde(rename_all = "camelCase")]
    ImportingStackStateFromCloudFormation {
        /// Name of the CloudFormation stack
        cfn_stack_name: String,
    },

    /// Deleting CloudFormation stack
    #[serde(rename_all = "camelCase")]
    DeletingCloudFormationStack {
        /// Name of the CloudFormation stack
        cfn_stack_name: String,
        /// Current stack status
        current_status: String,
    },

    /// Emptying S3 buckets before stack deletion
    #[serde(rename_all = "camelCase")]
    EmptyingBuckets {
        /// Names of the S3 buckets being emptied
        bucket_names: Vec<String>,
    },

    // ============================================================================
    // Events just for testing this module
    // ============================================================================
    #[cfg(test)]
    #[serde(rename_all = "camelCase")]
    TestBuildingStack { stack: String },

    #[cfg(test)]
    #[serde(rename_all = "camelCase")]
    TestBuildingImage { image: String },

    #[cfg(test)]
    #[serde(rename_all = "camelCase")]
    TestBuildImage { image: String, stage: String },

    #[cfg(test)]
    #[serde(rename_all = "camelCase")]
    TestPushImage { image: String },

    #[cfg(test)]
    #[serde(rename_all = "camelCase")]
    TestCreatingResource {
        resource_type: String,
        resource_name: String,
        details: Option<String>,
    },

    #[cfg(test)]
    #[serde(rename_all = "camelCase")]
    TestDeployingStack { stack: String },

    #[cfg(test)]
    #[serde(rename_all = "camelCase")]
    TestPerformingHealthCheck { target: String, check_type: String },
}

impl AlienEvent {
    /// Emit this event and wait for it to be handled
    pub async fn emit(self) -> Result<EventHandle> {
        EventBus::emit(self, None, EventState::None).await
    }

    /// Emit this event with a specific initial state
    pub async fn emit_with_state(self, state: EventState) -> Result<EventHandle> {
        EventBus::emit(self, None, state).await
    }

    /// Emit this event as a child of another event
    pub async fn emit_with_parent(self, parent_id: &str) -> Result<EventHandle> {
        EventBus::emit(self, Some(parent_id.to_string()), EventState::None).await
    }

    /// Start a scoped event that will track success/failure
    /// All events emitted within the scope will automatically be children of this event
    pub async fn in_scope<F, Fut, T, E>(self, f: F) -> std::result::Result<T, AlienError<E>>
    where
        F: FnOnce(EventHandle) -> Fut,
        Fut: std::future::Future<Output = std::result::Result<T, AlienError<E>>>,
        E: AlienErrorData + Clone + std::fmt::Debug + Serialize + Send + Sync + 'static,
    {
        let handle = match EventBus::emit(self, None, EventState::Started).await {
            Ok(handle) => handle,
            Err(e) => {
                // If we can't emit the event, we still want to run the function
                // but without event tracking. The error from emit is logged here.
                // This behavior is expected by `test_handler_failure_in_scoped_event`.
                eprintln!("Failed to emit event, continuing with no-op handle: {}", e);
                EventHandle::noop()
            }
        };

        // Establish parent context so all events emitted within the scope become children
        let result = handle.as_parent(|_| f(handle.clone())).await;

        match result {
            Ok(result) => {
                let _ = handle.complete().await; // Ignore errors in completion
                Ok(result)
            }
            Err(err) => {
                let _ = handle.fail(err.clone()).await; // Ignore errors in failure

                // Return the original error
                Err(err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_serialization() {
        let event = AlienEvent::BuildingStack {
            stack: "test-stack".to_string(),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"BuildingStack\""));
        assert!(json.contains("\"stack\":\"test-stack\""));

        let deserialized: AlienEvent = serde_json::from_str(&json).unwrap();
        match deserialized {
            AlienEvent::BuildingStack { stack } => assert_eq!(stack, "test-stack"),
            _ => panic!("Wrong event type"),
        }

        // Test that field names are camelCase
        let event_with_snake_case = AlienEvent::DownloadingAlienRuntime {
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            url: "https://example.com".to_string(),
        };

        let json = serde_json::to_string(&event_with_snake_case).unwrap();
        assert!(json.contains("\"type\":\"DownloadingAlienRuntime\""));
        assert!(json.contains("\"targetTriple\":\"x86_64-unknown-linux-gnu\""));
        assert!(json.contains("\"url\":\"https://example.com\""));

        let deserialized: AlienEvent = serde_json::from_str(&json).unwrap();
        match deserialized {
            AlienEvent::DownloadingAlienRuntime { target_triple, url } => {
                assert_eq!(target_triple, "x86_64-unknown-linux-gnu");
                assert_eq!(url, "https://example.com");
            }
            _ => panic!("Wrong event type"),
        }
    }
}
