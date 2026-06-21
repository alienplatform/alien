use std::collections::BTreeMap;
use std::sync::Arc;

use alien_core::{
    branded_tag_key, ArtifactRegistryHeartbeatData, ArtifactRegistryHeartbeatStatus,
    BuildHeartbeatData, BuildHeartbeatStatus, GcpArtifactRegistryHeartbeatData,
    GcpCloudBuildHeartbeatData, GcpCloudRunWorkerHeartbeatData, GcpCloudStorageHeartbeatData,
    GcpFirestoreKvHeartbeatData, GcpPubSubQueueHeartbeatData, GcpVpcNetworkHeartbeatData,
    HeartbeatBackend, HeartbeatCollectionIssue, HeartbeatCollectionIssueReason,
    HeartbeatIssueSeverity, HeartbeatSource, KvHeartbeatData, KvHeartbeatStatus,
    NetworkHeartbeatData, NetworkHeartbeatStatus, ObservedHealth, Platform, ProviderLifecycleState,
    QueueHeartbeatData, QueueHeartbeatStatus, RawHeartbeatSnippet, RawHeartbeatSnippetFormat,
    ResourceHeartbeat, ResourceHeartbeatData, ResourceType, StorageHeartbeatData,
    StorageHeartbeatStatus, WorkerHeartbeatData, WorkloadHeartbeatStatus, ALIEN_MANAGED_BY_TAG_KEY,
    ALIEN_MANAGED_BY_TAG_VALUE, ALIEN_RESOURCE_TAG_KEY, DEFAULT_ALIEN_LABEL_DOMAIN,
};
use alien_gcp_clients::cloudasset::{ResourceSearchResult, SearchAllResourcesRequest};
use alien_gcp_clients::monitoring::{ListTimeSeriesRequest, TimeInterval, TimeSeriesView};
use alien_gcp_clients::{CloudAssetApi, MonitoringApi};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use tracing::warn;

use crate::{ObserveScope, Observer, Result};

const MAX_SEARCH_ALL_RESOURCES_PAGES: usize = 20;

#[derive(Clone)]
pub struct GcpObserveContext {
    pub deployment_id: String,
    pub project_id: String,
    pub region: String,
    pub cloud_asset_client: Arc<dyn CloudAssetApi>,
    pub monitoring_client: Arc<dyn MonitoringApi>,
}

pub struct GcpObserver {
    context: GcpObserveContext,
    label_domain: String,
}

impl GcpObserver {
    pub fn new(context: GcpObserveContext) -> Self {
        Self {
            context,
            label_domain: DEFAULT_ALIEN_LABEL_DOMAIN.to_string(),
        }
    }

    pub fn with_label_domain(context: GcpObserveContext, label_domain: impl Into<String>) -> Self {
        Self {
            context,
            label_domain: label_domain.into(),
        }
    }

    async fn monitoring_issue(&self) -> Option<HeartbeatCollectionIssue> {
        let end = Utc::now();
        let start = end - Duration::minutes(5);
        let request = ListTimeSeriesRequest::builder()
            .filter("metric.type=\"run.googleapis.com/request_count\"".to_string())
            .interval(
                TimeInterval::builder()
                    .start_time(start.to_rfc3339())
                    .end_time(end.to_rfc3339())
                    .build(),
            )
            .view(TimeSeriesView::Headers)
            .page_size(1)
            .build();

        match self
            .context
            .monitoring_client
            .list_time_series(request)
            .await
        {
            Ok(_) => None,
            Err(error) => {
                warn!(error = %error, "GCP Cloud Monitoring observe probe failed");
                Some(collection_issue(
                    "gcp-monitoring",
                    HeartbeatCollectionIssueReason::Forbidden,
                    HeartbeatIssueSeverity::Warning,
                    "Cloud Monitoring metrics are unavailable; grant roles/monitoring.viewer",
                ))
            }
        }
    }

    async fn discover_resources(&self) -> Result<Vec<ResourceSearchResult>> {
        let mut resources = Vec::new();
        let mut page_token = None;

        for _ in 0..MAX_SEARCH_ALL_RESOURCES_PAGES {
            let response = self
                .context
                .cloud_asset_client
                .search_all_resources(
                    SearchAllResourcesRequest::builder()
                        .scope(format!("projects/{}", self.context.project_id))
                        .asset_types(observed_asset_types())
                        .page_size(500)
                        .read_mask(vec![
                            "name".to_string(),
                            "assetType".to_string(),
                            "project".to_string(),
                            "displayName".to_string(),
                            "location".to_string(),
                            "labels".to_string(),
                            "state".to_string(),
                            "createTime".to_string(),
                            "updateTime".to_string(),
                            "tags".to_string(),
                            "effectiveTags".to_string(),
                        ])
                        .maybe_page_token(page_token.clone())
                        .build(),
                )
                .await;

            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    warn!(error = %error, "GCP Cloud Asset Inventory observe pass failed");
                    return Ok(resources);
                }
            };

            resources.extend(response.results);
            page_token = response.next_page_token.filter(|token| !token.is_empty());
            if page_token.is_none() {
                break;
            }
        }

        Ok(resources)
    }

    fn heartbeat_for_resource(
        &self,
        resource: ResourceSearchResult,
        monitoring_issue: Option<&HeartbeatCollectionIssue>,
    ) -> Option<ResourceHeartbeat> {
        let alien_resource_id = alien_resource_id_from_labels(&self.label_domain, &resource.labels);
        if alien_resource_id.is_some() {
            return None;
        }

        let raw_identity = resource.name.clone()?;
        let data = resource_data_for_result(
            &resource,
            &self.context.project_id,
            &self.context.region,
            monitoring_issue.cloned(),
        )?;

        Some(ResourceHeartbeat {
            deployment_id: Some(self.context.deployment_id.clone()),
            resource_id: gcp_raw_identity(&raw_identity),
            source: HeartbeatSource::Observed,
            alien_resource_id,
            resource_type: resource_type_for_data(&data),
            controller_platform: Platform::Gcp,
            backend: HeartbeatBackend::Gcp,
            observed_at: Utc::now(),
            data,
            raw: vec![raw_snippet(&resource)],
        })
    }
}

#[async_trait]
impl Observer for GcpObserver {
    fn platform(&self) -> Platform {
        Platform::Gcp
    }

    async fn discover(&self, _scope: &ObserveScope) -> Result<Vec<ResourceHeartbeat>> {
        let monitoring_issue = self.monitoring_issue().await;
        let resources = self.discover_resources().await?;

        Ok(resources
            .into_iter()
            .filter_map(|resource| self.heartbeat_for_resource(resource, monitoring_issue.as_ref()))
            .collect())
    }
}

pub fn gcp_raw_identity(name: &str) -> String {
    name.to_string()
}

fn observed_asset_types() -> Vec<String> {
    [
        "storage.googleapis.com/Bucket",
        "run.googleapis.com/Service",
        "pubsub.googleapis.com/Topic",
        "firestore.googleapis.com/Database",
        "compute.googleapis.com/Network",
        "artifactregistry.googleapis.com/Repository",
        "cloudbuild.googleapis.com/BuildTrigger",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn resource_data_for_result(
    resource: &ResourceSearchResult,
    default_project_id: &str,
    default_region: &str,
    monitoring_issue: Option<HeartbeatCollectionIssue>,
) -> Option<ResourceHeartbeatData> {
    let asset_type = resource.asset_type.as_deref()?;
    let name = resource.name.as_deref()?;
    let display_name = resource
        .display_name
        .clone()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| resource_leaf(name));
    let location = resource
        .location
        .clone()
        .filter(|value| !value.is_empty())
        .or_else(|| location_from_name(name))
        .or_else(|| Some(default_region.to_string()));
    let project_id = resource
        .project
        .as_deref()
        .and_then(project_id_from_resource_name)
        .unwrap_or(default_project_id)
        .to_string();

    match asset_type {
        "storage.googleapis.com/Bucket" => Some(ResourceHeartbeatData::Storage(
            StorageHeartbeatData::GcpCloudStorage(GcpCloudStorageHeartbeatData {
                status: storage_status(monitoring_issue),
                name: display_name.clone(),
                bucket_id: Some(display_name),
                location,
                location_type: None,
                storage_class: None,
                versioning_enabled: None,
                lifecycle_present: false,
                lifecycle_rule_count: None,
                retention_policy_effective_time: None,
                retention_policy_is_locked: None,
                retention_period: None,
                soft_delete_retention_duration_seconds: None,
                soft_delete_effective_time: None,
                uniform_bucket_level_access_enabled: None,
                uniform_bucket_level_access_locked_time: None,
                public_access_prevention: None,
                encryption_config_present: false,
                default_kms_key_name: resource.kms_keys.first().cloned(),
            }),
        )),
        "run.googleapis.com/Service" => Some(ResourceHeartbeatData::Worker(
            WorkerHeartbeatData::GcpCloudRun(GcpCloudRunWorkerHeartbeatData {
                status: workload_status(monitoring_issue),
                service: display_name,
                region: location,
                uri: None,
                urls: vec![],
                latest_created_revision: None,
                latest_ready_revision: None,
                generation: None,
                observed_generation: None,
                traffic_count: 0,
                min_instance_count: None,
                max_instance_count: None,
                container_image: None,
                cpu_limit: None,
                memory_limit: None,
            }),
        )),
        "pubsub.googleapis.com/Topic" => Some(ResourceHeartbeatData::Queue(
            QueueHeartbeatData::GcpPubSub(GcpPubSubQueueHeartbeatData {
                status: queue_status(monitoring_issue),
                topic_name: display_name,
                project_id: Some(project_id),
                topic_full_name: Some(name.to_string()),
                topic_labels: resource.labels.clone(),
                ..Default::default()
            }),
        )),
        "firestore.googleapis.com/Database" => Some(ResourceHeartbeatData::Kv(
            KvHeartbeatData::GcpFirestore(GcpFirestoreKvHeartbeatData {
                status: kv_status(monitoring_issue),
                database_name: display_name,
                project_id: Some(project_id),
                endpoint: None,
                location_id: location,
                database_type: None,
                concurrency_mode: None,
                app_engine_integration_mode: None,
                delete_protection_state: None,
                point_in_time_recovery_enablement: None,
                version_retention_period: None,
                earliest_version_time: None,
                create_time: resource.create_time.clone(),
                update_time: resource.update_time.clone(),
                delete_time: None,
                database_edition: None,
                cmek_enabled: !resource.kms_keys.is_empty() || resource.kms_key.is_some(),
                source_info_present: false,
            }),
        )),
        "compute.googleapis.com/Network" => Some(ResourceHeartbeatData::Network(
            NetworkHeartbeatData::GcpVpc(GcpVpcNetworkHeartbeatData {
                status: network_status(monitoring_issue),
                network_name: Some(display_name),
                network_self_link: Some(name.to_string()),
                subnetwork_name: None,
                subnetwork_self_link: None,
                region: location,
                cidr_block: None,
                router_name: None,
                cloud_nat_name: None,
                firewall_name: None,
                is_byo_vpc: true,
            }),
        )),
        "artifactregistry.googleapis.com/Repository" => {
            Some(ResourceHeartbeatData::ArtifactRegistry(
                ArtifactRegistryHeartbeatData::GcpArtifactRegistry(
                    GcpArtifactRegistryHeartbeatData {
                        status: artifact_registry_status(monitoring_issue),
                        project_id,
                        location: location.unwrap_or_else(|| default_region.to_string()),
                        repository_id: display_name,
                        name: Some(name.to_string()),
                        format: None,
                        mode: None,
                        description: resource.description.clone(),
                        label_count: resource.labels.len() as u32,
                        cleanup_policy_count: 0,
                        cleanup_policy_dry_run: None,
                        kms_key_name_present: !resource.kms_keys.is_empty()
                            || resource.kms_key.is_some(),
                        size_bytes: None,
                        satisfies_pzs: None,
                        create_time: resource.create_time.clone(),
                        update_time: resource.update_time.clone(),
                        iam_policy_etag_present: false,
                        iam_binding_count: 0,
                        iam_roles: vec![],
                        pull_service_account_email: None,
                        push_service_account_email: None,
                    },
                ),
            ))
        }
        "cloudbuild.googleapis.com/BuildTrigger" => Some(ResourceHeartbeatData::Build(
            BuildHeartbeatData::GcpCloudBuild(GcpCloudBuildHeartbeatData {
                status: build_status(monitoring_issue),
                project_id,
                location: location.unwrap_or_else(|| default_region.to_string()),
                build_config_id: display_name,
                service_account: None,
                environment_variable_count: 0,
            }),
        )),
        _ => None,
    }
}

fn alien_resource_id_from_labels(
    label_domain: &str,
    labels: &BTreeMap<String, String>,
) -> Option<String> {
    let managed_by_key = branded_tag_key(label_domain, ALIEN_MANAGED_BY_TAG_KEY);
    if labels.get(&managed_by_key).map(String::as_str) != Some(ALIEN_MANAGED_BY_TAG_VALUE) {
        return None;
    }

    let resource_key = branded_tag_key(label_domain, ALIEN_RESOURCE_TAG_KEY);
    labels.get(&resource_key).cloned()
}

fn resource_type_for_data(data: &ResourceHeartbeatData) -> ResourceType {
    let resource_type = match data {
        ResourceHeartbeatData::Storage(_) => "storage",
        ResourceHeartbeatData::Worker(_) => "worker",
        ResourceHeartbeatData::Queue(_) => "queue",
        ResourceHeartbeatData::Kv(_) => "kv",
        ResourceHeartbeatData::Network(_) => "network",
        ResourceHeartbeatData::Build(_) => "build",
        ResourceHeartbeatData::ArtifactRegistry(_) => "artifact-registry",
        _ => "external",
    };
    ResourceType::from_static(resource_type)
}

fn resource_leaf(name: &str) -> String {
    name.rsplit('/').next().unwrap_or(name).to_string()
}

fn location_from_name(name: &str) -> Option<String> {
    let mut parts = name.split('/');
    while let Some(part) = parts.next() {
        if part == "locations" {
            return parts.next().map(str::to_string);
        }
    }
    None
}

fn project_id_from_resource_name(name: &str) -> Option<&str> {
    let mut parts = name.split('/');
    while let Some(part) = parts.next() {
        if part == "projects" {
            return parts.next();
        }
    }
    None
}

fn raw_snippet(resource: &ResourceSearchResult) -> RawHeartbeatSnippet {
    RawHeartbeatSnippet {
        source: "gcp-cloudasset:searchAllResources".to_string(),
        format: RawHeartbeatSnippetFormat::Json,
        collected_at: Utc::now(),
        body: serde_json::to_string(resource).unwrap_or_else(|_| "{}".to_string()),
        truncated: false,
    }
}

fn collection_issue(
    source: &str,
    reason: HeartbeatCollectionIssueReason,
    severity: HeartbeatIssueSeverity,
    message: &str,
) -> HeartbeatCollectionIssue {
    HeartbeatCollectionIssue {
        source: source.to_string(),
        reason,
        severity,
        message: message.to_string(),
    }
}

fn storage_status(issue: Option<HeartbeatCollectionIssue>) -> StorageHeartbeatStatus {
    StorageHeartbeatStatus {
        health: ObservedHealth::Unknown,
        lifecycle: ProviderLifecycleState::Unknown,
        message: Some(
            "Observed from GCP Cloud Asset Inventory; detailed storage reads not yet collected"
                .to_string(),
        ),
        stale: false,
        partial: true,
        collection_issues: issue.into_iter().collect(),
    }
}

fn workload_status(issue: Option<HeartbeatCollectionIssue>) -> WorkloadHeartbeatStatus {
    WorkloadHeartbeatStatus {
        health: ObservedHealth::Unknown,
        lifecycle: ProviderLifecycleState::Unknown,
        message: Some(
            "Observed from GCP Cloud Asset Inventory; detailed workload reads not yet collected"
                .to_string(),
        ),
        stale: false,
        partial: true,
        collection_issues: issue.into_iter().collect(),
    }
}

fn queue_status(issue: Option<HeartbeatCollectionIssue>) -> QueueHeartbeatStatus {
    QueueHeartbeatStatus {
        health: ObservedHealth::Unknown,
        lifecycle: ProviderLifecycleState::Unknown,
        message: Some(
            "Observed from GCP Cloud Asset Inventory; detailed queue reads not yet collected"
                .to_string(),
        ),
        stale: false,
        partial: true,
        collection_issues: issue.into_iter().collect(),
    }
}

fn kv_status(issue: Option<HeartbeatCollectionIssue>) -> KvHeartbeatStatus {
    KvHeartbeatStatus {
        health: ObservedHealth::Unknown,
        lifecycle: ProviderLifecycleState::Unknown,
        message: Some(
            "Observed from GCP Cloud Asset Inventory; detailed key-value reads not yet collected"
                .to_string(),
        ),
        stale: false,
        partial: true,
        collection_issues: issue.into_iter().collect(),
    }
}

fn network_status(issue: Option<HeartbeatCollectionIssue>) -> NetworkHeartbeatStatus {
    NetworkHeartbeatStatus {
        health: ObservedHealth::Unknown,
        lifecycle: ProviderLifecycleState::Unknown,
        message: Some(
            "Observed from GCP Cloud Asset Inventory; detailed network reads not yet collected"
                .to_string(),
        ),
        stale: false,
        partial: true,
        collection_issues: issue.into_iter().collect(),
    }
}

fn artifact_registry_status(
    issue: Option<HeartbeatCollectionIssue>,
) -> ArtifactRegistryHeartbeatStatus {
    ArtifactRegistryHeartbeatStatus {
        health: ObservedHealth::Unknown,
        lifecycle: ProviderLifecycleState::Unknown,
        message: Some(
            "Observed from GCP Cloud Asset Inventory; detailed registry reads not yet collected"
                .to_string(),
        ),
        stale: false,
        partial: true,
        collection_issues: issue.into_iter().collect(),
    }
}

fn build_status(issue: Option<HeartbeatCollectionIssue>) -> BuildHeartbeatStatus {
    BuildHeartbeatStatus {
        health: ObservedHealth::Unknown,
        lifecycle: ProviderLifecycleState::Unknown,
        message: Some(
            "Observed from GCP Cloud Asset Inventory; detailed build reads not yet collected"
                .to_string(),
        ),
        stale: false,
        partial: true,
        collection_issues: issue.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_gcp_clients::cloudasset::{ResourceSearchResult, SearchAllResourcesResponse};
    use alien_gcp_clients::monitoring::ListTimeSeriesResponse;

    #[test]
    fn maps_cloud_run_asset_to_worker_heartbeat() {
        let resource = ResourceSearchResult {
            name: Some(
                "//run.googleapis.com/projects/example/locations/us-central1/services/api"
                    .to_string(),
            ),
            asset_type: Some("run.googleapis.com/Service".to_string()),
            display_name: Some("api".to_string()),
            ..Default::default()
        };
        let data = resource_data_for_result(&resource, "example", "us-central1", None).unwrap();

        match data {
            ResourceHeartbeatData::Worker(WorkerHeartbeatData::GcpCloudRun(data)) => {
                assert_eq!(data.service, "api");
                assert_eq!(data.region.as_deref(), Some("us-central1"));
                assert!(data.status.partial);
            }
            other => panic!("expected GCP Cloud Run worker heartbeat, got {other:?}"),
        }
    }

    #[test]
    fn maps_pubsub_asset_to_queue_heartbeat() {
        let resource = ResourceSearchResult {
            name: Some("//pubsub.googleapis.com/projects/example/topics/events".to_string()),
            asset_type: Some("pubsub.googleapis.com/Topic".to_string()),
            ..Default::default()
        };
        let data = resource_data_for_result(&resource, "example", "us-central1", None).unwrap();

        match data {
            ResourceHeartbeatData::Queue(QueueHeartbeatData::GcpPubSub(data)) => {
                assert_eq!(data.topic_name, "events");
                assert_eq!(data.project_id.as_deref(), Some("example"));
                assert!(data.status.partial);
            }
            other => panic!("expected GCP Pub/Sub queue heartbeat, got {other:?}"),
        }
    }

    #[test]
    fn alien_labeled_cloud_resources_are_skipped() {
        let observer = GcpObserver::new(GcpObserveContext {
            deployment_id: "dep_1".to_string(),
            project_id: "example".to_string(),
            region: "us-central1".to_string(),
            cloud_asset_client: Arc::new(DummyCloudAssetClient),
            monitoring_client: Arc::new(DummyMonitoringClient),
        });
        let resource = ResourceSearchResult {
            name: Some(
                "//run.googleapis.com/projects/example/locations/us-central1/services/api"
                    .to_string(),
            ),
            asset_type: Some("run.googleapis.com/Service".to_string()),
            labels: BTreeMap::from([
                (
                    branded_tag_key(DEFAULT_ALIEN_LABEL_DOMAIN, ALIEN_RESOURCE_TAG_KEY),
                    "worker.api".to_string(),
                ),
                (
                    branded_tag_key(DEFAULT_ALIEN_LABEL_DOMAIN, ALIEN_MANAGED_BY_TAG_KEY),
                    ALIEN_MANAGED_BY_TAG_VALUE.to_string(),
                ),
            ]),
            ..Default::default()
        };

        assert!(observer.heartbeat_for_resource(resource, None).is_none());
    }

    #[derive(Debug)]
    struct DummyCloudAssetClient;

    #[async_trait]
    impl CloudAssetApi for DummyCloudAssetClient {
        async fn search_all_resources(
            &self,
            _request: SearchAllResourcesRequest,
        ) -> alien_client_core::Result<SearchAllResourcesResponse> {
            unreachable!("not called by mapping test")
        }
    }

    #[derive(Debug)]
    struct DummyMonitoringClient;

    #[async_trait]
    impl MonitoringApi for DummyMonitoringClient {
        async fn list_time_series(
            &self,
            _request: ListTimeSeriesRequest,
        ) -> alien_client_core::Result<ListTimeSeriesResponse> {
            unreachable!("not called by mapping test")
        }
    }
}
