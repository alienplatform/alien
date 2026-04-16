use crate::gcp::api_client::{GcpClientBase, GcpServiceConfig};
use crate::gcp::GcpClientConfig;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use alien_client_core::Result;
use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;
use std::fmt::Debug;

/// Cloud Scheduler service configuration
#[derive(Debug)]
pub struct CloudSchedulerServiceConfig;

impl GcpServiceConfig for CloudSchedulerServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://cloudscheduler.googleapis.com/v1"
    }

    fn default_audience(&self) -> &'static str {
        "https://cloudscheduler.googleapis.com/"
    }

    fn service_name(&self) -> &'static str {
        "Cloud Scheduler"
    }

    fn service_key(&self) -> &'static str {
        "cloudscheduler"
    }
}

/// A scheduled job managed by Cloud Scheduler.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerJob {
    /// The full resource name of the job (output only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A human-readable description for the job.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The cron schedule string (e.g. "*/5 * * * *").
    pub schedule: String,
    /// The IANA time zone for interpreting the schedule (e.g. "America/New_York").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,
    /// HTTP target configuration for the job.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_target: Option<HttpTarget>,
    /// The current state of the job (e.g. "ENABLED", "PAUSED", "DISABLED").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

/// HTTP target configuration for a Cloud Scheduler job.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpTarget {
    /// The full URI of the target endpoint.
    pub uri: String,
    /// The HTTP method to use (e.g. "POST", "GET").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_method: Option<String>,
    /// The base64-encoded body of the HTTP request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Additional HTTP headers to include in the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
    /// OIDC token configuration for authenticating the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_token: Option<SchedulerOidcToken>,
}

/// OIDC token configuration for Cloud Scheduler HTTP targets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerOidcToken {
    /// The service account email to use for generating the OIDC token.
    pub service_account_email: String,
    /// The audience for the generated OIDC token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<String>,
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait CloudSchedulerApi: Send + Sync + Debug {
    /// Creates a new scheduled job.
    async fn create_job(
        &self,
        location: String,
        job_id: String,
        job: SchedulerJob,
    ) -> Result<SchedulerJob>;

    /// Deletes a job by its full resource name.
    async fn delete_job(&self, job_name: String) -> Result<()>;

    /// Gets a job by its full resource name.
    async fn get_job(&self, job_name: String) -> Result<SchedulerJob>;
}

/// Cloud Scheduler client for managing scheduled jobs.
#[derive(Debug)]
pub struct CloudSchedulerClient {
    base: GcpClientBase,
    project_id: String,
}

impl CloudSchedulerClient {
    pub fn new(client: Client, config: GcpClientConfig) -> Self {
        let project_id = config.project_id.clone();
        Self {
            base: GcpClientBase::new(client, config, Box::new(CloudSchedulerServiceConfig)),
            project_id,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl CloudSchedulerApi for CloudSchedulerClient {
    /// Creates a new Cloud Scheduler job.
    /// See: https://cloud.google.com/scheduler/docs/reference/rest/v1/projects.locations.jobs/create
    async fn create_job(
        &self,
        location: String,
        job_id: String,
        job: SchedulerJob,
    ) -> Result<SchedulerJob> {
        let path = format!(
            "projects/{}/locations/{}/jobs",
            self.project_id, location
        );
        // The job ID is set via the `name` field in the request body, not a query param.
        // Full resource name format: projects/{project}/locations/{location}/jobs/{jobId}
        let mut job_with_name = job;
        job_with_name.name = Some(format!(
            "projects/{}/locations/{}/jobs/{}",
            self.project_id, location, job_id
        ));

        self.base
            .execute_request(Method::POST, &path, None, Some(job_with_name), &job_id)
            .await
    }

    /// Deletes a Cloud Scheduler job.
    /// See: https://cloud.google.com/scheduler/docs/reference/rest/v1/projects.locations.jobs/delete
    async fn delete_job(&self, job_name: String) -> Result<()> {
        self.base
            .execute_request_no_response(
                Method::DELETE,
                &job_name,
                None,
                Option::<()>::None,
                &job_name,
            )
            .await
    }

    /// Gets a Cloud Scheduler job.
    /// See: https://cloud.google.com/scheduler/docs/reference/rest/v1/projects.locations.jobs/get
    async fn get_job(&self, job_name: String) -> Result<SchedulerJob> {
        self.base
            .execute_request(
                Method::GET,
                &job_name,
                None,
                Option::<()>::None,
                &job_name,
            )
            .await
    }
}
