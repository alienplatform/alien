use alien_core::{
    HeartbeatBackend, KubernetesWorkloadKind, Platform, ResourceHeartbeat, ResourceType,
};
use alien_error::Context;
use alien_k8s_clients::read_kubernetes_workload;
pub use alien_k8s_clients::{
    label_selector, KubernetesWorkload, KubernetesWorkloadDataKind, KubernetesWorkloadReadInput,
};
use k8s_openapi::{
    api::apps::v1::{Deployment, StatefulSet},
    chrono::Utc,
};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};

pub(crate) const KUBERNETES_WORKLOAD_READY_MAX_POLLS: u32 = 360; // 360 * 5s = 30 minutes

/// Kubernetes omits replica counters when their value is zero.
pub(crate) fn deployment_has_ready_replicas(deployment: &Deployment) -> bool {
    let Some(status) = &deployment.status else {
        return false;
    };
    let desired = deployment
        .spec
        .as_ref()
        .and_then(|spec| spec.replicas)
        .unwrap_or(1);
    let ready = status.ready_replicas.unwrap_or(0);
    if desired == 0 {
        return status.observed_generation.unwrap_or(0)
            >= deployment.metadata.generation.unwrap_or(1)
            && ready == 0
            && status.replicas.unwrap_or(0) == 0;
    }
    // Do not report drift while Kubernetes is replacing or scaling replicas.
    ready > 0
        && (ready >= status.replicas.unwrap_or(0)
            || status.observed_generation.unwrap_or(0)
                < deployment.metadata.generation.unwrap_or(1)
            || status.updated_replicas.unwrap_or(0) < desired
            || status.replicas.unwrap_or(0) != desired)
}

/// Kubernetes sets `Progressing=False` with this reason when a rollout makes no progress for
/// the Deployment's `progressDeadlineSeconds`.
pub(crate) fn deployment_progress_deadline_exceeded(deployment: &Deployment) -> bool {
    deployment
        .status
        .as_ref()
        .and_then(|status| status.conditions.as_ref())
        .is_some_and(|conditions| {
            conditions.iter().any(|condition| {
                condition.type_ == "Progressing"
                    && condition.reason.as_deref() == Some("ProgressDeadlineExceeded")
            })
        })
        // The condition belongs to the generation Kubernetes last observed. After a new spec
        // is applied it describes the previous rollout until the controller observes the new one.
        && deployment.status.as_ref().and_then(|s| s.observed_generation).unwrap_or(0)
            >= deployment.metadata.generation.unwrap_or(1)
}

/// Whether Kubernetes is still rolling out a new revision of the Deployment, judged from its
/// own status and not from elapsed time. A stuck rollout ends when Kubernetes sets
/// `Progressing=False` (ProgressDeadlineExceeded), after which this is false.
pub(crate) fn deployment_rollout_in_progress(deployment: &Deployment) -> bool {
    let Some(status) = &deployment.status else {
        return false;
    };
    let desired = deployment
        .spec
        .as_ref()
        .and_then(|spec| spec.replicas)
        .unwrap_or(1);
    if deployment_progress_deadline_exceeded(deployment) {
        // Kubernetes gave up on this rollout: no pod becoming ready can be waited for.
        return false;
    }
    let progressing_on_new_replica_set = status.conditions.iter().flatten().any(|condition| {
        condition.type_ == "Progressing"
            && condition.status == "True"
            && condition.reason.as_deref() != Some("NewReplicaSetAvailable")
    });
    status.observed_generation.unwrap_or(0) < deployment.metadata.generation.unwrap_or(1)
        || progressing_on_new_replica_set
        || status.updated_replicas.unwrap_or(0) < desired
        || status.replicas.unwrap_or(0) > status.updated_replicas.unwrap_or(0)
}

/// Whether Kubernetes is still rolling out a new revision of the StatefulSet.
pub(crate) fn statefulset_rollout_in_progress(statefulset: &StatefulSet) -> bool {
    let Some(status) = &statefulset.status else {
        return false;
    };
    let revision_differs = match (&status.current_revision, &status.update_revision) {
        (Some(current), Some(update)) => current != update,
        _ => false,
    };
    status.observed_generation.unwrap_or(0) < statefulset.metadata.generation.unwrap_or(1)
        || revision_differs
        || status.updated_replicas.unwrap_or(0) < status.replicas
}

/// How long a StatefulSet rollout may take. StatefulSets have no progress deadline of their own;
/// this matches the default `progressDeadlineSeconds` of a Deployment.
pub(crate) const STATEFULSET_ROLLOUT_DEADLINE_SECS: i64 = 600;
const DEFAULT_PROGRESS_DEADLINE_SECS: i64 = 600;

/// Consecutive healthy checks a failed resource needs before it returns to Running, so a pod
/// that crash-loops does not flip the resource between Running and RefreshFailed on every
/// check. Checks are 10 s apart, so the third healthy check comes 20 s after the first.
pub(crate) const RECOVERY_HEALTHY_CHECKS: u32 = 3;

/// An unfinished rollout of one revision of a workload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RolloutProbe {
    /// Identifies the revision being rolled out; a new revision starts a new time bound.
    pub key: String,
    pub deadline_secs: i64,
}

/// When the controller first saw a rollout of one revision.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RolloutWatch {
    pub key: String,
    pub first_seen_unix: i64,
    /// Since when the workload has been fully rolled out and ready. The watch is dropped only
    /// after that holds for `ROLLOUT_COMPLETE_WINDOW_SECS`, so pods that flap between ready and
    /// unready cannot restart the time bound. Absent in state written by earlier releases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complete_since_unix: Option<i64>,
}

/// How long a rollout must stay complete and ready before its time bound is forgotten: the span
/// of `RECOVERY_HEALTHY_CHECKS` checks `RECOVERY_CHECK_MIN_GAP_SECS` apart.
pub(crate) const ROLLOUT_COMPLETE_WINDOW_SECS: i64 =
    (RECOVERY_HEALTHY_CHECKS as i64 - 1) * RECOVERY_CHECK_MIN_GAP_SECS;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RolloutWait {
    /// Not rolling out (or no longer): judge the workload as it is.
    NotRolling,
    /// Still rolling out and inside its time bound.
    Wait,
    /// Rolling out for longer than the bound.
    TimedOut { elapsed_secs: i64 },
}

/// The rollout of the Deployment's current revision, if it is unfinished.
pub(crate) fn deployment_rollout(deployment: &Deployment) -> Option<RolloutProbe> {
    deployment_rollout_in_progress(deployment).then(|| RolloutProbe {
        key: format!("generation-{}", deployment.metadata.generation.unwrap_or(0)),
        deadline_secs: deployment
            .spec
            .as_ref()
            .and_then(|spec| spec.progress_deadline_seconds)
            .map(i64::from)
            .unwrap_or(DEFAULT_PROGRESS_DEADLINE_SECS),
    })
}

/// The rollout of the StatefulSet's current revision, if it is unfinished.
pub(crate) fn statefulset_rollout(statefulset: &StatefulSet) -> Option<RolloutProbe> {
    statefulset_rollout_in_progress(statefulset).then(|| RolloutProbe {
        key: format!(
            "generation-{}-{}",
            statefulset.metadata.generation.unwrap_or(0),
            statefulset
                .status
                .as_ref()
                .and_then(|status| status.update_revision.clone())
                .unwrap_or_default()
        ),
        deadline_secs: STATEFULSET_ROLLOUT_DEADLINE_SECS,
    })
}

/// Measures how long the current rollout has been seen, from the first check that saw it.
/// The count lives in the controller state, so it survives restarts and does not use the
/// executor's retry budget. `fully_ready` is true when every replica is updated and ready on
/// the current generation; only a continuous `ROLLOUT_COMPLETE_WINDOW_SECS` of that ends the
/// watch.
pub(crate) fn track_rollout(
    watch: &mut Option<RolloutWatch>,
    probe: Option<RolloutProbe>,
    fully_ready: bool,
    now_unix: i64,
) -> RolloutWait {
    let Some(probe) = probe else {
        if let Some(existing) = watch {
            if fully_ready {
                let since = *existing.complete_since_unix.get_or_insert(now_unix);
                if now_unix.saturating_sub(since) >= ROLLOUT_COMPLETE_WINDOW_SECS {
                    *watch = None;
                }
            } else {
                existing.complete_since_unix = None;
            }
        }
        return RolloutWait::NotRolling;
    };
    let first_seen = match watch {
        Some(existing) if existing.key == probe.key => {
            existing.complete_since_unix = None;
            existing.first_seen_unix
        }
        _ => {
            *watch = Some(RolloutWatch {
                key: probe.key.clone(),
                first_seen_unix: now_unix,
                complete_since_unix: None,
            });
            now_unix
        }
    };
    let elapsed_secs = now_unix.saturating_sub(first_seen);
    if elapsed_secs >= probe.deadline_secs {
        RolloutWait::TimedOut { elapsed_secs }
    } else {
        RolloutWait::Wait
    }
}

/// Seconds of the progress deadline a Deployment's rollout exceeded, once Kubernetes has
/// given up on it. Old pods can still be serving: the new version is not rolling out.
pub(crate) fn deployment_stalled_rollout(deployment: &Deployment) -> Option<i64> {
    deployment_progress_deadline_exceeded(deployment).then(|| {
        deployment
            .spec
            .as_ref()
            .and_then(|spec| spec.progress_deadline_seconds)
            .map(i64::from)
            .unwrap_or(DEFAULT_PROGRESS_DEADLINE_SECS)
    })
}

/// Whether every replica of the workload is updated and ready on the current generation.
fn workload_fully_ready(workload: &KubernetesWorkload) -> bool {
    match workload {
        KubernetesWorkload::Deployment(deployment) => {
            let Some(status) = &deployment.status else {
                return false;
            };
            let desired = deployment
                .spec
                .as_ref()
                .and_then(|s| s.replicas)
                .unwrap_or(1);
            status.observed_generation.unwrap_or(0) >= deployment.metadata.generation.unwrap_or(1)
                && status.updated_replicas.unwrap_or(0) == desired
                && status.replicas.unwrap_or(0) == desired
                && status.ready_replicas.unwrap_or(0) == desired
        }
        KubernetesWorkload::StatefulSet(statefulset) => {
            let Some(status) = &statefulset.status else {
                return false;
            };
            let desired = statefulset
                .spec
                .as_ref()
                .and_then(|s| s.replicas)
                .unwrap_or(1);
            status.observed_generation.unwrap_or(0) >= statefulset.metadata.generation.unwrap_or(1)
                && status.updated_replicas.unwrap_or(0) == desired
                && status.replicas == desired
                && status.ready_replicas.unwrap_or(0) == desired
        }
        _ => false,
    }
}

/// What the rollout of a workload looks like on this check.
pub(crate) struct RolloutAssessment {
    pub wait: RolloutWait,
    /// Why the rollout is stalled, once Kubernetes or the time bound says so. A stalled rollout
    /// is drift even while old pods still serve.
    pub stall: Option<String>,
}

/// Updates the watch from the workload and says whether to wait for it or call it stalled.
pub(crate) fn assess_rollout(
    watch: &mut Option<RolloutWatch>,
    workload: &KubernetesWorkload,
    now_unix: i64,
) -> RolloutAssessment {
    let probe = match workload {
        KubernetesWorkload::Deployment(deployment) => deployment_rollout(deployment),
        KubernetesWorkload::StatefulSet(statefulset) => statefulset_rollout(statefulset),
        _ => None,
    };
    let wait = track_rollout(watch, probe, workload_fully_ready(workload), now_unix);
    let stall = match (workload, &wait) {
        (KubernetesWorkload::Deployment(deployment), _)
            if deployment_stalled_rollout(deployment).is_some() =>
        {
            let secs = deployment_stalled_rollout(deployment).unwrap_or_default();
            let detail = deployment
                .status
                .as_ref()
                .and_then(|status| status.conditions.as_ref())
                .and_then(|conditions| {
                    conditions
                        .iter()
                        .find(|c| c.type_ == "Progressing" && c.status != "True")
                })
                .and_then(|c| c.message.clone());
            Some(match detail {
                Some(detail) => format!(
                    "ProgressDeadlineExceeded: no progress for {secs} s ({detail})"
                ),
                None => format!("ProgressDeadlineExceeded: no progress for {secs} s"),
            })
        }
        (KubernetesWorkload::Deployment(_), RolloutWait::TimedOut { elapsed_secs }) => Some(
            format!("the rollout has not finished after {elapsed_secs} s"),
        ),
        (KubernetesWorkload::StatefulSet(_), RolloutWait::TimedOut { elapsed_secs }) => Some(
            format!(
                "the StatefulSet rollout has not finished within its {STATEFULSET_ROLLOUT_DEADLINE_SECS} s deadline ({elapsed_secs} s)"
            ),
        ),
        _ => None,
    };
    RolloutAssessment { wait, stall }
}

/// The message for a stalled rollout: names the workload, why it is stalled and, when known,
/// what the pods report (ImagePullBackOff, CrashLoopBackOff, a ReplicaFailure).
pub(crate) fn stalled_rollout_message(
    kind: &str,
    workload_name: &str,
    cause: &str,
    pod_reason: Option<&str>,
) -> String {
    match pod_reason {
        Some(reason) => format!("{kind} '{workload_name}' rollout is stalled: {cause}; {reason}"),
        None => format!("{kind} '{workload_name}' rollout is stalled: {cause}"),
    }
}

/// What the pods of a workload report while they are not ready: the first waiting container
/// reason (ImagePullBackOff, CrashLoopBackOff, ...) with the pod name.
pub(crate) fn pod_waiting_reason(pods: &[k8s_openapi::api::core::v1::Pod]) -> Option<String> {
    pods.iter()
        .filter(|pod| pod.metadata.deletion_timestamp.is_none())
        .find_map(|pod| {
            let waiting = pod
                .status
                .as_ref()?
                .container_statuses
                .as_deref()?
                .iter()
                .find_map(|status| status.state.as_ref()?.waiting.as_ref())?;
            let reason = waiting.reason.as_deref()?;
            Some(format!(
                "pod {} is waiting: {reason}",
                pod.metadata.name.as_deref().unwrap_or("?")
            ))
        })
}

/// Best effort: a failure to list pods must not hide the drift being reported.
pub(crate) async fn workload_pod_reason(
    ctx: &ResourceControllerContext<'_>,
    namespace: &str,
    selector: &std::collections::BTreeMap<String, String>,
    workload_failure: Option<&str>,
) -> Option<String> {
    let pods = async {
        let client = ctx
            .service_provider
            .get_kubernetes_pod_client(ctx.get_kubernetes_config().ok()?)
            .await
            .ok()?;
        client
            .list_pods(namespace, Some(label_selector(selector)), None)
            .await
            .ok()
    }
    .await;
    let from_pods = pods.and_then(|pods| pod_waiting_reason(&pods.items));
    match (from_pods, workload_failure) {
        (Some(pods), Some(failure)) => Some(format!("{pods}; {failure}")),
        (Some(pods), None) => Some(pods),
        (None, Some(failure)) => Some(failure.to_string()),
        (None, None) => None,
    }
}

/// Minimum seconds between two healthy checks that count towards recovery. The executor can
/// step a resource several times in quick succession; those must not add up to 30 s of health.
pub(crate) const RECOVERY_CHECK_MIN_GAP_SECS: i64 = 10;

/// Counts consecutive healthy checks of a `RefreshFailed` resource, at least
/// `RECOVERY_CHECK_MIN_GAP_SECS` apart. Returns true once there are `RECOVERY_HEALTHY_CHECKS` in
/// a row; any unhealthy check starts the count again.
pub(crate) fn recovery_check(
    healthy_checks: &mut u32,
    last_counted_unix: &mut Option<i64>,
    healthy: bool,
    now_unix: i64,
) -> bool {
    if !healthy {
        *healthy_checks = 0;
        *last_counted_unix = None;
        return false;
    }
    if last_counted_unix
        .is_some_and(|last| now_unix.saturating_sub(last) < RECOVERY_CHECK_MIN_GAP_SECS)
    {
        return false;
    }
    *last_counted_unix = Some(now_unix);
    *healthy_checks += 1;
    if *healthy_checks >= RECOVERY_HEALTHY_CHECKS {
        *healthy_checks = 0;
        *last_counted_unix = None;
        true
    } else {
        false
    }
}

pub(crate) fn deployment_rollout_complete(
    deployment: &Deployment,
    desired_replicas: i32,
    expected_generation: Option<i64>,
) -> bool {
    let (Some(generation), Some(status)) = (deployment.metadata.generation, &deployment.status)
    else {
        return false;
    };

    // Ready replicas can still belong to the old ReplicaSet after an update.
    Some(generation) == expected_generation
        && status
            .observed_generation
            .is_some_and(|observed| observed >= generation)
        // Kubernetes omits replica counters when a Deployment is scaled to
        // zero. An observed generation is enough to complete that rollout.
        && status.updated_replicas.unwrap_or(0) == desired_replicas
        && status.replicas.unwrap_or(0) == desired_replicas
        && status.available_replicas.unwrap_or(0) >= desired_replicas
        && status.ready_replicas.unwrap_or(0) >= desired_replicas
}

pub(crate) fn statefulset_has_ready_replicas(statefulset: &StatefulSet) -> bool {
    let Some(status) = &statefulset.status else {
        return false;
    };
    let desired = statefulset
        .spec
        .as_ref()
        .and_then(|spec| spec.replicas)
        .unwrap_or(1);
    if desired == 0 {
        status.observed_generation.unwrap_or(0) >= statefulset.metadata.generation.unwrap_or(1)
            && status.ready_replicas.unwrap_or(0) == 0
            && status.replicas == 0
    } else {
        let ready = status.ready_replicas.unwrap_or(0);
        ready > 0
            && (ready >= status.replicas
                || status.observed_generation.unwrap_or(0)
                    < statefulset.metadata.generation.unwrap_or(1)
                || status.updated_replicas.unwrap_or(0) < desired
                || status.replicas != desired)
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

impl From<&KubernetesWorkloadHeartbeatInput> for KubernetesWorkloadReadInput {
    fn from(input: &KubernetesWorkloadHeartbeatInput) -> Self {
        Self {
            data_kind: input.data_kind,
            command_supported: input.command_supported,
            namespace: input.namespace.clone(),
            workload_name: input.workload_name.clone(),
            workload_kind: input.workload_kind,
            workload: input.workload.clone(),
            label_selector: input.label_selector.clone(),
        }
    }
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

    let read_input = KubernetesWorkloadReadInput::from(&input);
    let data = read_kubernetes_workload(&pod_client, &event_client, &metrics_client, &read_input)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to collect Kubernetes workload heartbeat for '{}'",
                input.workload_name
            ),
            resource_id: Some(input.resource_id.clone()),
        })?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::apps::v1::{
        DeploymentCondition, DeploymentSpec, DeploymentStatus, StatefulSetSpec, StatefulSetStatus,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    #[test]
    fn steady_partial_failure_is_unhealthy() {
        let deployment: Deployment = serde_json::from_value(serde_json::json!({
            "metadata": {"generation": 4},
            "spec": {"replicas": 3, "selector": {}, "template": {}},
            "status": {"observedGeneration": 4, "replicas": 3, "readyReplicas": 1,
                "updatedReplicas": 3, "unavailableReplicas": 2}
        }))
        .unwrap();
        assert!(!deployment_has_ready_replicas(&deployment));
        let statefulset: StatefulSet = serde_json::from_value(serde_json::json!({
            "metadata": {"generation": 4},
            "spec": {"replicas": 3, "selector": {}, "template": {}, "serviceName": "app"},
            "status": {"observedGeneration": 4, "replicas": 3, "readyReplicas": 1,
                "updatedReplicas": 3}
        }))
        .unwrap();
        assert!(!statefulset_has_ready_replicas(&statefulset));
    }

    #[test]
    fn completed_replicaset_with_partial_readiness_is_unhealthy() {
        // A real scale-up retains NewReplicaSetAvailable while Available is false.
        let deployment: Deployment = serde_json::from_str(
            r#"{
  "metadata": {
    "generation": 6
  },
  "spec": {
    "replicas": 4,
    "minReadySeconds": 60,
    "selector": {},
    "template": {}
  },
  "status": {
    "availableReplicas": 1,
    "conditions": [
      {
        "lastTransitionTime": "2026-10-26T09:27:24Z",
        "lastUpdateTime": "2026-10-26T09:44:18Z",
        "message": "ReplicaSet \"app-69d8576d88\" has successfully progressed.",
        "reason": "NewReplicaSetAvailable",
        "status": "True",
        "type": "Progressing"
      },
      {
        "lastTransitionTime": "2026-10-26T09:50:11Z",
        "lastUpdateTime": "2026-10-26T09:50:11Z",
        "message": "Deployment does not have minimum availability.",
        "reason": "MinimumReplicasUnavailable",
        "status": "False",
        "type": "Available"
      }
    ],
    "observedGeneration": 6,
    "readyReplicas": 1,
    "replicas": 4,
    "unavailableReplicas": 3,
    "updatedReplicas": 4
  }
}"#,
        )
        .unwrap();
        assert!(!deployment_has_ready_replicas(&deployment));
        assert!(!deployment_rollout_complete(&deployment, 4, Some(6)));
    }

    #[test]
    fn statefulset_scale_up_keeps_serving() {
        let statefulset: StatefulSet = serde_json::from_value(serde_json::json!({
            "metadata": {"generation": 2},
            "spec": {"replicas": 3, "selector": {}, "template": {}, "serviceName": "app"},
            "status": {"observedGeneration": 2, "replicas": 3, "readyReplicas": 1,
                "currentRevision": "revision", "updateRevision": "revision"}
        }))
        .unwrap();
        assert!(statefulset_has_ready_replicas(&statefulset));
    }

    #[test]
    fn serving_replicas_remain_healthy_during_rollout_and_scale_up() {
        let mut deployment: Deployment = serde_json::from_value(serde_json::json!({
            "metadata": {"generation": 4},
            "spec": {"replicas": 2, "selector": {}, "template": {}},
            "status": {"observedGeneration": 3, "replicas": 2, "readyReplicas": 2,
                "availableReplicas": 2, "updatedReplicas": 2,
                "conditions": [{"type": "Available", "status": "True"},
                    {"type": "Progressing", "status": "True", "reason": "ReplicaSetUpdated"}]}
        }))
        .unwrap();
        assert!(deployment_has_ready_replicas(&deployment));
        assert!(!deployment_rollout_complete(&deployment, 2, Some(4)));
        let status = deployment.status.as_mut().unwrap();
        status.observed_generation = Some(4);
        status.replicas = Some(3);
        status.updated_replicas = Some(1);
        status.unavailable_replicas = Some(1);
        assert!(deployment_has_ready_replicas(&deployment));
        assert!(!deployment_rollout_complete(&deployment, 2, Some(4)));
        deployment.spec.as_mut().unwrap().replicas = Some(4);
        deployment
            .status
            .as_mut()
            .unwrap()
            .conditions
            .as_mut()
            .unwrap()[0]
            .status = "False".into();
        assert!(deployment_has_ready_replicas(&deployment));
        deployment.status.as_mut().unwrap().ready_replicas = None;
        assert!(!deployment_has_ready_replicas(&deployment));
    }

    #[test]
    fn omitted_zero_ready_replicas_are_unhealthy_and_recover() {
        let mut deployment = Deployment {
            metadata: ObjectMeta {
                generation: Some(3),
                ..Default::default()
            },
            spec: Some(DeploymentSpec {
                replicas: Some(1),
                ..Default::default()
            }),
            status: Some(DeploymentStatus {
                observed_generation: Some(3),
                replicas: Some(1),
                unavailable_replicas: Some(1),
                conditions: Some(vec![DeploymentCondition {
                    type_: "Available".into(),
                    status: "False".into(),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
        };
        assert!(!deployment_has_ready_replicas(&deployment));
        let status = deployment.status.as_mut().unwrap();
        status.ready_replicas = Some(1);
        status.unavailable_replicas = None;
        // Available=False can persist while ready replicas pass minReadySeconds.
        assert!(deployment_has_ready_replicas(&deployment));
        deployment
            .status
            .as_mut()
            .unwrap()
            .conditions
            .as_mut()
            .unwrap()[0]
            .status = "True".into();
        assert!(deployment_has_ready_replicas(&deployment));
        deployment.status.as_mut().unwrap().observed_generation = Some(2);
        assert!(deployment_has_ready_replicas(&deployment));
        deployment.status = None;
        assert!(!deployment_has_ready_replicas(&deployment));
    }

    #[test]
    fn rollout_does_not_accept_ready_replicas_from_the_previous_generation() {
        let mut deployment = Deployment {
            metadata: ObjectMeta {
                generation: Some(3),
                ..Default::default()
            },
            spec: Some(DeploymentSpec {
                replicas: Some(1),
                ..Default::default()
            }),
            status: Some(DeploymentStatus {
                observed_generation: Some(2),
                replicas: Some(1),
                ready_replicas: Some(1),
                updated_replicas: Some(1),
                available_replicas: Some(1),
                ..Default::default()
            }),
        };
        assert!(!deployment_rollout_complete(&deployment, 1, Some(3)));
        deployment.status.as_mut().unwrap().observed_generation = Some(3);
        deployment.status.as_mut().unwrap().updated_replicas = None;
        assert!(!deployment_rollout_complete(&deployment, 1, Some(3)));
        deployment.status.as_mut().unwrap().updated_replicas = Some(1);
        deployment.status.as_mut().unwrap().replicas = Some(2);
        assert!(!deployment_rollout_complete(&deployment, 1, Some(3)));
        deployment.status.as_mut().unwrap().replicas = Some(1);
        deployment.status.as_mut().unwrap().ready_replicas = None;
        assert!(!deployment_rollout_complete(&deployment, 1, Some(3)));
        deployment.status.as_mut().unwrap().ready_replicas = Some(1);
        assert!(deployment_rollout_complete(&deployment, 1, Some(3)));
        assert!(!deployment_rollout_complete(&deployment, 1, Some(2)));
    }

    #[test]
    fn observed_scale_to_zero_accepts_omitted_counters() {
        let mut deployment = Deployment {
            metadata: ObjectMeta {
                generation: Some(3),
                ..Default::default()
            },
            spec: Some(DeploymentSpec {
                replicas: Some(0),
                ..Default::default()
            }),
            status: Some(DeploymentStatus {
                observed_generation: Some(3),
                ..Default::default()
            }),
        };
        assert!(deployment_has_ready_replicas(&deployment));
        deployment.status.as_mut().unwrap().observed_generation = None;
        assert!(!deployment_has_ready_replicas(&deployment));
        deployment.status.as_mut().unwrap().observed_generation = Some(2);
        assert!(!deployment_has_ready_replicas(&deployment));
        deployment.status.as_mut().unwrap().observed_generation = Some(3);
        deployment.status.as_mut().unwrap().ready_replicas = Some(1);
        deployment.status.as_mut().unwrap().replicas = Some(1);
        assert!(!deployment_has_ready_replicas(&deployment));
        let mut statefulset = StatefulSet {
            metadata: ObjectMeta {
                generation: Some(3),
                ..Default::default()
            },
            spec: Some(StatefulSetSpec {
                replicas: Some(1),
                ..Default::default()
            }),
            status: Some(StatefulSetStatus {
                observed_generation: Some(3),
                replicas: 1,
                ..Default::default()
            }),
        };
        assert!(!statefulset_has_ready_replicas(&statefulset));
        statefulset.status.as_mut().unwrap().ready_replicas = Some(1);
        assert!(statefulset_has_ready_replicas(&statefulset));
        statefulset.spec.as_mut().unwrap().replicas = Some(0);
        statefulset.status.as_mut().unwrap().ready_replicas = None;
        statefulset.status.as_mut().unwrap().replicas = 0;
        assert!(statefulset_has_ready_replicas(&statefulset));
        statefulset.status = None;
        assert!(!statefulset_has_ready_replicas(&statefulset));
    }

    #[test]
    fn rollout_time_bound_counts_from_the_first_sighting_of_each_revision() {
        let probe = |key: &str| {
            Some(RolloutProbe {
                key: key.into(),
                deadline_secs: 600,
            })
        };
        let mut watch = None;
        assert_eq!(
            track_rollout(&mut watch, probe("a"), false, 1000),
            RolloutWait::Wait
        );
        assert_eq!(
            track_rollout(&mut watch, probe("a"), false, 1599),
            RolloutWait::Wait
        );
        assert_eq!(
            track_rollout(&mut watch, probe("a"), false, 1600),
            RolloutWait::TimedOut { elapsed_secs: 600 }
        );
        // A new revision starts over.
        assert_eq!(
            track_rollout(&mut watch, probe("b"), false, 1700),
            RolloutWait::Wait
        );
        // A finished, ready rollout forgets the watch after the completion window.
        for now in [1800, 1810, 1820] {
            assert_eq!(
                track_rollout(&mut watch, None, true, now),
                RolloutWait::NotRolling
            );
        }
        assert!(watch.is_none());
    }

    fn flapping_deployment(ready: i32) -> KubernetesWorkload {
        KubernetesWorkload::Deployment(
            serde_json::from_value(serde_json::json!({
                "metadata": {"generation": 5},
                "spec": {"replicas": 3, "progressDeadlineSeconds": 600, "selector": {}, "template": {}},
                "status": {"observedGeneration": 5, "replicas": 4, "updatedReplicas": 2, "readyReplicas": ready,
                    "conditions": [{"type": "Progressing", "status": "True", "reason": "ReplicaSetUpdated"}]}
            }))
            .unwrap(),
        )
    }

    #[test]
    fn pods_that_flap_ready_and_unready_do_not_restart_the_time_bound() {
        let mut watch = None;
        let mut now = 1000;
        // The rollout never finishes; ready replicas go 3 -> 1 -> 3 -> 1 for longer than the bound.
        for ready in [1, 3, 1, 3].iter().cycle().take(80) {
            let assessment = assess_rollout(&mut watch, &flapping_deployment(*ready), now);
            if now - 1000 >= 600 {
                assert!(
                    assessment.stall.is_some(),
                    "flapping past the bound is drift at {now}"
                );
            } else {
                assert!(assessment.stall.is_none());
            }
            now += 10;
        }
        assert!(now - 1000 > 600);
    }

    #[test]
    fn the_watch_ends_only_after_a_continuous_complete_window() {
        let probe = Some(RolloutProbe {
            key: "a".into(),
            deadline_secs: 600,
        });
        let mut watch = None;
        track_rollout(&mut watch, probe.clone(), false, 1000);
        // Complete for 10 s, then unfinished again: the watch and its first sighting stay.
        track_rollout(&mut watch, None, true, 1100);
        track_rollout(&mut watch, None, true, 1110);
        assert!(watch.is_some());
        assert_eq!(
            track_rollout(&mut watch, probe, false, 1120),
            RolloutWait::Wait
        );
        assert_eq!(watch.as_ref().unwrap().first_seen_unix, 1000);
        // Complete for the whole window: forgotten.
        track_rollout(&mut watch, None, true, 1200);
        track_rollout(&mut watch, None, true, 1210);
        assert!(watch.is_some());
        track_rollout(&mut watch, None, true, 1220);
        assert!(watch.is_none());
    }

    #[test]
    fn a_watch_written_before_the_completion_window_existed_still_loads() {
        let watch: RolloutWatch =
            serde_json::from_str(r#"{"key":"generation-3","first_seen_unix":1000}"#).unwrap();
        assert_eq!(watch.complete_since_unix, None);
        // Absent fields are not written back, so an older release reads what a newer one wrote.
        let written = serde_json::to_value(&watch).unwrap();
        assert!(written.get("complete_since_unix").is_none());
    }

    #[test]
    fn a_stalled_message_names_the_workload_cause_and_pod_reason() {
        let message = stalled_rollout_message(
            "Deployment",
            "gateway",
            "ProgressDeadlineExceeded: no progress for 600 s; 1 of 3 replicas ready",
            Some("pod gateway-1 is waiting: ImagePullBackOff"),
        );
        for part in ["gateway", "ProgressDeadlineExceeded", "ImagePullBackOff"] {
            assert!(message.contains(part), "{message}");
        }
    }

    #[test]
    fn rollout_probe_uses_the_deployment_deadline_and_a_fixed_one_for_statefulsets() {
        let deployment: Deployment = serde_json::from_value(serde_json::json!({
            "metadata": {"generation": 5},
            "spec": {"replicas": 2, "progressDeadlineSeconds": 120, "selector": {}, "template": {}},
            "status": {"observedGeneration": 5, "replicas": 2, "updatedReplicas": 2, "readyReplicas": 1,
                "conditions": [{"type": "Progressing", "status": "True", "reason": "ReplicaSetUpdated"}]}
        }))
        .unwrap();
        assert_eq!(deployment_rollout(&deployment).unwrap().deadline_secs, 120);
        // Kubernetes ended the rollout: no longer waited for.
        let mut stuck = deployment.clone();
        stuck.status.as_mut().unwrap().conditions = Some(vec![DeploymentCondition {
            type_: "Progressing".into(),
            status: "False".into(),
            reason: Some("ProgressDeadlineExceeded".into()),
            ..Default::default()
        }]);
        stuck.status.as_mut().unwrap().updated_replicas = Some(1);
        assert!(deployment_rollout(&stuck).is_none());
        let statefulset: StatefulSet = serde_json::from_value(serde_json::json!({
            "metadata": {"generation": 2},
            "spec": {"replicas": 2, "selector": {}, "template": {}, "serviceName": "app"},
            "status": {"observedGeneration": 2, "replicas": 2, "updatedReplicas": 1, "readyReplicas": 1,
                "currentRevision": "app-1", "updateRevision": "app-2"}
        }))
        .unwrap();
        let probe = statefulset_rollout(&statefulset).unwrap();
        assert_eq!(probe.deadline_secs, STATEFULSET_ROLLOUT_DEADLINE_SECS);
        assert!(probe.key.ends_with("app-2"));
    }

    #[test]
    fn recovery_needs_consecutive_healthy_checks_spaced_apart() {
        let (mut count, mut last) = (0, None);
        let mut now = 1_000;
        assert!(!recovery_check(&mut count, &mut last, true, now));
        now += 10;
        assert!(!recovery_check(&mut count, &mut last, true, now));
        now += 10;
        assert!(!recovery_check(&mut count, &mut last, false, now));
        assert_eq!((count, last), (0, None));
        for _ in 0..RECOVERY_HEALTHY_CHECKS - 1 {
            now += 10;
            assert!(!recovery_check(&mut count, &mut last, true, now));
        }
        now += 10;
        assert!(recovery_check(&mut count, &mut last, true, now));
        assert_eq!(count, 0);
    }

    #[test]
    fn recovery_checks_in_quick_succession_count_once() {
        let (mut count, mut last) = (0, None);
        for _ in 0..10 {
            assert!(!recovery_check(&mut count, &mut last, true, 5_000));
        }
        assert_eq!(count, 1);
        assert!(!recovery_check(&mut count, &mut last, true, 5_009));
        assert_eq!(count, 1);
        assert!(!recovery_check(&mut count, &mut last, true, 5_010));
        assert_eq!(count, 2);
        assert!(recovery_check(&mut count, &mut last, true, 5_020));
    }

    #[test]
    fn a_stale_progress_deadline_condition_from_the_previous_generation_is_ignored() {
        let deployment: Deployment = serde_json::from_value(serde_json::json!({
            "metadata": {"generation": 6},
            "spec": {"replicas": 2, "selector": {}, "template": {}},
            "status": {"observedGeneration": 5, "replicas": 2, "updatedReplicas": 0, "readyReplicas": 0,
                "conditions": [{"type": "Progressing", "status": "False", "reason": "ProgressDeadlineExceeded"}]}
        }))
        .unwrap();
        assert!(deployment_stalled_rollout(&deployment).is_none());
        assert!(deployment_rollout(&deployment).is_some());
        let mut observed = deployment.clone();
        observed.status.as_mut().unwrap().observed_generation = Some(6);
        assert!(deployment_stalled_rollout(&observed).is_some());
    }

    /// Reads Deployment objects captured from a real cluster (`kubectl get deploy -o json`).
    ///
    /// `ALIEN_ROLLOUT_SNAPSHOTS=<dir with rollout-*.json, settled.json, steady-failure.json>`
    #[test]
    #[ignore = "needs snapshots captured from a real cluster"]
    fn real_cluster_snapshots_are_classified_as_rollout_or_drift() {
        let dir = std::path::PathBuf::from(std::env::var("ALIEN_ROLLOUT_SNAPSHOTS").unwrap());
        let read = |name: &str| -> Deployment {
            serde_json::from_slice(&std::fs::read(dir.join(name)).unwrap()).unwrap()
        };
        let mut mid_rollout_partly_ready = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let name = entry.unwrap().file_name().into_string().unwrap();
            if !name.starts_with("rollout-") {
                continue;
            }
            let deployment = read(&name);
            let status = deployment.status.as_ref().unwrap();
            println!(
                "{name}: replicas={:?} updated={:?} ready={:?} healthy={} rolling_out={}",
                status.replicas,
                status.updated_replicas,
                status.ready_replicas,
                deployment_has_ready_replicas(&deployment),
                deployment_rollout_in_progress(&deployment)
            );
            // Nothing captured during the rollout is drift.
            assert!(
                deployment_has_ready_replicas(&deployment)
                    || deployment_rollout_in_progress(&deployment),
                "{name}"
            );
            if !deployment_has_ready_replicas(&deployment) || status.ready_replicas == Some(1) {
                mid_rollout_partly_ready += 1;
            }
        }
        assert!(
            mid_rollout_partly_ready > 0,
            "the capture caught a partly ready rollout"
        );
        let settled = read("settled.json");
        assert!(deployment_has_ready_replicas(&settled));
        assert!(!deployment_rollout_in_progress(&settled));
        let failing = read("steady-failure.json");
        assert!(!deployment_has_ready_replicas(&failing));
        assert!(
            !deployment_rollout_in_progress(&failing),
            "a steady failure is drift"
        );
    }
}
