use crate::gcp::api_client::{GcpClientBase, GcpServiceConfig};
use crate::gcp::longrunning::Operation;
use crate::gcp::GcpClientConfig;
use alien_client_core::Result;
use async_trait::async_trait;
use bon::Builder;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;

#[cfg(feature = "test-utils")]
use mockall::automock;

#[derive(Debug)]
pub struct ContainerServiceConfig;

impl GcpServiceConfig for ContainerServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://container.googleapis.com/v1"
    }

    fn default_audience(&self) -> &'static str {
        "https://container.googleapis.com/"
    }

    fn service_name(&self) -> &'static str {
        "GKE"
    }

    fn service_key(&self) -> &'static str {
        "container"
    }
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait ContainerApi: Send + Sync + Debug {
    async fn create_cluster(
        &self,
        location: &str,
        request: CreateClusterRequest,
    ) -> Result<Operation>;
    async fn get_cluster(&self, location: &str, cluster_name: &str) -> Result<GkeCluster>;
    async fn delete_cluster(&self, location: &str, cluster_name: &str) -> Result<Operation>;
    async fn create_node_pool(
        &self,
        location: &str,
        cluster_name: &str,
        request: CreateNodePoolRequest,
    ) -> Result<Operation>;
    async fn get_node_pool(
        &self,
        location: &str,
        cluster_name: &str,
        node_pool_name: &str,
    ) -> Result<NodePool>;
    async fn delete_node_pool(
        &self,
        location: &str,
        cluster_name: &str,
        node_pool_name: &str,
    ) -> Result<Operation>;
    async fn get_operation(&self, location: &str, operation_name: &str) -> Result<Operation>;
}

#[derive(Debug)]
pub struct ContainerClient {
    base: GcpClientBase,
    project_id: String,
}

impl ContainerClient {
    pub fn new(client: Client, config: GcpClientConfig) -> Self {
        let project_id = config.project_id.clone();
        Self {
            base: GcpClientBase::new(client, config, Box::new(ContainerServiceConfig)),
            project_id,
        }
    }

    fn cluster_path(&self, location: &str, cluster_name: &str) -> String {
        format!(
            "projects/{}/locations/{}/clusters/{}",
            self.project_id, location, cluster_name
        )
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ContainerApi for ContainerClient {
    async fn create_cluster(
        &self,
        location: &str,
        request: CreateClusterRequest,
    ) -> Result<Operation> {
        let resource_name = request
            .cluster
            .as_ref()
            .and_then(|cluster| cluster.name.as_deref())
            .unwrap_or("cluster")
            .to_string();
        let path = format!(
            "projects/{}/locations/{}/clusters",
            self.project_id, location
        );
        self.base
            .execute_request(Method::POST, &path, None, Some(request), &resource_name)
            .await
    }

    async fn get_cluster(&self, location: &str, cluster_name: &str) -> Result<GkeCluster> {
        self.base
            .execute_request(
                Method::GET,
                &self.cluster_path(location, cluster_name),
                None,
                Option::<()>::None,
                cluster_name,
            )
            .await
    }

    async fn delete_cluster(&self, location: &str, cluster_name: &str) -> Result<Operation> {
        self.base
            .execute_request(
                Method::DELETE,
                &self.cluster_path(location, cluster_name),
                None,
                Option::<()>::None,
                cluster_name,
            )
            .await
    }

    async fn create_node_pool(
        &self,
        location: &str,
        cluster_name: &str,
        request: CreateNodePoolRequest,
    ) -> Result<Operation> {
        let resource_name = request
            .node_pool
            .as_ref()
            .and_then(|pool| pool.name.as_deref())
            .unwrap_or("nodePool")
            .to_string();
        let path = format!("{}/nodePools", self.cluster_path(location, cluster_name));
        self.base
            .execute_request(Method::POST, &path, None, Some(request), &resource_name)
            .await
    }

    async fn get_node_pool(
        &self,
        location: &str,
        cluster_name: &str,
        node_pool_name: &str,
    ) -> Result<NodePool> {
        let path = format!(
            "{}/nodePools/{}",
            self.cluster_path(location, cluster_name),
            node_pool_name
        );
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, node_pool_name)
            .await
    }

    async fn delete_node_pool(
        &self,
        location: &str,
        cluster_name: &str,
        node_pool_name: &str,
    ) -> Result<Operation> {
        let path = format!(
            "{}/nodePools/{}",
            self.cluster_path(location, cluster_name),
            node_pool_name
        );
        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                node_pool_name,
            )
            .await
    }

    async fn get_operation(&self, location: &str, operation_name: &str) -> Result<Operation> {
        let operation_id = operation_name.rsplit('/').next().unwrap_or(operation_name);
        let path = format!(
            "projects/{}/locations/{}/operations/{}",
            self.project_id, location, operation_id
        );
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, operation_name)
            .await
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateClusterRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster: Option<GkeCluster>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateNodePoolRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_pool: Option<NodePool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct GkeCluster {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_node_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_config: Option<NodeConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_pools: Option<Vec<NodePool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autopilot: Option<Autopilot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnetwork: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_allocation_policy: Option<IpAllocationPolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_channel: Option<ReleaseChannel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_labels: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub master_auth: Option<MasterAuth>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workload_identity_config: Option<WorkloadIdentityConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Autopilot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NodePool {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<NodeConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_node_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autoscaling: Option<NodePoolAutoscaling>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub management: Option<NodeManagement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locations: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NodeConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk_size_gb: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_scopes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NodePoolAutoscaling {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_node_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_node_count: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NodeManagement {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_upgrade: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_repair: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct IpAllocationPolicy {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_ip_aliases: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_secondary_range_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub services_secondary_range_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseChannel {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct MasterAuth {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_ca_certificate: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadIdentityConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workload_pool: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_autopilot_cluster_shape() {
        let request = CreateClusterRequest::builder()
            .cluster(
                GkeCluster::builder()
                    .name("alien-e2e".to_string())
                    .autopilot(Autopilot::builder().enabled(true).build())
                    .network("projects/test/global/networks/default".to_string())
                    .subnetwork("projects/test/regions/us-central1/subnetworks/default".to_string())
                    .ip_allocation_policy(
                        IpAllocationPolicy::builder().use_ip_aliases(true).build(),
                    )
                    .build(),
            )
            .build();

        let value = serde_json::to_value(request).expect("request should serialize");
        assert_eq!(value["cluster"]["name"], "alien-e2e");
        assert_eq!(value["cluster"]["autopilot"]["enabled"], true);
        assert_eq!(value["cluster"]["ipAllocationPolicy"]["useIpAliases"], true);
    }
}
