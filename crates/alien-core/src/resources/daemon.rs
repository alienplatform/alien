use crate::error::{ErrorData, Result};
use crate::resource::{ResourceDefinition, ResourceOutputsDefinition, ResourceRef, ResourceType};
use crate::resources::{ComputeCluster, ResourceSpec, ToolchainConfig};
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
    /// When true, the auto-generated ComputeCluster for this daemon is
    /// constrained to instance types that expose nested virtualization
    /// (VT-x/EPT) to guest VMs. Required by workloads that boot QEMU/KVM
    /// inside the container (e.g. bear-agent's sandboxes).
    /// Defaults to false; ignored on platforms whose backends don't pick
    /// instance types (Kubernetes, Local).
    #[builder(default)]
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub nested_virtualization: bool,
}

impl Daemon {
    pub const RESOURCE_TYPE: ResourceType = ResourceType::from_static("daemon");

    pub fn get_permissions(&self) -> &str {
        &self.permissions
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
}
