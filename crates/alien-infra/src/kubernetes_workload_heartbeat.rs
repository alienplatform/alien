use std::collections::{BTreeMap, HashMap};

use crate::kubernetes_client::{
    optional_events_read, optional_metrics_read, OptionalKubernetesReadStatus,
};
use alien_core::{
    HeartbeatBackend, HeartbeatCollectionIssue, HeartbeatCollectionIssueReason,
    HeartbeatIssueSeverity, KubernetesContainerHeartbeatData, KubernetesDaemonHeartbeatData,
    KubernetesEventInvolvedObject, KubernetesEventSnapshot, KubernetesEventSource,
    KubernetesOwnerReference, KubernetesPodRuntimeUnitStatus, KubernetesWorkerHeartbeatData,
    KubernetesWorkloadCondition, KubernetesWorkloadKind, KubernetesWorkloadStatus, MetricSample,
    MetricUnit, ObservedHealth, Platform, ProviderLifecycleState, ResourceHeartbeat,
    ResourceHeartbeatData, ResourceType, WorkloadHeartbeatStatus, WorkloadReplicaStatus,
};
use alien_error::Context;
use k8s_openapi::api::apps::v1::{
    DaemonSet, DaemonSetStatus, Deployment, DeploymentStatus, StatefulSet, StatefulSetStatus,
};
use k8s_openapi::api::core::v1::{Event, Pod};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::chrono::Utc;
use kube::api::DynamicObject;

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};

const MAX_EVENTS: usize = 20;

pub enum KubernetesWorkload {
    DaemonSet(DaemonSet),
    Deployment(Deployment),
    StatefulSet(StatefulSet),
}

impl KubernetesWorkload {
    fn metadata_name(&self) -> Option<&str> {
        match self {
            Self::DaemonSet(workload) => workload.metadata.name.as_deref(),
            Self::Deployment(workload) => workload.metadata.name.as_deref(),
            Self::StatefulSet(workload) => workload.metadata.name.as_deref(),
        }
    }

    fn metadata_generation(&self) -> Option<i64> {
        match self {
            Self::DaemonSet(workload) => workload.metadata.generation,
            Self::Deployment(workload) => workload.metadata.generation,
            Self::StatefulSet(workload) => workload.metadata.generation,
        }
    }

    fn workload_status(&self) -> Option<KubernetesWorkloadStatus> {
        match self {
            Self::DaemonSet(workload) => workload
                .status
                .as_ref()
                .map(|status| daemonset_status_to_heartbeat(status, self.metadata_generation())),
            Self::Deployment(workload) => workload
                .status
                .as_ref()
                .map(|status| deployment_status_to_heartbeat(status, self.metadata_generation())),
            Self::StatefulSet(workload) => workload
                .status
                .as_ref()
                .map(|status| statefulset_status_to_heartbeat(status, self.metadata_generation())),
        }
    }

    fn desired_replicas(&self) -> Option<u32> {
        match self {
            Self::DaemonSet(workload) => workload
                .status
                .as_ref()
                .map(|status| status.desired_number_scheduled.max(0) as u32),
            Self::Deployment(workload) => workload
                .spec
                .as_ref()
                .and_then(|spec| spec.replicas)
                .map(|value| value.max(0) as u32),
            Self::StatefulSet(workload) => workload
                .spec
                .as_ref()
                .and_then(|spec| spec.replicas)
                .map(|value| value.max(0) as u32),
        }
    }

    fn current_replicas(&self) -> Option<u32> {
        match self {
            Self::DaemonSet(workload) => workload
                .status
                .as_ref()
                .map(|status| status.current_number_scheduled.max(0) as u32),
            Self::Deployment(workload) => workload
                .status
                .as_ref()
                .and_then(|status| status.replicas)
                .map(|value| value.max(0) as u32),
            Self::StatefulSet(workload) => workload
                .status
                .as_ref()
                .and_then(|status| status.current_replicas)
                .map(|value| value.max(0) as u32),
        }
    }

    fn misscheduled_replicas(&self) -> Option<u32> {
        match self {
            Self::DaemonSet(workload) => workload
                .status
                .as_ref()
                .map(|status| status.number_misscheduled.max(0) as u32),
            Self::Deployment(_) | Self::StatefulSet(_) => None,
        }
    }
}

pub struct KubernetesWorkloadHeartbeatInput {
    pub deployment_id: Option<String>,
    pub resource_id: String,
    pub resource_type: ResourceType,
    pub data_kind: KubernetesWorkloadDataKind,
    pub command_supported: bool,
    pub namespace: String,
    pub workload_name: String,
    pub workload_kind: KubernetesWorkloadKind,
    pub workload: KubernetesWorkload,
    pub label_selector: String,
}

pub enum KubernetesWorkloadDataKind {
    Container,
    Worker,
    Daemon,
}

struct KubernetesWorkloadSnapshot {
    status: WorkloadHeartbeatStatus,
    replicas: WorkloadReplicaStatus,
    restarts: Option<u32>,
    cpu: Option<MetricSample>,
    memory: Option<MetricSample>,
}

pub async fn emit_kubernetes_workload_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    input: KubernetesWorkloadHeartbeatInput,
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
    let metrics_client = ctx
        .service_provider
        .get_kubernetes_metrics_client(kubernetes_config)
        .await?;

    let pods = pod_client
        .list_pods(&input.namespace, Some(input.label_selector.clone()), None)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to list pods for Kubernetes workload '{}'",
                input.workload_name
            ),
            resource_id: Some(input.resource_id.clone()),
        })?;

    let events = optional_events_read(
        &input.resource_id,
        &input.namespace,
        Some(&input.workload_name),
        event_client.list_events(&input.namespace, None),
    )
    .await
    .context(ErrorData::CloudPlatformError {
        message: format!(
            "Failed optional event collection for Kubernetes workload '{}'",
            input.workload_name
        ),
        resource_id: Some(input.resource_id.clone()),
    })?;

    let metrics = optional_metrics_read(
        &input.resource_id,
        Some(&input.namespace),
        Some(&input.workload_name),
        metrics_client.list_pod_metrics(&input.namespace, Some(input.label_selector.clone())),
    )
    .await
    .context(ErrorData::CloudPlatformError {
        message: format!(
            "Failed optional pod metrics collection for Kubernetes workload '{}'",
            input.workload_name
        ),
        resource_id: Some(input.resource_id.clone()),
    })?;

    let metric_by_pod =
        metrics
            .value
            .as_ref()
            .map(|list| {
                list.items
                    .iter()
                    .filter_map(|metrics| {
                        metrics.metadata.name.as_ref().map(|name| {
                            (name.clone(), pod_metric_samples(pod_metric_usages(metrics)))
                        })
                    })
                    .collect::<HashMap<_, _>>()
            })
            .unwrap_or_default();

    let pod_statuses = pods
        .items
        .iter()
        .map(|pod| {
            pod_instance_status(
                pod,
                metric_by_pod.get(pod.metadata.name.as_deref().unwrap_or("")),
            )
        })
        .collect::<Vec<_>>();

    let kubernetes_event_snapshots = events
        .value
        .as_ref()
        .map(|list| {
            list.items
                .iter()
                .filter(|event| {
                    event_related_to_workload(event, &input.workload_name, &pod_statuses)
                })
                .filter_map(kubernetes_event_snapshot)
                .take(MAX_EVENTS)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let workload_status = input.workload.workload_status();
    let snapshot = workload_snapshot(
        &input.workload,
        workload_status.as_ref(),
        &pod_statuses,
        &metrics.status,
        &events.status,
    );
    let data = match input.data_kind {
        KubernetesWorkloadDataKind::Container => ResourceHeartbeatData::Container(
            alien_core::ContainerHeartbeatData::Kubernetes(KubernetesContainerHeartbeatData {
                status: snapshot.status,
                namespace: input.namespace,
                name: input.workload_name,
                workload_kind: input.workload_kind,
                replicas: snapshot.replicas,
                restarts: snapshot.restarts,
                cpu: snapshot.cpu,
                memory: snapshot.memory,
                workload: workload_status,
                pods: pod_statuses,
                events: kubernetes_event_snapshots,
            }),
        ),
        KubernetesWorkloadDataKind::Worker => ResourceHeartbeatData::Worker(
            alien_core::WorkerHeartbeatData::Kubernetes(KubernetesWorkerHeartbeatData {
                status: snapshot.status,
                namespace: input.namespace,
                name: input.workload_name,
                workload_kind: input.workload_kind,
                replicas: snapshot.replicas,
                restarts: snapshot.restarts,
                cpu: snapshot.cpu,
                memory: snapshot.memory,
                workload: workload_status,
                pods: pod_statuses,
                trigger_count: 0,
                events: kubernetes_event_snapshots,
            }),
        ),
        KubernetesWorkloadDataKind::Daemon => ResourceHeartbeatData::Daemon(
            alien_core::DaemonHeartbeatData::Kubernetes(KubernetesDaemonHeartbeatData {
                status: snapshot.status,
                namespace: input.namespace,
                name: input.workload_name,
                replicas: snapshot.replicas,
                restarts: snapshot.restarts,
                command_supported: input.command_supported,
                cpu: snapshot.cpu,
                memory: snapshot.memory,
                workload: workload_status,
                pods: pod_statuses,
                events: kubernetes_event_snapshots,
            }),
        ),
    };

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: input.deployment_id,
        resource_id: input.resource_id,
        resource_type: input.resource_type,
        controller_platform: Platform::Kubernetes,
        backend: HeartbeatBackend::Kubernetes,
        observed_at: Utc::now(),
        data,
        raw: vec![],
    });

    Ok(())
}

fn deployment_status_to_heartbeat(
    status: &DeploymentStatus,
    desired_generation: Option<i64>,
) -> KubernetesWorkloadStatus {
    KubernetesWorkloadStatus {
        desired_replicas: status.replicas.map(|value| value.max(0) as u32),
        ready_replicas: status.ready_replicas.map(|value| value.max(0) as u32),
        available_replicas: status.available_replicas.map(|value| value.max(0) as u32),
        updated_replicas: status.updated_replicas.map(|value| value.max(0) as u32),
        observed_generation: status.observed_generation,
        desired_generation,
        rollout_reason: rollout_reason(status.conditions.as_deref().unwrap_or(&[])),
        conditions: status
            .conditions
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|condition| KubernetesWorkloadCondition {
                condition_type: condition.type_.clone(),
                status: condition.status.clone(),
                reason: condition.reason.clone(),
                message: condition.message.clone(),
                last_transition_time: condition.last_transition_time.as_ref().map(|time| time.0),
            })
            .collect(),
    }
}

fn statefulset_status_to_heartbeat(
    status: &StatefulSetStatus,
    desired_generation: Option<i64>,
) -> KubernetesWorkloadStatus {
    KubernetesWorkloadStatus {
        desired_replicas: Some(status.replicas.max(0) as u32),
        ready_replicas: status.ready_replicas.map(|value| value.max(0) as u32),
        available_replicas: status.available_replicas.map(|value| value.max(0) as u32),
        updated_replicas: status.updated_replicas.map(|value| value.max(0) as u32),
        observed_generation: status.observed_generation,
        desired_generation,
        rollout_reason: None,
        conditions: status
            .conditions
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|condition| KubernetesWorkloadCondition {
                condition_type: condition.type_.clone(),
                status: condition.status.clone(),
                reason: condition.reason.clone(),
                message: condition.message.clone(),
                last_transition_time: condition.last_transition_time.as_ref().map(|time| time.0),
            })
            .collect(),
    }
}

fn daemonset_status_to_heartbeat(
    status: &DaemonSetStatus,
    desired_generation: Option<i64>,
) -> KubernetesWorkloadStatus {
    KubernetesWorkloadStatus {
        desired_replicas: Some(status.desired_number_scheduled.max(0) as u32),
        ready_replicas: Some(status.number_ready.max(0) as u32),
        available_replicas: status.number_available.map(|value| value.max(0) as u32),
        updated_replicas: status
            .updated_number_scheduled
            .map(|value| value.max(0) as u32),
        observed_generation: status.observed_generation,
        desired_generation,
        rollout_reason: daemonset_rollout_reason(status.conditions.as_deref().unwrap_or(&[])),
        conditions: status
            .conditions
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|condition| KubernetesWorkloadCondition {
                condition_type: condition.type_.clone(),
                status: condition.status.clone(),
                reason: condition.reason.clone(),
                message: condition.message.clone(),
                last_transition_time: condition.last_transition_time.as_ref().map(|time| time.0),
            })
            .collect(),
    }
}

fn rollout_reason(
    conditions: &[k8s_openapi::api::apps::v1::DeploymentCondition],
) -> Option<String> {
    conditions
        .iter()
        .find(|condition| condition.status != "True")
        .and_then(|condition| {
            condition
                .reason
                .clone()
                .or_else(|| condition.message.clone())
        })
}

fn daemonset_rollout_reason(
    conditions: &[k8s_openapi::api::apps::v1::DaemonSetCondition],
) -> Option<String> {
    conditions
        .iter()
        .find(|condition| condition.status != "True")
        .and_then(|condition| {
            condition
                .reason
                .clone()
                .or_else(|| condition.message.clone())
        })
}

fn workload_snapshot(
    workload: &KubernetesWorkload,
    status: Option<&KubernetesWorkloadStatus>,
    runtime_units: &[KubernetesPodRuntimeUnitStatus],
    metrics_status: &OptionalKubernetesReadStatus,
    events_status: &OptionalKubernetesReadStatus,
) -> KubernetesWorkloadSnapshot {
    let desired = workload
        .desired_replicas()
        .or_else(|| status.and_then(|status| status.desired_replicas));
    let ready = status.and_then(|status| status.ready_replicas);
    let current = workload.current_replicas().or_else(|| {
        status.and_then(|status| {
            status
                .available_replicas
                .or(status.ready_replicas)
                .or(status.desired_replicas)
        })
    });
    let partial = !metrics_status.available || !events_status.available;
    let health = if desired.is_some() && ready.is_some() && desired == ready {
        ObservedHealth::Healthy
    } else if ready.unwrap_or(0) > 0 {
        ObservedHealth::Degraded
    } else {
        ObservedHealth::Unhealthy
    };
    let mut collection_issues = Vec::new();
    push_collection_issue("metrics", metrics_status, &mut collection_issues);
    push_collection_issue("events", events_status, &mut collection_issues);

    KubernetesWorkloadSnapshot {
        status: WorkloadHeartbeatStatus {
            health,
            lifecycle: ProviderLifecycleState::Running,
            message: workload
                .metadata_name()
                .map(|name| format!("Kubernetes workload '{name}'")),
            stale: false,
            partial,
            collection_issues,
        },
        replicas: WorkloadReplicaStatus {
            desired,
            current,
            ready,
            available: status.and_then(|status| status.available_replicas),
            updated: status.and_then(|status| status.updated_replicas),
            misscheduled: workload.misscheduled_replicas(),
        },
        restarts: Some(
            runtime_units
                .iter()
                .map(|instance| instance.restart_count)
                .sum(),
        ),
        cpu: metric_sum(runtime_units.iter().filter_map(|instance| instance.cpu)),
        memory: metric_sum(runtime_units.iter().filter_map(|instance| instance.memory)),
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

fn pod_instance_status(
    pod: &Pod,
    metrics: Option<&(Option<MetricSample>, Option<MetricSample>)>,
) -> KubernetesPodRuntimeUnitStatus {
    let container_statuses = pod
        .status
        .as_ref()
        .and_then(|status| status.container_statuses.as_deref())
        .unwrap_or(&[]);
    let ready =
        container_statuses.iter().all(|status| status.ready) && !container_statuses.is_empty();
    let restart_count = container_statuses
        .iter()
        .map(|status| status.restart_count.max(0) as u32)
        .sum();
    let waiting_reason = container_statuses.iter().find_map(|status| {
        status
            .state
            .as_ref()
            .and_then(|state| state.waiting.as_ref())
            .and_then(|waiting| waiting.reason.clone())
    });
    let terminated_reason = container_statuses.iter().find_map(|status| {
        status
            .state
            .as_ref()
            .and_then(|state| state.terminated.as_ref())
            .and_then(|terminated| terminated.reason.clone())
    });
    let (cpu, memory) = metrics.cloned().unwrap_or((None, None));

    KubernetesPodRuntimeUnitStatus {
        name: pod.metadata.name.clone().unwrap_or_default(),
        uid: pod.metadata.uid.clone(),
        phase: pod.status.as_ref().and_then(|status| status.phase.clone()),
        ready,
        restart_count,
        waiting_reason,
        terminated_reason,
        node_name: pod.spec.as_ref().and_then(|spec| spec.node_name.clone()),
        pod_ip: pod.status.as_ref().and_then(|status| status.pod_ip.clone()),
        owner_references: pod
            .metadata
            .owner_references
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|owner| KubernetesOwnerReference {
                kind: owner.kind.clone(),
                name: owner.name.clone(),
                uid: owner.uid.clone(),
                controller: owner.controller.unwrap_or(false),
            })
            .collect(),
        cpu,
        memory,
    }
}

fn pod_metric_usages(
    metrics: &DynamicObject,
) -> impl Iterator<Item = BTreeMap<String, Quantity>> + '_ {
    metrics
        .data
        .get("containers")
        .and_then(|containers| containers.as_array())
        .into_iter()
        .flatten()
        .filter_map(|container| container.get("usage"))
        .filter_map(|usage| serde_json::from_value(usage.clone()).ok())
}

fn pod_metric_samples(
    containers: impl IntoIterator<Item = BTreeMap<String, Quantity>>,
) -> (Option<MetricSample>, Option<MetricSample>) {
    let mut cpu = 0.0;
    let mut memory = 0.0;
    let mut has_cpu = false;
    let mut has_memory = false;

    for usage in containers {
        if let Some(value) = usage.get("cpu").and_then(quantity_cpu_cores) {
            cpu += value;
            has_cpu = true;
        }
        if let Some(value) = usage.get("memory").and_then(quantity_bytes) {
            memory += value;
            has_memory = true;
        }
    }

    (
        has_cpu.then_some(MetricSample {
            value: cpu,
            unit: MetricUnit::Cores,
        }),
        has_memory.then_some(MetricSample {
            value: memory,
            unit: MetricUnit::Bytes,
        }),
    )
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

fn event_related_to_workload(
    event: &Event,
    workload_name: &str,
    runtime_units: &[KubernetesPodRuntimeUnitStatus],
) -> bool {
    event
        .involved_object
        .name
        .as_deref()
        .map(|name| {
            name == workload_name || runtime_units.iter().any(|instance| instance.name == name)
        })
        .unwrap_or(false)
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

pub fn label_selector(labels: &BTreeMap<String, String>) -> Result<String> {
    Ok(labels
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(","))
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::apps::v1::{DaemonSetCondition, DaemonSetStatus};
    use k8s_openapi::api::core::v1::{
        ContainerState, ContainerStateWaiting, ContainerStatus, EventSource, ObjectReference,
        PodSpec, PodStatus,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference, Time};

    #[test]
    fn pod_instance_maps_crash_loop_and_metrics() {
        let mut usage = BTreeMap::new();
        usage.insert("cpu".to_string(), Quantity("250m".to_string()));
        usage.insert("memory".to_string(), Quantity("128Mi".to_string()));
        let metrics = pod_metric_samples([usage]);
        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("api-abc".to_string()),
                uid: Some("pod-uid".to_string()),
                owner_references: Some(vec![OwnerReference {
                    api_version: "apps/v1".to_string(),
                    kind: "ReplicaSet".to_string(),
                    name: "api-abc".to_string(),
                    uid: "rs-uid".to_string(),
                    controller: Some(true),
                    block_owner_deletion: None,
                }]),
                ..Default::default()
            },
            spec: Some(PodSpec {
                node_name: Some("node-1".to_string()),
                ..Default::default()
            }),
            status: Some(PodStatus {
                phase: Some("Running".to_string()),
                pod_ip: Some("10.0.0.12".to_string()),
                container_statuses: Some(vec![ContainerStatus {
                    name: "api".to_string(),
                    ready: false,
                    restart_count: 3,
                    state: Some(ContainerState {
                        waiting: Some(ContainerStateWaiting {
                            reason: Some("CrashLoopBackOff".to_string()),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    image: "api:latest".to_string(),
                    image_id: "".to_string(),
                    container_id: None,
                    started: None,
                    last_state: None,
                    allocated_resources: None,
                    allocated_resources_status: None,
                    resources: None,
                    stop_signal: None,
                    user: None,
                    volume_mounts: None,
                }]),
                ..Default::default()
            }),
        };

        let instance = pod_instance_status(&pod, Some(&metrics));

        assert!(!instance.ready);
        assert_eq!(instance.restart_count, 3);
        assert_eq!(instance.waiting_reason.as_deref(), Some("CrashLoopBackOff"));
        assert_eq!(instance.node_name.as_deref(), Some("node-1"));
        assert_eq!(instance.cpu.unwrap().value, 0.25);
        assert_eq!(instance.memory.unwrap().value, 134_217_728.0);
    }

    #[test]
    fn workload_snapshot_marks_ready_and_partial_collection() {
        let workload = KubernetesWorkload::Deployment(Deployment {
            metadata: ObjectMeta {
                name: Some("api".to_string()),
                ..Default::default()
            },
            status: Some(DeploymentStatus {
                replicas: Some(2),
                ready_replicas: Some(2),
                available_replicas: Some(2),
                updated_replicas: Some(2),
                ..Default::default()
            }),
            ..Default::default()
        });
        let status = workload.workload_status();
        let runtime_units = vec![
            test_instance("api-a", true, 1, Some(0.25), Some(128.0)),
            test_instance("api-b", true, 2, Some(0.5), Some(256.0)),
        ];

        let snapshot = workload_snapshot(
            &workload,
            status.as_ref(),
            &runtime_units,
            &OptionalKubernetesReadStatus::unavailable(
                HeartbeatCollectionIssueReason::NotInstalled,
                "metrics.k8s.io API is not installed",
            ),
            &OptionalKubernetesReadStatus::available(),
        );

        assert_eq!(snapshot.status.health, ObservedHealth::Healthy);
        assert!(snapshot.status.partial);
        assert_eq!(snapshot.status.collection_issues.len(), 1);
        assert_eq!(snapshot.replicas.desired, Some(2));
        assert_eq!(snapshot.replicas.ready, Some(2));
        assert_eq!(snapshot.restarts, Some(3));
        assert_eq!(snapshot.cpu.unwrap().value, 0.75);
        assert_eq!(snapshot.memory.unwrap().value, 384.0);
    }

    #[test]
    fn workload_snapshot_marks_unready_workload_degraded() {
        let workload = KubernetesWorkload::Deployment(Deployment {
            metadata: ObjectMeta {
                name: Some("api".to_string()),
                ..Default::default()
            },
            status: Some(DeploymentStatus {
                replicas: Some(3),
                ready_replicas: Some(1),
                available_replicas: Some(1),
                ..Default::default()
            }),
            ..Default::default()
        });
        let status = workload.workload_status();

        let snapshot = workload_snapshot(
            &workload,
            status.as_ref(),
            &[],
            &OptionalKubernetesReadStatus::available(),
            &OptionalKubernetesReadStatus::available(),
        );

        assert_eq!(snapshot.status.health, ObservedHealth::Degraded);
        assert!(!snapshot.status.partial);
        assert_eq!(snapshot.replicas.desired, Some(3));
        assert_eq!(snapshot.replicas.ready, Some(1));
    }

    #[test]
    fn kubernetes_event_snapshot_maps_related_warning_event() {
        let runtime_units = vec![test_instance("api-a", true, 0, None, None)];
        let event = Event {
            involved_object: ObjectReference {
                name: Some("api-a".to_string()),
                kind: Some("Pod".to_string()),
                ..Default::default()
            },
            message: Some("Back-off restarting failed container".to_string()),
            reason: Some("BackOff".to_string()),
            source: Some(EventSource {
                component: Some("kubelet".to_string()),
                ..Default::default()
            }),
            type_: Some("Warning".to_string()),
            last_timestamp: Some(Time(Utc::now())),
            ..Default::default()
        };

        assert!(event_related_to_workload(&event, "api", &runtime_units));
        let heartbeat = kubernetes_event_snapshot(&event).unwrap();
        assert_eq!(heartbeat.reason, "BackOff");
        assert_eq!(heartbeat.type_.as_deref(), Some("Warning"));
        assert_eq!(
            heartbeat
                .source
                .as_ref()
                .and_then(|source| source.component.as_deref()),
            Some("kubelet")
        );
    }

    #[test]
    fn daemonset_status_maps_native_rollout_fields() {
        let status = daemonset_status_to_heartbeat(
            &DaemonSetStatus {
                desired_number_scheduled: 4,
                current_number_scheduled: 4,
                number_available: Some(3),
                number_ready: 3,
                number_misscheduled: 1,
                number_unavailable: Some(1),
                updated_number_scheduled: Some(2),
                observed_generation: Some(7),
                conditions: Some(vec![DaemonSetCondition {
                    type_: "Progressing".to_string(),
                    reason: Some("RollingUpdate".to_string()),
                    message: Some("daemonset is rolling".to_string()),
                    ..Default::default()
                }]),
                collision_count: None,
            },
            Some(9),
        );

        assert_eq!(status.desired_replicas, Some(4));
        assert_eq!(status.ready_replicas, Some(3));
        assert_eq!(status.available_replicas, Some(3));
        assert_eq!(status.updated_replicas, Some(2));
        assert_eq!(status.observed_generation, Some(7));
        assert_eq!(status.desired_generation, Some(9));
        assert_eq!(status.rollout_reason.as_deref(), Some("RollingUpdate"));

        let workload = KubernetesWorkload::DaemonSet(DaemonSet {
            metadata: ObjectMeta {
                name: Some("node-agent".to_string()),
                generation: Some(9),
                ..Default::default()
            },
            status: Some(DaemonSetStatus {
                desired_number_scheduled: 4,
                current_number_scheduled: 4,
                number_available: Some(3),
                number_ready: 3,
                number_misscheduled: 1,
                number_unavailable: Some(1),
                updated_number_scheduled: Some(2),
                observed_generation: Some(7),
                conditions: None,
                collision_count: None,
            }),
            ..Default::default()
        });
        let status = workload.workload_status();
        let snapshot = workload_snapshot(
            &workload,
            status.as_ref(),
            &[],
            &OptionalKubernetesReadStatus::available(),
            &OptionalKubernetesReadStatus::available(),
        );

        assert_eq!(snapshot.replicas.desired, Some(4));
        assert_eq!(snapshot.replicas.current, Some(4));
        assert_eq!(snapshot.replicas.ready, Some(3));
        assert_eq!(snapshot.replicas.available, Some(3));
        assert_eq!(snapshot.replicas.updated, Some(2));
        assert_eq!(snapshot.replicas.misscheduled, Some(1));
    }

    fn test_instance(
        name: &str,
        ready: bool,
        restart_count: u32,
        cpu: Option<f64>,
        memory: Option<f64>,
    ) -> KubernetesPodRuntimeUnitStatus {
        KubernetesPodRuntimeUnitStatus {
            name: name.to_string(),
            uid: None,
            phase: Some("Running".to_string()),
            ready,
            restart_count,
            waiting_reason: None,
            terminated_reason: None,
            node_name: None,
            pod_ip: None,
            owner_references: vec![],
            cpu: cpu.map(|value| MetricSample {
                value,
                unit: MetricUnit::Cores,
            }),
            memory: memory.map(|value| MetricSample {
                value,
                unit: MetricUnit::Bytes,
            }),
        }
    }
}
