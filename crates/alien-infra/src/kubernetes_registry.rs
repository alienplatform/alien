use std::collections::BTreeMap;

use alien_client_core::ErrorData as CloudClientErrorData;
use alien_error::{Context, ContextError, IntoAlienError};
use k8s_openapi::api::core::v1::Secret;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

use crate::error::{ErrorData, Result};

pub(crate) async fn ensure_registry_pull_secret(
    secrets_client: &std::sync::Arc<kube::Client>,
    namespace: &str,
    secret_name: &str,
    proxy_url: &str,
    deployment_token: &str,
) -> Result<()> {
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

    let secret = Secret {
        metadata: ObjectMeta {
            name: Some(secret_name.to_string()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        type_: Some("kubernetes.io/dockerconfigjson".to_string()),
        data: Some(BTreeMap::from([(
            ".dockerconfigjson".to_string(),
            k8s_openapi::ByteString(docker_config_bytes),
        )])),
        ..Default::default()
    };

    match kube::Api::<Secret>::namespaced(secrets_client.as_ref().clone(), namespace)
        .create(&kube::api::PostParams::default(), &secret)
        .await
        .into_alien_error()
        .context(CloudClientErrorData::HttpRequestFailed {
            message: "Kubernetes create operation failed".to_string(),
        }) {
        Ok(_) => Ok(()),
        Err(e) => {
            let err = format!("{e}");
            if err.contains("AlreadyExists") || err.contains("409") {
                kube::Api::<Secret>::namespaced(secrets_client.as_ref().clone(), namespace)
                    .replace(secret_name, &kube::api::PostParams::default(), &secret)
                    .await
                    .into_alien_error()
                    .context(CloudClientErrorData::HttpRequestFailed {
                        message: format!("Kubernetes replace operation failed for '{secret_name}'"),
                    })
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
    use super::registry_auth_host;

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
}
