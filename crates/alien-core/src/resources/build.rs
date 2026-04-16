use crate::error::{ErrorData, Result};
use crate::resource::{ResourceDefinition, ResourceOutputsDefinition, ResourceRef, ResourceType};
use alien_error::AlienError;
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::fmt::Debug;

/// Status of a build execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BuildStatus {
    /// Build is queued and waiting to start
    Queued,
    /// Build is currently running
    Running,
    /// Build completed successfully
    Succeeded,
    /// Build failed with errors
    Failed,
    /// Build was cancelled or stopped
    Cancelled,
    /// Build timed out
    TimedOut,
}

/// Compute type for build resources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ComputeType {
    /// Small compute resources (e.g., 0.25 vCPU, 0.5 GB RAM)
    Small,
    /// Medium compute resources (e.g., 0.5 vCPU, 1 GB RAM)
    Medium,
    /// Large compute resources (e.g., 1 vCPU, 2 GB RAM)
    Large,
    /// Extra large compute resources (e.g., 2 vCPU, 4 GB RAM)
    XLarge,
}

impl Default for ComputeType {
    fn default() -> Self {
        ComputeType::Medium
    }
}

/// Configuration for monitoring and observability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MonitoringConfig {
    /// The monitoring endpoint URL (e.g., "https://otel-collector.example.com:4318")
    pub endpoint: String,
    /// Optional HTTP headers to include in requests to the monitoring endpoint
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Optional URI path for logs (defaults to "/v1/logs")
    #[serde(default = "default_logs_uri")]
    pub logs_uri: String,
    /// Whether to enable TLS/HTTPS (defaults to true)
    #[serde(default = "default_tls_enabled")]
    pub tls_enabled: bool,
    /// Whether to verify TLS certificates (defaults to true)
    #[serde(default = "default_tls_verify")]
    pub tls_verify: bool,
}

fn default_logs_uri() -> String {
    "/v1/logs".to_string()
}

fn default_tls_enabled() -> bool {
    true
}

fn default_tls_verify() -> bool {
    true
}

/// Configuration for starting a build.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct BuildConfig {
    /// Base container image to use for the build environment.
    pub image: String,
    /// Bash script to execute for the build.
    pub script: String,
    /// Key-value pairs to set as environment variables for the build.
    #[serde(default)]
    pub environment: HashMap<String, String>,
    /// Maximum execution time for the build in seconds.
    pub timeout_seconds: u32,
    /// Amount of compute resources allocated to the build.
    #[serde(default)]
    pub compute_type: ComputeType,
    /// Optional monitoring configuration for sending build logs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitoring: Option<MonitoringConfig>,
}

/// Represents a build resource that executes bash scripts to build code.
/// Builds are designed to be stateless and can be triggered on-demand to compile,
/// test, or package application code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(start_fn = new)]
pub struct Build {
    /// Identifier for the build resource. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]).
    /// Maximum 64 characters.
    #[builder(start_fn)]
    pub id: String,

    /// List of resource references this build depends on.
    #[builder(field)]
    pub links: Vec<ResourceRef>,

    /// Permission profile name that defines the permissions granted to this build.
    /// This references a profile defined in the stack's permission definitions.
    pub permissions: String,

    /// Key-value pairs to set as environment variables for the build.
    #[builder(default)]
    #[serde(default)]
    pub environment: HashMap<String, String>,

    /// Amount of compute resources allocated to the build.
    #[builder(default)]
    #[serde(default)]
    pub compute_type: ComputeType,
}

impl Build {
    /// The resource type identifier for Builds
    pub const RESOURCE_TYPE: ResourceType = ResourceType::from_static("build");

    /// Returns the permission profile name for this build.
    pub fn get_permissions(&self) -> &str {
        &self.permissions
    }
}

use crate::resources::build::build_builder::State;

impl<S: State> BuildBuilder<S> {
    /// Links the build to another resource with specified permissions.
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
}

// Implementation of ResourceDefinition trait for Build
impl ResourceDefinition for Build {
    fn get_resource_type(&self) -> ResourceType {
        Self::RESOURCE_TYPE
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn get_dependencies(&self) -> Vec<ResourceRef> {
        // Builds only depend on their linked resources
        // The permission profile is resolved at the stack level to create ServiceAccount resources
        self.links.clone()
    }

    fn validate_update(&self, new_config: &dyn ResourceDefinition) -> Result<()> {
        // Downcast to Build type to use the existing validate_update method
        let new_build = new_config.as_any().downcast_ref::<Build>().ok_or_else(|| {
            AlienError::new(ErrorData::UnexpectedResourceType {
                resource_id: self.id.clone(),
                expected: Self::RESOURCE_TYPE,
                actual: new_config.get_resource_type(),
            })
        })?;

        if self.id != new_build.id {
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
        other.as_any().downcast_ref::<Build>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

/// Outputs generated by a successfully provisioned Build.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct BuildOutputs {
    /// The platform-specific build project identifier (ARN for AWS, project ID for GCP, resource ID for Azure).
    pub identifier: String,
}

impl ResourceOutputsDefinition for BuildOutputs {
    fn get_resource_type(&self) -> ResourceType {
        Build::RESOURCE_TYPE.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceOutputsDefinition> {
        Box::new(self.clone())
    }

    fn outputs_eq(&self, other: &dyn ResourceOutputsDefinition) -> bool {
        other.as_any().downcast_ref::<BuildOutputs>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

/// Information about a build execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct BuildExecution {
    /// Unique identifier for this build execution.
    pub id: String,
    /// Current status of the build.
    pub status: BuildStatus,
    /// Build start time (ISO 8601 format).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,
    /// Build end time (ISO 8601 format).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Storage;

    #[test]
    fn test_build_builder_direct_refs() {
        let dummy_storage = Storage::new("test-storage".to_string()).build();

        let build = Build::new("my-build".to_string())
            .permissions("build-execution".to_string())
            .link(&dummy_storage) // Pass reference directly
            .compute_type(ComputeType::Large)
            .build();

        assert_eq!(build.id, "my-build");
        assert_eq!(build.compute_type, ComputeType::Large);

        // Verify permissions was set correctly
        assert_eq!(build.permissions, "build-execution");

        // Verify links were added correctly
        assert!(build
            .links
            .contains(&ResourceRef::new(Storage::RESOURCE_TYPE, "test-storage")));
        assert_eq!(build.links.len(), 1);
    }

    #[test]
    fn test_build_defaults() {
        let build = Build::new("default-build".to_string())
            .permissions("build-execution".to_string())
            .build();

        assert_eq!(build.compute_type, ComputeType::Medium);
        assert!(build.environment.is_empty());
        assert!(build.links.is_empty());
    }

    #[test]
    fn test_build_with_environment() {
        let mut env = HashMap::new();
        env.insert("NODE_ENV".to_string(), "production".to_string());
        env.insert("API_KEY".to_string(), "secret".to_string());

        let build = Build::new("env-build".to_string())
            .environment(env.clone())
            .permissions("test".to_string())
            .build();

        assert_eq!(build.environment, env);
    }

    #[test]
    fn test_build_config_with_monitoring() {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer token123".to_string());
        headers.insert("X-Custom-Header".to_string(), "custom-value".to_string());

        let monitoring_config = MonitoringConfig {
            endpoint: "https://otel-collector.example.com:4318".to_string(),
            headers,
            logs_uri: "/v1/logs".to_string(),
            tls_enabled: true,
            tls_verify: false,
        };

        let build_config = BuildConfig {
            image: "ubuntu:20.04".to_string(),
            script: "echo 'Hello World'".to_string(),
            environment: HashMap::new(),
            timeout_seconds: 300,
            compute_type: ComputeType::Medium,
            monitoring: Some(monitoring_config.clone()),
        };

        assert_eq!(build_config.image, "ubuntu:20.04");
        assert_eq!(build_config.script, "echo 'Hello World'");
        assert_eq!(build_config.timeout_seconds, 300);
        assert_eq!(build_config.compute_type, ComputeType::Medium);
        assert!(build_config.monitoring.is_some());

        let monitoring = build_config.monitoring.unwrap();
        assert_eq!(
            monitoring.endpoint,
            "https://otel-collector.example.com:4318"
        );
        assert_eq!(monitoring.logs_uri, "/v1/logs");
        assert!(monitoring.tls_enabled);
        assert!(!monitoring.tls_verify);
        assert_eq!(monitoring.headers.len(), 2);
        assert_eq!(
            monitoring.headers.get("Authorization"),
            Some(&"Bearer token123".to_string())
        );
        assert_eq!(
            monitoring.headers.get("X-Custom-Header"),
            Some(&"custom-value".to_string())
        );
    }

    #[test]
    fn test_monitoring_config_defaults() {
        let monitoring_config = MonitoringConfig {
            endpoint: "https://otel-collector.example.com:4318".to_string(),
            headers: HashMap::new(),
            logs_uri: "/v1/logs".to_string(),
            tls_enabled: true,
            tls_verify: true,
        };

        assert_eq!(monitoring_config.logs_uri, "/v1/logs");
        assert!(monitoring_config.tls_enabled);
        assert!(monitoring_config.tls_verify);
        assert!(monitoring_config.headers.is_empty());
    }
}
