use crate::error::{ErrorData, Result};
use crate::resource::{ResourceDefinition, ResourceOutputsDefinition, ResourceRef, ResourceType};
use crate::resources::{
    ComputeCluster, ExposeProtocol, HealthCheck, PublicEndpoint, ResourceSpec, ToolchainConfig,
};
use crate::LoadBalancerEndpoint;
use alien_error::AlienError;
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::fmt::Debug;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum DaemonCode {
    #[serde(rename_all = "camelCase")]
    Image { image: String },
    #[serde(rename_all = "camelCase")]
    Source {
        src: String,
        toolchain: ToolchainConfig,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(start_fn = new)]
pub struct Daemon {
    #[builder(start_fn)]
    pub id: String,
    #[builder(field)]
    pub links: Vec<ResourceRef>,
    /// Public endpoints exposed by the daemon.
    #[builder(field)]
    pub ports: Vec<PublicEndpoint>,
    /// HTTP health check for public daemon endpoint load balancers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check: Option<HealthCheck>,
    /// ComputeCluster resource ID that this daemon runs on for Horizon-backed
    /// cloud platforms. Kubernetes and Local runtimes ignore this field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster: Option<String>,
    pub permissions: String,
    pub code: DaemonCode,
    /// CPU resource requirements for each daemon instance.
    #[builder(default = default_daemon_cpu())]
    #[serde(default = "default_daemon_cpu")]
    pub cpu: ResourceSpec,
    /// Memory resource requirements for each daemon instance.
    #[builder(default = default_daemon_memory())]
    #[serde(default = "default_daemon_memory")]
    pub memory: ResourceSpec,
    /// Capacity group/pool to run on for backends that expose machine pools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool: Option<String>,
    /// Command to override the image default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
    #[builder(default)]
    #[serde(default)]
    pub environment: HashMap<String, String>,
    #[builder(default = default_commands_enabled())]
    #[serde(default = "default_commands_enabled")]
    #[cfg_attr(feature = "openapi", schema(default = default_commands_enabled))]
    pub commands_enabled: bool,
}

impl Daemon {
    pub const RESOURCE_TYPE: ResourceType = ResourceType::from_static("daemon");

    pub fn get_permissions(&self) -> &str {
        &self.permissions
    }

    fn validate_ports(&self) -> Result<()> {
        if self.ports.len() > 1 {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "at most one daemon public endpoint is currently supported".to_string(),
            }));
        }

        for endpoint in &self.ports {
            endpoint.validate_for_resource(&self.id)?;
            if endpoint.protocol != ExposeProtocol::Http {
                return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                    resource_id: self.id.clone(),
                    reason: "daemon public endpoints currently support only HTTP".to_string(),
                }));
            }
        }

        Ok(())
    }
}

fn default_commands_enabled() -> bool {
    false
}

fn default_daemon_cpu() -> ResourceSpec {
    ResourceSpec {
        min: "0.1".to_string(),
        desired: "0.1".to_string(),
    }
}

fn default_daemon_memory() -> ResourceSpec {
    ResourceSpec {
        min: "128Mi".to_string(),
        desired: "128Mi".to_string(),
    }
}

impl<S: daemon_builder::State> DaemonBuilder<S> {
    pub fn link<R: ?Sized>(mut self, resource: &R) -> Self
    where
        for<'a> &'a R: Into<ResourceRef>,
    {
        let resource_ref: ResourceRef = resource.into();
        self.links.push(resource_ref);
        self
    }

    pub fn expose_port(mut self, port: u16, protocol: ExposeProtocol) -> Self {
        self.ports.push(PublicEndpoint {
            port,
            protocol,
            host_label: None,
            wildcard_subdomains: false,
        });
        self
    }

    pub fn public_endpoint(mut self, endpoint: PublicEndpoint) -> Self {
        self.ports.push(endpoint);
        self
    }
}

impl ResourceDefinition for Daemon {
    fn get_resource_type(&self) -> ResourceType {
        Self::RESOURCE_TYPE
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn get_dependencies(&self) -> Vec<ResourceRef> {
        let mut dependencies = self.links.clone();
        if let Some(cluster) = &self.cluster {
            dependencies.push(ResourceRef::new(
                ComputeCluster::RESOURCE_TYPE,
                cluster.clone(),
            ));
        }
        dependencies
    }

    fn get_permissions(&self) -> Option<&str> {
        Some(&self.permissions)
    }

    fn validate_update(&self, new_config: &dyn ResourceDefinition) -> Result<()> {
        let new_daemon = new_config
            .as_any()
            .downcast_ref::<Daemon>()
            .ok_or_else(|| {
                AlienError::new(ErrorData::UnexpectedResourceType {
                    resource_id: self.id.clone(),
                    expected: Self::RESOURCE_TYPE,
                    actual: new_config.get_resource_type(),
                })
            })?;

        if self.id != new_daemon.id {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'id' field is immutable".to_string(),
            }));
        }

        self.validate_ports()?;
        new_daemon.validate_ports()?;

        if self.ports != new_daemon.ports {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'ports' field is immutable".to_string(),
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
        other.as_any().downcast_ref::<Daemon>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DaemonOutputs {
    pub daemon_name: String,
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_balancer_endpoint: Option<LoadBalancerEndpoint>,
}

impl ResourceOutputsDefinition for DaemonOutputs {
    fn get_resource_type(&self) -> ResourceType {
        Daemon::RESOURCE_TYPE.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceOutputsDefinition> {
        Box::new(self.clone())
    }

    fn outputs_eq(&self, other: &dyn ResourceOutputsDefinition) -> bool {
        other.as_any().downcast_ref::<DaemonOutputs>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_serializes_with_resource_type() {
        let daemon = Daemon::new("endpoint-agent".to_string())
            .code(DaemonCode::Source {
                src: "./agent".to_string(),
                toolchain: ToolchainConfig::Rust {
                    binary_name: "agent".to_string(),
                },
            })
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();

        let resource = crate::Resource::new(daemon);
        let json = serde_json::to_value(&resource).expect("daemon should serialize");
        assert_eq!(json["type"], "daemon");

        let roundtrip: crate::Resource =
            serde_json::from_value(json).expect("daemon should deserialize");
        assert_eq!(roundtrip.resource_type().as_ref(), "daemon");
    }

    #[test]
    fn daemon_accepts_one_public_http_endpoint() {
        let daemon = Daemon::new("gateway".to_string())
            .code(DaemonCode::Image {
                image: "gateway:latest".to_string(),
            })
            .public_endpoint(PublicEndpoint {
                port: 8080,
                protocol: ExposeProtocol::Http,
                host_label: Some("public".to_string()),
                wildcard_subdomains: true,
            })
            .permissions("gateway".to_string())
            .build();

        assert!(daemon.validate_ports().is_ok());
        assert_eq!(daemon.ports.len(), 1);
        assert_eq!(daemon.ports[0].host_label.as_deref(), Some("public"));
        assert!(daemon.ports[0].wildcard_subdomains);
    }

    #[test]
    fn daemon_rejects_multiple_or_non_http_public_endpoints() {
        let multiple = Daemon::new("gateway".to_string())
            .code(DaemonCode::Image {
                image: "gateway:latest".to_string(),
            })
            .expose_port(8080, ExposeProtocol::Http)
            .expose_port(9090, ExposeProtocol::Http)
            .permissions("gateway".to_string())
            .build();
        assert!(multiple.validate_ports().is_err());

        let tcp = Daemon::new("gateway".to_string())
            .code(DaemonCode::Image {
                image: "gateway:latest".to_string(),
            })
            .expose_port(8080, ExposeProtocol::Tcp)
            .permissions("gateway".to_string())
            .build();
        assert!(tcp.validate_ports().is_err());
    }
}
