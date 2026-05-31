use serde::{Deserialize, Serialize};

use crate::{KubernetesClusterOwnership, KubernetesClusterProvider};

/// Application Gateway for Containers bootstrap data emitted by AKS setup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureApplicationGatewayForContainersBootstrap {
    pub alb_name: String,
    pub alb_namespace: String,
    pub association_subnet_id: String,
}

/// KubernetesCluster ImportData emitted by setup artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct KubernetesClusterImportData {
    pub provider: KubernetesClusterProvider,
    pub ownership: KubernetesClusterOwnership,
    pub namespace: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cluster_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cluster_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloud_metadata_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub azure_application_gateway_for_containers:
        Option<AzureApplicationGatewayForContainersBootstrap>,
}
