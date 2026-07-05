#[cfg(feature = "aws")]
mod aws;
#[cfg(feature = "azure")]
mod azure;
mod error;
#[cfg(feature = "gcp")]
mod gcp;
#[cfg(feature = "kubernetes")]
mod kubernetes;

use std::collections::BTreeMap;

use alien_core::{
    HeartbeatBackend, HeartbeatCollectionIssue, ObservedCounts, ObservedHealth,
    ObservedInventoryBatch, ObservedResourceSample, Platform, ProviderLifecycleState,
    RawHeartbeatSnippet, ResourceHeartbeatData, ResourceType,
};
use async_trait::async_trait;
use serde_json::Value as JsonValue;

#[cfg(feature = "aws")]
pub use aws::{aws_raw_identity, AwsObserveContext, AwsObserver};
#[cfg(feature = "azure")]
pub use azure::{azure_raw_identity, AzureObserveContext, AzureObserver};
pub use error::Result;
#[cfg(feature = "gcp")]
pub use gcp::{gcp_raw_identity, GcpObserveContext, GcpObserver};
#[cfg(feature = "kubernetes")]
pub use kubernetes::{
    alien_resource_id_from_labels, raw_identity, KubernetesObserveContext, KubernetesObserver,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObserveScope {
    pub namespace: String,
    pub label_selector: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObserveReport {
    pub inventory_batches: Vec<ObservedInventoryBatch>,
}

#[async_trait]
pub trait Observer: Send + Sync {
    fn platform(&self) -> Platform;

    async fn discover(&self, scope: &ObserveScope) -> Result<ObserveReport>;
}

pub(crate) struct ObservedResourceSampleInput {
    pub deployment_id: String,
    pub raw_identity: String,
    pub provider_kind: String,
    pub display_name: String,
    pub namespace: Option<String>,
    pub region: Option<String>,
    pub scope: Option<String>,
    pub resource_type_hint: Option<ResourceType>,
    pub alien_resource_id: Option<String>,
    pub controller_platform: Platform,
    pub backend: HeartbeatBackend,
    pub labels: BTreeMap<String, String>,
    pub attributes: BTreeMap<String, JsonValue>,
    pub data: ResourceHeartbeatData,
    pub raw: Vec<RawHeartbeatSnippet>,
}

pub(crate) fn observed_resource_sample(
    input: ObservedResourceSampleInput,
) -> ObservedResourceSample {
    let status_data = serde_json::to_value(&input.data).unwrap_or(JsonValue::Null);
    let status = status_data
        .get("data")
        .and_then(|data| data.get("status"))
        .unwrap_or(&JsonValue::Null);

    let mut attributes = input.attributes;
    attributes.insert("statusData".to_string(), status_data.clone());
    attributes.insert(
        "controllerPlatform".to_string(),
        JsonValue::String(platform_name(input.controller_platform).to_string()),
    );
    attributes.insert(
        "backend".to_string(),
        JsonValue::String(backend_name(input.backend).to_string()),
    );

    let version = observed_version_from_labels(&input.labels);

    ObservedResourceSample {
        deployment_id: Some(input.deployment_id),
        raw_identity: input.raw_identity,
        provider_kind: input.provider_kind,
        display_name: input.display_name,
        namespace: input.namespace,
        region: input.region,
        scope: input.scope,
        resource_type_hint: input.resource_type_hint,
        version,
        alien_resource_id: input.alien_resource_id,
        health: status
            .get("health")
            .and_then(JsonValue::as_str)
            .map(parse_observed_health)
            .unwrap_or(ObservedHealth::Unknown),
        lifecycle: status
            .get("lifecycle")
            .and_then(JsonValue::as_str)
            .map(parse_lifecycle)
            .unwrap_or(ProviderLifecycleState::Unknown),
        message: status
            .get("message")
            .and_then(JsonValue::as_str)
            .map(str::to_string),
        partial: status
            .get("partial")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false),
        provider_stale: status
            .get("stale")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false),
        counts: counts_from_status_data(&status_data),
        collection_issues: status
            .get("collectionIssues")
            .cloned()
            .and_then(|value| serde_json::from_value::<Vec<HeartbeatCollectionIssue>>(value).ok())
            .unwrap_or_default(),
        labels: input.labels,
        attributes,
        raw: input.raw,
    }
}

fn observed_version_from_labels(labels: &BTreeMap<String, String>) -> Option<String> {
    labels
        .iter()
        .find_map(|(key, value)| {
            if key.ends_with(".dev/version") {
                Some(value.trim())
            } else {
                None
            }
        })
        .or_else(|| labels.get("app.kubernetes.io/version").map(String::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn counts_from_status_data(value: &JsonValue) -> Option<ObservedCounts> {
    let data = value.get("data")?;
    ["replicas", "nodes", "nodeCounts", "podCounts"]
        .into_iter()
        .find_map(|field| data.get(field).and_then(parse_counts))
}

fn parse_counts(value: &JsonValue) -> Option<ObservedCounts> {
    Some(ObservedCounts {
        desired: value
            .get("desired")
            .and_then(JsonValue::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        current: value
            .get("current")
            .and_then(JsonValue::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        ready: value
            .get("ready")
            .and_then(JsonValue::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
    })
}

fn parse_observed_health(value: &str) -> ObservedHealth {
    match value {
        "healthy" => ObservedHealth::Healthy,
        "degraded" => ObservedHealth::Degraded,
        "unhealthy" => ObservedHealth::Unhealthy,
        _ => ObservedHealth::Unknown,
    }
}

fn parse_lifecycle(value: &str) -> ProviderLifecycleState {
    match value {
        "creating" => ProviderLifecycleState::Creating,
        "updating" => ProviderLifecycleState::Updating,
        "running" => ProviderLifecycleState::Running,
        "scaling" => ProviderLifecycleState::Scaling,
        "stopping" => ProviderLifecycleState::Stopping,
        "stopped" => ProviderLifecycleState::Stopped,
        "deleting" => ProviderLifecycleState::Deleting,
        "deleted" => ProviderLifecycleState::Deleted,
        "failed" => ProviderLifecycleState::Failed,
        _ => ProviderLifecycleState::Unknown,
    }
}

fn platform_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Aws => "aws",
        Platform::Gcp => "gcp",
        Platform::Azure => "azure",
        Platform::Kubernetes => "kubernetes",
        Platform::Machines => "machines",
        Platform::Local => "local",
        Platform::Test => "test",
    }
}

fn backend_name(backend: HeartbeatBackend) -> &'static str {
    match backend {
        HeartbeatBackend::Aws => "aws",
        HeartbeatBackend::Gcp => "gcp",
        HeartbeatBackend::Azure => "azure",
        HeartbeatBackend::Kubernetes => "kubernetes",
        HeartbeatBackend::Local => "local",
        HeartbeatBackend::Managed => "managed",
        HeartbeatBackend::External => "external",
        HeartbeatBackend::Test => "test",
    }
}
