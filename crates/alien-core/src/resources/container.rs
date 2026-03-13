//! Container resource for long-running container workloads.
//!
//! A Container represents a deployable unit that runs on a ContainerCluster.
//! It defines the container image, resource requirements, scaling configuration,
//! and networking settings.
//!
//! Containers are orchestrated by Horizon, which handles:
//! - Replica scheduling across machines
//! - Autoscaling based on CPU, memory, or HTTP metrics
//! - Health checking and crash recovery
//! - Service discovery and internal networking
//! - Load balancer registration for public-facing containers

use crate::error::{ErrorData, Result};
use crate::resource::{ResourceDefinition, ResourceOutputsDefinition, ResourceRef, ResourceType};
use crate::resources::{ContainerCluster, ToolchainConfig};
use crate::LoadBalancerEndpoint;
use alien_error::AlienError;
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::fmt::Debug;

/// Specifies the source of the container's executable code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ContainerCode {
    /// Container image reference
    #[serde(rename_all = "camelCase")]
    Image {
        /// Container image (e.g., `postgres:16`, `ghcr.io/myorg/myimage:latest`)
        image: String,
    },
    /// Source code to be built
    #[serde(rename_all = "camelCase")]
    Source {
        /// The source directory to build from
        src: String,
        /// Toolchain configuration with type-safe options
        toolchain: ToolchainConfig,
    },
}

/// Resource specification with min/desired values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ResourceSpec {
    /// Minimum resource allocation
    pub min: String,
    /// Desired resource allocation (used by scheduler)
    pub desired: String,
}

/// GPU specification for a container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ContainerGpuSpec {
    /// GPU type identifier (e.g., "nvidia-a100", "nvidia-t4")
    #[serde(rename = "type")]
    pub gpu_type: String,
    /// Number of GPUs required (1-8)
    pub count: u32,
}

/// Persistent storage configuration for stateful containers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct PersistentStorage {
    /// Storage size (e.g., "100Gi", "500Gi")
    pub size: String,
    /// Mount path inside the container
    pub mount_path: String,
    /// Storage type (e.g., "gp3", "io2" for AWS, "pd-ssd" for GCP)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_type: Option<String>,
    /// IOPS (for storage types that support it)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iops: Option<u32>,
    /// Throughput in MiB/s (for storage types that support it)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throughput: Option<u32>,
}

/// Autoscaling configuration for stateless containers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ContainerAutoscaling {
    /// Minimum replicas (always running)
    pub min: u32,
    /// Initial desired replicas at container creation
    pub desired: u32,
    /// Maximum replicas under load
    pub max: u32,
    /// Target CPU utilization percentage for scaling (default: 70%)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_cpu_percent: Option<f64>,
    /// Target memory utilization percentage for scaling (default: 80%)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_memory_percent: Option<f64>,
    /// Target in-flight HTTP requests per replica
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_http_in_flight_per_replica: Option<u32>,
    /// Maximum acceptable p95 HTTP latency in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_http_p95_latency_ms: Option<f64>,
}

/// HTTP health check configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HealthCheck {
    /// HTTP endpoint path to check (e.g., "/health", "/ready")
    #[serde(default = "default_health_path")]
    pub path: String,
    /// Port to check (defaults to container port if not specified)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// HTTP method to use for health check
    #[serde(default = "default_health_method")]
    pub method: String,
    /// Request timeout in seconds (1-5)
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u32,
    /// Number of consecutive failures before marking replica unhealthy
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: u32,
}

fn default_health_path() -> String {
    "/health".to_string()
}

fn default_health_method() -> String {
    "GET".to_string()
}

fn default_timeout_seconds() -> u32 {
    1
}

fn default_failure_threshold() -> u32 {
    3
}

/// Protocol for exposed ports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum ExposeProtocol {
    /// HTTP/HTTPS with TLS termination at load balancer
    Http,
    /// TCP passthrough without TLS
    Tcp,
}

/// Container port configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ContainerPort {
    /// Port number
    pub port: u16,
    /// Optional exposure protocol (if None, port is internal-only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expose: Option<ExposeProtocol>,
}

/// Container resource for running long-running container workloads.
///
/// A Container defines a deployable unit that runs on a ContainerCluster.
/// Horizon handles scheduling replicas across machines, autoscaling based on
/// various metrics, and service discovery.
///
/// ## Example
///
/// ```rust
/// use alien_core::{Container, ContainerCode, ResourceSpec, ContainerAutoscaling, ContainerPort, ExposeProtocol};
///
/// let container = Container::new("api".to_string())
///     .cluster("compute".to_string())
///     .code(ContainerCode::Image {
///         image: "myapp:latest".to_string(),
///     })
///     .cpu(ResourceSpec { min: "0.5".to_string(), desired: "1".to_string() })
///     .memory(ResourceSpec { min: "512Mi".to_string(), desired: "1Gi".to_string() })
///     .port(8080)
///     .expose_port(8080, ExposeProtocol::Http)
///     .autoscaling(ContainerAutoscaling {
///         min: 2,
///         desired: 3,
///         max: 10,
///         target_cpu_percent: Some(70.0),
///         target_memory_percent: None,
///         target_http_in_flight_per_replica: Some(100),
///         max_http_p95_latency_ms: None,
///     })
///     .permissions("container-execution".to_string())
///     .build();
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(start_fn = new)]
pub struct Container {
    /// Unique identifier for the container.
    /// Must be DNS-compatible: lowercase alphanumeric with hyphens.
    #[builder(start_fn)]
    pub id: String,

    /// Resource links (dependencies)
    #[builder(field)]
    pub links: Vec<ResourceRef>,

    /// Container ports to expose (at least one required)
    #[builder(field)]
    pub ports: Vec<ContainerPort>,

    /// ContainerCluster resource ID that this container runs on.
    /// If None, will be auto-assigned by ContainerClusterMutation at deployment time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster: Option<String>,

    /// Container code (image or source)
    pub code: ContainerCode,

    /// CPU resource requirements
    pub cpu: ResourceSpec,

    /// Memory resource requirements (must use Ki/Mi/Gi/Ti suffix)
    pub memory: ResourceSpec,

    /// GPU requirements (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<ContainerGpuSpec>,

    /// Ephemeral storage requirement (e.g., "10Gi")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ephemeral_storage: Option<String>,

    /// Persistent storage configuration (only for stateful containers)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persistent_storage: Option<PersistentStorage>,

    /// Fixed replica count (for stateful containers or stateless without autoscaling)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replicas: Option<u32>,

    /// Autoscaling configuration (only for stateless containers)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autoscaling: Option<ContainerAutoscaling>,

    /// Whether container is stateful (gets stable ordinals, optional persistent volumes)
    #[builder(default = false)]
    #[serde(default)]
    pub stateful: bool,

    /// Environment variables
    #[builder(default)]
    #[serde(default)]
    pub environment: HashMap<String, String>,

    /// Capacity group to run on (must exist in the cluster)
    /// If not specified, containers are scheduled to any available group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool: Option<String>,

    /// Permission profile name
    pub permissions: String,

    /// Health check configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check: Option<HealthCheck>,

    /// Command to override image default
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
}

impl Container {
    /// The resource type identifier for Container
    pub const RESOURCE_TYPE: ResourceType = ResourceType::from_static("container");

    /// Returns the container's unique identifier.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the permission profile name for this container.
    pub fn get_permissions(&self) -> &str {
        &self.permissions
    }

    /// Returns true if this container is stateless (not stateful).
    pub fn is_stateless(&self) -> bool {
        !self.stateful
    }

    /// Validates the ports configuration.
    fn validate_ports(&self) -> Result<()> {
        // Ports cannot be empty
        if self.ports.is_empty() {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "at least one port must be specified".to_string(),
            }));
        }

        // At most one HTTP port is allowed
        let http_ports: Vec<_> = self
            .ports
            .iter()
            .filter(|p| p.expose == Some(ExposeProtocol::Http))
            .collect();

        if http_ports.len() > 1 {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "at most one port can be exposed with HTTP protocol (multiple TCP ports are allowed)".to_string(),
            }));
        }

        Ok(())
    }
}

impl<S: container_builder::State> ContainerBuilder<S> {
    /// Links the container to another resource with specified permissions.
    pub fn link<R: ?Sized>(mut self, resource: &R) -> Self
    where
        for<'a> &'a R: Into<ResourceRef>,
    {
        let resource_ref: ResourceRef = resource.into();
        self.links.push(resource_ref);
        self
    }

    /// Adds an internal-only port to the container.
    pub fn port(mut self, port: u16) -> Self {
        self.ports.push(ContainerPort { port, expose: None });
        self
    }

    /// Exposes a specific port publicly via load balancer.
    pub fn expose_port(mut self, port: u16, protocol: ExposeProtocol) -> Self {
        // Find existing port or add new one
        if let Some(existing) = self.ports.iter_mut().find(|p| p.port == port) {
            existing.expose = Some(protocol);
        } else {
            self.ports.push(ContainerPort {
                port,
                expose: Some(protocol),
            });
        }
        self
    }
}

/// Container status in Horizon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum ContainerStatus {
    /// Waiting for replicas to start
    Pending,
    /// Min replicas healthy and serving
    Running,
    /// Manually stopped
    Stopped,
    /// Something is wrong — see statusReason/statusMessage; scheduler keeps retrying.
    /// Covers all failure modes: crash-looping, unschedulable, replica failures, etc.
    Failing,
}

/// Status of a single container replica.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ReplicaStatus {
    /// Replica ID (e.g., "api-0", "api-1")
    pub replica_id: String,
    /// Ordinal (for stateful containers)
    pub ordinal: Option<u32>,
    /// Machine ID the replica is running on
    pub machine_id: Option<String>,
    /// Whether the replica is healthy
    pub healthy: bool,
    /// Container IP address (for service discovery)
    pub container_ip: Option<String>,
}

/// Outputs generated by a successfully provisioned Container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ContainerOutputs {
    /// Container name in Horizon
    pub name: String,
    /// Current container status
    pub status: ContainerStatus,
    /// Number of current replicas
    pub current_replicas: u32,
    /// Desired number of replicas
    pub desired_replicas: u32,
    /// Internal DNS name (e.g., "api.svc")
    pub internal_dns: String,
    /// Public URL (if exposed publicly)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Status of each replica
    pub replicas: Vec<ReplicaStatus>,
    /// Load balancer endpoint information for DNS management (optional).
    /// Used by the DNS controller to create custom domain mappings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_balancer_endpoint: Option<LoadBalancerEndpoint>,
}

#[typetag::serde(name = "container")]
impl ResourceOutputsDefinition for ContainerOutputs {
    fn resource_type() -> ResourceType {
        Container::RESOURCE_TYPE.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceOutputsDefinition> {
        Box::new(self.clone())
    }

    fn outputs_eq(&self, other: &dyn ResourceOutputsDefinition) -> bool {
        other.as_any().downcast_ref::<ContainerOutputs>() == Some(self)
    }
}

#[typetag::serde(name = "container")]
impl ResourceDefinition for Container {
    fn resource_type() -> ResourceType {
        Self::RESOURCE_TYPE.clone()
    }

    fn get_resource_type(&self) -> ResourceType {
        Self::resource_type()
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn get_dependencies(&self) -> Vec<ResourceRef> {
        let mut deps = self.links.clone();
        // Add dependency on the container cluster if explicitly specified.
        // If None, ContainerClusterMutation will auto-assign at deployment time.
        if let Some(cluster) = &self.cluster {
            deps.push(ResourceRef::new(
                ContainerCluster::RESOURCE_TYPE.clone(),
                cluster,
            ));
        }
        deps
    }

    fn get_permissions(&self) -> Option<&str> {
        Some(&self.permissions)
    }

    fn validate_update(&self, new_config: &dyn ResourceDefinition) -> Result<()> {
        let new_container = new_config
            .as_any()
            .downcast_ref::<Container>()
            .ok_or_else(|| {
                AlienError::new(ErrorData::UnexpectedResourceType {
                    resource_id: self.id.clone(),
                    expected: Self::RESOURCE_TYPE,
                    actual: new_config.get_resource_type(),
                })
            })?;

        // Validate the new config's ports
        new_container.validate_ports()?;

        if self.id != new_container.id {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'id' field is immutable".to_string(),
            }));
        }

        // Cluster is immutable
        if self.cluster != new_container.cluster {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'cluster' field is immutable".to_string(),
            }));
        }

        // Stateful is immutable
        if self.stateful != new_container.stateful {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'stateful' field is immutable".to_string(),
            }));
        }

        // Ports are immutable (requires load balancer reconfiguration)
        if self.ports != new_container.ports {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'ports' field is immutable".to_string(),
            }));
        }

        // Pool (capacity group) is immutable
        if self.pool != new_container.pool {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'pool' field is immutable".to_string(),
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
        other.as_any().downcast_ref::<Container>() == Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_creation_with_autoscaling() {
        let container = Container::new("api".to_string())
            .cluster("compute".to_string())
            .code(ContainerCode::Image {
                image: "myapp:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .expose_port(8080, ExposeProtocol::Http)
            .autoscaling(ContainerAutoscaling {
                min: 2,
                desired: 3,
                max: 10,
                target_cpu_percent: Some(70.0),
                target_memory_percent: None,
                target_http_in_flight_per_replica: Some(100),
                max_http_p95_latency_ms: None,
            })
            .permissions("container-execution".to_string())
            .build();

        assert_eq!(container.id(), "api");
        assert_eq!(container.cluster, Some("compute".to_string()));
        assert!(!container.stateful);
        assert!(container.autoscaling.is_some());
        assert_eq!(container.ports.len(), 1);
        assert_eq!(container.ports[0].port, 8080);
    }

    #[test]
    fn test_stateful_container_with_storage() {
        let container = Container::new("postgres".to_string())
            .cluster("compute".to_string())
            .code(ContainerCode::Image {
                image: "postgres:16".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "2".to_string(),
            })
            .memory(ResourceSpec {
                min: "2Gi".to_string(),
                desired: "4Gi".to_string(),
            })
            .port(5432)
            .stateful(true)
            .replicas(1)
            .persistent_storage(PersistentStorage {
                size: "100Gi".to_string(),
                mount_path: "/var/lib/postgresql/data".to_string(),
                storage_type: Some("gp3".to_string()),
                iops: Some(3000),
                throughput: Some(125),
            })
            .permissions("database".to_string())
            .build();

        assert_eq!(container.id(), "postgres");
        assert!(container.stateful);
        assert!(container.replicas.is_some());
        assert!(container.persistent_storage.is_some());
    }

    #[test]
    fn test_public_container() {
        let container = Container::new("frontend".to_string())
            .cluster("compute".to_string())
            .code(ContainerCode::Image {
                image: "frontend:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.25".to_string(),
                desired: "0.5".to_string(),
            })
            .memory(ResourceSpec {
                min: "256Mi".to_string(),
                desired: "512Mi".to_string(),
            })
            .port(3000)
            .expose_port(3000, ExposeProtocol::Http)
            .autoscaling(ContainerAutoscaling {
                min: 2,
                desired: 2,
                max: 20,
                target_cpu_percent: None,
                target_memory_percent: None,
                target_http_in_flight_per_replica: Some(50),
                max_http_p95_latency_ms: Some(100.0),
            })
            .health_check(HealthCheck {
                path: "/health".to_string(),
                port: None,
                method: "GET".to_string(),
                timeout_seconds: 1,
                failure_threshold: 3,
            })
            .permissions("frontend".to_string())
            .build();

        assert_eq!(container.ports[0].port, 3000);
        assert!(container.ports[0].expose.is_some());
        assert!(container.health_check.is_some());
    }

    #[test]
    fn test_container_with_links() {
        use crate::Storage;

        let storage = Storage::new("data".to_string()).build();

        let container = Container::new("worker".to_string())
            .cluster("compute".to_string())
            .code(ContainerCode::Image {
                image: "worker:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .replicas(3)
            .link(&storage)
            .permissions("worker".to_string())
            .build();

        // Should have 2 dependencies: cluster + linked storage
        let deps = container.get_dependencies();
        assert_eq!(deps.len(), 2);
    }

    #[test]
    fn test_container_validate_update_immutable_cluster() {
        let container1 = Container::new("api".to_string())
            .cluster("cluster-1".to_string())
            .code(ContainerCode::Image {
                image: "myapp:v1".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .replicas(2)
            .permissions("execution".to_string())
            .build();

        let container2 = Container::new("api".to_string())
            .cluster("cluster-2".to_string()) // Changed cluster
            .code(ContainerCode::Image {
                image: "myapp:v2".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .replicas(2)
            .permissions("execution".to_string())
            .build();

        let result = container1.validate_update(&container2);
        assert!(result.is_err());
    }

    #[test]
    fn test_container_validate_update_allowed_changes() {
        let container1 = Container::new("api".to_string())
            .cluster("compute".to_string())
            .code(ContainerCode::Image {
                image: "myapp:v1".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .replicas(2)
            .permissions("execution".to_string())
            .build();

        let container2 = Container::new("api".to_string())
            .cluster("compute".to_string())
            .code(ContainerCode::Image {
                image: "myapp:v2".to_string(), // Image can change
            })
            .cpu(ResourceSpec {
                min: "1".to_string(), // Resources can change
                desired: "2".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "2Gi".to_string(),
            })
            .port(8080)
            .replicas(5) // Replicas can change
            .permissions("execution".to_string())
            .build();

        let result = container1.validate_update(&container2);
        assert!(result.is_ok());
    }

    #[test]
    fn test_container_serialization() {
        let container = Container::new("test".to_string())
            .cluster("compute".to_string())
            .code(ContainerCode::Image {
                image: "test:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .replicas(1)
            .permissions("test".to_string())
            .build();

        let json = serde_json::to_string(&container).unwrap();
        let deserialized: Container = serde_json::from_str(&json).unwrap();
        assert_eq!(container, deserialized);
    }

    #[test]
    fn test_container_multi_port_validation() {
        // Valid: Multiple TCP ports
        let container = Container::new("multi-tcp".to_string())
            .cluster("compute".to_string())
            .code(ContainerCode::Image {
                image: "test:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .expose_port(8080, ExposeProtocol::Tcp)
            .port(9090)
            .expose_port(9090, ExposeProtocol::Tcp)
            .replicas(1)
            .permissions("test".to_string())
            .build();

        assert!(container.validate_ports().is_ok());

        // Invalid: Multiple HTTP ports
        let invalid_container = Container::new("multi-http".to_string())
            .cluster("compute".to_string())
            .code(ContainerCode::Image {
                image: "test:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .expose_port(8080, ExposeProtocol::Http)
            .port(9090)
            .expose_port(9090, ExposeProtocol::Http)
            .replicas(1)
            .permissions("test".to_string())
            .build();

        assert!(invalid_container.validate_ports().is_err());
    }

    #[test]
    fn test_container_empty_ports_validation() {
        // Build container with at least one port, then manually clear for testing
        let mut container = Container::new("no-ports".to_string())
            .cluster("compute".to_string())
            .code(ContainerCode::Image {
                image: "test:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080) // Need at least one port to build
            .replicas(1)
            .permissions("test".to_string())
            .build();

        // Clear ports to test validation
        container.ports.clear();
        assert!(container.validate_ports().is_err());
    }
}
