use crate::kubernetes::kubernetes_client::KubernetesClient;
use crate::kubernetes::kubernetes_request_utils::{sign_send_json, sign_send_no_response};
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use reqwest::Method;

use k8s_openapi::api::apps::v1::{Deployment, StatefulSet};
use k8s_openapi::List;

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;

#[cfg_attr(feature = "test-utils", automock)]
#[async_trait]
pub trait DeploymentApi: Send + Sync + std::fmt::Debug {
    async fn create_deployment(
        &self,
        namespace: &str,
        deployment: &Deployment,
    ) -> Result<Deployment>;
    async fn get_deployment(&self, namespace: &str, name: &str) -> Result<Deployment>;
    async fn list_deployments(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Deployment>>;
    async fn update_deployment(
        &self,
        namespace: &str,
        name: &str,
        deployment: &Deployment,
    ) -> Result<Deployment>;
    async fn delete_deployment(&self, namespace: &str, name: &str) -> Result<()>;

    async fn create_statefulset(
        &self,
        namespace: &str,
        statefulset: &StatefulSet,
    ) -> Result<StatefulSet>;
    async fn get_statefulset(&self, namespace: &str, name: &str) -> Result<StatefulSet>;
    async fn update_statefulset(
        &self,
        namespace: &str,
        name: &str,
        statefulset: &StatefulSet,
    ) -> Result<StatefulSet>;
    async fn delete_statefulset(&self, namespace: &str, name: &str) -> Result<()>;
}

impl KubernetesClient {
    /// Create a deployment in the specified namespace
    pub async fn create_deployment(
        &self,
        namespace: &str,
        deployment: &Deployment,
    ) -> Result<Deployment> {
        let body = serde_json::to_string(deployment)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize Deployment '{}'",
                    deployment.metadata.name.as_deref().unwrap_or("unknown")
                ),
            })?;

        let url = format!(
            "{}/apis/apps/v1/namespaces/{}/deployments",
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

    /// Get a deployment by name in the specified namespace
    pub async fn get_deployment(&self, namespace: &str, name: &str) -> Result<Deployment> {
        let url = format!(
            "{}/apis/apps/v1/namespaces/{}/deployments/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            urlencoding::encode(name)
        );
        let builder = self.client().request(Method::GET, &url);

        sign_send_json(builder, &self.auth_config()).await
    }

    /// List deployments in the specified namespace with optional selectors
    pub async fn list_deployments(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Deployment>> {
        let mut url = format!(
            "{}/apis/apps/v1/namespaces/{}/deployments",
            self.get_base_url(),
            urlencoding::encode(namespace)
        );
        let mut query_params = Vec::new();

        if let Some(ls) = &label_selector {
            query_params.push(("labelSelector", ls.as_str()));
        }
        if let Some(fs) = &field_selector {
            query_params.push(("fieldSelector", fs.as_str()));
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

    /// Update a deployment in the specified namespace
    pub async fn update_deployment(
        &self,
        namespace: &str,
        name: &str,
        deployment: &Deployment,
    ) -> Result<Deployment> {
        let body = serde_json::to_string(deployment)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Failed to serialize Deployment '{}'", name),
            })?;

        let url = format!(
            "{}/apis/apps/v1/namespaces/{}/deployments/{}",
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

    /// Delete a deployment in the specified namespace
    pub async fn delete_deployment(&self, namespace: &str, name: &str) -> Result<()> {
        let url = format!(
            "{}/apis/apps/v1/namespaces/{}/deployments/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            urlencoding::encode(name)
        );
        let builder = self.client().request(Method::DELETE, &url);

        sign_send_no_response(builder, &self.auth_config()).await
    }

    /// Create a statefulset in the specified namespace
    pub async fn create_statefulset(
        &self,
        namespace: &str,
        statefulset: &StatefulSet,
    ) -> Result<StatefulSet> {
        let body = serde_json::to_string(statefulset)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize StatefulSet '{}'",
                    statefulset.metadata.name.as_deref().unwrap_or("unknown")
                ),
            })?;

        let url = format!(
            "{}/apis/apps/v1/namespaces/{}/statefulsets",
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

    /// Get a statefulset by name in the specified namespace
    pub async fn get_statefulset(&self, namespace: &str, name: &str) -> Result<StatefulSet> {
        let url = format!(
            "{}/apis/apps/v1/namespaces/{}/statefulsets/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            urlencoding::encode(name)
        );
        let builder = self.client().request(Method::GET, &url);

        sign_send_json(builder, &self.auth_config()).await
    }

    /// Update a statefulset in the specified namespace
    pub async fn update_statefulset(
        &self,
        namespace: &str,
        name: &str,
        statefulset: &StatefulSet,
    ) -> Result<StatefulSet> {
        let body = serde_json::to_string(statefulset)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Failed to serialize StatefulSet '{}'", name),
            })?;

        let url = format!(
            "{}/apis/apps/v1/namespaces/{}/statefulsets/{}",
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

    /// Delete a statefulset in the specified namespace
    pub async fn delete_statefulset(&self, namespace: &str, name: &str) -> Result<()> {
        let url = format!(
            "{}/apis/apps/v1/namespaces/{}/statefulsets/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            urlencoding::encode(name)
        );
        let builder = self.client().request(Method::DELETE, &url);

        sign_send_no_response(builder, &self.auth_config()).await
    }
}

#[async_trait]
impl DeploymentApi for KubernetesClient {
    async fn create_deployment(
        &self,
        namespace: &str,
        deployment: &Deployment,
    ) -> Result<Deployment> {
        self.create_deployment(namespace, deployment).await
    }

    async fn get_deployment(&self, namespace: &str, name: &str) -> Result<Deployment> {
        self.get_deployment(namespace, name).await
    }

    async fn list_deployments(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Deployment>> {
        self.list_deployments(namespace, label_selector, field_selector)
            .await
    }

    async fn update_deployment(
        &self,
        namespace: &str,
        name: &str,
        deployment: &Deployment,
    ) -> Result<Deployment> {
        self.update_deployment(namespace, name, deployment).await
    }

    async fn delete_deployment(&self, namespace: &str, name: &str) -> Result<()> {
        self.delete_deployment(namespace, name).await
    }

    async fn create_statefulset(
        &self,
        namespace: &str,
        statefulset: &StatefulSet,
    ) -> Result<StatefulSet> {
        self.create_statefulset(namespace, statefulset).await
    }

    async fn get_statefulset(&self, namespace: &str, name: &str) -> Result<StatefulSet> {
        self.get_statefulset(namespace, name).await
    }

    async fn update_statefulset(
        &self,
        namespace: &str,
        name: &str,
        statefulset: &StatefulSet,
    ) -> Result<StatefulSet> {
        self.update_statefulset(namespace, name, statefulset).await
    }

    async fn delete_statefulset(&self, namespace: &str, name: &str) -> Result<()> {
        self.delete_statefulset(namespace, name).await
    }
}
