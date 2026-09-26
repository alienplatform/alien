use super::{backend_name, failed, labels, missing, owned};
use crate::error::Result;
use alien_core::sync::TargetDynamicContainer;
use alien_k8s_clients::KubernetesClient;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use k8s_openapi::api::core::v1::Secret;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use std::collections::BTreeMap;

pub(super) async fn read_secret(
    client: &KubernetesClient,
    namespace: &str,
    name: &str,
) -> Result<Option<Secret>> {
    match client.get_secret(namespace, name).await {
        Ok(value) => Ok(Some(value)),
        Err(error) if missing(&error) => Ok(None),
        Err(_) => Err(failed(format!("Failed to read Kubernetes Secret {name}"))),
    }
}

pub(super) async fn put_secret(
    client: &KubernetesClient,
    namespace: &str,
    target: &TargetDynamicContainer,
    deployment_id: &str,
) -> Result<()> {
    let name = format!("{}-env", backend_name(deployment_id, &target.name));
    let current = read_secret(client, namespace, &name).await?;
    if let Some(existing) = &current {
        if !owned(&existing.metadata, deployment_id, &target.name) {
            return Err(failed(format!(
                "Refusing to replace foreign Kubernetes Secret {name}"
            )));
        }
    }
    if target.secret_env.is_empty() {
        return Ok(());
    }
    if current.as_ref().is_some_and(|secret| {
        target.secret_env.iter().all(|(key, value)| {
            secret
                .data
                .as_ref()
                .and_then(|data| data.get(key))
                .is_some_and(|stored| stored.0 == value.as_bytes())
        }) && secret
            .data
            .as_ref()
            .is_some_and(|data| data.len() == target.secret_env.len())
    }) {
        return Ok(());
    }
    let secret = Secret {
        metadata: ObjectMeta {
            name: Some(name.clone()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels(deployment_id, &target.name)),
            resource_version: current
                .as_ref()
                .and_then(|value| value.metadata.resource_version.clone()),
            ..Default::default()
        },
        string_data: Some(target.secret_env.clone()),
        type_: Some("Opaque".to_string()),
        ..Default::default()
    };
    let result = if current.is_some() {
        client.update_secret(namespace, &name, &secret).await
    } else {
        client.create_secret(namespace, &secret).await
    };
    result
        .map(|_| ())
        .map_err(|_| failed("Failed to apply dynamic container Secret"))
}

/// A deployment token is used only for the manager's own registry host. Never
/// place it in credentials for a caller-selected external registry.
pub(super) async fn put_registry_secret(
    client: &KubernetesClient,
    namespace: &str,
    deployment_id: &str,
    target: &TargetDynamicContainer,
    registry_auth: Option<(&str, &str)>,
) -> Result<bool> {
    let name = format!("{}-registry", backend_name(deployment_id, &target.name));
    let authorized = registry_auth
        .filter(|(manager_host, _)| target.image.split('/').next() == Some(*manager_host));
    let Some((manager_host, token)) = authorized else {
        if let Some(existing) = read_secret(client, namespace, &name).await? {
            if !owned(&existing.metadata, deployment_id, &target.name) {
                return Err(failed(format!(
                    "Refusing to delete foreign Kubernetes Secret {name}"
                )));
            }
            client
                .delete_secret(namespace, &name)
                .await
                .map_err(|_| failed("Failed to remove unused registry Secret"))?;
        }
        return Ok(false);
    };
    let current = read_secret(client, namespace, &name).await?;
    if let Some(existing) = &current {
        if !owned(&existing.metadata, deployment_id, &target.name) {
            return Err(failed(format!(
                "Refusing to replace foreign Kubernetes Secret {name}"
            )));
        }
    }
    let auths = BTreeMap::from([(
        manager_host.to_string(),
        serde_json::json!({
            "username": "deployment",
            "password": token,
            "auth": BASE64.encode(format!("deployment:{token}")),
        }),
    )]);
    let docker_config = serde_json::to_vec(&serde_json::json!({ "auths": auths }))
        .map_err(|_| failed("Failed to encode registry credentials"))?;
    if current.as_ref().is_some_and(|secret| {
        secret
            .data
            .as_ref()
            .and_then(|data| data.get(".dockerconfigjson"))
            .is_some_and(|stored| stored.0 == docker_config)
    }) {
        return Ok(true);
    }
    let secret = Secret {
        metadata: ObjectMeta {
            name: Some(name.clone()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels(deployment_id, &target.name)),
            resource_version: current
                .as_ref()
                .and_then(|value| value.metadata.resource_version.clone()),
            ..Default::default()
        },
        type_: Some("kubernetes.io/dockerconfigjson".to_string()),
        data: Some(BTreeMap::from([(
            ".dockerconfigjson".to_string(),
            k8s_openapi::ByteString(docker_config),
        )])),
        ..Default::default()
    };
    let result = if current.is_some() {
        client.update_secret(namespace, &name, &secret).await
    } else {
        client.create_secret(namespace, &secret).await
    };
    result.map_err(|_| failed("Failed to apply dynamic container registry Secret"))?;
    Ok(true)
}
