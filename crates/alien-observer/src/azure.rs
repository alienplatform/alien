use std::collections::BTreeMap;
use std::sync::Arc;

use alien_azure_clients::monitor::ListMetricsRequest;
use alien_azure_clients::resource_graph::{
    ResourceGraphQueryOptions, ResourceGraphQueryRequest, ResourceGraphResource,
};
use alien_azure_clients::{MonitorApi, ResourceGraphApi};
use alien_core::{
    branded_tag_key, ArtifactRegistryHeartbeatData, ArtifactRegistryHeartbeatStatus,
    AzureContainerAppsEnvironmentHeartbeatData, AzureContainerAppsEnvironmentHeartbeatStatus,
    AzureContainerAppsWorkerHeartbeatData, AzureContainerRegistryHeartbeatData,
    AzureServiceBusNamespaceHeartbeatData, AzureServiceBusQueueHeartbeatData,
    AzureStorageAccountEndpoints, AzureStorageAccountHeartbeatData, AzureVnetNetworkHeartbeatData,
    HeartbeatBackend, HeartbeatCollectionIssue, HeartbeatCollectionIssueReason,
    HeartbeatIssueSeverity, HeartbeatSource, NetworkHeartbeatData, NetworkHeartbeatStatus,
    ObservedHealth, Platform, ProviderLifecycleState, QueueHeartbeatData, QueueHeartbeatStatus,
    RawHeartbeatSnippet, RawHeartbeatSnippetFormat, ResourceHeartbeat, ResourceHeartbeatData,
    ResourceType, WorkerHeartbeatData, WorkloadHeartbeatStatus, ALIEN_MANAGED_BY_TAG_KEY,
    ALIEN_MANAGED_BY_TAG_VALUE, ALIEN_RESOURCE_TAG_KEY, DEFAULT_ALIEN_LABEL_DOMAIN,
};
use async_trait::async_trait;
use chrono::Utc;
use tracing::warn;

use crate::{ObserveScope, Observer, Result};

const MAX_RESOURCE_GRAPH_PAGES: usize = 20;

#[derive(Clone)]
pub struct AzureObserveContext {
    pub deployment_id: String,
    pub subscription_id: String,
    pub resource_graph_client: Arc<dyn ResourceGraphApi>,
    pub monitor_client: Arc<dyn MonitorApi>,
}

pub struct AzureObserver {
    context: AzureObserveContext,
    label_domain: String,
}

impl AzureObserver {
    pub fn new(context: AzureObserveContext) -> Self {
        Self {
            context,
            label_domain: DEFAULT_ALIEN_LABEL_DOMAIN.to_string(),
        }
    }

    pub fn with_label_domain(
        context: AzureObserveContext,
        label_domain: impl Into<String>,
    ) -> Self {
        Self {
            context,
            label_domain: label_domain.into(),
        }
    }

    async fn monitor_issue(
        &self,
        resources: &[ResourceGraphResource],
    ) -> Option<HeartbeatCollectionIssue> {
        let resource_uri = resources.iter().find_map(|resource| resource.id.clone())?;
        let mut request = ListMetricsRequest::new(resource_uri);
        request.result_type = Some("Metadata".to_string());
        request.top = Some(1);
        request.auto_adjust_timegrain = Some(true);
        request.validate_dimensions = Some(false);

        match self.context.monitor_client.list_metrics(request).await {
            Ok(_) => None,
            Err(error) => {
                warn!(error = %error, "Azure Monitor observe probe failed");
                Some(collection_issue(
                    "azure-monitor",
                    HeartbeatCollectionIssueReason::Forbidden,
                    HeartbeatIssueSeverity::Warning,
                    "Azure Monitor metrics are unavailable; grant Reader on the observed scope",
                ))
            }
        }
    }

    async fn discover_resources(&self) -> Result<Vec<ResourceGraphResource>> {
        let mut resources = Vec::new();
        let mut skip_token = None;

        for _ in 0..MAX_RESOURCE_GRAPH_PAGES {
            let mut request = ResourceGraphQueryRequest::for_subscription(
                self.context.subscription_id.clone(),
                observed_resource_graph_query(),
            );
            request.options = Some(ResourceGraphQueryOptions {
                top: Some(1000),
                skip_token: skip_token.clone(),
                result_format: Some("objectArray".to_string()),
            });

            let response = self.context.resource_graph_client.resources(request).await;
            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    warn!(error = %error, "Azure Resource Graph observe pass failed");
                    return Ok(resources);
                }
            };

            resources.extend(response.data);
            skip_token = response.skip_token.filter(|token| !token.is_empty());
            if skip_token.is_none() {
                break;
            }
        }

        Ok(resources)
    }

    fn heartbeat_for_resource(
        &self,
        resource: ResourceGraphResource,
        monitor_issue: Option<&HeartbeatCollectionIssue>,
    ) -> Option<ResourceHeartbeat> {
        let alien_resource_id = alien_resource_id_from_tags(&self.label_domain, &resource.tags);
        if alien_resource_id.is_some() {
            return None;
        }

        let resource_id = resource.id.clone()?;
        let data = resource_data_for_graph_resource(&resource, monitor_issue.cloned())?;

        Some(ResourceHeartbeat {
            deployment_id: Some(self.context.deployment_id.clone()),
            resource_id: azure_raw_identity(&resource_id),
            source: HeartbeatSource::Observed,
            alien_resource_id,
            resource_type: resource_type_for_data(&data),
            controller_platform: Platform::Azure,
            backend: HeartbeatBackend::Azure,
            observed_at: Utc::now(),
            data,
            raw: vec![raw_snippet(&resource)],
        })
    }
}

#[async_trait]
impl Observer for AzureObserver {
    fn platform(&self) -> Platform {
        Platform::Azure
    }

    async fn discover(&self, _scope: &ObserveScope) -> Result<Vec<ResourceHeartbeat>> {
        let resources = self.discover_resources().await?;
        let monitor_issue = self.monitor_issue(&resources).await;

        Ok(resources
            .into_iter()
            .filter_map(|resource| self.heartbeat_for_resource(resource, monitor_issue.as_ref()))
            .collect())
    }
}

pub fn azure_raw_identity(id: &str) -> String {
    id.to_string()
}

fn observed_resource_graph_query() -> String {
    [
        "Resources",
        "| where type in~ (",
        "'microsoft.storage/storageaccounts',",
        "'microsoft.app/containerapps',",
        "'microsoft.app/managedenvironments',",
        "'microsoft.servicebus/namespaces',",
        "'microsoft.servicebus/namespaces/queues',",
        "'microsoft.network/virtualnetworks',",
        "'microsoft.containerregistry/registries'",
        ")",
        "| project id, name, type, location, resourceGroup, subscriptionId, tags, kind, sku, properties",
    ]
    .join(" ")
}

fn resource_data_for_graph_resource(
    resource: &ResourceGraphResource,
    monitor_issue: Option<HeartbeatCollectionIssue>,
) -> Option<ResourceHeartbeatData> {
    let type_ = resource.type_.as_deref()?.to_ascii_lowercase();
    let id = resource.id.clone()?;
    let name = resource.name.clone().unwrap_or_else(|| resource_leaf(&id));
    let resource_group = resource
        .resource_group
        .clone()
        .or_else(|| resource_group_from_id(&id).map(str::to_string));
    let location = resource.location.clone();

    match type_.as_str() {
        "microsoft.storage/storageaccounts" => Some(ResourceHeartbeatData::AzureStorageAccount(
            AzureStorageAccountHeartbeatData {
                status: storage_status(monitor_issue),
                name,
                resource_id: Some(id),
                resource_group,
                location,
                kind: resource.kind.clone(),
                sku_name: sku_string(resource, "name"),
                sku_tier: sku_string(resource, "tier"),
                provisioning_state: property_string(resource, &["provisioningState"]),
                primary_endpoints: AzureStorageAccountEndpoints::default(),
                secondary_endpoints: AzureStorageAccountEndpoints::default(),
                public_network_access: property_string(resource, &["publicNetworkAccess"]),
                allow_blob_public_access: property_bool(resource, &["allowBlobPublicAccess"]),
                allow_shared_key_access: property_bool(resource, &["allowSharedKeyAccess"]),
                minimum_tls_version: property_string(resource, &["minimumTlsVersion"]),
                supports_https_traffic_only: property_bool(resource, &["supportsHttpsTrafficOnly"]),
                encryption_key_source: property_string(resource, &["encryption", "keySource"]),
                require_infrastructure_encryption: property_bool(
                    resource,
                    &["encryption", "requireInfrastructureEncryption"],
                ),
                network_default_action: property_string(
                    resource,
                    &["networkAcls", "defaultAction"],
                ),
                network_bypass: property_string(resource, &["networkAcls", "bypass"]),
                network_ip_rule_count: property_array_len(resource, &["networkAcls", "ipRules"]),
                network_virtual_network_rule_count: property_array_len(
                    resource,
                    &["networkAcls", "virtualNetworkRules"],
                ),
                network_resource_access_rule_count: property_array_len(
                    resource,
                    &["networkAcls", "resourceAccessRules"],
                ),
            },
        )),
        "microsoft.app/containerapps" => Some(ResourceHeartbeatData::Worker(
            WorkerHeartbeatData::AzureContainerApps(AzureContainerAppsWorkerHeartbeatData {
                status: workload_status(monitor_issue),
                app_name: name,
                revision: property_string(resource, &["latestRevisionName"]),
                environment_name: property_string(resource, &["environmentId"])
                    .map(|value| resource_leaf(&value)),
                provisioning_state: property_string(resource, &["provisioningState"]),
                running_status: property_string(resource, &["runningStatus"]),
                ingress_fqdn: property_string(resource, &["configuration", "ingress", "fqdn"]),
                min_replicas: property_i32(resource, &["template", "scale", "minReplicas"]),
                max_replicas: property_i32(resource, &["template", "scale", "maxReplicas"]),
                cpu: None,
                memory: None,
            }),
        )),
        "microsoft.app/managedenvironments" => {
            Some(ResourceHeartbeatData::AzureContainerAppsEnvironment(
                AzureContainerAppsEnvironmentHeartbeatData {
                    status: container_apps_environment_status(monitor_issue),
                    name,
                    resource_id: Some(id),
                    resource_group,
                    location,
                    kind: resource.kind.clone(),
                    provisioning_state: property_string(resource, &["provisioningState"]),
                    default_domain: property_string(resource, &["defaultDomain"]),
                    static_ip: property_string(resource, &["staticIp"]),
                    custom_domain_verification_id: property_string(
                        resource,
                        &["customDomainConfiguration", "customDomainVerificationId"],
                    ),
                    infrastructure_resource_group: property_string(
                        resource,
                        &["infrastructureResourceGroup"],
                    ),
                    event_stream_endpoint: property_string(resource, &["eventStreamEndpoint"]),
                    zone_redundant: property_bool(resource, &["zoneRedundant"]),
                    workload_profile_count: property_array_len(resource, &["workloadProfiles"])
                        .unwrap_or(0),
                    workload_profiles: vec![],
                },
            ))
        }
        "microsoft.servicebus/namespaces" => Some(ResourceHeartbeatData::AzureServiceBusNamespace(
            AzureServiceBusNamespaceHeartbeatData {
                status: queue_status(monitor_issue),
                name,
                resource_id: Some(id),
                resource_group,
                location,
                sku_name: sku_string(resource, "name"),
                sku_tier: sku_string(resource, "tier"),
                sku_capacity: sku_i32(resource, "capacity"),
                namespace_status: property_string(resource, &["status"]),
                provisioning_state: property_string(resource, &["provisioningState"]),
                service_bus_endpoint: property_string(resource, &["serviceBusEndpoint"]),
                metric_id: property_string(resource, &["metricId"]),
                public_network_access: property_string(resource, &["publicNetworkAccess"]),
                disable_local_auth: property_bool(resource, &["disableLocalAuth"]),
                minimum_tls_version: property_string(resource, &["minimumTlsVersion"]),
                premium_messaging_partitions: property_i32(
                    resource,
                    &["premiumMessagingPartitions"],
                ),
                private_endpoint_connection_count: property_array_len(
                    resource,
                    &["privateEndpointConnections"],
                )
                .unwrap_or(0),
                zone_redundant: property_bool(resource, &["zoneRedundant"]),
                created_at: property_string(resource, &["createdAt"]),
                updated_at: property_string(resource, &["updatedAt"]),
            },
        )),
        "microsoft.servicebus/namespaces/queues" => {
            let namespace_name = parent_name_for_child_type(&id, "namespaces")
                .unwrap_or_else(|| resource_group.clone().unwrap_or_default());
            Some(ResourceHeartbeatData::Queue(
                QueueHeartbeatData::AzureServiceBus(AzureServiceBusQueueHeartbeatData {
                    status: queue_status(monitor_issue),
                    name,
                    namespace_name,
                    resource_group,
                    resource_id: Some(id),
                    endpoint: None,
                    queue_status: property_string(resource, &["status"]),
                    lock_duration: property_string(resource, &["lockDuration"]),
                    max_delivery_count: property_u32(resource, &["maxDeliveryCount"]),
                    requires_duplicate_detection: property_bool(
                        resource,
                        &["requiresDuplicateDetection"],
                    ),
                    duplicate_detection_history_time_window: property_string(
                        resource,
                        &["duplicateDetectionHistoryTimeWindow"],
                    ),
                    requires_session: property_bool(resource, &["requiresSession"]),
                    dead_lettering_on_message_expiration: property_bool(
                        resource,
                        &["deadLetteringOnMessageExpiration"],
                    ),
                    forward_dead_lettered_messages_to: property_string(
                        resource,
                        &["forwardDeadLetteredMessagesTo"],
                    ),
                    forward_to: property_string(resource, &["forwardTo"]),
                    default_message_time_to_live: property_string(
                        resource,
                        &["defaultMessageTimeToLive"],
                    ),
                    auto_delete_on_idle: property_string(resource, &["autoDeleteOnIdle"]),
                    enable_batched_operations: property_bool(
                        resource,
                        &["enableBatchedOperations"],
                    ),
                    enable_express: property_bool(resource, &["enableExpress"]),
                    enable_partitioning: property_bool(resource, &["enablePartitioning"]),
                    max_message_size_in_kilobytes: property_u64(
                        resource,
                        &["maxMessageSizeInKilobytes"],
                    ),
                    max_size_in_megabytes: property_u32(resource, &["maxSizeInMegabytes"]),
                    message_count: property_u64(resource, &["countDetails", "activeMessageCount"]),
                    active_message_count: property_u64(
                        resource,
                        &["countDetails", "activeMessageCount"],
                    ),
                    dead_letter_message_count: property_u64(
                        resource,
                        &["countDetails", "deadLetterMessageCount"],
                    ),
                    scheduled_message_count: property_u64(
                        resource,
                        &["countDetails", "scheduledMessageCount"],
                    ),
                    transfer_message_count: property_u64(
                        resource,
                        &["countDetails", "transferMessageCount"],
                    ),
                    transfer_dead_letter_message_count: property_u64(
                        resource,
                        &["countDetails", "transferDeadLetterMessageCount"],
                    ),
                    size_in_bytes: property_u64(resource, &["sizeInBytes"]),
                    accessed_at: property_string(resource, &["accessedAt"]),
                    created_at: property_string(resource, &["createdAt"]),
                    updated_at: property_string(resource, &["updatedAt"]),
                }),
            ))
        }
        "microsoft.network/virtualnetworks" => Some(ResourceHeartbeatData::Network(
            NetworkHeartbeatData::AzureVnet(AzureVnetNetworkHeartbeatData {
                status: network_status(monitor_issue),
                vnet_name: Some(name),
                vnet_resource_id: Some(id),
                resource_group,
                location,
                cidr_block: property_first_string(resource, &["addressSpace", "addressPrefixes"]),
                public_subnet_name: None,
                private_subnet_name: None,
                application_gateway_subnet_name: None,
                nat_gateway_id: None,
                public_ip_id: None,
                nsg_id: None,
                is_byo_vnet: true,
                last_byo_vnet_verification_error_code: None,
            }),
        )),
        "microsoft.containerregistry/registries" => Some(ResourceHeartbeatData::ArtifactRegistry(
            ArtifactRegistryHeartbeatData::AzureContainerRegistry(
                AzureContainerRegistryHeartbeatData {
                    status: artifact_registry_status(monitor_issue),
                    name,
                    resource_id: Some(id),
                    resource_group: resource_group.unwrap_or_default(),
                    location: location.unwrap_or_default(),
                    type_: resource.type_.clone(),
                    login_server: property_string(resource, &["loginServer"]),
                    sku_name: sku_string(resource, "name").unwrap_or_default(),
                    sku_tier: sku_string(resource, "tier"),
                    provisioning_state: property_string(resource, &["provisioningState"]),
                    admin_user_enabled: property_bool(resource, &["adminUserEnabled"])
                        .unwrap_or(false),
                    anonymous_pull_enabled: property_bool(resource, &["anonymousPullEnabled"])
                        .unwrap_or(false),
                    public_network_access: property_string(resource, &["publicNetworkAccess"])
                        .unwrap_or_default(),
                    network_rule_bypass_options: property_string(
                        resource,
                        &["networkRuleBypassOptions"],
                    )
                    .unwrap_or_default(),
                    network_rule_default_action: property_string(
                        resource,
                        &["networkRuleSet", "defaultAction"],
                    ),
                    ip_rule_count: property_array_len(resource, &["networkRuleSet", "ipRules"])
                        .unwrap_or(0),
                    encryption_status: property_string(resource, &["encryption", "status"]),
                    encryption_key_vault_uri_present: property_string(
                        resource,
                        &["encryption", "keyVaultProperties", "keyIdentifier"],
                    )
                    .is_some(),
                    encryption_key_identifier_present: property_string(
                        resource,
                        &["encryption", "keyVaultProperties", "keyIdentifier"],
                    )
                    .is_some(),
                    policies_present: resource
                        .properties
                        .as_ref()
                        .and_then(|value| value.get("policies"))
                        .is_some(),
                    policy_count: resource
                        .properties
                        .as_ref()
                        .and_then(|value| value.get("policies"))
                        .and_then(|value| value.as_object())
                        .map(|value| value.len() as u32)
                        .unwrap_or(0),
                    private_endpoint_connection_count: property_array_len(
                        resource,
                        &["privateEndpointConnections"],
                    )
                    .unwrap_or(0),
                    data_endpoint_enabled: property_bool(resource, &["dataEndpointEnabled"]),
                    data_endpoint_host_names: vec![],
                    zone_redundancy: property_string(resource, &["zoneRedundancy"])
                        .unwrap_or_default(),
                    creation_date: property_string(resource, &["creationDate"]),
                    managed_tag_count: resource.tags.len() as u32,
                },
            ),
        )),
        _ => None,
    }
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

fn resource_type_for_data(data: &ResourceHeartbeatData) -> ResourceType {
    let resource_type = match data {
        ResourceHeartbeatData::AzureStorageAccount(_) => "azure_storage_account",
        ResourceHeartbeatData::AzureContainerAppsEnvironment(_) => {
            "azure_container_apps_environment"
        }
        ResourceHeartbeatData::AzureServiceBusNamespace(_) => "azure_service_bus_namespace",
        ResourceHeartbeatData::Worker(_) => "worker",
        ResourceHeartbeatData::Queue(_) => "queue",
        ResourceHeartbeatData::Network(_) => "network",
        ResourceHeartbeatData::ArtifactRegistry(_) => "artifact-registry",
        _ => "external",
    };
    ResourceType::from_static(resource_type)
}

fn raw_snippet(resource: &ResourceGraphResource) -> RawHeartbeatSnippet {
    RawHeartbeatSnippet {
        source: "azure-resourcegraph:resources".to_string(),
        format: RawHeartbeatSnippetFormat::Json,
        collected_at: Utc::now(),
        body: serde_json::to_string(resource).unwrap_or_else(|_| "{}".to_string()),
        truncated: false,
    }
}

fn resource_leaf(id: &str) -> String {
    id.rsplit('/').next().unwrap_or(id).to_string()
}

fn resource_group_from_id(id: &str) -> Option<&str> {
    let mut parts = id.split('/');
    while let Some(part) = parts.next() {
        if part.eq_ignore_ascii_case("resourceGroups") {
            return parts.next();
        }
    }
    None
}

fn parent_name_for_child_type(id: &str, segment: &str) -> Option<String> {
    let mut parts = id.split('/');
    while let Some(part) = parts.next() {
        if part.eq_ignore_ascii_case(segment) {
            return parts.next().map(str::to_string);
        }
    }
    None
}

fn property_value<'a>(
    resource: &'a ResourceGraphResource,
    path: &[&str],
) -> Option<&'a serde_json::Value> {
    let mut current = resource.properties.as_ref()?;
    for segment in path {
        current = current.get(*segment)?;
    }
    Some(current)
}

fn property_string(resource: &ResourceGraphResource, path: &[&str]) -> Option<String> {
    property_value(resource, path)
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

fn property_bool(resource: &ResourceGraphResource, path: &[&str]) -> Option<bool> {
    property_value(resource, path).and_then(|value| value.as_bool())
}

fn property_i32(resource: &ResourceGraphResource, path: &[&str]) -> Option<i32> {
    property_value(resource, path)
        .and_then(|value| value.as_i64())
        .and_then(|value| i32::try_from(value).ok())
}

fn property_u32(resource: &ResourceGraphResource, path: &[&str]) -> Option<u32> {
    property_value(resource, path)
        .and_then(|value| value.as_u64())
        .and_then(|value| u32::try_from(value).ok())
}

fn property_u64(resource: &ResourceGraphResource, path: &[&str]) -> Option<u64> {
    property_value(resource, path).and_then(|value| value.as_u64())
}

fn property_array_len(resource: &ResourceGraphResource, path: &[&str]) -> Option<u32> {
    property_value(resource, path)
        .and_then(|value| value.as_array())
        .map(|value| value.len() as u32)
}

fn property_first_string(resource: &ResourceGraphResource, path: &[&str]) -> Option<String> {
    property_value(resource, path)
        .and_then(|value| value.as_array())
        .and_then(|values| values.first())
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

fn sku_string(resource: &ResourceGraphResource, key: &str) -> Option<String> {
    resource
        .sku
        .as_ref()
        .and_then(|sku| sku.get(key))
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

fn sku_i32(resource: &ResourceGraphResource, key: &str) -> Option<i32> {
    resource
        .sku
        .as_ref()
        .and_then(|sku| sku.get(key))
        .and_then(|value| value.as_i64())
        .and_then(|value| i32::try_from(value).ok())
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

fn storage_status(issue: Option<HeartbeatCollectionIssue>) -> alien_core::StorageHeartbeatStatus {
    alien_core::StorageHeartbeatStatus {
        health: ObservedHealth::Unknown,
        lifecycle: ProviderLifecycleState::Unknown,
        message: Some(
            "Observed from Azure Resource Graph; detailed storage reads not yet collected"
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
            "Observed from Azure Resource Graph; detailed workload reads not yet collected"
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
            "Observed from Azure Resource Graph; detailed queue reads not yet collected"
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
            "Observed from Azure Resource Graph; detailed network reads not yet collected"
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
            "Observed from Azure Resource Graph; detailed registry reads not yet collected"
                .to_string(),
        ),
        stale: false,
        partial: true,
        collection_issues: issue.into_iter().collect(),
    }
}

fn container_apps_environment_status(
    issue: Option<HeartbeatCollectionIssue>,
) -> AzureContainerAppsEnvironmentHeartbeatStatus {
    AzureContainerAppsEnvironmentHeartbeatStatus {
        health: ObservedHealth::Unknown,
        lifecycle: ProviderLifecycleState::Unknown,
        message: Some(
            "Observed from Azure Resource Graph; detailed environment reads not yet collected"
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
    use alien_azure_clients::monitor::ListMetricsResponse;
    use alien_azure_clients::resource_graph::ResourceGraphQueryResponse;

    #[test]
    fn maps_storage_account_to_azure_storage_heartbeat() {
        let resource = ResourceGraphResource {
            id: Some("/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Storage/storageAccounts/logs".to_string()),
            name: Some("logs".to_string()),
            type_: Some("microsoft.storage/storageaccounts".to_string()),
            location: Some("eastus".to_string()),
            resource_group: Some("rg".to_string()),
            properties: Some(serde_json::json!({
                "provisioningState": "Succeeded",
                "supportsHttpsTrafficOnly": true
            })),
            ..Default::default()
        };

        let data = resource_data_for_graph_resource(&resource, None).unwrap();

        match data {
            ResourceHeartbeatData::AzureStorageAccount(data) => {
                assert_eq!(data.name, "logs");
                assert_eq!(data.provisioning_state.as_deref(), Some("Succeeded"));
                assert_eq!(data.supports_https_traffic_only, Some(true));
                assert!(data.status.partial);
            }
            other => panic!("expected Azure storage account heartbeat, got {other:?}"),
        }
    }

    #[test]
    fn maps_container_app_to_worker_heartbeat() {
        let resource = ResourceGraphResource {
            id: Some(
                "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.App/containerApps/api"
                    .to_string(),
            ),
            name: Some("api".to_string()),
            type_: Some("microsoft.app/containerapps".to_string()),
            properties: Some(serde_json::json!({
                "provisioningState": "Succeeded",
                "runningStatus": "Running",
                "template": { "scale": { "minReplicas": 1, "maxReplicas": 3 } }
            })),
            ..Default::default()
        };

        let data = resource_data_for_graph_resource(&resource, None).unwrap();

        match data {
            ResourceHeartbeatData::Worker(WorkerHeartbeatData::AzureContainerApps(data)) => {
                assert_eq!(data.app_name, "api");
                assert_eq!(data.running_status.as_deref(), Some("Running"));
                assert_eq!(data.min_replicas, Some(1));
                assert_eq!(data.max_replicas, Some(3));
            }
            other => panic!("expected Azure Container Apps worker heartbeat, got {other:?}"),
        }
    }

    #[test]
    fn alien_labeled_cloud_resources_are_skipped() {
        let observer = AzureObserver::new(AzureObserveContext {
            deployment_id: "dep_1".to_string(),
            subscription_id: "sub".to_string(),
            resource_graph_client: Arc::new(DummyResourceGraphClient),
            monitor_client: Arc::new(DummyMonitorClient),
        });
        let resource = ResourceGraphResource {
            id: Some(
                "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.App/containerApps/api"
                    .to_string(),
            ),
            name: Some("api".to_string()),
            type_: Some("microsoft.app/containerapps".to_string()),
            tags: BTreeMap::from([
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
    struct DummyResourceGraphClient;

    #[async_trait]
    impl ResourceGraphApi for DummyResourceGraphClient {
        async fn resources(
            &self,
            _request: ResourceGraphQueryRequest,
        ) -> alien_client_core::Result<ResourceGraphQueryResponse> {
            unreachable!("not called by mapping test")
        }
    }

    #[derive(Debug)]
    struct DummyMonitorClient;

    #[async_trait]
    impl MonitorApi for DummyMonitorClient {
        async fn list_metrics(
            &self,
            _request: ListMetricsRequest,
        ) -> alien_client_core::Result<ListMetricsResponse> {
            unreachable!("not called by mapping test")
        }
    }
}
