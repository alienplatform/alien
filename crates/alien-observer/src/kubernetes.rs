use std::collections::BTreeMap;
use std::sync::Arc;

use alien_client_core::ErrorData as ClientErrorData;
use alien_core::{
    branded_tag_key, HeartbeatBackend, HeartbeatSource, KubernetesWorkloadKind, Platform,
    ResourceHeartbeat, ResourceType, ALIEN_MANAGED_BY_TAG_KEY, ALIEN_MANAGED_BY_TAG_VALUE,
    ALIEN_RESOURCE_TAG_KEY, DEFAULT_ALIEN_LABEL_DOMAIN,
};
use alien_k8s_clients::{
    label_selector, optional_kubernetes_read, DeploymentApi, EventApi, KubernetesWorkload,
    KubernetesWorkloadDataKind, KubernetesWorkloadReadInput, MetricsApi,
    OptionalKubernetesReadContext, OptionalKubernetesReadSource, PodApi,
};
use async_trait::async_trait;
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, StatefulSet};
use k8s_openapi::List;
use tracing::warn;

use crate::{ObserveScope, Observer, Result};

#[derive(Clone)]
pub struct KubernetesObserveContext {
    pub deployment_id: String,
    pub deployment_client: Arc<dyn DeploymentApi>,
    pub pod_client: Arc<dyn PodApi>,
    pub event_client: Arc<dyn EventApi>,
    pub metrics_client: Arc<dyn MetricsApi>,
}

pub struct KubernetesObserver {
    context: KubernetesObserveContext,
    label_domain: String,
}

impl KubernetesObserver {
    pub fn new(context: KubernetesObserveContext) -> Self {
        Self {
            context,
            label_domain: DEFAULT_ALIEN_LABEL_DOMAIN.to_string(),
        }
    }

    pub fn with_label_domain(
        context: KubernetesObserveContext,
        label_domain: impl Into<String>,
    ) -> Self {
        Self {
            context,
            label_domain: label_domain.into(),
        }
    }

    async fn observe_deployments(&self, scope: &ObserveScope) -> Result<Vec<ResourceHeartbeat>> {
        let deployments = list_or_empty(
            "Deployment",
            scope,
            self.context.deployment_client.list_deployments(
                &scope.namespace,
                scope.label_selector.clone(),
                None,
            ),
        )
        .await?;

        let mut heartbeats = Vec::new();
        for deployment in deployments.items {
            if let Some(heartbeat) = self.deployment_heartbeat(scope, deployment).await? {
                heartbeats.push(heartbeat);
            }
        }
        Ok(heartbeats)
    }

    async fn observe_statefulsets(&self, scope: &ObserveScope) -> Result<Vec<ResourceHeartbeat>> {
        let statefulsets = list_or_empty(
            "StatefulSet",
            scope,
            self.context.deployment_client.list_statefulsets(
                &scope.namespace,
                scope.label_selector.clone(),
                None,
            ),
        )
        .await?;

        let mut heartbeats = Vec::new();
        for statefulset in statefulsets.items {
            if let Some(heartbeat) = self.statefulset_heartbeat(scope, statefulset).await? {
                heartbeats.push(heartbeat);
            }
        }
        Ok(heartbeats)
    }

    async fn observe_daemonsets(&self, scope: &ObserveScope) -> Result<Vec<ResourceHeartbeat>> {
        let daemonsets = list_or_empty(
            "DaemonSet",
            scope,
            self.context.deployment_client.list_daemonsets(
                &scope.namespace,
                scope.label_selector.clone(),
                None,
            ),
        )
        .await?;

        let mut heartbeats = Vec::new();
        for daemonset in daemonsets.items {
            if let Some(heartbeat) = self.daemonset_heartbeat(scope, daemonset).await? {
                heartbeats.push(heartbeat);
            }
        }
        Ok(heartbeats)
    }

    async fn deployment_heartbeat(
        &self,
        scope: &ObserveScope,
        deployment: Deployment,
    ) -> Result<Option<ResourceHeartbeat>> {
        let Some(name) = deployment.metadata.name.clone() else {
            return Ok(None);
        };
        let labels = deployment.metadata.labels.clone();
        let selector = deployment
            .spec
            .as_ref()
            .and_then(|spec| spec.selector.match_labels.clone())
            .unwrap_or_default();

        self.workload_heartbeat(
            scope,
            &name,
            labels.as_ref(),
            selector,
            KubernetesWorkloadKind::Deployment,
            KubernetesWorkloadDataKind::Container,
            false,
            KubernetesWorkload::Deployment(deployment),
        )
        .await
    }

    async fn statefulset_heartbeat(
        &self,
        scope: &ObserveScope,
        statefulset: StatefulSet,
    ) -> Result<Option<ResourceHeartbeat>> {
        let Some(name) = statefulset.metadata.name.clone() else {
            return Ok(None);
        };
        let labels = statefulset.metadata.labels.clone();
        let selector = statefulset
            .spec
            .as_ref()
            .and_then(|spec| spec.selector.match_labels.clone())
            .unwrap_or_default();

        self.workload_heartbeat(
            scope,
            &name,
            labels.as_ref(),
            selector,
            KubernetesWorkloadKind::StatefulSet,
            KubernetesWorkloadDataKind::Container,
            false,
            KubernetesWorkload::StatefulSet(statefulset),
        )
        .await
    }

    async fn daemonset_heartbeat(
        &self,
        scope: &ObserveScope,
        daemonset: DaemonSet,
    ) -> Result<Option<ResourceHeartbeat>> {
        let Some(name) = daemonset.metadata.name.clone() else {
            return Ok(None);
        };
        let labels = daemonset.metadata.labels.clone();
        let selector = daemonset
            .spec
            .as_ref()
            .and_then(|spec| spec.selector.match_labels.clone())
            .unwrap_or_default();

        self.workload_heartbeat(
            scope,
            &name,
            labels.as_ref(),
            selector,
            KubernetesWorkloadKind::DaemonSet,
            KubernetesWorkloadDataKind::Daemon,
            true,
            KubernetesWorkload::DaemonSet(daemonset),
        )
        .await
    }

    async fn workload_heartbeat(
        &self,
        scope: &ObserveScope,
        name: &str,
        labels: Option<&BTreeMap<String, String>>,
        selector_labels: BTreeMap<String, String>,
        workload_kind: KubernetesWorkloadKind,
        data_kind: KubernetesWorkloadDataKind,
        command_supported: bool,
        workload: KubernetesWorkload,
    ) -> Result<Option<ResourceHeartbeat>> {
        let raw_id = raw_identity(workload_kind, &scope.namespace, name);
        let data = match alien_k8s_clients::read_kubernetes_workload(
            &self.context.pod_client,
            &self.context.event_client,
            &self.context.metrics_client,
            &KubernetesWorkloadReadInput {
                namespace: scope.namespace.clone(),
                workload_name: name.to_string(),
                workload_kind,
                data_kind,
                command_supported,
                label_selector: label_selector(&selector_labels),
                workload,
            },
        )
        .await
        {
            Ok(data) => data,
            Err(error) if is_optional_observe_read_error(&error) => {
                warn!(
                    kind = ?workload_kind,
                    namespace = %scope.namespace,
                    name = %name,
                    error = %error,
                    "Kubernetes observe workload read unavailable; skipping workload"
                );
                return Ok(None);
            }
            Err(error) => return Err(error),
        };

        Ok(Some(ResourceHeartbeat {
            deployment_id: Some(self.context.deployment_id.clone()),
            resource_id: raw_id,
            source: HeartbeatSource::Observed,
            alien_resource_id: alien_resource_id_from_labels(&self.label_domain, labels),
            resource_type: resource_type_for_workload(workload_kind),
            controller_platform: Platform::Kubernetes,
            backend: HeartbeatBackend::Kubernetes,
            observed_at: chrono::Utc::now(),
            data,
            raw: vec![],
        }))
    }
}

#[async_trait]
impl Observer for KubernetesObserver {
    fn platform(&self) -> Platform {
        Platform::Kubernetes
    }

    async fn discover(&self, scope: &ObserveScope) -> Result<Vec<ResourceHeartbeat>> {
        let mut heartbeats = self.observe_deployments(scope).await?;
        heartbeats.extend(self.observe_statefulsets(scope).await?);
        heartbeats.extend(self.observe_daemonsets(scope).await?);
        Ok(heartbeats)
    }
}

async fn list_or_empty<T, F>(kind: &'static str, scope: &ObserveScope, read: F) -> Result<List<T>>
where
    T: k8s_openapi::ListableResource,
    F: std::future::Future<Output = Result<List<T>>>,
{
    let result = optional_kubernetes_read(
        OptionalKubernetesReadContext {
            source: OptionalKubernetesReadSource::Workloads,
            resource_id: kind,
            namespace: Some(&scope.namespace),
            kubernetes_resource: Some(kind),
        },
        read,
    )
    .await?;

    if !result.status.available {
        warn!(
            kind,
            namespace = %scope.namespace,
            reason = ?result.status.reason,
            "Kubernetes observe disabled for workload kind; grant namespace list/get"
        );
    }

    Ok(result.value.unwrap_or_else(|| List {
        metadata: Default::default(),
        items: Vec::new(),
    }))
}

pub fn raw_identity(kind: KubernetesWorkloadKind, namespace: &str, name: &str) -> String {
    format!("apps/v1:{}:{namespace}:{name}", workload_kind_name(kind))
}

pub fn alien_resource_id_from_labels(
    label_domain: &str,
    labels: Option<&BTreeMap<String, String>>,
) -> Option<String> {
    let labels = labels?;
    let managed_by_key = branded_tag_key(label_domain, ALIEN_MANAGED_BY_TAG_KEY);
    if labels.get(&managed_by_key).map(String::as_str) != Some(ALIEN_MANAGED_BY_TAG_VALUE) {
        return None;
    }

    let resource_key = branded_tag_key(label_domain, ALIEN_RESOURCE_TAG_KEY);
    labels.get(&resource_key).cloned()
}

fn resource_type_for_workload(kind: KubernetesWorkloadKind) -> ResourceType {
    match kind {
        KubernetesWorkloadKind::DaemonSet => ResourceType::from_static("daemon"),
        _ => ResourceType::from_static("container"),
    }
}

fn workload_kind_name(kind: KubernetesWorkloadKind) -> &'static str {
    match kind {
        KubernetesWorkloadKind::Deployment => "Deployment",
        KubernetesWorkloadKind::StatefulSet => "StatefulSet",
        KubernetesWorkloadKind::DaemonSet => "DaemonSet",
        KubernetesWorkloadKind::ReplicaSet => "ReplicaSet",
        KubernetesWorkloadKind::Pod => "Pod",
    }
}

fn is_optional_observe_read_error(error: &alien_client_core::Error) -> bool {
    matches!(
        error.error.as_ref(),
        Some(ClientErrorData::RemoteAccessDenied { .. })
            | Some(ClientErrorData::RemoteResourceNotFound { .. })
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{ALIEN_STACK_TAG_KEY, DEFAULT_ALIEN_LABEL_DOMAIN};

    #[test]
    fn raw_identity_includes_api_kind_namespace_and_name() {
        assert_eq!(
            raw_identity(
                KubernetesWorkloadKind::StatefulSet,
                "turso",
                "turso-servers"
            ),
            "apps/v1:StatefulSet:turso:turso-servers"
        );
    }

    #[test]
    fn alien_resource_link_requires_branded_managed_by_label() {
        let labels = BTreeMap::from([
            (
                branded_tag_key(DEFAULT_ALIEN_LABEL_DOMAIN, ALIEN_MANAGED_BY_TAG_KEY),
                ALIEN_MANAGED_BY_TAG_VALUE.to_string(),
            ),
            (
                branded_tag_key(DEFAULT_ALIEN_LABEL_DOMAIN, ALIEN_RESOURCE_TAG_KEY),
                "api".to_string(),
            ),
            (
                branded_tag_key(DEFAULT_ALIEN_LABEL_DOMAIN, ALIEN_STACK_TAG_KEY),
                "stack".to_string(),
            ),
        ]);

        assert_eq!(
            alien_resource_id_from_labels(DEFAULT_ALIEN_LABEL_DOMAIN, Some(&labels)).as_deref(),
            Some("api")
        );
    }

    #[test]
    fn alien_resource_link_ignores_unmanaged_workloads() {
        let labels = BTreeMap::from([(
            branded_tag_key(DEFAULT_ALIEN_LABEL_DOMAIN, ALIEN_RESOURCE_TAG_KEY),
            "api".to_string(),
        )]);

        assert_eq!(
            alien_resource_id_from_labels(DEFAULT_ALIEN_LABEL_DOMAIN, Some(&labels)),
            None
        );
    }
}
