use std::sync::Arc;

use alien_core::{
    ClientConfig, DeploymentConfig, KubernetesClientConfig, Platform, ResourceHeartbeat,
};
use alien_error::Context;
use alien_observer::{
    AwsObserveContext, AwsObserver, GcpObserveContext, GcpObserver, KubernetesObserveContext,
    KubernetesObserver, ObserveScope, Observer,
};
use tracing::debug;

use crate::{ErrorData, Result};

pub async fn run_observe_pass(
    platform: Platform,
    client_config: &ClientConfig,
    service_provider: &Arc<dyn alien_infra::PlatformServiceProvider>,
    deployment_id: &str,
    _config: &DeploymentConfig,
) -> Result<Vec<ResourceHeartbeat>> {
    match platform {
        Platform::Aws => {
            return run_aws_observe_pass(client_config, deployment_id)
                .await
                .context(ErrorData::DeploymentError {
                    message: "Failed to run AWS observe pass".to_string(),
                });
        }
        Platform::Gcp => {
            return run_gcp_observe_pass(client_config, deployment_id)
                .await
                .context(ErrorData::DeploymentError {
                    message: "Failed to run GCP observe pass".to_string(),
                });
        }
        Platform::Kubernetes => {}
        _ => return Ok(vec![]),
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

async fn run_aws_observe_pass(
    client_config: &ClientConfig,
    deployment_id: &str,
) -> Result<Vec<ResourceHeartbeat>> {
    let Some(aws_config) = client_config.aws_config() else {
        debug!("Skipping observe pass because client config is not AWS");
        return Ok(vec![]);
    };

    let credentials = alien_aws_clients::AwsCredentialProvider::from_config(aws_config.clone())
        .await
        .context(ErrorData::DeploymentError {
            message: "Failed to create AWS observer credentials".to_string(),
        })?;
    let http_client = reqwest::Client::new();
    let tagging_client = std::sync::Arc::new(alien_aws_clients::ResourceGroupsTaggingClient::new(
        http_client.clone(),
        credentials.clone(),
    ));
    let cloudwatch_client = std::sync::Arc::new(alien_aws_clients::CloudWatchClient::new(
        http_client,
        credentials,
    ));

    let observer = AwsObserver::new(AwsObserveContext {
        deployment_id: deployment_id.to_string(),
        account_id: aws_config.account_id.clone(),
        region: aws_config.region.clone(),
        resource_groups_tagging_client: tagging_client,
        cloudwatch_client,
    });
    let scope = ObserveScope {
        namespace: aws_config.region.clone(),
        label_selector: None,
    };

    observer
        .discover(&scope)
        .await
        .context(ErrorData::DeploymentError {
            message: "Failed to run AWS resource observer".to_string(),
        })
}

async fn run_gcp_observe_pass(
    client_config: &ClientConfig,
    deployment_id: &str,
) -> Result<Vec<ResourceHeartbeat>> {
    let Some(gcp_config) = client_config.gcp_config() else {
        debug!("Skipping observe pass because client config is not GCP");
        return Ok(vec![]);
    };

    let http_client = reqwest::Client::new();
    let cloud_asset_client = std::sync::Arc::new(alien_gcp_clients::CloudAssetClient::new(
        http_client.clone(),
        gcp_config.clone(),
    ));
    let monitoring_client = std::sync::Arc::new(alien_gcp_clients::MonitoringClient::new(
        http_client,
        gcp_config.clone(),
    ));

    let observer = GcpObserver::new(GcpObserveContext {
        deployment_id: deployment_id.to_string(),
        project_id: gcp_config.project_id.clone(),
        region: gcp_config.region.clone(),
        cloud_asset_client,
        monitoring_client,
    });
    let scope = ObserveScope {
        namespace: gcp_config.project_id.clone(),
        label_selector: None,
    };

    observer
        .discover(&scope)
        .await
        .context(ErrorData::DeploymentError {
            message: "Failed to run GCP resource observer".to_string(),
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
