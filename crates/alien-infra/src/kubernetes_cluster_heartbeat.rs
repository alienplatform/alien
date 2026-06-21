use std::collections::HashMap;

use alien_core::{
    HeartbeatBackend, HeartbeatCollectionIssue, HeartbeatCollectionIssueReason,
    HeartbeatIssueSeverity, KubernetesCluster, KubernetesClusterHeartbeatData,
    KubernetesClusterNodeStatus, KubernetesEventInvolvedObject, KubernetesEventSnapshot,
    KubernetesEventSource, KubernetesNodeConditionStatus, KubernetesNodeResources,
    KubernetesNodeUsage, MetricSample, MetricUnit, ObservedCounts, ObservedHealth, Platform,
    ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, WorkloadHeartbeatStatus,
};
use alien_error::Context;
use alien_k8s_clients::{
    optional_events_read, optional_metrics_read, optional_nodes_read, OptionalKubernetesReadStatus,
};
use k8s_openapi::api::core::v1::{Event, Node, Pod};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::chrono::Utc;

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};

pub struct KubernetesClusterHeartbeatInput<'a> {
    pub config: &'a KubernetesCluster,
    pub cluster_name: Option<&'a str>,
    pub api_reachable: bool,
    pub namespace_ready: bool,
    pub rbac_ready: bool,
    pub agent_ready: bool,
    pub status_message: Option<String>,
}

pub async fn emit_kubernetes_cluster_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    input: KubernetesClusterHeartbeatInput<'_>,
) -> Result<()> {
    let kubernetes_config = ctx.get_kubernetes_config()?;
    let pod_client = ctx
        .service_provider
        .get_kubernetes_pod_client(kubernetes_config)
        .await?;
    let event_client = ctx
        .service_provider
        .get_kubernetes_event_client(kubernetes_config)
        .await?;
    let node_client = ctx
        .service_provider
        .get_kubernetes_node_client(kubernetes_config)
        .await?;
    let metrics_client = ctx
        .service_provider
        .get_kubernetes_metrics_client(kubernetes_config)
        .await?;
    let version_client = ctx
        .service_provider
        .get_kubernetes_version_client(kubernetes_config)
        .await?;

    let pods = pod_client
        .list_pods(&input.config.namespace, None, None)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to list pods for Kubernetes cluster heartbeat in namespace '{}'",
                input.config.namespace
            ),
            resource_id: Some(input.config.id.clone()),
        })?;

    let events_read = optional_events_read(
        &input.config.id,
        &input.config.namespace,
        None,
        event_client.list_events(&input.config.namespace, None),
    )
    .await
    .context(ErrorData::CloudPlatformError {
        message: "Failed optional Kubernetes cluster event collection".to_string(),
        resource_id: Some(input.config.id.clone()),
    })?;

    let pod_metrics = optional_metrics_read(
        &input.config.id,
        Some(&input.config.namespace),
        None,
        metrics_client.list_pod_metrics(&input.config.namespace, None),
    )
    .await
    .context(ErrorData::CloudPlatformError {
        message: "Failed optional Kubernetes pod metrics collection".to_string(),
        resource_id: Some(input.config.id.clone()),
    })?;

    let nodes = optional_nodes_read(&input.config.id, node_client.list_nodes(None, None))
        .await
        .context(ErrorData::CloudPlatformError {
            message: "Failed optional Kubernetes node collection".to_string(),
            resource_id: Some(input.config.id.clone()),
        })?;

    let node_metrics = optional_metrics_read(
        &input.config.id,
        None,
        None,
        metrics_client.list_node_metrics(None),
    )
    .await
    .context(ErrorData::CloudPlatformError {
        message: "Failed optional Kubernetes node metrics collection".to_string(),
        resource_id: Some(input.config.id.clone()),
    })?;

    let version = match version_client.get_version().await {
        Ok(version) => version.git_version,
        Err(error) => {
            tracing::debug!(
                resource_id = %input.config.id,
                error = %error,
                "Kubernetes API server version collection failed"
            );
            None
        }
    };

    let node_usage_by_name = node_metrics
        .value
        .as_ref()
        .map(|metrics| {
            metrics
                .items
                .iter()
                .filter_map(|metric| {
                    metric.metadata.name.as_ref().map(|name| {
                        (
                            name.clone(),
                            KubernetesNodeUsage {
                                cpu: metric.usage.get("cpu").and_then(quantity_cpu_cores).map(
                                    |value| MetricSample {
                                        value,
                                        unit: MetricUnit::Cores,
                                    },
                                ),
                                memory: metric.usage.get("memory").and_then(quantity_bytes).map(
                                    |value| MetricSample {
                                        value,
                                        unit: MetricUnit::Bytes,
                                    },
                                ),
                            },
                        )
                    })
                })
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();

    let node_rows = nodes
        .value
        .as_ref()
        .map(|list| {
            list.items
                .iter()
                .map(|node| node_status(node, node_usage_by_name.get(node_name(node))))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let event_status = events_read.status.clone();
    let events = events_read
        .value
        .as_ref()
        .map(|list| {
            list.items
                .iter()
                .filter_map(kubernetes_event_snapshot)
                .take(20)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut collection_issues = Vec::new();
    push_collection_issue("metrics", &pod_metrics.status, &mut collection_issues);
    push_collection_issue("events", &event_status, &mut collection_issues);
    push_collection_issue("nodes", &nodes.status, &mut collection_issues);
    push_collection_issue("nodeMetrics", &node_metrics.status, &mut collection_issues);

    let data = cluster_data(
        &input,
        &pods.items,
        &node_rows,
        version,
        events,
        &pod_metrics.status,
        &event_status,
        &nodes.status,
        &node_metrics.status,
        collection_issues,
    );

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: input.config.id.clone(),
        resource_type: KubernetesCluster::RESOURCE_TYPE,
        controller_platform: Platform::Kubernetes,
        backend: HeartbeatBackend::Kubernetes,
            source: Default::default(),
            alien_resource_id: None,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::KubernetesCluster(data),
        raw: vec![],
    });

    Ok(())
}

fn cluster_data(
    input: &KubernetesClusterHeartbeatInput<'_>,
    pods: &[Pod],
    nodes: &[KubernetesClusterNodeStatus],
    version: Option<String>,
    events: Vec<KubernetesEventSnapshot>,
    metrics_status: &OptionalKubernetesReadStatus,
    events_status: &OptionalKubernetesReadStatus,
    nodes_status: &OptionalKubernetesReadStatus,
    node_metrics_status: &OptionalKubernetesReadStatus,
    collection_issues: Vec<HeartbeatCollectionIssue>,
) -> KubernetesClusterHeartbeatData {
    let ready_nodes = nodes.iter().filter(|node| node.ready).count() as u32;
    let node_count = nodes.len() as u32;
    let ready_pods = pods.iter().filter(|pod| pod_ready(pod)).count() as u32;
    let pod_count = pods.len() as u32;
    let partial = !metrics_status.available
        || !events_status.available
        || !nodes_status.available
        || !node_metrics_status.available;
    let health = if !input.api_reachable
        || !input.namespace_ready
        || !input.rbac_ready
        || !input.agent_ready
    {
        ObservedHealth::Unhealthy
    } else if nodes_status.available && node_count > 0 && ready_nodes == 0 {
        ObservedHealth::Degraded
    } else {
        ObservedHealth::Healthy
    };

    KubernetesClusterHeartbeatData {
        status: WorkloadHeartbeatStatus {
            health,
            lifecycle: ProviderLifecycleState::Running,
            message: input.status_message.clone(),
            stale: false,
            partial,
            collection_issues,
        },
        node_counts: ObservedCounts {
            desired: nodes_status.available.then_some(node_count),
            current: nodes_status.available.then_some(node_count),
            ready: nodes_status.available.then_some(ready_nodes),
        },
        pod_counts: ObservedCounts {
            desired: None,
            current: Some(pod_count),
            ready: Some(ready_pods),
        },
        cpu: metric_sum(nodes.iter().filter_map(|node| node.usage.as_ref()?.cpu)),
        memory: metric_sum(nodes.iter().filter_map(|node| node.usage.as_ref()?.memory)),
        name: input
            .cluster_name
            .unwrap_or(input.config.id.as_str())
            .to_string(),
        region: None,
        namespace: Some(input.config.namespace.clone()),
        version,
        node_statuses: nodes.to_vec(),
        events,
    }
}

fn push_collection_issue(
    source: &str,
    status: &OptionalKubernetesReadStatus,
    issues: &mut Vec<HeartbeatCollectionIssue>,
) {
    if status.available {
        return;
    }
    issues.push(HeartbeatCollectionIssue {
        source: source.to_string(),
        reason: status
            .reason
            .unwrap_or(HeartbeatCollectionIssueReason::CollectionFailed),
        severity: HeartbeatIssueSeverity::Warning,
        message: status
            .message
            .clone()
            .unwrap_or_else(|| format!("{source} collection is unavailable")),
    });
}

fn node_status(node: &Node, usage: Option<&KubernetesNodeUsage>) -> KubernetesClusterNodeStatus {
    KubernetesClusterNodeStatus {
        name: node_name(node).to_string(),
        uid: node.metadata.uid.clone(),
        ready: node_ready(node),
        conditions: node_conditions(node),
        roles: node_roles(node),
        labels: node.metadata.labels.clone().unwrap_or_default(),
        allocatable: node
            .status
            .as_ref()
            .and_then(|status| status.allocatable.as_ref())
            .map(quantity_map_resources)
            .unwrap_or_default(),
        capacity: node
            .status
            .as_ref()
            .and_then(|status| status.capacity.as_ref())
            .map(quantity_map_resources)
            .unwrap_or_default(),
        usage: usage.cloned(),
        kubelet_version: node
            .status
            .as_ref()
            .and_then(|status| status.node_info.as_ref())
            .map(|info| info.kubelet_version.clone()),
        container_runtime_version: node
            .status
            .as_ref()
            .and_then(|status| status.node_info.as_ref())
            .map(|info| info.container_runtime_version.clone()),
    }
}

fn node_conditions(node: &Node) -> Vec<KubernetesNodeConditionStatus> {
    node.status
        .as_ref()
        .and_then(|status| status.conditions.as_ref())
        .map(|conditions| {
            conditions
                .iter()
                .map(|condition| KubernetesNodeConditionStatus {
                    type_: condition.type_.clone(),
                    status: condition.status.clone(),
                    reason: condition.reason.clone(),
                    message: condition.message.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn node_name(node: &Node) -> &str {
    node.metadata.name.as_deref().unwrap_or("")
}

fn node_ready(node: &Node) -> bool {
    node.status
        .as_ref()
        .and_then(|status| status.conditions.as_ref())
        .and_then(|conditions| {
            conditions
                .iter()
                .find(|condition| condition.type_ == "Ready")
                .map(|condition| condition.status == "True")
        })
        .unwrap_or(false)
}

fn node_roles(node: &Node) -> Vec<String> {
    let mut roles = node
        .metadata
        .labels
        .as_ref()
        .map(|labels| {
            labels
                .keys()
                .filter_map(|key| key.strip_prefix("node-role.kubernetes.io/"))
                .map(|role| {
                    if role.is_empty() {
                        "control-plane".to_string()
                    } else {
                        role.to_string()
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    roles.sort();
    roles.dedup();
    roles
}

fn quantity_map_resources(
    values: &std::collections::BTreeMap<String, Quantity>,
) -> KubernetesNodeResources {
    KubernetesNodeResources {
        cpu: values
            .get("cpu")
            .and_then(quantity_cpu_cores)
            .map(|value| MetricSample {
                value,
                unit: MetricUnit::Cores,
            }),
        memory: values
            .get("memory")
            .and_then(quantity_bytes)
            .map(|value| MetricSample {
                value,
                unit: MetricUnit::Bytes,
            }),
        pods: values
            .get("pods")
            .and_then(|quantity| quantity.0.parse().ok()),
    }
}

fn pod_ready(pod: &Pod) -> bool {
    let statuses = pod
        .status
        .as_ref()
        .and_then(|status| status.container_statuses.as_deref())
        .unwrap_or(&[]);
    !statuses.is_empty() && statuses.iter().all(|status| status.ready)
}

fn kubernetes_event_snapshot(event: &Event) -> Option<KubernetesEventSnapshot> {
    Some(KubernetesEventSnapshot {
        reason: event
            .reason
            .clone()
            .unwrap_or_else(|| "KubernetesEvent".to_string()),
        type_: event.type_.clone(),
        message: event.message.clone().unwrap_or_default(),
        count: event.count,
        first_timestamp: event.first_timestamp.as_ref().map(|time| time.0),
        last_timestamp: event.last_timestamp.as_ref().map(|time| time.0),
        event_time: event.event_time.as_ref().map(|time| time.0),
        source: event.source.as_ref().map(|source| KubernetesEventSource {
            component: source.component.clone(),
            host: source.host.clone(),
        }),
        involved_object: Some(KubernetesEventInvolvedObject {
            kind: event.involved_object.kind.clone(),
            namespace: event.involved_object.namespace.clone(),
            name: event.involved_object.name.clone(),
            uid: event.involved_object.uid.clone(),
            api_version: event.involved_object.api_version.clone(),
            resource_version: event.involved_object.resource_version.clone(),
            field_path: event.involved_object.field_path.clone(),
        }),
        raw: serde_json::to_value(event).ok(),
    })
}

fn quantity_cpu_cores(quantity: &Quantity) -> Option<f64> {
    let value = quantity.0.trim();
    if let Some(millis) = value.strip_suffix('m') {
        return millis.parse::<f64>().ok().map(|value| value / 1000.0);
    }
    if let Some(nanos) = value.strip_suffix('n') {
        return nanos
            .parse::<f64>()
            .ok()
            .map(|value| value / 1_000_000_000.0);
    }
    value.parse::<f64>().ok()
}

fn quantity_bytes(quantity: &Quantity) -> Option<f64> {
    let value = quantity.0.trim();
    parse_scaled_quantity(value, "Ki", 1024.0)
        .or_else(|| parse_scaled_quantity(value, "Mi", 1024.0 * 1024.0))
        .or_else(|| parse_scaled_quantity(value, "Gi", 1024.0 * 1024.0 * 1024.0))
        .or_else(|| parse_scaled_quantity(value, "K", 1000.0))
        .or_else(|| parse_scaled_quantity(value, "M", 1000.0 * 1000.0))
        .or_else(|| parse_scaled_quantity(value, "G", 1000.0 * 1000.0 * 1000.0))
        .or_else(|| value.parse::<f64>().ok())
}

fn parse_scaled_quantity(value: &str, suffix: &str, multiplier: f64) -> Option<f64> {
    value
        .strip_suffix(suffix)
        .and_then(|value| value.parse::<f64>().ok())
        .map(|value| value * multiplier)
}

fn metric_sum(metrics: impl Iterator<Item = MetricSample>) -> Option<MetricSample> {
    let mut total = 0.0;
    let mut unit = None;
    for metric in metrics {
        total += metric.value;
        unit = Some(metric.unit);
    }
    unit.map(|unit| MetricSample { value: total, unit })
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{NodeCondition, NodeStatus};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::collections::BTreeMap;

    #[test]
    fn node_status_maps_ready_roles_capacity_and_usage() {
        let mut labels = BTreeMap::new();
        labels.insert("node-role.kubernetes.io/worker".to_string(), "".to_string());

        let mut capacity = BTreeMap::new();
        capacity.insert("cpu".to_string(), Quantity("4".to_string()));
        capacity.insert("memory".to_string(), Quantity("8Gi".to_string()));
        capacity.insert("pods".to_string(), Quantity("110".to_string()));

        let usage = KubernetesNodeUsage {
            cpu: Some(MetricSample {
                value: 0.5,
                unit: MetricUnit::Cores,
            }),
            memory: Some(MetricSample {
                value: 1024.0,
                unit: MetricUnit::Bytes,
            }),
        };

        let node = Node {
            metadata: ObjectMeta {
                name: Some("node-1".to_string()),
                uid: Some("node-uid".to_string()),
                labels: Some(labels),
                ..Default::default()
            },
            status: Some(NodeStatus {
                capacity: Some(capacity.clone()),
                allocatable: Some(capacity),
                conditions: Some(vec![NodeCondition {
                    type_: "Ready".to_string(),
                    status: "True".to_string(),
                    reason: Some("KubeletReady".to_string()),
                    message: Some("kubelet is posting ready status".to_string()),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let status = node_status(&node, Some(&usage));

        assert!(status.ready);
        assert_eq!(status.roles, vec!["worker"]);
        assert_eq!(status.capacity.cpu.unwrap().value, 4.0);
        assert_eq!(status.capacity.memory.unwrap().value, 8_589_934_592.0);
        assert_eq!(status.capacity.pods, Some(110));
        assert_eq!(status.usage.unwrap().cpu.unwrap().value, 0.5);
        assert_eq!(status.conditions.len(), 1);
        assert_eq!(status.conditions[0].type_, "Ready");
        assert_eq!(status.conditions[0].status, "True");
        assert_eq!(status.conditions[0].reason.as_deref(), Some("KubeletReady"));
    }
}
