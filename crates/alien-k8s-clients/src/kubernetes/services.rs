use crate::kubernetes::kubernetes_client::KubernetesClient;
use crate::kubernetes::kubernetes_request_utils::{sign_send_json, sign_send_no_response};
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use reqwest::Method;

use k8s_openapi::api::core::v1::Service;
use k8s_openapi::List;

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;

#[cfg_attr(feature = "test-utils", automock)]
#[async_trait]
pub trait ServiceApi: Send + Sync + std::fmt::Debug {
    async fn create_service(&self, namespace: &str, service: &Service) -> Result<Service>;
    async fn get_service(&self, namespace: &str, name: &str) -> Result<Service>;
    async fn list_services(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Service>>;
    async fn update_service(
        &self,
        namespace: &str,
        name: &str,
        service: &Service,
    ) -> Result<Service>;
    async fn delete_service(&self, namespace: &str, name: &str) -> Result<()>;
}

impl KubernetesClient {
    /// Create a service in the specified namespace
    pub async fn create_service(&self, namespace: &str, service: &Service) -> Result<Service> {
        let body = serde_json::to_string(service).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize Service '{}'",
                    service.metadata.name.as_deref().unwrap_or("unknown")
                ),
            },
        )?;

        let url = format!(
            "{}/api/v1/namespaces/{}/services",
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

    /// Get a service by name in the specified namespace
    pub async fn get_service(&self, namespace: &str, name: &str) -> Result<Service> {
        let url = format!(
            "{}/api/v1/namespaces/{}/services/{}",
            self.get_base_url(),
            namespace,
            name
        );
        let builder = self.client().request(Method::GET, &url);

        sign_send_json(builder, &self.auth_config()).await
    }

    /// List services in the specified namespace with optional selectors
    pub async fn list_services(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Service>> {
        let mut url = format!(
            "{}/api/v1/namespaces/{}/services",
            self.get_base_url(),
            namespace
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

    /// Update a service in the specified namespace
    pub async fn update_service(
        &self,
        namespace: &str,
        name: &str,
        service: &Service,
    ) -> Result<Service> {
        let body = serde_json::to_string(service).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize Service '{}'", name),
            },
        )?;

        let url = format!(
            "{}/api/v1/namespaces/{}/services/{}",
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

    /// Delete a service in the specified namespace
    pub async fn delete_service(&self, namespace: &str, name: &str) -> Result<()> {
        let url = format!(
            "{}/api/v1/namespaces/{}/services/{}",
            self.get_base_url(),
            namespace,
            name
        );
        let builder = self.client().request(Method::DELETE, &url);

        sign_send_no_response(builder, &self.auth_config()).await
    }
}

#[async_trait]
impl ServiceApi for KubernetesClient {
    async fn create_service(&self, namespace: &str, service: &Service) -> Result<Service> {
        self.create_service(namespace, service).await
    }

    async fn get_service(&self, namespace: &str, name: &str) -> Result<Service> {
        self.get_service(namespace, name).await
    }

    async fn list_services(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Service>> {
        self.list_services(namespace, label_selector, field_selector)
            .await
    }

    async fn update_service(
        &self,
        namespace: &str,
        name: &str,
        service: &Service,
    ) -> Result<Service> {
        self.update_service(namespace, name, service).await
    }

    async fn delete_service(&self, namespace: &str, name: &str) -> Result<()> {
        self.delete_service(namespace, name).await
    }
}
