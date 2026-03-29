use crate::error::{ErrorData, Result};
use crate::resource::{ResourceDefinition, ResourceOutputsDefinition, ResourceRef, ResourceType};
use crate::LoadBalancerEndpoint;
use alien_error::AlienError;
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::fmt::Debug;

/// Controls network accessibility of the function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum Ingress {
    /// Function accessible from the internet with an HTTPS URL
    Public,
    /// Function accessible only from within the customer's cloud via triggers, other functions, etc. No external HTTP URL is created.
    Private,
}

/// Specifies the source of the function's executable code.
/// This can be a pre-built container image or source code that the system will build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum FunctionCode {
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

/// Defines what triggers a function execution
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum FunctionTrigger {
    /// Function triggered by queue messages (always 1 message per invocation)
    Queue {
        /// Reference to the queue resource
        queue: ResourceRef,
    },
    /// Function triggered by storage events (future implementation)
    #[allow(dead_code)]
    Storage {
        /// Reference to the storage resource
        storage: ResourceRef,
        /// Events to trigger on (e.g., ["created", "deleted"])
        events: Vec<String>,
    },
    /// Function triggered by scheduled events (future implementation)
    #[allow(dead_code)]
    Schedule {
        /// Cron expression for scheduling
        cron: String,
    },
}

impl FunctionTrigger {
    /// Creates a queue trigger for the specified queue resource.
    /// The function will be automatically invoked when messages arrive in the queue.
    /// Each message is processed individually (batch size of 1).
    pub fn queue<R: ?Sized>(queue: &R) -> Self
    where
        for<'a> &'a R: Into<ResourceRef>,
    {
        let queue_ref: ResourceRef = queue.into();
        FunctionTrigger::Queue { queue: queue_ref }
    }

    /// Creates a storage trigger for the specified storage resource (future implementation).
    #[allow(dead_code)]
    pub fn storage<R: ?Sized>(storage: &R, events: Vec<String>) -> Self
    where
        for<'a> &'a R: Into<ResourceRef>,
    {
        let storage_ref: ResourceRef = storage.into();
        FunctionTrigger::Storage {
            storage: storage_ref,
            events,
        }
    }

    /// Creates a schedule trigger with the specified cron expression (future implementation).
    #[allow(dead_code)]
    pub fn schedule<S: Into<String>>(cron: S) -> Self {
        FunctionTrigger::Schedule { cron: cron.into() }
    }
}

/// Represents a serverless function that executes code in response to triggers or direct invocations.
/// Functions are the primary compute resource in serverless applications, designed to be stateless and ephemeral.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(start_fn = new)]
pub struct Function {
    /// Identifier for the function. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]).
    /// Maximum 64 characters.
    #[builder(start_fn)]
    pub id: String,

    /// List of resource references this function depends on.
    // TODO: We need to verify that the same link isn't added multiple times.
    #[builder(field)]
    pub links: Vec<ResourceRef>,

    /// List of triggers that define what events automatically invoke this function.
    /// If empty, the function is only invokable directly via HTTP calls or platform-specific invocation APIs.
    /// When configured, the function will be automatically invoked when any of the specified trigger conditions are met.
    #[builder(field)]
    pub triggers: Vec<FunctionTrigger>,

    /// Permission profile name that defines the permissions granted to this function.
    /// This references a profile defined in the stack's permission definitions.
    pub permissions: String,

    /// Code for the function, either a pre-built image or source code to be built.
    pub code: FunctionCode,

    /// Memory allocated to the function in megabytes (MB).
    /// Constraints: 128‑32768 MB (platform-specific limits may apply)
    /// Default: 256
    #[builder(default = default_memory_mb())]
    #[serde(default = "default_memory_mb")]
    #[cfg_attr(feature = "openapi", schema(default = default_memory_mb))]
    pub memory_mb: u32,

    /// Maximum execution time for the function in seconds.
    /// Constraints: 1‑3600 seconds (platform-specific limits may apply)
    /// Default: 30
    #[builder(default = default_timeout_seconds())]
    #[serde(default = "default_timeout_seconds")]
    #[cfg_attr(feature = "openapi", schema(default = default_timeout_seconds))]
    pub timeout_seconds: u32,

    /// Key-value pairs to set as environment variables for the function.
    #[builder(default)]
    #[serde(default)]
    pub environment: HashMap<String, String>,

    /// Controls network accessibility of the function.
    #[builder(default = default_ingress())]
    #[serde(default = "default_ingress")]
    #[cfg_attr(feature = "openapi", schema(default = default_ingress))]
    pub ingress: Ingress,

    /// Whether the function can receive remote commands via the Commands protocol.
    /// When enabled, the runtime polls the manager for pending commands and executes registered handlers.
    #[builder(default = default_commands_enabled())]
    #[serde(default = "default_commands_enabled")]
    #[cfg_attr(feature = "openapi", schema(default = default_commands_enabled))]
    pub commands_enabled: bool,

    /// Maximum number of concurrent executions allowed for the function.
    /// None means platform default applies.
    pub concurrency_limit: Option<u32>,

    /// Optional readiness probe configuration.
    /// Only applicable for functions with Public ingress.
    /// When configured, the probe will be executed after provisioning/update to verify the function is ready.
    pub readiness_probe: Option<ReadinessProbe>,
}

impl Function {
    /// The resource type identifier for Functions
    pub const RESOURCE_TYPE: ResourceType = ResourceType::from_static("function");

    /// Returns the permission profile name for this function.
    pub fn get_permissions(&self) -> &str {
        &self.permissions
    }
}

fn default_memory_mb() -> u32 {
    256
}

fn default_timeout_seconds() -> u32 {
    180
}

fn default_ingress() -> Ingress {
    Ingress::Private
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
/// This probe is executed after function provisioning/update to verify the function is ready to serve traffic.
/// Only works with functions that have Public ingress.
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

use crate::resources::function::function_builder::State;

impl<S: State> FunctionBuilder<S> {
    /// Links the function to another resource with specified permissions.
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

    /// Adds a trigger to the function. Functions can have multiple triggers.
    /// Each trigger will independently invoke the function when its conditions are met.
    ///
    /// # Examples
    /// ```rust
    /// # use alien_core::{Function, FunctionTrigger, FunctionCode, Queue};
    /// # let queue1 = Queue::new("queue-1".to_string()).build();
    /// # let queue2 = Queue::new("queue-2".to_string()).build();
    /// let function = Function::new("my-function".to_string())
    ///     .code(FunctionCode::Image { image: "test:latest".to_string() })
    ///     .permissions("execution".to_string())
    ///     .trigger(FunctionTrigger::queue(&queue1))
    ///     .trigger(FunctionTrigger::queue(&queue2))
    ///     .build();
    /// ```
    pub fn trigger(mut self, trigger: FunctionTrigger) -> Self {
        self.triggers.push(trigger);
        self
    }
}

// Implementation of ResourceDefinition trait for Function
impl ResourceDefinition for Function {
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
                FunctionTrigger::Queue { queue } => {
                    dependencies.push(queue.clone());
                }
                FunctionTrigger::Storage { storage, .. } => {
                    dependencies.push(storage.clone());
                }
                FunctionTrigger::Schedule { .. } => {
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
        // Downcast to Function type to use the existing validate_update method
        let new_function = new_config
            .as_any()
            .downcast_ref::<Function>()
            .ok_or_else(|| {
                AlienError::new(ErrorData::UnexpectedResourceType {
                    resource_id: self.id.clone(),
                    expected: Self::RESOURCE_TYPE,
                    actual: new_config.get_resource_type(),
                })
            })?;

        if self.id != new_function.id {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'id' field is immutable".to_string(),
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
        other.as_any().downcast_ref::<Function>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

/// Outputs generated by a successfully provisioned Function.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct FunctionOutputs {
    /// The name of the function.
    pub function_name: String,
    /// The invocation URL (if applicable, e.g., for public ingress or specific platforms).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The ARN or platform-specific identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    /// Load balancer endpoint information for DNS management (optional).
    /// Used by the DNS controller to create custom domain mappings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_balancer_endpoint: Option<LoadBalancerEndpoint>,
    /// Push target for commands delivery. Platform-specific:
    /// - AWS: Lambda function name or ARN
    /// - GCP: Full Pub/Sub topic path (projects/{project}/topics/{topic})
    /// - Azure: Service Bus "{namespace}/{queue}"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commands_push_target: Option<String>,
}

impl ResourceOutputsDefinition for FunctionOutputs {
    fn get_resource_type(&self) -> ResourceType {
        Function::RESOURCE_TYPE.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceOutputsDefinition> {
        Box::new(self.clone())
    }

    fn outputs_eq(&self, other: &dyn ResourceOutputsDefinition) -> bool {
        other.as_any().downcast_ref::<FunctionOutputs>() == Some(self)
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
    fn test_function_builder_direct_refs() {
        let dummy_storage = Storage::new("test-storage".to_string()).build();
        let dummy_storage_2 = Storage::new("test-storage-2".to_string()).build();

        let function = Function::new("my-func".to_string())
            .code(FunctionCode::Image {
                image: "test-image".to_string(),
            })
            .permissions("execution".to_string())
            .link(&dummy_storage) // Pass reference directly
            .link(&dummy_storage_2) // Add a second link
            .build();

        assert_eq!(function.id, "my-func");
        assert_eq!(
            function.code,
            FunctionCode::Image {
                image: "test-image".to_string()
            }
        );

        // Verify permissions was set correctly
        assert_eq!(function.permissions, "execution");

        // Verify links were added correctly
        assert!(function
            .links
            .contains(&ResourceRef::new(Storage::RESOURCE_TYPE, "test-storage")));
        assert!(function
            .links
            .contains(&ResourceRef::new(Storage::RESOURCE_TYPE, "test-storage-2")));
        assert_eq!(function.links.len(), 2); // Expect 2 links now
    }

    #[test]
    fn test_function_with_readiness_probe() {
        let probe = ReadinessProbe {
            method: HttpMethod::Post,
            path: "/health".to_string(),
        };

        let function = Function::new("my-func".to_string())
            .code(FunctionCode::Image {
                image: "test-image".to_string(),
            })
            .permissions("execution".to_string())
            .ingress(Ingress::Public)
            .readiness_probe(probe.clone())
            .build();

        assert_eq!(function.id, "my-func");
        assert_eq!(function.ingress, Ingress::Public);
        assert_eq!(function.readiness_probe, Some(probe));
    }

    #[test]
    fn test_readiness_probe_defaults() {
        let probe = ReadinessProbe::default();
        assert_eq!(probe.method, HttpMethod::Get);
        assert_eq!(probe.path, "/");
    }

    #[test]
    fn test_function_with_rust_toolchain() {
        let function = Function::new("my-rust-func".to_string())
            .code(FunctionCode::Source {
                src: "./".to_string(),
                toolchain: ToolchainConfig::Rust {
                    binary_name: "my-app".to_string(),
                },
            })
            .permissions("execution".to_string())
            .build();

        assert_eq!(function.id, "my-rust-func");

        match &function.code {
            FunctionCode::Source { src, toolchain } => {
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
    fn test_function_with_typescript_toolchain() {
        let function = Function::new("my-ts-func".to_string())
            .code(FunctionCode::Source {
                src: "./".to_string(),
                toolchain: ToolchainConfig::TypeScript {
                    binary_name: Some("my-ts-func".to_string()),
                },
            })
            .permissions("execution".to_string())
            .build();

        assert_eq!(function.id, "my-ts-func");

        match &function.code {
            FunctionCode::Source { src, toolchain } => {
                assert_eq!(src, "./");
                assert_eq!(
                    toolchain,
                    &ToolchainConfig::TypeScript {
                        binary_name: Some("my-ts-func".to_string())
                    }
                );
            }
            _ => panic!("Expected Source code"),
        }
    }

    #[test]
    fn test_function_with_queue_trigger() {
        use crate::Queue;

        let queue = Queue::new("test-queue".to_string()).build();

        let function = Function::new("triggered-func".to_string())
            .code(FunctionCode::Image {
                image: "test-image".to_string(),
            })
            .permissions("execution".to_string())
            .trigger(FunctionTrigger::queue(&queue))
            .build();

        assert_eq!(function.triggers.len(), 1);
        if let FunctionTrigger::Queue { queue: queue_ref } = &function.triggers[0] {
            assert_eq!(queue_ref.resource_type, Queue::RESOURCE_TYPE);
            assert_eq!(queue_ref.id, "test-queue");
        } else {
            panic!("Expected queue trigger");
        }
    }

    #[test]
    fn test_function_trigger_dependencies() {
        use crate::Queue;

        let queue = Queue::new("test-queue".to_string()).build();
        let storage = Storage::new("test-storage".to_string()).build();

        let function = Function::new("triggered-func".to_string())
            .code(FunctionCode::Image {
                image: "test-image".to_string(),
            })
            .permissions("execution".to_string())
            .link(&storage) // regular link dependency
            .trigger(FunctionTrigger::queue(&queue)) // trigger dependency
            .build();

        let dependencies = function.get_dependencies();

        // Should have both link and trigger dependencies
        assert_eq!(dependencies.len(), 2);
        assert!(dependencies.contains(&ResourceRef::new(Storage::RESOURCE_TYPE, "test-storage")));
        assert!(dependencies.contains(&ResourceRef::new(Queue::RESOURCE_TYPE, "test-queue")));
    }

    #[test]
    fn test_function_trigger_helper_methods() {
        use crate::Queue;

        let queue = Queue::new("my-queue".to_string()).build();

        // Test the helper method
        let trigger = FunctionTrigger::queue(&queue);

        if let FunctionTrigger::Queue { queue: queue_ref } = trigger {
            assert_eq!(queue_ref.resource_type, Queue::RESOURCE_TYPE);
            assert_eq!(queue_ref.id, "my-queue");
        } else {
            panic!("Expected queue trigger");
        }
    }

    #[test]
    fn test_function_with_multiple_triggers() {
        use crate::Queue;

        let queue1 = Queue::new("queue-1".to_string()).build();
        let queue2 = Queue::new("queue-2".to_string()).build();

        let function = Function::new("multi-triggered-func".to_string())
            .code(FunctionCode::Image {
                image: "test-image".to_string(),
            })
            .permissions("execution".to_string())
            .trigger(FunctionTrigger::queue(&queue1))
            .trigger(FunctionTrigger::queue(&queue2))
            .trigger(FunctionTrigger::schedule("0 * * * *".to_string()))
            .build();

        assert_eq!(function.triggers.len(), 3);

        // Check first queue trigger
        if let FunctionTrigger::Queue { queue: queue_ref } = &function.triggers[0] {
            assert_eq!(queue_ref.id, "queue-1");
        } else {
            panic!("Expected first trigger to be queue-1");
        }

        // Check second queue trigger
        if let FunctionTrigger::Queue { queue: queue_ref } = &function.triggers[1] {
            assert_eq!(queue_ref.id, "queue-2");
        } else {
            panic!("Expected second trigger to be queue-2");
        }

        // Check schedule trigger
        if let FunctionTrigger::Schedule { cron } = &function.triggers[2] {
            assert_eq!(cron, "0 * * * *");
        } else {
            panic!("Expected third trigger to be schedule");
        }

        // Check dependencies include both queues
        let dependencies = function.get_dependencies();
        assert_eq!(dependencies.len(), 2); // Only queues, schedule has no dependency
        assert!(dependencies.contains(&ResourceRef::new(Queue::RESOURCE_TYPE, "queue-1")));
        assert!(dependencies.contains(&ResourceRef::new(Queue::RESOURCE_TYPE, "queue-2")));
    }

    #[test]
    fn test_function_with_commands_enabled() {
        let function = Function::new("cmd-func".to_string())
            .code(FunctionCode::Image {
                image: "test-image".to_string(),
            })
            .permissions("execution".to_string())
            .ingress(Ingress::Private)
            .commands_enabled(true)
            .build();

        assert_eq!(function.id, "cmd-func");
        assert_eq!(function.ingress, Ingress::Private);
        assert_eq!(function.commands_enabled, true);
    }

    #[test]
    fn test_function_defaults() {
        let function = Function::new("default-func".to_string())
            .code(FunctionCode::Image {
                image: "test-image".to_string(),
            })
            .permissions("execution".to_string())
            .build();

        // Test that defaults are applied correctly
        assert_eq!(function.ingress, Ingress::Private);
        assert_eq!(function.commands_enabled, false);
        assert_eq!(function.memory_mb, 256);
        assert_eq!(function.timeout_seconds, 180);
    }

    #[test]
    fn test_function_public_ingress_with_commands() {
        let function = Function::new("public-cmd-func".to_string())
            .code(FunctionCode::Image {
                image: "test-image".to_string(),
            })
            .permissions("execution".to_string())
            .ingress(Ingress::Public)
            .commands_enabled(true)
            .build();

        assert_eq!(function.ingress, Ingress::Public);
        assert_eq!(function.commands_enabled, true);
    }
}
