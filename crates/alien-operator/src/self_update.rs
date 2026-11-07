//! Keeps the Operator on the image its manager targets.
//!
//! The manager advertises the Operator image its charts install. An Operator
//! installed with `OPERATOR_SELF_UPDATE_DEPLOYMENT` updates its own
//! Deployment to that image; Kubernetes rolls the new pod out and keeps the
//! old one until the new one is ready. The chart is installed once and the
//! Operator still moves forward with the manager.

use std::collections::BTreeMap;

use alien_error::{AlienError, Context};
use alien_k8s_clients::{KubernetesClient, KubernetesClientConfig, KubernetesClientConfigExt};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use k8s_openapi::{api::core::v1::Secret, ByteString};
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

/// Keep the Operator's own pull Secret filled with its registry pull
/// credential. The chart creates the Secret with the one-hour enrollment
/// code. The credential written here lasts as long as the daily key, so the
/// Operator pod still pulls its image on a new node after the code expired or
/// while the Operator is down. Token installs keep the chart's token.
pub async fn refresh_pull_secret(state: &OperatorState) -> Result<()> {
    let Some(secret_name) = state.config.registry_secret.as_deref() else {
        return Ok(());
    };
    if state.db.read_daily_identity().await?.is_none() {
        return Ok(());
    }
    let failed = |message: &str| ErrorData::ConfigurationError {
        message: format!("pull Secret '{secret_name}': {message}"),
    };
    let namespace = state
        .config
        .namespace
        .as_deref()
        .ok_or_else(|| AlienError::new(failed("no namespace")))?;
    let host = state
        .config
        .sync
        .as_ref()
        .and_then(|sync| {
            let host = sync.url.host_str()?;
            Some(match sync.url.port() {
                Some(port) => format!("{host}:{port}"),
                None => host.to_string(),
            })
        })
        .ok_or_else(|| AlienError::new(failed("no manager host")))?;
    let token =
        crate::daily_identity::request_token(state, serde_json::json!({"type":"registryPull"}))
            .await?;
    let client = KubernetesClient::new(
        KubernetesClientConfig::try_incluster()
            .await
            .context(failed("loading in-cluster credentials"))?,
    )
    .await
    .context(failed("creating Kubernetes client"))?;
    // The chart owns the Secret. Only its data changes here.
    let secret = client
        .get_secret(namespace, secret_name)
        .await
        .context(failed("reading the Secret"))?;
    let Some(secret) = with_pull_credential(secret, &host, &token)? else {
        return Ok(());
    };
    client
        .update_secret(namespace, secret_name, &secret)
        .await
        .context(failed("updating the Secret"))?;
    info!(secret = secret_name, "Refreshed the Operator pull Secret");
    Ok(())
}

/// `secret` with a Docker config for `host` and `token`, or `None` when it
/// already holds exactly that.
fn with_pull_credential(mut secret: Secret, host: &str, token: &str) -> Result<Option<Secret>> {
    let config = serde_json::to_vec(&serde_json::json!({
        "auths": {
            host: {
                "username": "deployment",
                "password": token,
                "auth": STANDARD.encode(format!("deployment:{token}")),
            }
        }
    }))
    .map_err(|_| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "cannot encode the pull credential".to_string(),
        })
    })?;
    let data = secret.data.get_or_insert_with(BTreeMap::new);
    if data
        .get(".dockerconfigjson")
        .is_some_and(|stored| stored.0 == config)
    {
        return Ok(None);
    }
    data.insert(".dockerconfigjson".to_string(), ByteString(config));
    Ok(Some(secret))
}

#[cfg(test)]
mod tests {
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    use super::*;

    #[test]
    fn pull_credential_replaces_the_code_and_keeps_the_chart_metadata() {
        let annotations = BTreeMap::from([(
            "alien.dev/enrollment-code-sha256".to_string(),
            "hash".to_string(),
        )]);
        let chart_secret = Secret {
            metadata: ObjectMeta {
                name: Some("prod-registry".into()),
                annotations: Some(annotations.clone()),
                resource_version: Some("7".into()),
                ..Default::default()
            },
            type_: Some("kubernetes.io/dockerconfigjson".into()),
            data: Some(BTreeMap::from([(
                ".dockerconfigjson".to_string(),
                ByteString(b"{\"auths\":{}}".to_vec()),
            )])),
            ..Default::default()
        };
        let updated = with_pull_credential(chart_secret, "manager.example.test", "ax_cap_v1_pull")
            .unwrap()
            .expect("the code is replaced");
        assert_eq!(updated.metadata.annotations, Some(annotations));
        assert_eq!(updated.metadata.resource_version.as_deref(), Some("7"));
        let config: serde_json::Value =
            serde_json::from_slice(&updated.data.as_ref().unwrap()[".dockerconfigjson"].0).unwrap();
        let auth = STANDARD
            .decode(
                config["auths"]["manager.example.test"]["auth"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(auth, b"deployment:ax_cap_v1_pull");
        // The same credential again writes nothing.
        assert!(
            with_pull_credential(updated, "manager.example.test", "ax_cap_v1_pull")
                .unwrap()
                .is_none()
        );
    }
}
