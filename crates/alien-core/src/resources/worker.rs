use crate::error::{ErrorData, Result};
use crate::resource::{ResourceDefinition, ResourceOutputsDefinition, ResourceRef, ResourceType};
use crate::{PublicEndpointOutput, WorkerPublicEndpoint, APEX_HOST_LABEL};
use alien_error::AlienError;
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::fmt::Debug;

/// Specifies the source of the worker's executable code.
/// This can be a pre-built container image or source code that the system will build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum WorkerCode {
    /// Container image.
    #[serde(rename_all = "camelCase")]
    Image {
        /// Container image (e.g., `ghcr.io/myorg/myimage:latest`).
        image: String,
    },
    /// Source code to be built.
    #[serde(rename_all = "camelCase")]
    Source {
        /// The source directory to build from
        src: String,
        /// Toolchain configuration with type-safe options
        toolchain: ToolchainConfig,
    },
}

/// Configuration for different programming language toolchains.
/// Each toolchain provides type-safe build configuration and auto-detection capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum ToolchainConfig {
    /// Rust with Cargo build system
    #[serde(rename_all = "camelCase")]
    Rust {
        /// Name of the binary to build and run
        binary_name: String,
    },
    /// TypeScript/JavaScript compiled to single executable with Bun
    #[serde(rename_all = "camelCase")]
    TypeScript {
        /// Name of the compiled binary (defaults to package.json name if not specified)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        binary_name: Option<String>,
    },
    /// Docker build from Dockerfile
    #[serde(rename_all = "camelCase")]
    Docker {
        /// Dockerfile path relative to src (default: "Dockerfile")
        #[serde(skip_serializing_if = "Option::is_none")]
        dockerfile: Option<String>,
        /// Build arguments for docker build
        #[serde(skip_serializing_if = "Option::is_none")]
        build_args: Option<HashMap<String, String>>,
        /// Multi-stage build target
        #[serde(skip_serializing_if = "Option::is_none")]
        target: Option<String>,
    },
}

/// Defines what triggers a worker execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WorkerTrigger {
    /// Worker triggered by queue messages (always 1 message per invocation)
    Queue {
        /// Reference to the queue resource
        queue: ResourceRef,
    },
    /// Worker triggered by storage events (object created, deleted, etc.)
    Storage {
        /// Reference to the storage resource
        storage: ResourceRef,
        /// Events to trigger on (e.g., ["created", "deleted"])
        events: Vec<String>,
    },
    /// Worker triggered on a schedule (cron expression)
    Schedule {
        /// Cron expression for scheduling (standard 5-field unix cron)
        cron: String,
    },
}

impl WorkerTrigger {
    /// Creates a queue trigger for the specified queue resource.
    /// The worker will be automatically invoked when messages arrive in the queue.
    /// Each message is processed individually (batch size of 1).
    pub fn queue<R: ?Sized>(queue: &R) -> Self
    where
        for<'a> &'a R: Into<ResourceRef>,
    {
        let queue_ref: ResourceRef = queue.into();
        WorkerTrigger::Queue { queue: queue_ref }
    }

    /// Creates a storage trigger for the specified storage resource.
    /// The worker will be invoked when matching events occur on the storage resource.
    pub fn storage<R: ?Sized>(storage: &R, events: Vec<String>) -> Self
    where
        for<'a> &'a R: Into<ResourceRef>,
    {
        let storage_ref: ResourceRef = storage.into();
        WorkerTrigger::Storage {
            storage: storage_ref,
            events,
        }
    }

    /// Creates a schedule trigger with the specified cron expression.
    /// Uses standard 5-field unix cron format (minute hour day-of-month month day-of-week).
    pub fn schedule<S: Into<String>>(cron: S) -> Self {
        WorkerTrigger::Schedule { cron: cron.into() }
    }
}

/// Represents a serverless worker that executes code in response to triggers or direct invocations.
/// Workers are the primary compute resource in serverless applications, designed to be stateless and ephemeral.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(start_fn = new)]
pub struct Worker {
    /// Identifier for the worker. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]).
    /// Maximum 64 characters.
    #[builder(start_fn)]
    pub id: String,

    /// List of resource references this worker depends on.
    // TODO: We need to verify that the same link isn't added multiple times.
    #[builder(field)]
    pub links: Vec<ResourceRef>,

    /// List of triggers that define what events automatically invoke this worker.
    /// If empty, the worker is only invokable directly via HTTP calls or platform-specific invocation APIs.
    /// When configured, the worker will be automatically invoked when any of the specified trigger conditions are met.
    #[builder(field)]
    pub triggers: Vec<WorkerTrigger>,

    /// Public endpoints exposed by this worker.
    #[builder(field)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub public_endpoints: Vec<WorkerPublicEndpoint>,

    /// Permission profile name that defines the permissions granted to this worker.
    /// This references a profile defined in the stack's permission definitions.
    pub permissions: String,

    /// Code for the worker, either a pre-built image or source code to be built.
    pub code: WorkerCode,

    /// Memory allocated to the worker in megabytes (MB).
    /// Default: 512
    ///
    /// Platform-specific constraints:
    /// - **AWS Lambda**: 128–10240 MB in 1 MB increments
    /// - **GCP Cloud Run**: 128–32768 MB
    /// - **Azure Container Apps**: fixed CPU/memory pairs — 512, 1024, 1536, 2048, 2560,
    ///   3072, 3584, 4096 MB. Values below 512 are automatically rounded up at deploy time.
    #[builder(default = default_memory_mb())]
    #[serde(default = "default_memory_mb")]
    #[cfg_attr(feature = "openapi", schema(default = default_memory_mb))]
    pub memory_mb: u32,

    /// Maximum execution time for the worker in seconds.
    /// Constraints: 1‑3600 seconds (platform-specific limits may apply)
    /// Default: 180
    #[builder(
        default = default_timeout_seconds(),
        with = |timeout_seconds: u32| -> crate::Result<_> {
            validate_timeout_seconds(timeout_seconds)
        }
    )]
    #[serde(
        default = "default_timeout_seconds",
        deserialize_with = "deserialize_timeout_seconds"
    )]
    #[cfg_attr(
        feature = "openapi",
        schema(default = default_timeout_seconds, minimum = 1, maximum = 3600)
    )]
    pub timeout_seconds: u32,

    /// Key-value pairs to set as environment variables for the worker.
    #[builder(default)]
    #[serde(default)]
    pub environment: HashMap<String, String>,

    /// Whether the worker can receive remote commands via the Commands protocol.
    /// When enabled, the platform pushes commands into the Worker runtime,
    /// which executes registered handlers.
    #[builder(default = default_commands_enabled())]
    #[serde(default = "default_commands_enabled")]
    #[cfg_attr(feature = "openapi", schema(default = default_commands_enabled))]
    pub commands_enabled: bool,

    /// Maximum number of concurrent executions allowed for the worker.
    /// None means platform default applies.
    pub concurrency_limit: Option<u32>,

    /// Optional readiness probe configuration.
    /// Only applicable for workers with Public ingress.
    /// When configured, the probe will be executed after provisioning/update to verify the worker is ready.
    pub readiness_probe: Option<ReadinessProbe>,
}

impl Worker {
    /// The resource type identifier for Workers
    pub const RESOURCE_TYPE: ResourceType = ResourceType::from_static("worker");

    /// Returns the permission profile name for this worker.
    pub fn get_permissions(&self) -> &str {
        &self.permissions
    }

    fn validate_public_endpoints(&self) -> Result<()> {
        let mut endpoint_names = std::collections::HashSet::new();
        let mut apex_endpoint_name: Option<&str> = None;
        for endpoint in &self.public_endpoints {
            endpoint.validate_for_resource(&self.id)?;
            if !endpoint_names.insert(endpoint.name.as_str()) {
                return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                    resource_id: self.id.clone(),
                    reason: format!("duplicate public endpoint name '{}'", endpoint.name),
                }));
            }
            if endpoint.host_label.as_deref() == Some(APEX_HOST_LABEL) {
                if let Some(existing_name) = apex_endpoint_name {
                    return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                        resource_id: self.id.clone(),
                        reason: format!(
                            "only one apex public endpoint is allowed per resource; '{}' already uses hostLabel '@'",
                            existing_name
                        ),
                    }));
                }
                apex_endpoint_name = Some(endpoint.name.as_str());
            }
        }

        Ok(())
    }
}

fn default_memory_mb() -> u32 {
    512
}

fn default_timeout_seconds() -> u32 {
    180
}

/// Longest Worker execution supported by every Commands delivery path.
pub const MAX_WORKER_TIMEOUT_SECONDS: u32 = 3600;

fn deserialize_timeout_seconds<'de, D>(deserializer: D) -> std::result::Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = u32::deserialize(deserializer)?;
    validate_timeout_seconds(value).map_err(serde::de::Error::custom)
}

fn validate_timeout_seconds(timeout_seconds: u32) -> Result<u32> {
    if (1..=MAX_WORKER_TIMEOUT_SECONDS).contains(&timeout_seconds) {
        return Ok(timeout_seconds);
    }

    Err(AlienError::new(ErrorData::WorkerTimeoutInvalid {
        timeout_seconds,
        max_timeout_seconds: MAX_WORKER_TIMEOUT_SECONDS,
    }))
}

fn default_commands_enabled() -> bool {
    false
}

/// HTTP method for readiness probe requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "UPPERCASE")]
#[derive(Default)]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
    Put,
    Delete,
    Head,
    Options,
    Patch,
}

/// Configuration for HTTP-based readiness probe.
/// This probe is executed after worker provisioning/update to verify the worker is ready to serve traffic.
/// Only works with workers that have Public ingress.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ReadinessProbe {
    /// HTTP method to use for the probe request.
    /// Default: GET
    #[serde(default)]
    pub method: HttpMethod,

    /// Path to request for the probe (e.g., "/health", "/ready").
    /// Default: "/"
    #[serde(default = "default_probe_path")]
    pub path: String,
}

fn default_probe_path() -> String {
    "/".to_string()
}

impl Default for ReadinessProbe {
    fn default() -> Self {
        Self {
            method: HttpMethod::default(),
            path: default_probe_path(),
        }
    }
}

use crate::resources::worker::worker_builder::State;

impl<S: State> WorkerBuilder<S> {
    /// Links the worker to another resource with specified permissions.
    /// Accepts a reference to any type `R` where `&R` can be converted into `ResourceRef`.
    pub fn link<R: ?Sized>(mut self, resource: &R) -> Self
    where
        for<'a> &'a R: Into<ResourceRef>, // Use Higher-Rank Trait Bound (HRTB)
    {
        // Perform the conversion from &R to ResourceRef using .into()
        let resource_ref: ResourceRef = resource.into();
        self.links.push(resource_ref);
        self
    }

    /// Adds a trigger to the worker. Workers can have multiple triggers.
    /// Each trigger will independently invoke the worker when its conditions are met.
    ///
    /// # Examples
    /// ```rust
    /// # use alien_core::{Worker, WorkerTrigger, WorkerCode, Queue};
    /// # let queue1 = Queue::new("queue-1".to_string()).build();
    /// # let queue2 = Queue::new("queue-2".to_string()).build();
    /// let worker = Worker::new("my-worker".to_string())
    ///     .code(WorkerCode::Image { image: "test:latest".to_string() })
    ///     .permissions("execution".to_string())
    ///     .trigger(WorkerTrigger::queue(&queue1))
    ///     .trigger(WorkerTrigger::queue(&queue2))
    ///     .build();
    /// ```
    pub fn trigger(mut self, trigger: WorkerTrigger) -> Self {
        self.triggers.push(trigger);
        self
    }

    /// Exposes a named public endpoint for the worker.
    pub fn public_endpoint(mut self, endpoint: WorkerPublicEndpoint) -> Self {
        self.public_endpoints.push(endpoint);
        self
    }
}

// Implementation of ResourceDefinition trait for Worker
impl ResourceDefinition for Worker {
    fn get_resource_type(&self) -> ResourceType {
        Self::RESOURCE_TYPE
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn get_dependencies(&self) -> Vec<ResourceRef> {
        let mut dependencies = self.links.clone();

        // Add trigger dependencies
        for trigger in &self.triggers {
            match trigger {
                WorkerTrigger::Queue { queue } => {
                    dependencies.push(queue.clone());
                }
                WorkerTrigger::Storage { storage, .. } => {
                    dependencies.push(storage.clone());
                }
                WorkerTrigger::Schedule { .. } => {
                    // Schedule triggers don't depend on other resources
                }
            }
        }

        dependencies
    }

    fn get_permissions(&self) -> Option<&str> {
        Some(&self.permissions)
    }

    fn validate_update(&self, new_config: &dyn ResourceDefinition) -> Result<()> {
        // Downcast to Worker type to use the existing validate_update method
        let new_worker = new_config
            .as_any()
            .downcast_ref::<Worker>()
            .ok_or_else(|| {
                AlienError::new(ErrorData::UnexpectedResourceType {
                    resource_id: self.id.clone(),
                    expected: Self::RESOURCE_TYPE,
                    actual: new_config.get_resource_type(),
                })
            })?;

        if self.id != new_worker.id {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'id' field is immutable".to_string(),
            }));
        }
        self.validate_public_endpoints()?;
        new_worker.validate_public_endpoints()?;
        if self.public_endpoints != new_worker.public_endpoints {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'publicEndpoints' field is immutable".to_string(),
            }));
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceDefinition> {
        Box::new(self.clone())
    }

    fn resource_eq(&self, other: &dyn ResourceDefinition) -> bool {
        other.as_any().downcast_ref::<Worker>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

/// Outputs generated by a successfully provisioned Worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct WorkerOutputs {
    /// The platform-specific worker name.
    pub worker_name: String,
    /// Public endpoints resolved for this worker.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub public_endpoints: HashMap<String, PublicEndpointOutput>,
    /// The ARN or platform-specific identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    /// Push target for commands delivery. Platform-specific:
    /// - AWS: Lambda function name or ARN
    /// - GCP: Full Pub/Sub topic path (projects/{project}/topics/{topic})
    /// - Azure: Service Bus "{namespace}/{queue}"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commands_push_target: Option<String>,
}

impl ResourceOutputsDefinition for WorkerOutputs {
    fn get_resource_type(&self) -> ResourceType {
        Worker::RESOURCE_TYPE.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceOutputsDefinition> {
        Box::new(self.clone())
    }

    fn outputs_eq(&self, other: &dyn ResourceOutputsDefinition) -> bool {
        other.as_any().downcast_ref::<WorkerOutputs>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Storage;

    #[test]
    fn test_worker_builder_direct_refs() {
        let dummy_storage = Storage::new("test-storage".to_string()).build();
        let dummy_storage_2 = Storage::new("test-storage-2".to_string()).build();

        let worker = Worker::new("my-worker".to_string())
            .code(WorkerCode::Image {
                image: "test-image".to_string(),
            })
            .permissions("execution".to_string())
            .link(&dummy_storage) // Pass reference directly
            .link(&dummy_storage_2) // Add a second link
            .build();

        assert_eq!(worker.id, "my-worker");
        assert_eq!(
            worker.code,
            WorkerCode::Image {
                image: "test-image".to_string()
            }
        );

        // Verify permissions was set correctly
        assert_eq!(worker.permissions, "execution");

        // Verify links were added correctly
        assert!(worker
            .links
            .contains(&ResourceRef::new(Storage::RESOURCE_TYPE, "test-storage")));
        assert!(worker
            .links
            .contains(&ResourceRef::new(Storage::RESOURCE_TYPE, "test-storage-2")));
        assert_eq!(worker.links.len(), 2); // Expect 2 links now
    }

    #[test]
    fn test_worker_with_readiness_probe() {
        let probe = ReadinessProbe {
            method: HttpMethod::Post,
            path: "/health".to_string(),
        };

        let worker = Worker::new("my-worker".to_string())
            .code(WorkerCode::Image {
                image: "test-image".to_string(),
            })
            .permissions("execution".to_string())
            .public_endpoint(WorkerPublicEndpoint {
                name: "api".to_string(),
                host_label: None,
                wildcard_subdomains: false,
            })
            .readiness_probe(probe.clone())
            .build();

        assert_eq!(worker.id, "my-worker");
        assert_eq!(worker.public_endpoints[0].name, "api");
        assert_eq!(worker.readiness_probe, Some(probe));
    }

    #[test]
    fn test_readiness_probe_defaults() {
        let probe = ReadinessProbe::default();
        assert_eq!(probe.method, HttpMethod::Get);
        assert_eq!(probe.path, "/");
    }

    #[test]
    fn test_worker_with_rust_toolchain() {
        let worker = Worker::new("my-rust-worker".to_string())
            .code(WorkerCode::Source {
                src: "./".to_string(),
                toolchain: ToolchainConfig::Rust {
                    binary_name: "my-app".to_string(),
                },
            })
            .permissions("execution".to_string())
            .build();

        assert_eq!(worker.id, "my-rust-worker");

        match &worker.code {
            WorkerCode::Source { src, toolchain } => {
                assert_eq!(src, "./");
                assert_eq!(
                    toolchain,
                    &ToolchainConfig::Rust {
                        binary_name: "my-app".to_string(),
                    }
                );
            }
            _ => panic!("Expected Source code"),
        }
    }

    #[test]
    fn test_worker_with_typescript_toolchain() {
        let worker = Worker::new("my-ts-worker".to_string())
            .code(WorkerCode::Source {
                src: "./".to_string(),
                toolchain: ToolchainConfig::TypeScript {
                    binary_name: Some("my-ts-worker".to_string()),
                },
            })
            .permissions("execution".to_string())
            .build();

        assert_eq!(worker.id, "my-ts-worker");

        match &worker.code {
            WorkerCode::Source { src, toolchain } => {
                assert_eq!(src, "./");
                assert_eq!(
                    toolchain,
                    &ToolchainConfig::TypeScript {
                        binary_name: Some("my-ts-worker".to_string())
                    }
                );
            }
            _ => panic!("Expected Source code"),
        }
    }

    #[test]
    fn test_worker_with_queue_trigger() {
        use crate::Queue;

        let queue = Queue::new("test-queue".to_string()).build();

        let worker = Worker::new("triggered-worker".to_string())
            .code(WorkerCode::Image {
                image: "test-image".to_string(),
            })
            .permissions("execution".to_string())
            .trigger(WorkerTrigger::queue(&queue))
            .build();

        assert_eq!(worker.triggers.len(), 1);
        if let WorkerTrigger::Queue { queue: queue_ref } = &worker.triggers[0] {
            assert_eq!(queue_ref.resource_type, Queue::RESOURCE_TYPE);
            assert_eq!(queue_ref.id, "test-queue");
        } else {
            panic!("Expected queue trigger");
        }
    }

    #[test]
    fn test_worker_trigger_dependencies() {
        use crate::Queue;

        let queue = Queue::new("test-queue".to_string()).build();
        let storage = Storage::new("test-storage".to_string()).build();

        let worker = Worker::new("triggered-worker".to_string())
            .code(WorkerCode::Image {
                image: "test-image".to_string(),
            })
            .permissions("execution".to_string())
            .link(&storage) // regular link dependency
            .trigger(WorkerTrigger::queue(&queue)) // trigger dependency
            .build();

        let dependencies = worker.get_dependencies();

        // Should have both link and trigger dependencies
        assert_eq!(dependencies.len(), 2);
        assert!(dependencies.contains(&ResourceRef::new(Storage::RESOURCE_TYPE, "test-storage")));
        assert!(dependencies.contains(&ResourceRef::new(Queue::RESOURCE_TYPE, "test-queue")));
    }

    #[test]
    fn test_worker_trigger_helper_methods() {
        use crate::Queue;

        let queue = Queue::new("my-queue".to_string()).build();

        // Test the helper method
        let trigger = WorkerTrigger::queue(&queue);

        if let WorkerTrigger::Queue { queue: queue_ref } = trigger {
            assert_eq!(queue_ref.resource_type, Queue::RESOURCE_TYPE);
            assert_eq!(queue_ref.id, "my-queue");
        } else {
            panic!("Expected queue trigger");
        }
    }

    #[test]
    fn test_worker_with_multiple_triggers() {
        use crate::Queue;

        let queue1 = Queue::new("queue-1".to_string()).build();
        let queue2 = Queue::new("queue-2".to_string()).build();

        let worker = Worker::new("multi-triggered-worker".to_string())
            .code(WorkerCode::Image {
                image: "test-image".to_string(),
            })
            .permissions("execution".to_string())
            .trigger(WorkerTrigger::queue(&queue1))
            .trigger(WorkerTrigger::queue(&queue2))
            .trigger(WorkerTrigger::schedule("0 * * * *".to_string()))
            .build();

        assert_eq!(worker.triggers.len(), 3);

        // Check first queue trigger
        if let WorkerTrigger::Queue { queue: queue_ref } = &worker.triggers[0] {
            assert_eq!(queue_ref.id, "queue-1");
        } else {
            panic!("Expected first trigger to be queue-1");
        }

        // Check second queue trigger
        if let WorkerTrigger::Queue { queue: queue_ref } = &worker.triggers[1] {
            assert_eq!(queue_ref.id, "queue-2");
        } else {
            panic!("Expected second trigger to be queue-2");
        }

        // Check schedule trigger
        if let WorkerTrigger::Schedule { cron } = &worker.triggers[2] {
            assert_eq!(cron, "0 * * * *");
        } else {
            panic!("Expected third trigger to be schedule");
        }

        // Check dependencies include both queues
        let dependencies = worker.get_dependencies();
        assert_eq!(dependencies.len(), 2); // Only queues, schedule has no dependency
        assert!(dependencies.contains(&ResourceRef::new(Queue::RESOURCE_TYPE, "queue-1")));
        assert!(dependencies.contains(&ResourceRef::new(Queue::RESOURCE_TYPE, "queue-2")));
    }

    #[test]
    fn test_worker_with_commands_enabled() {
        let worker = Worker::new("cmd-worker".to_string())
            .code(WorkerCode::Image {
                image: "test-image".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();

        assert_eq!(worker.id, "cmd-worker");
        assert!(worker.public_endpoints.is_empty());
        assert_eq!(worker.commands_enabled, true);
    }

    #[test]
    fn test_worker_defaults() {
        let worker = Worker::new("default-worker".to_string())
            .code(WorkerCode::Image {
                image: "test-image".to_string(),
            })
            .permissions("execution".to_string())
            .build();

        // Test that defaults are applied correctly
        assert!(worker.public_endpoints.is_empty());
        assert_eq!(worker.commands_enabled, false);
        assert_eq!(worker.memory_mb, 512);
        assert_eq!(worker.timeout_seconds, 180);
    }

    #[test]
    fn worker_deserialization_rejects_timeout_outside_supported_range() {
        let worker = Worker::new("timeout-worker".to_string())
            .code(WorkerCode::Image {
                image: "test-image".to_string(),
            })
            .permissions("execution".to_string())
            .build();
        let mut value = serde_json::to_value(worker).expect("serialize worker");

        value["timeoutSeconds"] = serde_json::json!(0);
        assert!(serde_json::from_value::<Worker>(value.clone()).is_err());

        value["timeoutSeconds"] = serde_json::json!(MAX_WORKER_TIMEOUT_SECONDS + 1);
        assert!(serde_json::from_value::<Worker>(value).is_err());
    }

    #[test]
    fn worker_builder_rejects_zero_timeout() {
        let Err(error) = Worker::new("timeout-worker".to_string()).timeout_seconds(0) else {
            panic!("zero timeout must be rejected");
        };

        assert_eq!(error.code, "WORKER_TIMEOUT_INVALID");
        assert_eq!(error.http_status_code, Some(400));
    }

    #[test]
    fn worker_builder_rejects_timeout_above_maximum() {
        let Err(error) = Worker::new("timeout-worker".to_string())
            .timeout_seconds(MAX_WORKER_TIMEOUT_SECONDS + 1)
        else {
            panic!("timeout above maximum must be rejected");
        };

        assert_eq!(error.code, "WORKER_TIMEOUT_INVALID");
    }

    #[test]
    fn worker_builder_accepts_minimum_timeout() {
        let worker = Worker::new("timeout-worker".to_string())
            .timeout_seconds(1)
            .expect("minimum timeout is valid")
            .code(WorkerCode::Image {
                image: "test-image".to_string(),
            })
            .permissions("execution".to_string())
            .build();

        assert_eq!(worker.timeout_seconds, 1);
    }

    #[test]
    fn worker_builder_accepts_maximum_timeout() {
        let worker = Worker::new("timeout-worker".to_string())
            .timeout_seconds(MAX_WORKER_TIMEOUT_SECONDS)
            .expect("maximum timeout is valid")
            .code(WorkerCode::Image {
                image: "test-image".to_string(),
            })
            .permissions("execution".to_string())
            .build();

        assert_eq!(worker.timeout_seconds, MAX_WORKER_TIMEOUT_SECONDS);
    }

    #[test]
    fn test_worker_public_ingress_with_commands() {
        let worker = Worker::new("public-cmd-worker".to_string())
            .code(WorkerCode::Image {
                image: "test-image".to_string(),
            })
            .permissions("execution".to_string())
            .public_endpoint(WorkerPublicEndpoint {
                name: "api".to_string(),
                host_label: None,
                wildcard_subdomains: false,
            })
            .commands_enabled(true)
            .build();

        assert_eq!(worker.public_endpoints[0].name, "api");
        assert_eq!(worker.commands_enabled, true);
    }

    #[test]
    fn worker_rejects_multiple_apex_public_endpoints() {
        let worker = Worker::new("apex-worker".to_string())
            .code(WorkerCode::Image {
                image: "test-image".to_string(),
            })
            .permissions("execution".to_string())
            .public_endpoint(WorkerPublicEndpoint {
                name: "api".to_string(),
                host_label: Some(APEX_HOST_LABEL.to_string()),
                wildcard_subdomains: false,
            })
            .public_endpoint(WorkerPublicEndpoint {
                name: "admin".to_string(),
                host_label: Some(APEX_HOST_LABEL.to_string()),
                wildcard_subdomains: false,
            })
            .build();

        assert!(worker.validate_public_endpoints().is_err());
    }
}
