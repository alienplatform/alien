use std::sync::Arc;

use alien_core::{
    ClientConfig, DeploymentConfig, KubernetesClientConfig, Platform, ResourceHeartbeat,
};
use alien_error::Context;
use alien_observer::{KubernetesObserveContext, KubernetesObserver, ObserveScope, Observer};
use tracing::debug;

use crate::{ErrorData, Result};

pub async fn run_observe_pass(
    platform: Platform,
    client_config: &ClientConfig,
    service_provider: &Arc<dyn alien_infra::PlatformServiceProvider>,
    deployment_id: &str,
    _config: &DeploymentConfig,
) -> Result<Vec<ResourceHeartbeat>> {
    if platform != Platform::Kubernetes {
        return Ok(vec![]);
    }

    let Some(kubernetes_config) = client_config.kubernetes_config() else {
        debug!("Skipping observe pass because client config is not Kubernetes");
        return Ok(vec![]);
    };

    let deployment_client = service_provider
        .get_kubernetes_deployment_client(kubernetes_config)
        .await
        .context(ErrorData::DeploymentError {
            message: "Failed to create Kubernetes deployment observer client".to_string(),
        })?;
    let pod_client = service_provider
        .get_kubernetes_pod_client(kubernetes_config)
        .await
        .context(ErrorData::DeploymentError {
            message: "Failed to create Kubernetes pod observer client".to_string(),
        })?;
    let event_client = service_provider
        .get_kubernetes_event_client(kubernetes_config)
        .await
        .context(ErrorData::DeploymentError {
            message: "Failed to create Kubernetes event observer client".to_string(),
        })?;
    let metrics_client = service_provider
        .get_kubernetes_metrics_client(kubernetes_config)
        .await
        .context(ErrorData::DeploymentError {
            message: "Failed to create Kubernetes metrics observer client".to_string(),
        })?;

    let observer = KubernetesObserver::new(KubernetesObserveContext {
        deployment_id: deployment_id.to_string(),
        deployment_client,
        pod_client,
        event_client,
        metrics_client,
    });
    let scope = ObserveScope {
        namespace: namespace_from(kubernetes_config),
        label_selector: None,
    };

    observer
        .discover(&scope)
        .await
        .context(ErrorData::DeploymentError {
            message: "Failed to run Kubernetes observe pass".to_string(),
        })
}

fn namespace_from(config: &KubernetesClientConfig) -> String {
    match config {
        KubernetesClientConfig::InCluster { namespace, .. }
        | KubernetesClientConfig::Kubeconfig { namespace, .. }
        | KubernetesClientConfig::Manual { namespace, .. } => {
            namespace.clone().unwrap_or_else(|| "default".to_string())
        }
    }
}
