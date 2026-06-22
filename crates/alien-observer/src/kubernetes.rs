use std::collections::BTreeMap;
use std::sync::Arc;

use alien_client_core::ErrorData as ClientErrorData;
use alien_core::{
    branded_tag_key, HeartbeatBackend, KubernetesWorkloadKind, Platform, ResourceType,
    ALIEN_MANAGED_BY_TAG_KEY, ALIEN_MANAGED_BY_TAG_VALUE, ALIEN_RESOURCE_TAG_KEY,
    DEFAULT_ALIEN_LABEL_DOMAIN,
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

use crate::{
    observed_resource_sample, ObserveReport, ObserveScope, ObservedResourceSampleInput, Observer,
    Result,
};

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

    async fn observe_deployments(&self, scope: &ObserveScope) -> Result<ObserveReport> {
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

        let mut resources = Vec::new();
        for deployment in deployments.list.items {
            if let Some(resource) = self.deployment_resource(scope, deployment).await? {
                resources.push(resource);
            }
        }
        Ok(report_for_kind(
            scope,
            "Deployment",
            HeartbeatBackend::Kubernetes,
            resources,
            deployments.complete,
        ))
    }

    async fn observe_statefulsets(&self, scope: &ObserveScope) -> Result<ObserveReport> {
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

        let mut resources = Vec::new();
        for statefulset in statefulsets.list.items {
            if let Some(resource) = self.statefulset_resource(scope, statefulset).await? {
                resources.push(resource);
            }
        }
        Ok(report_for_kind(
            scope,
            "StatefulSet",
            HeartbeatBackend::Kubernetes,
            resources,
            statefulsets.complete,
        ))
    }

    async fn observe_daemonsets(&self, scope: &ObserveScope) -> Result<ObserveReport> {
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

        let mut resources = Vec::new();
        for daemonset in daemonsets.list.items {
            if let Some(resource) = self.daemonset_resource(scope, daemonset).await? {
                resources.push(resource);
            }
        }
        Ok(report_for_kind(
            scope,
            "DaemonSet",
            HeartbeatBackend::Kubernetes,
            resources,
            daemonsets.complete,
        ))
    }

    async fn deployment_resource(
        &self,
        scope: &ObserveScope,
        deployment: Deployment,
    ) -> Result<Option<alien_core::ObservedResourceSample>> {
        let Some(name) = deployment.metadata.name.clone() else {
            return Ok(None);
        };
        let labels = deployment.metadata.labels.clone();
        let selector = deployment
            .spec
            .as_ref()
            .and_then(|spec| spec.selector.match_labels.clone())
            .unwrap_or_default();

        self.workload_resource(
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

    async fn statefulset_resource(
        &self,
        scope: &ObserveScope,
        statefulset: StatefulSet,
    ) -> Result<Option<alien_core::ObservedResourceSample>> {
        let Some(name) = statefulset.metadata.name.clone() else {
            return Ok(None);
        };
        let labels = statefulset.metadata.labels.clone();
        let selector = statefulset
            .spec
            .as_ref()
            .and_then(|spec| spec.selector.match_labels.clone())
            .unwrap_or_default();

        self.workload_resource(
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

    async fn daemonset_resource(
        &self,
        scope: &ObserveScope,
        daemonset: DaemonSet,
    ) -> Result<Option<alien_core::ObservedResourceSample>> {
        let Some(name) = daemonset.metadata.name.clone() else {
            return Ok(None);
        };
        let labels = daemonset.metadata.labels.clone();
        let selector = daemonset
            .spec
            .as_ref()
            .and_then(|spec| spec.selector.match_labels.clone())
            .unwrap_or_default();

        self.workload_resource(
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

    async fn workload_resource(
        &self,
        scope: &ObserveScope,
        name: &str,
        labels: Option<&BTreeMap<String, String>>,
        selector_labels: BTreeMap<String, String>,
        workload_kind: KubernetesWorkloadKind,
        data_kind: KubernetesWorkloadDataKind,
        command_supported: bool,
        workload: KubernetesWorkload,
    ) -> Result<Option<alien_core::ObservedResourceSample>> {
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

        Ok(Some(observed_resource_sample(
            ObservedResourceSampleInput {
                deployment_id: self.context.deployment_id.clone(),
                raw_identity: raw_id,
                provider_kind: format!("apps/v1/{}", workload_kind_name(workload_kind)),
                display_name: name.to_string(),
                namespace: Some(scope.namespace.clone()),
                region: None,
                scope: Some(kubernetes_inventory_scope(
                    workload_kind_name(workload_kind),
                    &scope.namespace,
                )),
                resource_type_hint: Some(resource_type_for_workload(workload_kind)),
                alien_resource_id: alien_resource_id_from_labels(&self.label_domain, labels),
                controller_platform: Platform::Kubernetes,
                backend: HeartbeatBackend::Kubernetes,
                labels: labels.cloned().unwrap_or_default(),
                attributes: BTreeMap::new(),
                data,
                raw: vec![],
            },
        )))
    }
}

#[async_trait]
impl Observer for KubernetesObserver {
    fn platform(&self) -> Platform {
        Platform::Kubernetes
    }

    async fn discover(&self, scope: &ObserveScope) -> Result<ObserveReport> {
        let mut report = self.observe_deployments(scope).await?;
        report.extend(self.observe_statefulsets(scope).await?);
        report.extend(self.observe_daemonsets(scope).await?);
        Ok(report)
    }
}

struct KubernetesListResult<T: k8s_openapi::ListableResource> {
    list: List<T>,
    complete: bool,
}

async fn list_or_empty<T, F>(
    kind: &'static str,
    scope: &ObserveScope,
    read: F,
) -> Result<KubernetesListResult<T>>
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

    Ok(KubernetesListResult {
        complete: result.status.available,
        list: result.value.unwrap_or_else(|| List {
            metadata: Default::default(),
            items: Vec::new(),
        }),
    })
}

impl ObserveReport {
    fn extend(&mut self, other: ObserveReport) {
        self.inventory_batches.extend(other.inventory_batches);
    }
}

fn report_for_kind(
    scope: &ObserveScope,
    kind: &'static str,
    backend: HeartbeatBackend,
    resources: Vec<alien_core::ObservedResourceSample>,
    complete: bool,
) -> ObserveReport {
    ObserveReport {
        inventory_batches: vec![alien_core::ObservedInventoryBatch {
            source_kind: "operator".to_string(),
            inventory_scope: kubernetes_inventory_scope(kind, &scope.namespace),
            controller_platform: Platform::Kubernetes,
            backend,
            observed_at: chrono::Utc::now(),
            complete,
            resources,
        }],
    }
}

pub fn raw_identity(kind: KubernetesWorkloadKind, namespace: &str, name: &str) -> String {
    format!("apps/v1:{}:{namespace}:{name}", workload_kind_name(kind))
}

pub fn kubernetes_inventory_scope(kind: &str, namespace: &str) -> String {
    format!("kubernetes:apps/v1:{kind}:{namespace}")
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
