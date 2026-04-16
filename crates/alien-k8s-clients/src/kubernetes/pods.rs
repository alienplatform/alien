use crate::kubernetes::kubernetes_client::KubernetesClient;
use crate::kubernetes::kubernetes_request_utils::{sign_send_json, sign_send_no_response};
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use reqwest::Method;

use k8s_openapi::api::core::v1::Pod;
use k8s_openapi::List;

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;

#[cfg_attr(feature = "test-utils", automock)]
#[async_trait]
pub trait PodApi: Send + Sync + std::fmt::Debug {
    async fn create_pod(&self, namespace: &str, pod: &Pod) -> Result<Pod>;
    async fn get_pod(&self, namespace: &str, name: &str) -> Result<Pod>;
    async fn list_pods(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Pod>>;
    async fn update_pod(&self, namespace: &str, name: &str, pod: &Pod) -> Result<Pod>;
    async fn delete_pod(&self, namespace: &str, name: &str) -> Result<()>;
}

impl KubernetesClient {
    /// Create a pod in the specified namespace
    pub async fn create_pod(&self, namespace: &str, pod: &Pod) -> Result<Pod> {
        let body = serde_json::to_string(pod).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize Pod '{}'",
                    pod.metadata.name.as_deref().unwrap_or("unknown")
                ),
            },
        )?;

        let url = format!(
            "{}/api/v1/namespaces/{}/pods",
            self.get_base_url(),
            urlencoding::encode(namespace)
        );
        let builder = self
            .client()
            .request(Method::POST, &url)
            .header("Content-Type", "application/json")
            .body(body);

        sign_send_json(builder, &self.auth_config()).await
    }

    /// Get a pod by name in the specified namespace
    pub async fn get_pod(&self, namespace: &str, name: &str) -> Result<Pod> {
        let url = format!(
            "{}/api/v1/namespaces/{}/pods/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            urlencoding::encode(name)
        );
        let builder = self.client().request(Method::GET, &url);

        sign_send_json(builder, &self.auth_config()).await
    }

    /// List pods in the specified namespace with optional selectors
    pub async fn list_pods(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Pod>> {
        let mut url = format!(
            "{}/api/v1/namespaces/{}/pods",
            self.get_base_url(),
            urlencoding::encode(namespace)
        );
        let mut query_params = Vec::new();

        if let Some(ls) = label_selector {
            query_params.push(("labelSelector", ls));
        }
        if let Some(fs) = field_selector {
            query_params.push(("fieldSelector", fs));
        }

        if !query_params.is_empty() {
            url.push('?');
            url.push_str(
                &query_params
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
                    .collect::<Vec<_>>()
                    .join("&"),
            );
        }

        let builder = self.client().request(Method::GET, &url);

        sign_send_json(builder, &self.auth_config()).await
    }

    /// Update a pod in the specified namespace
    pub async fn update_pod(&self, namespace: &str, name: &str, pod: &Pod) -> Result<Pod> {
        let body = serde_json::to_string(pod).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize Pod '{}'", name),
            },
        )?;

        let url = format!(
            "{}/api/v1/namespaces/{}/pods/{}",
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

    /// Delete a pod in the specified namespace
    pub async fn delete_pod(&self, namespace: &str, name: &str) -> Result<()> {
        let url = format!(
            "{}/api/v1/namespaces/{}/pods/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            urlencoding::encode(name)
        );
        let builder = self.client().request(Method::DELETE, &url);

        sign_send_no_response(builder, &self.auth_config()).await
    }
}

#[async_trait]
impl PodApi for KubernetesClient {
    async fn create_pod(&self, namespace: &str, pod: &Pod) -> Result<Pod> {
        self.create_pod(namespace, pod).await
    }

    async fn get_pod(&self, namespace: &str, name: &str) -> Result<Pod> {
        self.get_pod(namespace, name).await
    }

    async fn list_pods(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Pod>> {
        self.list_pods(namespace, label_selector, field_selector)
            .await
    }

    async fn update_pod(&self, namespace: &str, name: &str, pod: &Pod) -> Result<Pod> {
        self.update_pod(namespace, name, pod).await
    }

    async fn delete_pod(&self, namespace: &str, name: &str) -> Result<()> {
        self.delete_pod(namespace, name).await
    }
}
