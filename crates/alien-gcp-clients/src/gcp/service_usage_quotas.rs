//! Read applied quota values from the Service Usage quota API.

use crate::gcp::{
    api_client::{GcpClientBase, GcpServiceConfig},
    GcpClientConfig,
};
use alien_client_core::Result;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Debug;

#[cfg(feature = "test-utils")]
use mockall::automock;

#[derive(Debug)]
pub struct ServiceUsageQuotasServiceConfig;

impl GcpServiceConfig for ServiceUsageQuotasServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://serviceusage.googleapis.com/v1beta1"
    }
    fn default_audience(&self) -> &'static str {
        "https://serviceusage.googleapis.com/"
    }
    fn service_name(&self) -> &'static str {
        "Service Usage quota"
    }
    fn service_key(&self) -> &'static str {
        "serviceusage"
    }
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait ServiceUsageQuotasApi: Send + Sync + Debug {
    /// Get an applied quota, including its dimension-specific values.
    async fn get_quota_metric(&self, service: &str, metric: &str) -> Result<QuotaMetric>;
}

/// Service Usage quota client restricted to reads in the configured target project.
#[derive(Debug)]
pub struct ServiceUsageQuotasClient {
    base: GcpClientBase,
    project_id: String,
}

impl ServiceUsageQuotasClient {
    pub fn new(client: Client, config: GcpClientConfig) -> Self {
        Self {
            project_id: config.project_id.clone(),
            base: GcpClientBase::new(client, config, Box::new(ServiceUsageQuotasServiceConfig)),
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl ServiceUsageQuotasApi for ServiceUsageQuotasClient {
    /// https://cloud.google.com/service-usage/docs/reference/rest/v1beta1/services.consumerQuotaMetrics/get
    async fn get_quota_metric(&self, service: &str, metric: &str) -> Result<QuotaMetric> {
        let path = format!(
            "projects/{}/services/{}/consumerQuotaMetrics/{}",
            self.project_id,
            urlencoding::encode(service),
            urlencoding::encode(metric),
        );
        self.base
            .execute_request(
                Method::GET,
                &path,
                Some(vec![("view", "FULL".into())]),
                Option::<()>::None,
                metric,
            )
            .await
    }
}

/// A quota metric and its applied limits from Service Usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaMetric {
    /// Fully qualified metric resource name.
    pub name: String,
    /// Provider metric identifier.
    pub metric: String,
    /// Applied limits; missing values are not a zero limit.
    pub consumer_quota_limits: Option<Vec<ConsumerQuotaLimit>>,
}

/// A limit for one quota unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsumerQuotaLimit {
    /// Metric whose limit this describes.
    pub metric: String,
    /// Provider unit, e.g. one per project.
    pub unit: String,
    /// Applied values grouped by dimensions.
    pub quota_buckets: Option<Vec<QuotaBucket>>,
}

/// Applied quota for a dimension group. Catalog defaults are intentionally omitted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaBucket {
    /// Dimension filters, empty for a project-wide quota.
    #[serde(default)]
    pub dimensions: BTreeMap<String, String>,
    /// Applied int64 value, encoded as a decimal string by the provider.
    pub effective_limit: Option<String>,
}
