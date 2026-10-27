//! Keeps the Operator on the image its manager targets.
//!
//! The manager advertises the Operator image its charts install. An Operator
//! installed with `OPERATOR_SELF_UPDATE_DEPLOYMENT` updates its own
//! Deployment after a disposable pod validates the candidate. The current
//! Operator keeps syncing while that check runs. Recreate then releases the
//! persistent-state lock before the replacement starts.

use alien_error::{AlienError, Context};
use alien_k8s_clients::{KubernetesClient, KubernetesClientConfig, KubernetesClientConfigExt};
use k8s_openapi::{
    api::{
        apps::v1::Deployment,
        core::v1::{Container, Pod, PodSpec},
    },
    apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference},
};
use sha2::{Digest, Sha256};
use tracing::info;

use crate::{
    error::{ErrorData, Result},
    OperatorState,
};

/// Container in the Operator Deployment that runs the Operator.
const OPERATOR_CONTAINER: &str = "operator";
pub(crate) const IMAGE_CHECK_SUCCESS: &str = "alien-operator-image-check-v1";

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
    apply_with_client(&client, namespace, deployment_name, target_image).await
}

async fn apply_with_client(
    client: &KubernetesClient,
    namespace: &str,
    deployment_name: &str,
    target_image: &str,
) -> Result<()> {
    let failed = |message: &str| ErrorData::ConfigurationError {
        message: format!("self-update of Deployment '{deployment_name}': {message}"),
    };
    let mut deployment = client
        .get_deployment(namespace, deployment_name)
        .await
        .context(failed("reading the Deployment"))?;

    let current_image = deployment
        .spec
        .as_ref()
        .and_then(|spec| spec.template.spec.as_ref())
        .and_then(|pod| {
            pod.containers
                .iter()
                .find(|container| container.name == OPERATOR_CONTAINER)
        })
        .and_then(|container| container.image.as_deref());
    if current_image == Some(target_image) {
        return Ok(());
    }
    if !image_checked(client, namespace, &deployment, target_image).await? {
        return Ok(());
    }

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

/// Poll once per sync so image pulls never block heartbeats or lease renewal.
async fn image_checked(
    client: &KubernetesClient,
    namespace: &str,
    deployment: &Deployment,
    target_image: &str,
) -> Result<bool> {
    let invalid = |message: &str| ErrorData::ConfigurationError {
        message: format!("Operator image check: {message}"),
    };
    let uid = deployment
        .metadata
        .uid
        .as_deref()
        .ok_or_else(|| AlienError::new(invalid("Deployment has no UID")))?;
    let name = format!("operator-image-check-{:x}", Sha256::digest(uid.as_bytes()));
    let name = &name[..53];
    let pod = match client.get_pod(namespace, name).await {
        Ok(pod) => pod,
        Err(error) if error.http_status_code == Some(404) => {
            let pod = image_check_pod(deployment, name, target_image)?;
            client
                .create_pod(namespace, &pod)
                .await
                .context(invalid("creating validation pod"))?;
            info!(
                image = target_image,
                "Checking Operator image before replacement"
            );
            return Ok(false);
        }
        Err(error) => return Err(error).context(invalid("reading validation pod")),
    };
    if !pod
        .metadata
        .owner_references
        .as_ref()
        .is_some_and(|owners| {
            owners
                .iter()
                .any(|owner| owner.uid == uid && owner.kind == "Deployment")
        })
    {
        return Err(AlienError::new(invalid(
            "validation pod belongs to another Deployment",
        )));
    }
    let pod_uid = pod
        .metadata
        .uid
        .as_deref()
        .ok_or_else(|| AlienError::new(invalid("validation pod has no UID")))?;
    let same_image = pod.spec.as_ref().is_some_and(|spec| {
        spec.containers.len() == 1 && spec.containers[0].image.as_deref() == Some(target_image)
    });
    let phase = pod
        .status
        .as_ref()
        .and_then(|status| status.phase.as_deref());
    let passed = pod
        .status
        .as_ref()
        .and_then(|status| status.container_statuses.as_ref())
        .is_some_and(|statuses| {
            statuses.iter().any(|status| {
                status.name == OPERATOR_CONTAINER
                    && status
                        .state
                        .as_ref()
                        .and_then(|state| state.terminated.as_ref())
                        .is_some_and(|state| {
                            state.exit_code == 0
                                && state.message.as_deref() == Some(IMAGE_CHECK_SUCCESS)
                        })
            })
        });
    let failed_pull = pod
        .status
        .as_ref()
        .and_then(|status| status.container_statuses.as_ref())
        .is_some_and(|statuses| {
            statuses.iter().any(|status| {
                status
                    .state
                    .as_ref()
                    .and_then(|state| state.waiting.as_ref())
                    .and_then(|waiting| waiting.reason.as_deref())
                    .is_some_and(|reason| {
                        matches!(
                            reason,
                            "ErrImagePull"
                                | "ImagePullBackOff"
                                | "InvalidImageName"
                                | "CreateContainerError"
                                | "CreateContainerConfigError"
                        )
                    })
            })
        });
    let expired = pod
        .metadata
        .creation_timestamp
        .as_ref()
        .is_some_and(|time| {
            chrono::Utc::now()
                .signed_duration_since(time.0)
                .num_seconds()
                >= 120
        });
    if !same_image
        || phase == Some("Succeeded")
        || phase == Some("Failed")
        || failed_pull
        || expired
    {
        client
            .delete_pod_if_uid(namespace, name, pod_uid)
            .await
            .context(invalid("removing validation pod"))?;
        if !same_image {
            return Ok(false);
        }
        if phase == Some("Succeeded") && passed {
            return Ok(true);
        }
        return Err(AlienError::new(invalid("candidate could not be pulled or pass --self-update-check; the installed Operator is unchanged")));
    }
    Ok(false)
}

fn image_check_pod(deployment: &Deployment, name: &str, target_image: &str) -> Result<Pod> {
    let invalid = || {
        AlienError::new(ErrorData::ConfigurationError {
            message: "Operator Deployment has no pod template or operator container".into(),
        })
    };
    let template = deployment
        .spec
        .as_ref()
        .and_then(|spec| spec.template.spec.as_ref())
        .ok_or_else(invalid)?;
    let container = template
        .containers
        .iter()
        .find(|container| container.name == OPERATOR_CONTAINER)
        .ok_or_else(invalid)?;
    Ok(Pod {
        metadata: ObjectMeta {
            name: Some(name.into()),
            owner_references: Some(vec![OwnerReference {
                api_version: "apps/v1".into(),
                kind: "Deployment".into(),
                name: deployment.metadata.name.clone().ok_or_else(invalid)?,
                uid: deployment.metadata.uid.clone().ok_or_else(invalid)?,
                ..Default::default()
            }]),
            ..Default::default()
        },
        spec: Some(PodSpec {
            containers: vec![Container {
                name: OPERATOR_CONTAINER.into(),
                image: Some(target_image.into()),
                command: container.command.clone(),
                args: Some(vec![
                    "--platform".into(),
                    "kubernetes".into(),
                    "--self-update-check".into(),
                ]),
                image_pull_policy: container.image_pull_policy.clone(),
                security_context: container.security_context.clone(),
                resources: container.resources.clone(),
                ..Default::default()
            }],
            restart_policy: Some("Never".into()),
            active_deadline_seconds: Some(120),
            automount_service_account_token: Some(false),
            service_account_name: template.service_account_name.clone(),
            image_pull_secrets: template.image_pull_secrets.clone(),
            security_context: template.security_context.clone(),
            node_selector: template.node_selector.clone(),
            tolerations: template.tolerations.clone(),
            affinity: template.affinity.clone(),
            runtime_class_name: template.runtime_class_name.clone(),
            scheduler_name: template.scheduler_name.clone(),
            ..Default::default()
        }),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::Path, time::Duration};

    #[tokio::test]
    #[ignore = "requires a disposable namespace and ALIEN_SELF_UPDATE_TEST_CONFIG"]
    async fn self_update_checks_image_before_replacing_operator() {
        let config_path =
            std::env::var("ALIEN_SELF_UPDATE_TEST_CONFIG").expect("explicit test credentials path");
        let config: KubernetesClientConfig =
            serde_json::from_slice(&std::fs::read(Path::new(&config_path)).unwrap()).unwrap();
        let client = KubernetesClient::new(config).await.unwrap();
        let namespace = std::env::var("ALIEN_SELF_UPDATE_TEST_NAMESPACE").unwrap();
        let good = std::env::var("ALIEN_SELF_UPDATE_TEST_IMAGE").unwrap();
        let original = client.get_deployment(&namespace, "operator").await.unwrap();
        let original_spec = original.spec.clone();
        for target in [
            "registry.invalid/absent/operator:missing",
            "registry.k8s.io/pause:3.10",
        ] {
            let result = apply_with_client(&client, &namespace, "operator", target).await;
            assert_eq!(
                client
                    .get_deployment(&namespace, "operator")
                    .await
                    .unwrap()
                    .spec,
                original_spec,
                "unverified image changed the Operator: {result:?}"
            );
            let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
            loop {
                if apply_with_client(&client, &namespace, "operator", target)
                    .await
                    .is_err()
                {
                    break;
                }
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "bad image was not rejected"
                );
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            assert_eq!(
                client
                    .get_deployment(&namespace, "operator")
                    .await
                    .unwrap()
                    .spec,
                original_spec
            );
        }
        let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
        loop {
            apply_with_client(&client, &namespace, "operator", &good)
                .await
                .unwrap();
            let updated = client.get_deployment(&namespace, "operator").await.unwrap();
            if updated
                .spec
                .as_ref()
                .unwrap()
                .template
                .spec
                .as_ref()
                .unwrap()
                .containers[0]
                .image
                .as_deref()
                == Some(&good)
            {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "good image never installed"
            );
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let updated = client.get_deployment(&namespace, "operator").await.unwrap();
        let mut expected = original_spec.unwrap();
        expected.template.spec.as_mut().unwrap().containers[0].image = Some(good.clone());
        assert_eq!(
            updated.spec,
            Some(expected),
            "only image changes; scope, volume and configuration preserved"
        );
        apply_with_client(&client, &namespace, "operator", &good)
            .await
            .unwrap();
        assert_eq!(
            client
                .get_deployment(&namespace, "operator")
                .await
                .unwrap()
                .metadata
                .resource_version,
            updated.metadata.resource_version,
            "same image is a no-op"
        );
    }
}
