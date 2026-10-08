//! Read-only workload health for settled pull-model Kubernetes deployments.
//!
//! The deployment loop drops its target config after success. Workload identity
//! remains in stack state, so health reporting can survive an Operator restart
//! without retaining the deployment's credentials or reconciling resources.

use std::sync::Arc;
use std::time::Duration;

use alien_core::{
    branded_standard_resource_tags, Container, ContainerHeartbeatData, DeploymentState,
    DeploymentStatus, HeartbeatBackend, HeartbeatCollectionIssue, HeartbeatCollectionIssueReason,
    HeartbeatIssueSeverity, KubernetesClientConfig, KubernetesContainerHeartbeatData,
    KubernetesWorkloadKind, ObservedHealth, Platform, ProviderLifecycleState, ResourceHeartbeat,
    ResourceHeartbeatData, ResourceStatus, WorkloadHeartbeatStatus, WorkloadReplicaStatus,
};
use alien_error::Context;
use alien_k8s_clients::{
    label_selector, read_kubernetes_workload, KubernetesWorkload, KubernetesWorkloadDataKind,
    KubernetesWorkloadReadInput,
};
use chrono::Utc;
use tracing::{debug, error, info, warn};

use crate::error::{ErrorData, Result};
use crate::OperatorState;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

pub async fn run_kubernetes_heartbeat_loop(state: Arc<OperatorState>) {
    info!("Starting Kubernetes workload heartbeat loop");
    loop {
        if let Err(error) = collect_heartbeats(&state).await {
            error!(%error, "Kubernetes workload heartbeat collection failed");
        }
        tokio::select! {
            _ = tokio::time::sleep(HEARTBEAT_INTERVAL) => {},
            _ = state.cancel.cancelled() => return,
        }
    }
}

async fn collect_heartbeats(state: &OperatorState) -> Result<()> {
    let Some(deployment) = state.db.get_deployment_state().await? else {
        return Ok(());
    };
    if !is_settled(&deployment) {
        return Ok(());
    }
    let Some(stack) = deployment.stack_state.as_ref() else {
        return Ok(());
    };
    let deployment_id = state.db.get_deployment_id().await?;
    let service_provider = state
        .service_provider
        .clone()
        .unwrap_or_else(|| Arc::new(alien_infra::DefaultPlatformServiceProvider::default()));
    let mut heartbeats = Vec::new();
    for (resource_id, resource) in &stack.resources {
        if resource.resource_type != Container::RESOURCE_TYPE.as_ref()
            || !matches!(
                resource.status,
                ResourceStatus::Running | ResourceStatus::RefreshFailed
            )
            || resource
                .controller_platform
                .is_some_and(|platform| platform != Platform::Kubernetes)
        {
            continue;
        }
        let Some(internal) = resource.internal_state.as_ref() else {
            continue;
        };
        if internal.get("type").and_then(|value| value.as_str())
            != Some("KubernetesContainerController")
        {
            continue;
        }
        let (Some(namespace), Some(name), Some(is_stateful)) = (
            internal.get("namespace").and_then(|value| value.as_str()),
            internal
                .get("workloadName")
                .and_then(|value| value.as_str()),
            internal.get("isStateful").and_then(|value| value.as_bool()),
        ) else {
            warn!(%resource_id, "Kubernetes workload identity is incomplete");
            continue;
        };
        let kind = if is_stateful {
            KubernetesWorkloadKind::StatefulSet
        } else {
            KubernetesWorkloadKind::Deployment
        };
        let data = match read_container(
            service_provider.as_ref(),
            namespace,
            name,
            kind,
            &stack.resource_prefix,
            resource_id,
        )
        .await
        {
            Ok(data) => data,
            Err(error) => {
                warn!(%resource_id, %error, "Kubernetes workload read failed");
                unknown_container(namespace, name, kind)
            }
        };
        heartbeats.push(ResourceHeartbeat {
            deployment_id: deployment_id.clone(),
            resource_id: resource_id.clone(),
            resource_type: Container::RESOURCE_TYPE,
            controller_platform: Platform::Kubernetes,
            backend: HeartbeatBackend::Kubernetes,
            observed_at: Utc::now(),
            data,
            raw: vec![],
        });
    }

    if heartbeats.is_empty() {
        return Ok(());
    }
    // A release may arrive while Kubernetes is being read. Compare and merge
    // under the DB lock so a new target or deployment-step heartbeat wins.
    if !state
        .db
        .merge_pending_heartbeats_if_state_matches(&deployment, &heartbeats)
        .await?
    {
        debug!("Skipping Kubernetes heartbeats during a deployment update");
    }
    Ok(())
}

fn is_settled(state: &DeploymentState) -> bool {
    state.platform == Platform::Kubernetes
        && matches!(
            state.status,
            DeploymentStatus::Running | DeploymentStatus::RefreshFailed
        )
        && state.target_release.is_none()
        && state.current_release.is_some()
}

async fn read_container(
    provider: &dyn alien_infra::PlatformServiceProvider,
    namespace: &str,
    name: &str,
    kind: KubernetesWorkloadKind,
    resource_prefix: &str,
    resource_id: &str,
) -> Result<ResourceHeartbeatData> {
    let config = KubernetesClientConfig::InCluster {
        namespace: Some(namespace.to_string()),
        additional_headers: None,
    };
    let deployment_client = provider
        .get_kubernetes_deployment_client(&config)
        .await
        .context(ErrorData::ConfigurationError {
            message: "Failed to create Kubernetes workload client".to_string(),
        })?;
    let pod_client = provider.get_kubernetes_pod_client(&config).await.context(
        ErrorData::ConfigurationError {
            message: "Failed to create Kubernetes Pod client".to_string(),
        },
    )?;
    let event_client = provider
        .get_kubernetes_event_client(&config)
        .await
        .context(ErrorData::ConfigurationError {
            message: "Failed to create Kubernetes Event client".to_string(),
        })?;
    let metrics_client = provider
        .get_kubernetes_metrics_client(&config)
        .await
        .context(ErrorData::ConfigurationError {
            message: "Failed to create Kubernetes metrics client".to_string(),
        })?;
    let workload = match kind {
        KubernetesWorkloadKind::StatefulSet => KubernetesWorkload::StatefulSet(
            deployment_client
                .get_statefulset(namespace, name)
                .await
                .context(ErrorData::ConfigurationError {
                    message: "Failed to read Kubernetes StatefulSet".to_string(),
                })?,
        ),
        _ => KubernetesWorkload::Deployment(
            deployment_client
                .get_deployment(namespace, name)
                .await
                .context(ErrorData::ConfigurationError {
                    message: "Failed to read Kubernetes Deployment".to_string(),
                })?,
        ),
    };
    let (labels, selector) = match &workload {
        KubernetesWorkload::Deployment(workload) => (
            workload.metadata.labels.as_ref(),
            workload.spec.as_ref().map(|spec| &spec.selector),
        ),
        KubernetesWorkload::StatefulSet(workload) => (
            workload.metadata.labels.as_ref(),
            workload.spec.as_ref().map(|spec| &spec.selector),
        ),
        KubernetesWorkload::DaemonSet(_) => unreachable!(),
    };
    let labels = labels.ok_or_else(|| {
        alien_error::AlienError::new(ErrorData::ConfigurationError {
            message: "Kubernetes workload has no ownership labels".to_string(),
        })
    })?;
    let domain = labels.get("alien.dev/label-domain").ok_or_else(|| {
        alien_error::AlienError::new(ErrorData::ConfigurationError {
            message: "Kubernetes workload has no ownership domain".to_string(),
        })
    })?;
    let expected = branded_standard_resource_tags(domain, resource_prefix, resource_id);
    if expected
        .iter()
        .any(|(key, value)| labels.get(key) != Some(value))
    {
        return Err(alien_error::AlienError::new(
            ErrorData::ConfigurationError {
                message: "Kubernetes workload ownership labels do not match stack state"
                    .to_string(),
            },
        ));
    }
    let selector = selector
        .and_then(|selector| selector.match_labels.as_ref())
        .filter(|labels| !labels.is_empty())
        .ok_or_else(|| {
            alien_error::AlienError::new(ErrorData::ConfigurationError {
                message: "Kubernetes workload has no scoped Pod selector".to_string(),
            })
        })?;
    let pod_selector = label_selector(selector);
    let input = KubernetesWorkloadReadInput {
        data_kind: KubernetesWorkloadDataKind::Container,
        command_supported: false,
        namespace: namespace.to_string(),
        workload_name: name.to_string(),
        workload_kind: kind,
        workload,
        label_selector: pod_selector,
    };
    read_kubernetes_workload(&pod_client, &event_client, &metrics_client, &input)
        .await
        .context(ErrorData::ConfigurationError {
            message: "Kubernetes workload heartbeat read failed".to_string(),
        })
}

fn unknown_container(
    namespace: &str,
    name: &str,
    kind: KubernetesWorkloadKind,
) -> ResourceHeartbeatData {
    let message = "Kubernetes workload status could not be read".to_string();
    ResourceHeartbeatData::Container(ContainerHeartbeatData::Kubernetes(
        KubernetesContainerHeartbeatData {
            status: WorkloadHeartbeatStatus {
                health: ObservedHealth::Unknown,
                lifecycle: ProviderLifecycleState::Unknown,
                message: Some(message.clone()),
                stale: false,
                partial: true,
                collection_issues: vec![HeartbeatCollectionIssue {
                    source: "kubernetes-workload".to_string(),
                    reason: HeartbeatCollectionIssueReason::CollectionFailed,
                    severity: HeartbeatIssueSeverity::Error,
                    message,
                }],
            },
            namespace: namespace.to_string(),
            name: name.to_string(),
            workload_kind: kind,
            replicas: WorkloadReplicaStatus {
                desired: None,
                current: None,
                ready: None,
                available: None,
                updated: None,
                misscheduled: None,
            },
            restarts: None,
            cpu: None,
            memory: None,
            workload: None,
            pods: vec![],
            events: vec![],
        },
    ))
}
