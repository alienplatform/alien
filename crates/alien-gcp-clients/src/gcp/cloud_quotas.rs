//! Read applied quota values from the Cloud Quotas API.

use crate::gcp::{
    api_client::{GcpClientBase, GcpServiceConfig},
    GcpClientConfig,
};
use alien_client_core::Result;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

#[cfg(feature = "test-utils")]
use mockall::automock;

#[derive(Debug)]
pub struct CloudQuotasServiceConfig;

impl GcpServiceConfig for CloudQuotasServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://cloudquotas.googleapis.com/v1"
    }
    fn default_audience(&self) -> &'static str {
        "https://cloudquotas.googleapis.com/"
    }
    fn service_name(&self) -> &'static str {
        "Cloud Quotas"
    }
    fn service_key(&self) -> &'static str {
        "cloudquotas"
    }
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait CloudQuotasApi: Send + Sync + Debug {
    /// Get an applied quota, including its dimension-specific values.
    async fn get_quota_info(&self, service: &str, quota_id: &str) -> Result<QuotaInfo>;
}

/// Cloud Quotas client restricted to reads in the configured target project.
#[derive(Debug)]
pub struct CloudQuotasClient {
    base: GcpClientBase,
    project_id: String,
}

impl CloudQuotasClient {
    pub fn new(client: Client, config: GcpClientConfig) -> Self {
        Self {
            project_id: config.project_id.clone(),
            base: GcpClientBase::new(client, config, Box::new(CloudQuotasServiceConfig)),
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl CloudQuotasApi for CloudQuotasClient {
    /// https://cloud.google.com/docs/quotas/reference/rest/v1/projects.locations.services.quotaInfos/get
    async fn get_quota_info(&self, service: &str, quota_id: &str) -> Result<QuotaInfo> {
        let path = format!(
            "projects/{}/locations/global/services/{}/quotaInfos/{}",
            self.project_id,
            urlencoding::encode(service),
            urlencoding::encode(quota_id),
        );
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, quota_id)
            .await
    }
}

/// Applied quota metadata. This API does not report current usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaInfo {
    /// Provider quota identifier.
    pub quota_id: String,
    /// Provider metric identifier.
    pub metric: String,
    /// Dimension keys; empty for project-wide quotas.
    #[serde(default)]
    pub dimensions: Vec<String>,
    /// Applied values for dimension groups.
    pub dimensions_infos: Option<Vec<DimensionsInfo>>,
}

/// Applied value for one dimension group.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DimensionsInfo {
    /// Applied quota, distinct from the catalog default.
    pub details: Option<QuotaDetails>,
}

/// Applied int64 quota value encoded as a decimal string.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaDetails {
    /// Missing or invalid values must not be treated as zero.
    pub value: Option<String>,
}

impl QuotaInfo {
    /// Only a single dimensionless project-wide value is safe to use here.
    pub fn project_limit(&self) -> Option<f64> {
        if !self.dimensions.is_empty() {
            return None;
        }
        let [info] = self.dimensions_infos.as_deref()? else {
            return None;
        };
        let value = info
            .details
            .as_ref()?
            .value
            .as_deref()?
            .parse::<u64>()
            .ok()?;
        Some(value as f64)
    }
}
