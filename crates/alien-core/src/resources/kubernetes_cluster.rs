//! KubernetesCluster resource for Kubernetes runtime substrates.

use crate::error::{ErrorData, Result};
use crate::resource::{ResourceDefinition, ResourceOutputsDefinition, ResourceRef};
use crate::ResourceType;
use alien_error::AlienError;
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::any::Any;

/// Kubernetes provider backing the runtime substrate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub enum KubernetesClusterProvider {
    Eks,
    Gke,
    Aks,
    Generic,
}

/// Ownership model for the Kubernetes cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub enum KubernetesClusterOwnership {
    Managed,
    Existing,
    External,
}

/// How Alien should heartbeat this Kubernetes substrate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub enum KubernetesHeartbeatMode {
    KubernetesApi,
    KubernetesApiAndCloudMetadata,
    Disabled,
}

/// Optional provider-specific identity for a cloud-backed Kubernetes cluster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KubernetesCloudReference {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_group: Option<String>,
}

/// Runtime substrate for Kubernetes deployments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(start_fn = new)]
pub struct KubernetesCluster {
    #[builder(start_fn)]
    pub id: String,
    pub provider: KubernetesClusterProvider,
    pub ownership: KubernetesClusterOwnership,
    pub namespace: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud: Option<KubernetesCloudReference>,
    pub heartbeat_mode: KubernetesHeartbeatMode,
}

impl KubernetesCluster {
    pub const RESOURCE_TYPE: ResourceType = ResourceType::from_static("kubernetes-cluster");

    pub fn id(&self) -> &str {
        &self.id
    }
}

/// Outputs produced once the Kubernetes substrate is ready for workloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct KubernetesClusterOutputs {
    pub provider: KubernetesClusterProvider,
    pub ownership: KubernetesClusterOwnership,
    pub namespace: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_id: Option<String>,
    pub kubernetes_api_reachable: bool,
    pub namespace_ready: bool,
    pub rbac_ready: bool,
    pub agent_ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_metadata_ready: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
}

impl ResourceOutputsDefinition for KubernetesClusterOutputs {
    fn get_resource_type(&self) -> ResourceType {
        KubernetesCluster::RESOURCE_TYPE
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceOutputsDefinition> {
        Box::new(self.clone())
    }

    fn outputs_eq(&self, other: &dyn ResourceOutputsDefinition) -> bool {
        other.as_any().downcast_ref::<KubernetesClusterOutputs>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

impl ResourceDefinition for KubernetesCluster {
    fn get_resource_type(&self) -> ResourceType {
        Self::RESOURCE_TYPE
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn get_dependencies(&self) -> Vec<ResourceRef> {
        Vec::new()
    }

    fn validate_update(&self, new_config: &dyn ResourceDefinition) -> Result<()> {
        let new_cluster = new_config
            .as_any()
            .downcast_ref::<KubernetesCluster>()
            .ok_or_else(|| {
                AlienError::new(ErrorData::UnexpectedResourceType {
                    resource_id: self.id.clone(),
                    expected: Self::RESOURCE_TYPE,
                    actual: new_config.get_resource_type(),
                })
            })?;

        if self != new_cluster {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "KubernetesCluster is a frozen runtime substrate and cannot be changed during runtime updates".to_string(),
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
        other.as_any().downcast_ref::<KubernetesCluster>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_external_cluster_serializes_as_resource() {
        let cluster = KubernetesCluster::new("kubernetes".to_string())
            .provider(KubernetesClusterProvider::Generic)
            .ownership(KubernetesClusterOwnership::External)
            .namespace("default".to_string())
            .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApi)
            .build();

        let resource = crate::Resource::new(cluster);
        let value = serde_json::to_value(&resource).unwrap();

        assert_eq!(value["type"], "kubernetes-cluster");
        assert_eq!(value["provider"], "generic");
        assert_eq!(value["ownership"], "external");
    }
}
