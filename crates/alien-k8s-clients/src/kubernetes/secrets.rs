use crate::kubernetes::kubernetes_client::KubernetesClient;
use crate::kubernetes::kubernetes_request_utils::{sign_send_json, sign_send_no_response};
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use reqwest::Method;

use k8s_openapi::api::core::v1::Secret;
use k8s_openapi::List;

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;

#[cfg_attr(feature = "test-utils", automock)]
#[async_trait]
pub trait SecretsApi: Send + Sync + std::fmt::Debug {
    async fn create_secret(&self, namespace: &str, secret: &Secret) -> Result<Secret>;
    async fn get_secret(&self, namespace: &str, name: &str) -> Result<Secret>;
    async fn list_secrets(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Secret>>;
    async fn update_secret(&self, namespace: &str, name: &str, secret: &Secret) -> Result<Secret>;
    async fn delete_secret(&self, namespace: &str, name: &str) -> Result<()>;
}

impl KubernetesClient {
    /// Create a secret in the specified namespace
    pub async fn create_secret(&self, namespace: &str, secret: &Secret) -> Result<Secret> {
        let body = serde_json::to_string(secret).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize Secret '{}'",
                    secret.metadata.name.as_deref().unwrap_or("unknown")
                ),
            },
        )?;

        let url = format!(
            "{}/api/v1/namespaces/{}/secrets",
            self.get_base_url(),
            namespace
        );
        let builder = self
            .client()
            .request(Method::POST, &url)
            .header("Content-Type", "application/json")
            .body(body);

        sign_send_json(builder, &self.auth_config()).await
    }

    /// Get a secret by name in the specified namespace
    pub async fn get_secret(&self, namespace: &str, name: &str) -> Result<Secret> {
        let url = format!(
            "{}/api/v1/namespaces/{}/secrets/{}",
            self.get_base_url(),
            namespace,
            name
        );
        let builder = self.client().request(Method::GET, &url);

        sign_send_json(builder, &self.auth_config()).await
    }

    /// List secrets in the specified namespace with optional selectors
    pub async fn list_secrets(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Secret>> {
        let mut url = format!(
            "{}/api/v1/namespaces/{}/secrets",
            self.get_base_url(),
            namespace
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

    /// Update a secret in the specified namespace
    pub async fn update_secret(
        &self,
        namespace: &str,
        name: &str,
        secret: &Secret,
    ) -> Result<Secret> {
        let body = serde_json::to_string(secret).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize Secret '{}'", name),
            },
        )?;

        let url = format!(
            "{}/api/v1/namespaces/{}/secrets/{}",
            self.get_base_url(),
            namespace,
            name
        );
        let builder = self
            .client()
            .request(Method::PUT, &url)
            .header("Content-Type", "application/json")
            .body(body);

        sign_send_json(builder, &self.auth_config()).await
    }

    /// Delete a secret in the specified namespace
    pub async fn delete_secret(&self, namespace: &str, name: &str) -> Result<()> {
        let url = format!(
            "{}/api/v1/namespaces/{}/secrets/{}",
            self.get_base_url(),
            namespace,
            name
        );
        let builder = self.client().request(Method::DELETE, &url);

        sign_send_no_response(builder, &self.auth_config()).await
    }
}

#[async_trait]
impl SecretsApi for KubernetesClient {
    async fn create_secret(&self, namespace: &str, secret: &Secret) -> Result<Secret> {
        self.create_secret(namespace, secret).await
    }

    async fn get_secret(&self, namespace: &str, name: &str) -> Result<Secret> {
        self.get_secret(namespace, name).await
    }

    async fn list_secrets(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Secret>> {
        self.list_secrets(namespace, label_selector, field_selector)
            .await
    }

    async fn update_secret(&self, namespace: &str, name: &str, secret: &Secret) -> Result<Secret> {
        self.update_secret(namespace, name, secret).await
    }

    async fn delete_secret(&self, namespace: &str, name: &str) -> Result<()> {
        self.delete_secret(namespace, name).await
    }
}
