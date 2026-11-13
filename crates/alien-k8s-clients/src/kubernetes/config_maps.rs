use crate::kubernetes::kubernetes_client::KubernetesClient;
use crate::kubernetes::kubernetes_request_utils::sign_send_json;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use async_trait::async_trait;
use k8s_openapi::api::core::v1::{ConfigMap, Namespace};
#[cfg(feature = "test-utils")]
use mockall::automock;
use reqwest::Method;

/// ConfigMap reads and replaces, and namespace reads.
#[cfg_attr(feature = "test-utils", automock)]
#[async_trait]
pub trait ConfigMapApi: Send + Sync + std::fmt::Debug {
    /// The ConfigMap `name` in `namespace`.
    async fn get_config_map(&self, namespace: &str, name: &str) -> Result<ConfigMap>;
    /// Replace ConfigMap `name` in `namespace` with `config_map`. Its
    /// `resourceVersion`, when set, makes a concurrent change a conflict.
    async fn replace_config_map(
        &self,
        namespace: &str,
        name: &str,
        config_map: &ConfigMap,
    ) -> Result<ConfigMap>;
    /// The namespace `name`.
    async fn get_namespace(&self, name: &str) -> Result<Namespace>;
}

impl KubernetesClient {
    /// The ConfigMap `name` in `namespace`.
    pub async fn get_config_map(&self, namespace: &str, name: &str) -> Result<ConfigMap> {
        let url = format!(
            "{}/api/v1/namespaces/{}/configmaps/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            urlencoding::encode(name)
        );
        let builder = self.client().request(Method::GET, &url);
        sign_send_json(builder, &self.auth_config()).await
    }

    /// Replace ConfigMap `name` in `namespace` with `config_map`.
    pub async fn replace_config_map(
        &self,
        namespace: &str,
        name: &str,
        config_map: &ConfigMap,
    ) -> Result<ConfigMap> {
        let body = serde_json::to_string(config_map)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Failed to serialize ConfigMap '{name}'"),
            })?;
        let url = format!(
            "{}/api/v1/namespaces/{}/configmaps/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            urlencoding::encode(name)
        );
        let builder = self
            .client()
            .request(Method::PUT, &url)
            .header("Content-Type", "application/json")
            .body(body);
        sign_send_json(builder, &self.auth_config()).await
    }

    /// The namespace `name`.
    pub async fn get_namespace(&self, name: &str) -> Result<Namespace> {
        let url = format!(
            "{}/api/v1/namespaces/{}",
            self.get_base_url(),
            urlencoding::encode(name)
        );
        let builder = self.client().request(Method::GET, &url);
        sign_send_json(builder, &self.auth_config()).await
    }
}

#[async_trait]
impl ConfigMapApi for KubernetesClient {
    async fn get_config_map(&self, namespace: &str, name: &str) -> Result<ConfigMap> {
        self.get_config_map(namespace, name).await
    }

    async fn replace_config_map(
        &self,
        namespace: &str,
        name: &str,
        config_map: &ConfigMap,
    ) -> Result<ConfigMap> {
        self.replace_config_map(namespace, name, config_map).await
    }

    async fn get_namespace(&self, name: &str) -> Result<Namespace> {
        self.get_namespace(name).await
    }
}
