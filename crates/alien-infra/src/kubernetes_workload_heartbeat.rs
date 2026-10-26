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

pub(crate) fn deployment_initial_replicas_ready(deployment: &Deployment) -> bool {
    let desired = deployment
        .spec
        .as_ref()
        .and_then(|spec| spec.replicas)
        .unwrap_or(1);
    deployment_has_ready_replicas(deployment)
        && deployment
            .status
            .as_ref()
            .is_some_and(|status| status.ready_replicas.unwrap_or(0) >= desired)
}

pub(crate) fn statefulset_initial_replicas_ready(statefulset: &StatefulSet) -> bool {
    let desired = statefulset
        .spec
        .as_ref()
        .and_then(|spec| spec.replicas)
        .unwrap_or(1);
    statefulset_has_ready_replicas(statefulset)
        && statefulset
            .status
            .as_ref()
            .is_some_and(|status| status.ready_replicas.unwrap_or(0) >= desired)
}

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
    // Scale-up and rolling updates can temporarily miss the requested capacity
    // while existing replicas still serve. Heartbeats report that capacity gap;
    // rollout completion separately checks the requested generation and count.
    ready > 0
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
        && deployment_has_ready_replicas(deployment)
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
        status.ready_replicas.unwrap_or(0) > 0
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
    fn scale_up_with_a_completed_replicaset_keeps_serving() {
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
        assert!(deployment_has_ready_replicas(&deployment));
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
}
