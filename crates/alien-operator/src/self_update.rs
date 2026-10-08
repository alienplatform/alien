//! Keeps the Operator on the image its manager targets.
//!
//! The manager advertises the Operator image its charts install. An Operator
//! installed with `OPERATOR_SELF_UPDATE_DEPLOYMENT` updates its own
//! Deployment to that image; Kubernetes rolls the new pod out and keeps the
//! old one until the new one is ready. The chart is installed once and the
//! Operator still moves forward with the manager.

use alien_error::{AlienError, Context};
use alien_k8s_clients::{KubernetesClient, KubernetesClientConfig, KubernetesClientConfigExt};
use tracing::info;

use crate::{
    error::{ErrorData, Result},
    OperatorState,
};

/// Container in the Operator Deployment that runs the Operator.
const OPERATOR_CONTAINER: &str = "operator";

/// Move the Operator's own Deployment to `target_image` if it runs another.
pub async fn apply(state: &OperatorState, target_image: &str) -> Result<()> {
    let Some(deployment_name) = state.config.self_update_deployment.as_deref() else {
        return Ok(());
    };
    let namespace = state.config.namespace.as_deref().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "self-update needs the Operator's namespace".to_string(),
        })
    })?;
    let failed = |message: &str| ErrorData::ConfigurationError {
        message: format!("self-update of Deployment '{deployment_name}': {message}"),
    };

    let client = KubernetesClient::new(
        KubernetesClientConfig::try_incluster()
            .await
            .context(failed("loading in-cluster credentials"))?,
    )
    .await
    .context(failed("creating Kubernetes client"))?;
    let mut deployment = client
        .get_deployment(namespace, deployment_name)
        .await
        .context(failed("reading the Deployment"))?;

    let container = deployment
        .spec
        .as_mut()
        .and_then(|spec| spec.template.spec.as_mut())
        .and_then(|pod| {
            pod.containers
                .iter_mut()
                .find(|container| container.name == OPERATOR_CONTAINER)
        })
        .ok_or_else(|| AlienError::new(failed("no 'operator' container")))?;
    if container.image.as_deref() == Some(target_image) {
        return Ok(());
    }
    let current = container.image.clone().unwrap_or_default();
    container.image = Some(target_image.to_string());

    info!(from = %current, to = %target_image, "Updating Operator image");
    client
        .update_deployment(namespace, deployment_name, &deployment)
        .await
        .context(failed("updating the Deployment"))?;
    Ok(())
}
