use std::collections::BTreeMap;
use std::sync::Arc;

use alien_aws_clients::cloudwatch::ListMetricsRequest;
use alien_aws_clients::resourcegroupstagging::{GetResourcesRequest, ResourceTagMapping, Tag};
use alien_aws_clients::{CloudWatchApi, ResourceGroupsTaggingApi};
use alien_core::{
    branded_tag_key, ArtifactRegistryHeartbeatData, ArtifactRegistryHeartbeatStatus,
    AwsCodeBuildHeartbeatData, AwsDynamoDbKvHeartbeatData, AwsLambdaWorkerHeartbeatData,
    AwsS3StorageHeartbeatData, AwsSqsQueueHeartbeatData, AwsVpcNetworkHeartbeatData,
    BuildHeartbeatData, BuildHeartbeatStatus, HeartbeatBackend, HeartbeatCollectionIssue,
    HeartbeatCollectionIssueReason, HeartbeatIssueSeverity, HeartbeatSource, KvHeartbeatData,
    KvHeartbeatStatus, NetworkHeartbeatData, NetworkHeartbeatStatus, ObservedHealth, Platform,
    ProviderLifecycleState, QueueHeartbeatData, QueueHeartbeatStatus, RawHeartbeatSnippet,
    RawHeartbeatSnippetFormat, ResourceHeartbeat, ResourceHeartbeatData, ResourceType,
    StorageHeartbeatData, StorageHeartbeatStatus, WorkerHeartbeatData, WorkloadHeartbeatStatus,
    ALIEN_MANAGED_BY_TAG_KEY, ALIEN_MANAGED_BY_TAG_VALUE, ALIEN_RESOURCE_TAG_KEY,
    DEFAULT_ALIEN_LABEL_DOMAIN,
};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use tracing::warn;

use crate::{ObserveScope, Observer, Result};

const MAX_GET_RESOURCES_PAGES: usize = 20;

#[derive(Clone)]
pub struct AwsObserveContext {
    pub deployment_id: String,
    pub account_id: String,
    pub region: String,
    pub resource_groups_tagging_client: Arc<dyn ResourceGroupsTaggingApi>,
    pub cloudwatch_client: Arc<dyn CloudWatchApi>,
}

pub struct AwsObserver {
    context: AwsObserveContext,
    label_domain: String,
}

impl AwsObserver {
    pub fn new(context: AwsObserveContext) -> Self {
        Self {
            context,
            label_domain: DEFAULT_ALIEN_LABEL_DOMAIN.to_string(),
        }
    }

    pub fn with_label_domain(context: AwsObserveContext, label_domain: impl Into<String>) -> Self {
        Self {
            context,
            label_domain: label_domain.into(),
        }
    }

    async fn cloudwatch_issue(&self) -> Option<HeartbeatCollectionIssue> {
        let request = ListMetricsRequest::builder()
            .namespace("AWS/EC2".to_string())
            .recently_active("PT3H".to_string())
            .build();

        match self.context.cloudwatch_client.list_metrics(request).await {
            Ok(_) => None,
            Err(error) => {
                warn!(error = %error, "AWS CloudWatch observe probe failed");
                Some(collection_issue(
                    "aws-cloudwatch",
                    HeartbeatCollectionIssueReason::Forbidden,
                    HeartbeatIssueSeverity::Warning,
                    "CloudWatch metrics are unavailable; grant cloudwatch:ListMetrics/GetMetricData",
                ))
            }
        }
    }

    async fn discover_tagged_resources(&self) -> Result<Vec<ResourceTagMapping>> {
        let mut resources = Vec::new();
        let mut pagination_token = None;

        for _ in 0..MAX_GET_RESOURCES_PAGES {
            let response = self
                .context
                .resource_groups_tagging_client
                .get_resources(
                    GetResourcesRequest::builder()
                        .resources_per_page(100)
                        .maybe_pagination_token(pagination_token.clone())
                        .build(),
                )
                .await;

            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    warn!(error = %error, "AWS tag inventory observe pass failed");
                    return Ok(resources);
                }
            };

            resources.extend(response.resource_tag_mapping_list);
            pagination_token = response.pagination_token.filter(|token| !token.is_empty());
            if pagination_token.is_none() {
                break;
            }
        }

        Ok(resources)
    }

    fn heartbeat_for_mapping(
        &self,
        mapping: ResourceTagMapping,
        cloudwatch_issue: Option<&HeartbeatCollectionIssue>,
    ) -> Option<ResourceHeartbeat> {
        let arn = AwsArn::parse(&mapping.resource_arn)?;
        let tags = tags_to_map(&mapping.tags);
        let alien_resource_id = alien_resource_id_from_tags(&self.label_domain, &tags);
        if alien_resource_id.is_some() {
            return None;
        }
        let raw = raw_snippet(&mapping.resource_arn, &tags);
        let issue = cloudwatch_issue.cloned();
        let observed_at = Utc::now();
        let data = resource_data_for_arn(
            &arn,
            &self.context.region,
            &self.context.account_id,
            issue.clone(),
        )?;

        Some(ResourceHeartbeat {
            deployment_id: Some(self.context.deployment_id.clone()),
            resource_id: aws_raw_identity(&mapping.resource_arn),
            source: HeartbeatSource::Observed,
            alien_resource_id,
            resource_type: resource_type_for_data(&data),
            controller_platform: Platform::Aws,
            backend: HeartbeatBackend::Aws,
            observed_at,
            data,
            raw: vec![raw],
        })
    }
}

#[async_trait]
impl Observer for AwsObserver {
    fn platform(&self) -> Platform {
        Platform::Aws
    }

    async fn discover(&self, _scope: &ObserveScope) -> Result<Vec<ResourceHeartbeat>> {
        let cloudwatch_issue = self.cloudwatch_issue().await;
        let resources = self.discover_tagged_resources().await?;

        Ok(resources
            .into_iter()
            .filter_map(|mapping| self.heartbeat_for_mapping(mapping, cloudwatch_issue.as_ref()))
            .collect())
    }
}

pub fn aws_raw_identity(arn: &str) -> String {
    arn.to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AwsArn<'a> {
    arn: &'a str,
    service: &'a str,
    region: Option<&'a str>,
    account_id: Option<&'a str>,
    resource: &'a str,
}

impl<'a> AwsArn<'a> {
    fn parse(value: &'a str) -> Option<Self> {
        let mut parts = value.splitn(6, ':');
        let prefix = parts.next()?;
        if prefix != "arn" {
            return None;
        }
        let _partition = parts.next()?;
        let service = parts.next()?;
        let region = parts.next().filter(|value| !value.is_empty());
        let account_id = parts.next().filter(|value| !value.is_empty());
        let resource = parts.next()?;

        Some(Self {
            arn: value,
            service,
            region,
            account_id,
            resource,
        })
    }

    fn resource_name(&self) -> String {
        self.resource
            .rsplit(['/', ':'])
            .next()
            .unwrap_or(self.resource)
            .to_string()
    }
}

fn resource_data_for_arn(
    arn: &AwsArn<'_>,
    default_region: &str,
    default_account_id: &str,
    cloudwatch_issue: Option<HeartbeatCollectionIssue>,
) -> Option<ResourceHeartbeatData> {
    let region = arn.region.unwrap_or(default_region).to_string();
    let account_id = arn.account_id.unwrap_or(default_account_id).to_string();
    let name = arn.resource_name();

    match arn.service {
        "s3" => Some(ResourceHeartbeatData::Storage(StorageHeartbeatData::AwsS3(
            AwsS3StorageHeartbeatData {
                status: storage_status(cloudwatch_issue),
                name,
                region: Some(region),
                bucket_location: None,
                versioning_status: None,
                versioning_enabled: None,
                lifecycle_present: false,
                lifecycle_rule_count: None,
                encryption_config_present: false,
                encryption_enabled: None,
                public_access_block_present: false,
                block_public_acls: None,
                ignore_public_acls: None,
                block_public_policy: None,
                restrict_public_buckets: None,
                bucket_policy_present: None,
                bucket_acl_present: None,
            },
        ))),
        "lambda" if arn.resource.starts_with("function:") => Some(ResourceHeartbeatData::Worker(
            WorkerHeartbeatData::AwsLambda(AwsLambdaWorkerHeartbeatData {
                status: workload_status(cloudwatch_issue),
                function_name: name,
                runtime: None,
                package_type: None,
                memory_size_mb: None,
                timeout_seconds: None,
                version: None,
                revision_id: None,
                last_modified: None,
                state: None,
                state_reason: None,
                state_reason_code: None,
                last_update_status: None,
                last_update_status_reason: None,
                last_update_status_reason_code: None,
                code_sha256: None,
                layer_count: 0,
                function_url_auth_type: None,
                function_url_cors_present: false,
                trigger_count: 0,
            }),
        )),
        "sqs" => Some(ResourceHeartbeatData::Queue(QueueHeartbeatData::AwsSqs(
            AwsSqsQueueHeartbeatData {
                status: queue_status(cloudwatch_issue),
                name,
                region: Some(region),
                queue_url: None,
                queue_arn: Some(arn.arn.to_string()),
                visibility_timeout_seconds: None,
                message_retention_period_seconds: None,
                delay_seconds: None,
                receive_message_wait_time_seconds: None,
                maximum_message_size: None,
                redrive_policy: None,
                redrive_allow_policy: None,
                fifo_queue: None,
                content_based_deduplication: None,
                deduplication_scope: None,
                fifo_throughput_limit: None,
                sse_enabled: None,
                kms_master_key_id: None,
                kms_data_key_reuse_period_seconds: None,
                sqs_managed_sse_enabled: None,
                approximate_visible_messages: None,
                approximate_in_flight_messages: None,
                approximate_delayed_messages: None,
                approximate_counts: false,
            },
        ))),
        "dynamodb" if arn.resource.starts_with("table/") => Some(ResourceHeartbeatData::Kv(
            KvHeartbeatData::AwsDynamoDb(AwsDynamoDbKvHeartbeatData {
                status: kv_status(cloudwatch_issue),
                name,
                region: Some(region),
                table_arn: Some(arn.arn.to_string()),
                table_status: None,
                billing_mode: None,
                key_schema: vec![],
                global_secondary_index_count: None,
                local_secondary_index_count: None,
                item_count: None,
                table_size_bytes: None,
                stream_enabled: None,
                stream_view_type: None,
                ttl_status: None,
                ttl_attribute_name: None,
                deletion_protection_enabled: None,
                sse_status: None,
                sse_type: None,
                table_class: None,
                replica_count: None,
                restore_in_progress: None,
            }),
        )),
        "ec2" if arn.resource.starts_with("vpc/") => Some(ResourceHeartbeatData::Network(
            NetworkHeartbeatData::AwsVpc(AwsVpcNetworkHeartbeatData {
                status: network_status(cloudwatch_issue),
                vpc_id: Some(name),
                vpc_state: None,
                cidr_block: None,
                public_subnet_ids: vec![],
                private_subnet_ids: vec![],
                availability_zones: vec![],
                internet_gateway_id: None,
                nat_gateway_id: None,
                route_table_count: 0,
                security_group_id: None,
                is_byo_vpc: true,
            }),
        )),
        "codebuild" if arn.resource.starts_with("project/") => Some(ResourceHeartbeatData::Build(
            BuildHeartbeatData::AwsCodeBuild(AwsCodeBuildHeartbeatData {
                status: build_status(cloudwatch_issue),
                project_name: name,
                project_arn: Some(arn.arn.to_string()),
                description: None,
                source_type: None,
                artifacts_type: None,
                artifacts_encryption_disabled: None,
                environment_type: None,
                environment_image: None,
                compute_type: None,
                image_pull_credentials_type: None,
                privileged_mode: None,
                environment_variable_count: 0,
                service_role_present: false,
                encryption_key_present: false,
                cloud_watch_logs_status: None,
                s3_logs_status: None,
                timeout_in_minutes: None,
                queued_timeout_in_minutes: None,
                created: None,
                last_modified: None,
            }),
        )),
        "ecr" if arn.resource.starts_with("repository/") => Some(
            ResourceHeartbeatData::ArtifactRegistry(ArtifactRegistryHeartbeatData::AwsEcr(
                alien_core::AwsEcrArtifactRegistryHeartbeatData {
                    status: artifact_registry_status(cloudwatch_issue),
                    registry_id: account_id,
                    region,
                    registry_uri: String::new(),
                    repository_prefix: name,
                    pull_role_arn: None,
                    push_role_arn: None,
                    repository_count: 0,
                    repositories_truncated: false,
                    repositories: vec![],
                },
            )),
        ),
        _ => None,
    }
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

fn tags_to_map(tags: &[Tag]) -> BTreeMap<String, String> {
    tags.iter()
        .map(|tag| (tag.key.clone(), tag.value.clone()))
        .collect()
}

fn alien_resource_id_from_tags(
    label_domain: &str,
    tags: &BTreeMap<String, String>,
) -> Option<String> {
    let managed_by_key = branded_tag_key(label_domain, ALIEN_MANAGED_BY_TAG_KEY);
    if tags.get(&managed_by_key).map(String::as_str) != Some(ALIEN_MANAGED_BY_TAG_VALUE) {
        return None;
    }

    let resource_key = branded_tag_key(label_domain, ALIEN_RESOURCE_TAG_KEY);
    tags.get(&resource_key).cloned()
}

fn raw_snippet(resource_arn: &str, tags: &BTreeMap<String, String>) -> RawHeartbeatSnippet {
    RawHeartbeatSnippet {
        source: "aws-resourcegroupstagging:GetResources".to_string(),
        format: RawHeartbeatSnippetFormat::Json,
        collected_at: Utc::now(),
        body: json!({
            "ResourceARN": resource_arn,
            "Tags": tags,
        })
        .to_string(),
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
            "Observed from AWS tag inventory; detailed storage reads not yet collected".to_string(),
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
            "Observed from AWS tag inventory; detailed workload reads not yet collected"
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
            "Observed from AWS tag inventory; detailed queue reads not yet collected".to_string(),
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
            "Observed from AWS tag inventory; detailed key-value reads not yet collected"
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
            "Observed from AWS tag inventory; detailed network reads not yet collected".to_string(),
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
            "Observed from AWS tag inventory; detailed build reads not yet collected".to_string(),
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
            "Observed from AWS tag inventory; detailed registry reads not yet collected"
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
    use alien_aws_clients::cloudwatch::{
        GetMetricDataRequest, GetMetricDataResponse, ListMetricsResponse,
    };
    use alien_aws_clients::resourcegroupstagging::GetResourcesResponse;

    #[test]
    fn parses_aws_arn_resource_name() {
        let arn = AwsArn::parse("arn:aws:lambda:us-east-1:111111111111:function:api").unwrap();

        assert_eq!(arn.service, "lambda");
        assert_eq!(arn.region, Some("us-east-1"));
        assert_eq!(arn.account_id, Some("111111111111"));
        assert_eq!(arn.resource_name(), "api");
    }

    #[test]
    fn maps_s3_arn_to_storage_heartbeat() {
        let arn = AwsArn::parse("arn:aws:s3:::example-bucket").unwrap();
        let data = resource_data_for_arn(&arn, "us-east-1", "111111111111", None).unwrap();

        match data {
            ResourceHeartbeatData::Storage(StorageHeartbeatData::AwsS3(data)) => {
                assert_eq!(data.name, "example-bucket");
                assert!(data.status.partial);
            }
            other => panic!("expected AWS S3 storage heartbeat, got {other:?}"),
        }
    }

    #[test]
    fn maps_lambda_arn_to_worker_heartbeat() {
        let arn = AwsArn::parse("arn:aws:lambda:us-east-1:111111111111:function:api").unwrap();
        let data = resource_data_for_arn(&arn, "us-east-1", "111111111111", None).unwrap();

        match data {
            ResourceHeartbeatData::Worker(WorkerHeartbeatData::AwsLambda(data)) => {
                assert_eq!(data.function_name, "api");
                assert!(data.status.partial);
            }
            other => panic!("expected AWS Lambda worker heartbeat, got {other:?}"),
        }
    }

    #[test]
    fn alien_resource_id_requires_managed_by_tag() {
        let tags = tags_to_map(&[
            Tag {
                key: branded_tag_key(DEFAULT_ALIEN_LABEL_DOMAIN, ALIEN_RESOURCE_TAG_KEY),
                value: "storage.logs".to_string(),
            },
            Tag {
                key: branded_tag_key(DEFAULT_ALIEN_LABEL_DOMAIN, ALIEN_MANAGED_BY_TAG_KEY),
                value: ALIEN_MANAGED_BY_TAG_VALUE.to_string(),
            },
        ]);

        assert_eq!(
            alien_resource_id_from_tags(DEFAULT_ALIEN_LABEL_DOMAIN, &tags).as_deref(),
            Some("storage.logs")
        );
    }

    #[test]
    fn alien_labeled_cloud_resources_are_skipped() {
        let observer = AwsObserver::new(AwsObserveContext {
            deployment_id: "dep_1".to_string(),
            account_id: "111111111111".to_string(),
            region: "us-east-1".to_string(),
            resource_groups_tagging_client: Arc::new(DummyTaggingClient),
            cloudwatch_client: Arc::new(DummyCloudWatchClient),
        });
        let mapping = ResourceTagMapping {
            resource_arn: "arn:aws:lambda:us-east-1:111111111111:function:api".to_string(),
            tags: vec![
                Tag {
                    key: branded_tag_key(DEFAULT_ALIEN_LABEL_DOMAIN, ALIEN_RESOURCE_TAG_KEY),
                    value: "worker.api".to_string(),
                },
                Tag {
                    key: branded_tag_key(DEFAULT_ALIEN_LABEL_DOMAIN, ALIEN_MANAGED_BY_TAG_KEY),
                    value: ALIEN_MANAGED_BY_TAG_VALUE.to_string(),
                },
            ],
            compliance_details: None,
        };

        assert!(observer.heartbeat_for_mapping(mapping, None).is_none());
    }

    #[derive(Debug)]
    struct DummyTaggingClient;

    #[async_trait]
    impl ResourceGroupsTaggingApi for DummyTaggingClient {
        async fn get_resources(
            &self,
            _request: GetResourcesRequest,
        ) -> alien_client_core::Result<GetResourcesResponse> {
            unreachable!("not called by mapping test")
        }
    }

    #[derive(Debug)]
    struct DummyCloudWatchClient;

    #[async_trait]
    impl CloudWatchApi for DummyCloudWatchClient {
        async fn get_metric_data(
            &self,
            _request: GetMetricDataRequest,
        ) -> alien_client_core::Result<GetMetricDataResponse> {
            unreachable!("not called by mapping test")
        }

        async fn list_metrics(
            &self,
            _request: ListMetricsRequest,
        ) -> alien_client_core::Result<ListMetricsResponse> {
            unreachable!("not called by mapping test")
        }
    }
}
