//! Explicit infrastructure metric collection configuration; absent means disabled.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, net::IpAddr};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsMetricSource {
    pub namespace: String,
    pub metric_name: String,
    pub dimensions: BTreeMap<String, String>,
    pub statistic: String,
    pub period_seconds: i32,
    pub unit: String,
    /// True only for non-negative count/byte totals; arbitrary sums may decrease.
    #[serde(default)]
    pub monotonic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureMetricSource {
    pub resource_uri: String,
    pub metric_namespace: String,
    pub metric_names: Vec<String>,
    pub aggregation: String,
    /// True for count/byte totals, false for arbitrary aggregated values.
    #[serde(default)]
    pub monotonic: bool,
    pub interval_seconds: u32,
    /// Explicit dimension filter, for example `EntityName eq 'approved-queue'`.
    pub filter: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpMetricSource {
    pub project_id: String,
    pub filter: String,
    /// Whether numeric DELTA points are known non-negative counter increments.
    #[serde(default)]
    pub monotonic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ScrapeEndpoint {
    pub url: String,
    pub replica_id: String,
    /// Replica addresses supplied by the trusted runtime resolver; DNS is never consulted.
    pub addresses: Vec<IpAddr>,
    pub max_response_bytes: usize,
    pub max_samples: usize,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MetricResource {
    pub deployment_id: String,
    pub resource_id: String,
    pub provider: String,
    pub region: String,
}

/// An explicitly approved collection source. Provider IAM authorization remains separate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", content = "config", rename_all = "camelCase")]
pub enum MetricsSource {
    Aws(Vec<AwsMetricSource>),
    Gcp(GcpMetricSource),
    Azure(AzureMetricSource),
    Scrape(ScrapeEndpoint),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MetricsCollectionJob {
    /// Stable job ID; source changes automatically invalidate its prior checkpoint.
    pub id: String,
    pub resource_id: String,
    pub source: MetricsSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MetricsCollectionConfig {
    /// Derive sources only from trusted deployed resource outputs; never grants IAM rights.
    #[serde(default)]
    pub built_in_resources: bool,
    /// Explicit sources. Configuring sources never adds IAM permissions.
    #[serde(default)]
    pub jobs: Vec<MetricsCollectionJob>,
    /// Poll at least this many seconds apart (minimum 60).
    pub interval_seconds: u32,
    /// Wait for provider emission before closing an interval (at least 60 for cloud sources).
    pub source_lag_seconds: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MetricsCollectionCheckpoint {
    pub source_hash: String,
    /// Last fully exported interval end. Updated only after OTLP acknowledgement.
    pub collected_through: DateTime<Utc>,
}
