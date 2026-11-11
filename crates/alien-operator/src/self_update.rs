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

/// The image to run: the manager's target, moved to the site registry on a mirrored install.
fn resolve_target(target_image: &str, image_registry: Option<&str>) -> String {
    match image_registry {
        Some(registry) => {
            alien_core::image_rewrite::rewrite_to_site_registry(target_image, registry)
        }
        None => target_image.to_string(),
    }
}

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
    // The manager advertises its own registry; a mirrored install pulls every image from its mirror.
    let target_image = resolve_target(target_image, state.config.image_registry.as_deref());
    let target_image = target_image.as_str();
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

#[cfg(test)]
mod tests {
    use super::resolve_target;

    #[test]
    fn mirrored_install_pulls_the_target_from_its_mirror() {
        let digest = format!("sha256:{}", "a".repeat(64));
        let target = format!("registry.example.com/alien/operator@{digest}");
        let resolved = resolve_target(&target, Some("mirror.local:5000/site"));
        assert!(resolved.starts_with("mirror.local:5000/"), "{resolved}");
        assert!(resolved.ends_with(&format!("@{digest}")), "{resolved}");
    }

    #[test]
    fn unmirrored_install_keeps_the_target() {
        assert_eq!(resolve_target("r.io/op:1", None), "r.io/op:1");
    }
}
