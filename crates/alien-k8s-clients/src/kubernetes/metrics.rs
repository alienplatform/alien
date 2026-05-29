use crate::kubernetes::kubernetes_client::KubernetesClient;
use crate::kubernetes::kubernetes_request_utils::sign_send_json;
use alien_client_core::Result;
use async_trait::async_trait;
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ListMeta, ObjectMeta};
#[cfg(feature = "test-utils")]
use mockall::automock;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PodMetricsList {
    pub metadata: ListMeta,
    pub items: Vec<PodMetrics>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PodMetrics {
    pub metadata: ObjectMeta,
    pub timestamp: Option<String>,
    pub window: Option<String>,
    pub containers: Vec<ContainerMetrics>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerMetrics {
    pub name: String,
    pub usage: BTreeMap<String, Quantity>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeMetricsList {
    pub metadata: ListMeta,
    pub items: Vec<NodeMetrics>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeMetrics {
    pub metadata: ObjectMeta,
    pub timestamp: Option<String>,
    pub window: Option<String>,
    pub usage: BTreeMap<String, Quantity>,
}

#[cfg_attr(feature = "test-utils", automock)]
#[async_trait]
pub trait MetricsApi: Send + Sync + std::fmt::Debug {
    async fn list_pod_metrics(
        &self,
        namespace: &str,
        label_selector: Option<String>,
    ) -> Result<PodMetricsList>;

    async fn list_node_metrics(&self, label_selector: Option<String>) -> Result<NodeMetricsList>;
}

impl KubernetesClient {
    pub async fn list_pod_metrics(
        &self,
        namespace: &str,
        label_selector: Option<String>,
    ) -> Result<PodMetricsList> {
        let mut url = format!(
            "{}/apis/metrics.k8s.io/v1beta1/namespaces/{}/pods",
            self.get_base_url(),
            urlencoding::encode(namespace)
        );

        if let Some(ls) = label_selector {
            url.push_str(&format!("?labelSelector={}", urlencoding::encode(&ls)));
        }

        let builder = self.client().request(Method::GET, &url);
        sign_send_json(builder, &self.auth_config()).await
    }

    pub async fn list_node_metrics(
        &self,
        label_selector: Option<String>,
    ) -> Result<NodeMetricsList> {
        let mut url = format!("{}/apis/metrics.k8s.io/v1beta1/nodes", self.get_base_url());

        if let Some(ls) = label_selector {
            url.push_str(&format!("?labelSelector={}", urlencoding::encode(&ls)));
        }

        let builder = self.client().request(Method::GET, &url);
        sign_send_json(builder, &self.auth_config()).await
    }
}

#[async_trait]
impl MetricsApi for KubernetesClient {
    async fn list_pod_metrics(
        &self,
        namespace: &str,
        label_selector: Option<String>,
    ) -> Result<PodMetricsList> {
        self.list_pod_metrics(namespace, label_selector).await
    }

    async fn list_node_metrics(&self, label_selector: Option<String>) -> Result<NodeMetricsList> {
        self.list_node_metrics(label_selector).await
    }
}
