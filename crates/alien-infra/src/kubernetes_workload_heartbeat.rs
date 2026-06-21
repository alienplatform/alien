use alien_core::{
    HeartbeatBackend, HeartbeatSource, KubernetesWorkloadKind, Platform, ResourceHeartbeat,
    ResourceType,
};
use alien_error::Context;
use alien_k8s_clients::read_kubernetes_workload;
pub use alien_k8s_clients::{
    label_selector, KubernetesWorkload, KubernetesWorkloadDataKind, KubernetesWorkloadReadInput,
};
use k8s_openapi::chrono::Utc;

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};

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
        source: HeartbeatSource::Managed,
        alien_resource_id: None,
        observed_at: Utc::now(),
        data,
        raw: vec![],
    });

    Ok(())
}
