use crate::kubernetes::kubernetes_client::KubernetesClient;
use crate::kubernetes::kubernetes_request_utils::{sign_send_json, sign_send_no_response};
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use k8s_openapi::api::batch::v1::Job;
use k8s_openapi::List;
use reqwest::Method;

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;

#[cfg_attr(feature = "test-utils", automock)]
#[async_trait]
pub trait JobApi: Send + Sync + std::fmt::Debug {
    async fn create_job(&self, namespace: &str, job: &Job) -> Result<Job>;
    async fn get_job(&self, namespace: &str, name: &str) -> Result<Job>;
    async fn list_jobs(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Job>>;
    async fn update_job(&self, namespace: &str, name: &str, job: &Job) -> Result<Job>;
    async fn delete_job(&self, namespace: &str, name: &str) -> Result<()>;
}

impl KubernetesClient {
    /// Create a job in the specified namespace
    pub async fn create_job(&self, namespace: &str, job: &Job) -> Result<Job> {
        let body = serde_json::to_string(job).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize Job '{}'",
                    job.metadata.name.as_deref().unwrap_or("unknown")
                ),
            },
        )?;

        let url = format!(
            "{}/apis/batch/v1/namespaces/{}/jobs",
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

    /// Get a job by name in the specified namespace
    pub async fn get_job(&self, namespace: &str, name: &str) -> Result<Job> {
        let url = format!(
            "{}/apis/batch/v1/namespaces/{}/jobs/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            urlencoding::encode(name)
        );
        let builder = self.client().request(Method::GET, &url);

        sign_send_json(builder, &self.auth_config()).await
    }

    /// List jobs in the specified namespace with optional selectors
    pub async fn list_jobs(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Job>> {
        let mut url = format!(
            "{}/apis/batch/v1/namespaces/{}/jobs",
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

    /// Update a job in the specified namespace
    pub async fn update_job(&self, namespace: &str, name: &str, job: &Job) -> Result<Job> {
        let body = serde_json::to_string(job).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize Job '{}'", name),
            },
        )?;

        let url = format!(
            "{}/apis/batch/v1/namespaces/{}/jobs/{}",
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

    /// Delete a job in the specified namespace
    pub async fn delete_job(&self, namespace: &str, name: &str) -> Result<()> {
        let url = format!(
            "{}/apis/batch/v1/namespaces/{}/jobs/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            urlencoding::encode(name)
        );
        let builder = self.client().request(Method::DELETE, &url);

        sign_send_no_response(builder, &self.auth_config()).await
    }
}

#[async_trait]
impl JobApi for KubernetesClient {
    async fn create_job(&self, namespace: &str, job: &Job) -> Result<Job> {
        self.create_job(namespace, job).await
    }

    async fn get_job(&self, namespace: &str, name: &str) -> Result<Job> {
        self.get_job(namespace, name).await
    }

    async fn list_jobs(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Job>> {
        self.list_jobs(namespace, label_selector, field_selector)
            .await
    }

    async fn update_job(&self, namespace: &str, name: &str, job: &Job) -> Result<Job> {
        self.update_job(namespace, name, job).await
    }

    async fn delete_job(&self, namespace: &str, name: &str) -> Result<()> {
        self.delete_job(namespace, name).await
    }
}
