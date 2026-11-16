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
/// check. Checks are 10 s apart, so this is about 30 s of continuous health.
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
}

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
/// executor's retry budget.
pub(crate) fn track_rollout(
    watch: &mut Option<RolloutWatch>,
    probe: Option<RolloutProbe>,
    now_unix: i64,
) -> RolloutWait {
    let Some(probe) = probe else {
        *watch = None;
        return RolloutWait::NotRolling;
    };
    let first_seen = match watch {
        Some(existing) if existing.key == probe.key => existing.first_seen_unix,
        _ => {
            *watch = Some(RolloutWatch {
                key: probe.key.clone(),
                first_seen_unix: now_unix,
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

/// Counts consecutive healthy checks of a `RefreshFailed` resource. Returns true once there
/// are `RECOVERY_HEALTHY_CHECKS` in a row; any unhealthy check starts the count again.
pub(crate) fn recovery_check(healthy_checks: &mut u32, healthy: bool) -> bool {
    if !healthy {
        *healthy_checks = 0;
        return false;
    }
    *healthy_checks += 1;
    if *healthy_checks >= RECOVERY_HEALTHY_CHECKS {
        *healthy_checks = 0;
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
            track_rollout(&mut watch, probe("a"), 1000),
            RolloutWait::Wait
        );
        assert_eq!(
            track_rollout(&mut watch, probe("a"), 1599),
            RolloutWait::Wait
        );
        assert_eq!(
            track_rollout(&mut watch, probe("a"), 1600),
            RolloutWait::TimedOut { elapsed_secs: 600 }
        );
        // A new revision starts over.
        assert_eq!(
            track_rollout(&mut watch, probe("b"), 1700),
            RolloutWait::Wait
        );
        // A finished rollout forgets the watch.
        assert_eq!(
            track_rollout(&mut watch, None, 1800),
            RolloutWait::NotRolling
        );
        assert!(watch.is_none());
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
    fn recovery_needs_consecutive_healthy_checks() {
        let mut count = 0;
        assert!(!recovery_check(&mut count, true));
        assert!(!recovery_check(&mut count, true));
        assert!(!recovery_check(&mut count, false));
        assert_eq!(count, 0);
        for _ in 0..RECOVERY_HEALTHY_CHECKS - 1 {
            assert!(!recovery_check(&mut count, true));
        }
        assert!(recovery_check(&mut count, true));
        assert_eq!(count, 0);
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
