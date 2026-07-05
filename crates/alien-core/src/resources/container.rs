//! Container resource for long-running container workloads.
//!
//! A Container represents a deployable unit that runs on a ComputeCluster.
//! It defines the container image, resource requirements, scaling configuration,
//! and networking settings.
//!
//! Containers are orchestrated by the managed container backend, which handles:
//! - Replica scheduling across machines
//! - Autoscaling based on CPU, memory, or HTTP metrics
//! - Health checking and crash recovery
//! - Service discovery and internal networking
//! - Load balancer registration for public-facing containers

use crate::error::{ErrorData, Result};
use crate::resource::{ResourceDefinition, ResourceOutputsDefinition, ResourceRef, ResourceType};
use crate::resources::{ComputeCluster, PublicEndpoint, PublicEndpointOutput, ToolchainConfig};
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

/// Container port configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ContainerPort {
    /// Port number
    pub port: u16,
}

/// Container resource for running long-running container workloads.
///
/// A Container defines a deployable unit that runs on a ComputeCluster.
/// The managed container backend handles scheduling replicas across machines,
/// autoscaling based on various metrics, and service discovery.
///
/// ## Example
///
/// ```rust
/// use alien_core::{Container, ContainerCode, ResourceSpec, ContainerAutoscaling, PublicEndpoint, ExposeProtocol};
///
/// let container = Container::new("api".to_string())
///     .cluster("compute".to_string())
///     .code(ContainerCode::Image {
///         image: "myapp:latest".to_string(),
///     })
///     .cpu(ResourceSpec { min: "0.5".to_string(), desired: "1".to_string() })
///     .memory(ResourceSpec { min: "512Mi".to_string(), desired: "1Gi".to_string() })
///     .port(8080)
///     .public_endpoint(PublicEndpoint {
///         name: "api".to_string(),
///         port: 8080,
///         protocol: ExposeProtocol::Http,
///         host_label: None,
///         wildcard_subdomains: false,
///     })
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

    /// Internal container ports (at least one required).
    #[builder(field)]
    pub ports: Vec<ContainerPort>,

    /// Public endpoints exposed by the container.
    #[builder(field)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub public_endpoints: Vec<PublicEndpoint>,

    /// ComputeCluster resource ID that this container runs on.
    /// If None, will be auto-assigned by ComputeClusterMutation at deployment time.
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

    /// Whether the container can receive remote commands via the Commands protocol.
    /// When enabled, the container polls the manager for pending commands and executes registered handlers.
    #[builder(default = default_commands_enabled())]
    #[serde(default = "default_commands_enabled")]
    #[cfg_attr(feature = "openapi", schema(default = default_commands_enabled))]
    pub commands_enabled: bool,

    /// Grace period in seconds for stopping replicas during updates, drains, and deletes.
    ///
    /// When omitted, the runtime backend applies its default. Valid values are
    /// 1 second through 24 hours.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(minimum = 1, maximum = 86400))]
    pub stop_grace_period_seconds: Option<u32>,
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

    /// Validates the public endpoint configuration.
    fn validate_public_endpoints(&self) -> Result<()> {
        let mut endpoint_names = std::collections::HashSet::new();
        let mut backend_ports = std::collections::HashSet::new();

        for endpoint in &self.public_endpoints {
            endpoint.validate_for_resource(&self.id)?;

            if !endpoint_names.insert(endpoint.name.as_str()) {
                return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                    resource_id: self.id.clone(),
                    reason: format!("duplicate public endpoint name '{}'", endpoint.name),
                }));
            }

            backend_ports.insert(endpoint.port);

            if !self.ports.iter().any(|port| port.port == endpoint.port) {
                return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                    resource_id: self.id.clone(),
                    reason: format!(
                        "public endpoint '{}' references undeclared port {}",
                        endpoint.name, endpoint.port
                    ),
                }));
            }
        }

        if backend_ports.len() > 1 {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason:
                    "public endpoints on one container must currently route to the same backend port"
                        .to_string(),
            }));
        }

        Ok(())
    }
}

fn default_commands_enabled() -> bool {
    false
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
        self.ports.push(ContainerPort { port });
        self
    }

    /// Exposes a named public endpoint.
    pub fn public_endpoint(mut self, endpoint: PublicEndpoint) -> Self {
        if !self.ports.iter().any(|p| p.port == endpoint.port) {
            self.ports.push(ContainerPort {
                port: endpoint.port,
            });
        }
        self.public_endpoints.push(endpoint);
        self
    }
}

/// Container status in the managed container backend.
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
    /// Container name in the managed container backend
    pub name: String,
    /// Current container status
    pub status: ContainerStatus,
    /// Number of current replicas
    pub current_replicas: u32,
    /// Desired number of replicas
    pub desired_replicas: u32,
    /// Internal DNS name (e.g., "api.svc")
    pub internal_dns: String,
    /// Public endpoints resolved for this container.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub public_endpoints: HashMap<String, PublicEndpointOutput>,
    /// Status of each replica
    pub replicas: Vec<ReplicaStatus>,
}

impl ResourceOutputsDefinition for ContainerOutputs {
    fn get_resource_type(&self) -> ResourceType {
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

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

impl ResourceDefinition for Container {
    fn get_resource_type(&self) -> ResourceType {
        Self::RESOURCE_TYPE
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn get_dependencies(&self) -> Vec<ResourceRef> {
        let mut deps = self.links.clone();
        // Add dependency on the container cluster if explicitly specified.
        // If None, ComputeClusterMutation will auto-assign at deployment time.
        if let Some(cluster) = &self.cluster {
            deps.push(ResourceRef::new(
                ComputeCluster::RESOURCE_TYPE.clone(),
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

        // Validate the new config's public endpoints.
        new_container.validate_public_endpoints()?;

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

        if self.public_endpoints != new_container.public_endpoints {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'publicEndpoints' field is immutable".to_string(),
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

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::ExposeProtocol;

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
            .public_endpoint(PublicEndpoint {
                name: "api".to_string(),
                port: 8080,
                protocol: ExposeProtocol::Http,
                host_label: None,
                wildcard_subdomains: false,
            })
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
    fn container_serializes_stop_grace_period_when_set() {
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
            .permissions("container-execution".to_string())
            .stop_grace_period_seconds(21_600)
            .build();

        let json = serde_json::to_value(&container).expect("container should serialize");
        assert_eq!(json["stopGracePeriodSeconds"], 21_600);
    }

    #[test]
    fn container_omits_stop_grace_period_when_absent() {
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
            .permissions("container-execution".to_string())
            .build();

        let json = serde_json::to_value(&container).expect("container should serialize");
        assert!(json.get("stopGracePeriodSeconds").is_none());
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
            .public_endpoint(PublicEndpoint {
                name: "web".to_string(),
                port: 3000,
                protocol: ExposeProtocol::Http,
                host_label: None,
                wildcard_subdomains: false,
            })
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
        assert_eq!(container.public_endpoints[0].name, "web");
        assert!(container.health_check.is_some());
    }

    #[test]
    fn test_public_container_endpoint_options() {
        let container = Container::new("router".to_string())
            .cluster("compute".to_string())
            .code(ContainerCode::Image {
                image: "router:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.25".to_string(),
                desired: "0.5".to_string(),
            })
            .memory(ResourceSpec {
                min: "256Mi".to_string(),
                desired: "512Mi".to_string(),
            })
            .public_endpoint(PublicEndpoint {
                name: "gateway".to_string(),
                port: 8080,
                protocol: ExposeProtocol::Http,
                host_label: Some("gateway".to_string()),
                wildcard_subdomains: true,
            })
            .permissions("router".to_string())
            .build();

        assert!(container.validate_public_endpoints().is_ok());
        assert_eq!(container.ports.len(), 1);
        assert_eq!(container.public_endpoints.len(), 1);
        assert_eq!(
            container.public_endpoints[0].host_label.as_deref(),
            Some("gateway")
        );
        assert!(container.public_endpoints[0].wildcard_subdomains);
    }

    #[test]
    fn test_public_container_rejects_invalid_host_label() {
        let container = Container::new("router".to_string())
            .cluster("compute".to_string())
            .code(ContainerCode::Image {
                image: "router:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.25".to_string(),
                desired: "0.5".to_string(),
            })
            .memory(ResourceSpec {
                min: "256Mi".to_string(),
                desired: "512Mi".to_string(),
            })
            .public_endpoint(PublicEndpoint {
                name: "gateway".to_string(),
                port: 8080,
                protocol: ExposeProtocol::Http,
                host_label: Some("bad.label".to_string()),
                wildcard_subdomains: true,
            })
            .permissions("router".to_string())
            .build();

        assert!(container.validate_public_endpoints().is_err());
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
    fn test_container_multi_endpoint_validation() {
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
            .public_endpoint(PublicEndpoint {
                name: "api".to_string(),
                port: 8080,
                protocol: ExposeProtocol::Http,
                host_label: None,
                wildcard_subdomains: false,
            })
            .public_endpoint(PublicEndpoint {
                name: "wildcard".to_string(),
                port: 8080,
                protocol: ExposeProtocol::Http,
                host_label: Some("wildcard".to_string()),
                wildcard_subdomains: true,
            })
            .replicas(1)
            .permissions("test".to_string())
            .build();

        assert!(container.validate_public_endpoints().is_ok());

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
            .port(9090)
            .public_endpoint(PublicEndpoint {
                name: "api".to_string(),
                port: 8080,
                protocol: ExposeProtocol::Http,
                host_label: None,
                wildcard_subdomains: false,
            })
            .public_endpoint(PublicEndpoint {
                name: "admin".to_string(),
                port: 9090,
                protocol: ExposeProtocol::Http,
                host_label: None,
                wildcard_subdomains: false,
            })
            .replicas(1)
            .permissions("test".to_string())
            .build();

        assert!(invalid_container.validate_public_endpoints().is_err());
    }

    #[test]
    fn test_container_empty_ports_validation() {
        let container = Container::new("no-ports".to_string())
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
            .replicas(1)
            .permissions("test".to_string())
            .build();

        assert!(container.validate_public_endpoints().is_ok());
    }

    #[test]
    fn test_container_commands_enabled_defaults_false() {
        let container = Container::new("no-commands".to_string())
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
            .permissions("test".to_string())
            .build();

        assert!(!container.commands_enabled);
    }

    #[test]
    fn test_container_commands_enabled_builder() {
        let container = Container::new("cmd-container".to_string())
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
            .permissions("test".to_string())
            .commands_enabled(true)
            .build();

        assert!(container.commands_enabled);
    }

    #[test]
    fn test_container_commands_enabled_serializes_camel_case() {
        let container = Container::new("cmd-container".to_string())
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
            .permissions("test".to_string())
            .commands_enabled(true)
            .build();

        let json = serde_json::to_value(&container).expect("container should serialize");
        assert_eq!(json["commandsEnabled"], true);

        let deserialized: Container =
            serde_json::from_value(json).expect("container should deserialize");
        assert_eq!(deserialized, container);
    }
}
