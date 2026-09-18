use std::collections::BTreeMap;

use crate::error::{ErrorData, Result};
use alien_error::{Context, ContextError, IntoAlienError};
use k8s_openapi::api::core::v1::PodSpec;
use k8s_openapi::api::core::v1::Secret;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

pub(crate) async fn ensure_registry_pull_secret(
    secrets_client: &std::sync::Arc<dyn alien_k8s_clients::SecretsApi>,
    namespace: &str,
    secret_name: &str,
    proxy_url: &str,
    deployment_token: &str,
    labels: BTreeMap<String, String>,
    legacy_owner_proven: bool,
) -> Result<()> {
    let mut secret =
        registry_pull_secret(namespace, secret_name, proxy_url, deployment_token, labels)?;

    match secrets_client.create_secret(namespace, &secret).await {
        Ok(_) => Ok(()),
        Err(e) => {
            let err = format!("{e}");
            if err.contains("AlreadyExists") || err.contains("409") {
                let existing = secrets_client
                    .get_secret(namespace, secret_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to read registry pull secret '{secret_name}' before update"
                        ),
                        resource_id: None,
                    })?;
                prepare_registry_secret_update(
                    &mut secret,
                    &existing,
                    secret_name,
                    legacy_owner_proven,
                )?;
                secrets_client
                    .update_secret(namespace, secret_name, &secret)
                    .await
                    .map(|_| ())
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to update registry pull secret '{secret_name}'"),
                        resource_id: None,
                    })
            } else {
                Err(e.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create registry pull secret '{secret_name}'"),
                    resource_id: None,
                }))
            }
        }
    }
}

pub(crate) fn pod_spec_references_image_pull_secret(
    pod_spec: Option<&PodSpec>,
    secret_name: &str,
) -> bool {
    pod_spec
        .and_then(|spec| spec.image_pull_secrets.as_ref())
        .is_some_and(|secrets| {
            secrets
                .iter()
                .any(|reference| reference.name == secret_name)
        })
}

fn prepare_registry_secret_update(
    desired: &mut Secret,
    existing: &Secret,
    secret_name: &str,
    legacy_owner_proven: bool,
) -> Result<()> {
    let compatible_legacy_secret = existing.type_.as_deref()
        == Some("kubernetes.io/dockerconfigjson")
        && existing
            .data
            .as_ref()
            .is_some_and(|data| data.contains_key(".dockerconfigjson"));
    let desired_labels = desired
        .metadata
        .labels
        .as_ref()
        .cloned()
        .unwrap_or_default();
    let has_scope = existing
        .metadata
        .labels
        .as_ref()
        .is_some_and(|labels| labels.keys().any(|key| key.ends_with("/deployment")));
    if !compatible_legacy_secret
        || (!has_scope && !legacy_owner_proven)
        || !crate::core::kubernetes_labels_have_compatible_scope(
            existing.metadata.labels.as_ref(),
            &desired_labels,
        )
    {
        return Err(alien_error::AlienError::new(
            ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Refusing to replace Kubernetes Secret '{secret_name}' because it is not a compatible registry pull Secret"
                ),
                resource_id: None,
            },
        ));
    }
    let resource_version = existing.metadata.resource_version.clone().ok_or_else(|| {
        alien_error::AlienError::new(ErrorData::CloudPlatformError {
            message: format!(
                "Existing registry pull secret '{secret_name}' has no Kubernetes resourceVersion"
            ),
            resource_id: None,
        })
    })?;
    desired.metadata.resource_version = Some(resource_version);
    Ok(())
}

fn registry_pull_secret(
    namespace: &str,
    secret_name: &str,
    proxy_url: &str,
    deployment_token: &str,
    labels: BTreeMap<String, String>,
) -> Result<Secret> {
    use base64::engine::{general_purpose::STANDARD as BASE64, Engine as _};

    let registry_host = registry_auth_host(proxy_url);
    let auth = BASE64.encode(format!("deployment:{deployment_token}"));
    let docker_config = serde_json::json!({
        "auths": {
            registry_host: {
                "username": "deployment",
                "password": deployment_token,
                "auth": auth,
            }
        }
    });

    let docker_config_bytes = serde_json::to_vec(&docker_config)
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Failed to serialize Docker config".to_string(),
            resource_id: None,
        })?;

    Ok(Secret {
        metadata: ObjectMeta {
            name: Some(secret_name.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        type_: Some("kubernetes.io/dockerconfigjson".to_string()),
        data: Some(BTreeMap::from([(
            ".dockerconfigjson".to_string(),
            k8s_openapi::ByteString(docker_config_bytes),
        )])),
        ..Default::default()
    })
}

fn registry_auth_host(proxy_url: &str) -> String {
    let without_scheme = proxy_url
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    without_scheme
        .split('/')
        .next()
        .unwrap_or(without_scheme)
        .trim_end_matches('/')
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use k8s_openapi::api::core::v1::Secret;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use k8s_openapi::ByteString;

    use super::{prepare_registry_secret_update, registry_auth_host, registry_pull_secret};

    #[test]
    fn registry_auth_host_strips_scheme_and_path() {
        assert_eq!(
            registry_auth_host("https://alien-manager.example.com/v1"),
            "alien-manager.example.com"
        );
        assert_eq!(
            registry_auth_host("http://localhost:8080/registry"),
            "localhost:8080"
        );
        assert_eq!(
            registry_auth_host("registry.example.com"),
            "registry.example.com"
        );
    }

    #[test]
    fn registry_secret_preserves_cleanup_ownership_labels() {
        let secret = registry_pull_secret(
            "test-ns",
            "api-registry",
            "https://registry.example.test/v1",
            "token",
            BTreeMap::from([
                ("managed-by".to_string(), "runtime".to_string()),
                (
                    "alien.dev/deployment".to_string(),
                    "test-release".to_string(),
                ),
            ]),
        )
        .expect("registry Secret");
        let labels = secret.metadata.labels.expect("labels");
        assert_eq!(
            labels.get("managed-by").map(String::as_str),
            Some("runtime")
        );
        assert_eq!(
            labels.get("alien.dev/deployment").map(String::as_str),
            Some("test-release")
        );
    }

    #[test]
    fn registry_secret_update_preserves_api_server_resource_version() {
        let mut desired = registry_pull_secret(
            "test-ns",
            "api-registry",
            "registry.example.test",
            "new-token",
            BTreeMap::from([
                (
                    "alien.dev/label-domain".to_string(),
                    "alien.dev".to_string(),
                ),
                ("alien.dev/deployment".to_string(), "release-a".to_string()),
                ("alien.dev/resource".to_string(), "registry".to_string()),
            ]),
        )
        .expect("desired Secret");
        let existing = Secret {
            metadata: ObjectMeta {
                resource_version: Some("42".to_string()),
                ..Default::default()
            },
            type_: Some("kubernetes.io/dockerconfigjson".to_string()),
            data: Some(BTreeMap::from([(
                ".dockerconfigjson".to_string(),
                ByteString(vec![1]),
            )])),
            ..Default::default()
        };

        prepare_registry_secret_update(&mut desired, &existing, "api-registry", true)
            .expect("compatible legacy Secret");
        assert_eq!(desired.metadata.resource_version.as_deref(), Some("42"));

        let mut foreign = existing.clone();
        foreign.metadata.labels = Some(BTreeMap::from([(
            "alien.dev/deployment".to_string(),
            "release-b".to_string(),
        )]));
        assert!(
            prepare_registry_secret_update(&mut desired, &foreign, "api-registry", true).is_err()
        );
        assert!(
            prepare_registry_secret_update(&mut desired, &existing, "api-registry", false).is_err()
        );
    }
}
