//! Azure AI Foundry (Azure OpenAI) fine-tuning — a **data-plane** client.
//!
//! Unlike [`cognitive_services`](crate::azure::cognitive_services), which drives
//! the ARM/control-plane to provision the account and its deployments, this
//! client talks to the *account's own OpenAI data-plane endpoint*
//! (`https://{account}.openai.azure.com` / `.cognitiveservices.azure.com`).
//!
//! Two differences from the control-plane client:
//! - The base URL is the **account endpoint**, supplied per call by the caller
//!   (the controller already learns it when the account is provisioned). There
//!   is no subscription/resource-group path.
//! - The bearer token is minted for the **cognitive-services data-plane scope**
//!   `https://cognitiveservices.azure.com/.default`, not
//!   `https://management.azure.com/.default`. Data-plane RBAC (e.g. the
//!   `Cognitive Services OpenAI Contributor` role the controller applies) is
//!   distinct from ARM Contributor.
//!
//! REST surface (Azure OpenAI fine-tuning, API version `2024-10-21`):
//! - `POST   {endpoint}/openai/fine_tuning/jobs?api-version=...`
//! - `GET    {endpoint}/openai/fine_tuning/jobs/{job_id}?api-version=...`

use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};

#[cfg(feature = "test-utils")]
use mockall::automock;

/// Fine-tuning REST API version. Azure pins the whole `fine_tuning.jobs`
/// surface to a dated version supplied as an `api-version` query parameter.
pub const OPENAI_FINE_TUNING_API_VERSION: &str = "2024-10-21";

/// OAuth scope for the Azure Cognitive Services **data plane**.
///
/// Distinct from the ARM management scope (`https://management.azure.com/.default`)
/// used by the control-plane client: fine-tuning is a data action authorized by
/// the data-plane RBAC role assigned on the account.
pub const COGNITIVE_SERVICES_DATA_PLANE_SCOPE: &str = "https://cognitiveservices.azure.com/.default";

// -------------------------------------------------------------------------
// Data-plane models
// -------------------------------------------------------------------------

/// Request body for `POST /openai/fine_tuning/jobs`.
///
/// `training_file` is the identifier Azure OpenAI accepts for the dataset. For
/// a Foundry import from customer Blob storage this is the blob reference; see
/// [`FoundryFineTuningApi::create_fine_tuning_job`] for the network gotcha.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FineTuningJobCreateRequest {
    /// Provider-native base model to fine-tune (e.g. `gpt-4o-mini`).
    pub model: String,
    /// The training dataset reference the job reads from.
    pub training_file: String,
}

/// A fine-tuning job as returned by `POST` (creation) and `GET` (poll).
///
/// Field names are Azure OpenAI's native `snake_case`, so this type does **not**
/// use the workspace's usual `camelCase` rename — matching the wire shape is the
/// whole point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FineTuningJob {
    /// The job id (e.g. `ftjob-abc123`). Stored by the controller to poll.
    pub id: String,
    /// Lifecycle status: `pending`, `validating_files`, `queued`, `running`,
    /// `succeeded`, `failed`, or `cancelled`.
    pub status: String,
    /// Populated only once `status == "succeeded"`: the tuned model name the
    /// OpenAI chat endpoint accepts as a deployment target.
    #[serde(default)]
    pub fine_tuned_model: Option<String>,
}

impl FineTuningJob {
    /// Whether the job reached the successful terminal state.
    pub fn is_succeeded(&self) -> bool {
        self.status.eq_ignore_ascii_case("succeeded")
    }

    /// Whether the job reached a terminal *failure* state (`failed`/`cancelled`).
    /// The controller fails fast on these rather than polling forever.
    pub fn is_terminal_failure(&self) -> bool {
        self.status.eq_ignore_ascii_case("failed")
            || self.status.eq_ignore_ascii_case("cancelled")
            || self.status.eq_ignore_ascii_case("canceled")
    }
}

// -------------------------------------------------------------------------
// Foundry fine-tuning API trait
// -------------------------------------------------------------------------

/// Data-plane fine-tuning operations against an Azure AI Foundry account.
///
/// The `endpoint` argument is the account's OpenAI data-plane base URL (e.g.
/// `https://my-account.openai.azure.com`), learned by the controller when the
/// account is provisioned.
#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait FoundryFineTuningApi: Send + Sync + std::fmt::Debug {
    /// Submit a fine-tuning job (`POST /openai/fine_tuning/jobs`).
    ///
    /// `training_file` is the dataset reference the job reads. Foundry supports
    /// importing directly from customer Blob storage, but **that import path
    /// requires the account to allow public network access** — a private-endpoint
    /// account will reject the blob URL at submit time. This client passes the
    /// caller's reference through unchanged; enforcing network posture is the
    /// controller/account concern.
    async fn create_fine_tuning_job(
        &self,
        endpoint: &str,
        model: &str,
        training_file: &str,
    ) -> Result<FineTuningJob>;

    /// Poll a fine-tuning job's status (`GET /openai/fine_tuning/jobs/{job_id}`).
    /// On success the returned job carries `fine_tuned_model`.
    async fn get_fine_tuning_job(&self, endpoint: &str, job_id: &str) -> Result<FineTuningJob>;
}

// -------------------------------------------------------------------------
// Foundry fine-tuning client struct
// -------------------------------------------------------------------------

#[derive(Debug)]
pub struct AzureFoundryFineTuningClient {
    pub base: AzureClientBase,
    pub token_cache: AzureTokenCache,
}

impl AzureFoundryFineTuningClient {
    pub fn new(client: Client, token_cache: AzureTokenCache) -> Self {
        // The base endpoint is supplied per call (the account endpoint), so the
        // base's own endpoint field is unused for URL building here. Seed it with
        // the management endpoint for parity with the other clients.
        let endpoint = token_cache.management_endpoint().to_string();
        Self {
            base: AzureClientBase::with_client_config(
                client,
                endpoint,
                token_cache.config().clone(),
            ),
            token_cache,
        }
    }

    /// Builds `{endpoint}/openai/fine_tuning/jobs[/{job_id}]?api-version=...`.
    fn jobs_url(&self, endpoint: &str, job_id: Option<&str>) -> String {
        let base = endpoint.trim_end_matches('/');
        let path = match job_id {
            Some(id) => format!("{base}/openai/fine_tuning/jobs/{id}"),
            None => format!("{base}/openai/fine_tuning/jobs"),
        };
        format!("{path}?api-version={OPENAI_FINE_TUNING_API_VERSION}")
    }

    /// Reads and JSON-parses a `FineTuningJob` from a successful response.
    async fn parse_job(resp: reqwest::Response, url: &str, op: &str) -> Result<FineTuningJob> {
        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!("Azure {op}: failed to read fine-tuning job response body"),
                url: url.to_string(),
                http_status: status,
                http_request_text: None,
                http_response_text: None,
            })?;

        serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!("Azure {op}: JSON parse error for fine-tuning job"),
                url: url.to_string(),
                http_status: status,
                http_request_text: None,
                http_response_text: Some(body),
            })
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl FoundryFineTuningApi for AzureFoundryFineTuningClient {
    async fn create_fine_tuning_job(
        &self,
        endpoint: &str,
        model: &str,
        training_file: &str,
    ) -> Result<FineTuningJob> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope(COGNITIVE_SERVICES_DATA_PLANE_SCOPE)
            .await?;

        let url = self.jobs_url(endpoint, None);

        let request = FineTuningJobCreateRequest {
            model: model.to_string(),
            training_file: training_file.to_string(),
        };
        let body = serde_json::to_string(&request)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize fine-tuning job create request for model '{model}'"
                ),
            })?;

        let builder = AzureRequestBuilder::new(Method::POST, url.clone())
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "CreateFineTuningJob", model)
            .await?;

        Self::parse_job(resp, &url, "CreateFineTuningJob").await
    }

    async fn get_fine_tuning_job(&self, endpoint: &str, job_id: &str) -> Result<FineTuningJob> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope(COGNITIVE_SERVICES_DATA_PLANE_SCOPE)
            .await?;

        let url = self.jobs_url(endpoint, Some(job_id));

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");
        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetFineTuningJob", job_id)
            .await?;

        Self::parse_job(resp, &url, "GetFineTuningJob").await
    }
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_pinned_api_version_and_data_plane_scope() {
        assert_eq!(OPENAI_FINE_TUNING_API_VERSION, "2024-10-21");
        // The data-plane scope must NOT be the ARM management scope — a token for
        // the wrong audience is rejected by Azure.
        assert_eq!(
            COGNITIVE_SERVICES_DATA_PLANE_SCOPE,
            "https://cognitiveservices.azure.com/.default"
        );
        assert_ne!(
            COGNITIVE_SERVICES_DATA_PLANE_SCOPE,
            "https://management.azure.com/.default"
        );
    }

    #[test]
    fn create_request_serializes_snake_case_wire_shape() {
        let req = FineTuningJobCreateRequest {
            model: "gpt-4o-mini".to_string(),
            training_file: "https://acct.blob.core.windows.net/data/training.jsonl".to_string(),
        };
        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
        assert_eq!(value["model"], "gpt-4o-mini");
        assert_eq!(
            value["training_file"],
            "https://acct.blob.core.windows.net/data/training.jsonl"
        );
    }

    #[test]
    fn job_status_helpers_classify_terminal_states() {
        let succeeded = FineTuningJob {
            id: "ftjob-1".to_string(),
            status: "succeeded".to_string(),
            fine_tuned_model: Some("gpt-4o-mini.ft-1".to_string()),
        };
        assert!(succeeded.is_succeeded());
        assert!(!succeeded.is_terminal_failure());

        for failed in ["failed", "cancelled", "canceled", "FAILED"] {
            let job = FineTuningJob {
                id: "ftjob-2".to_string(),
                status: failed.to_string(),
                fine_tuned_model: None,
            };
            assert!(job.is_terminal_failure(), "'{failed}' must be terminal failure");
            assert!(!job.is_succeeded());
        }

        for pending in ["pending", "running", "queued", "validating_files"] {
            let job = FineTuningJob {
                id: "ftjob-3".to_string(),
                status: pending.to_string(),
                fine_tuned_model: None,
            };
            assert!(!job.is_succeeded(), "'{pending}' is not succeeded");
            assert!(!job.is_terminal_failure(), "'{pending}' is not terminal failure");
        }
    }

    #[test]
    fn deserializes_get_response_with_fine_tuned_model() {
        // Azure's GET response once the job succeeds.
        let json = r#"{
            "id": "ftjob-abc123",
            "status": "succeeded",
            "model": "gpt-4o-mini",
            "fine_tuned_model": "gpt-4o-mini.ft-abc123",
            "object": "fine_tuning.job"
        }"#;
        let job: FineTuningJob = serde_json::from_str(json).expect("should deserialize");
        assert_eq!(job.id, "ftjob-abc123");
        assert!(job.is_succeeded());
        assert_eq!(job.fine_tuned_model.as_deref(), Some("gpt-4o-mini.ft-abc123"));
    }

    #[test]
    fn deserializes_pending_response_without_fine_tuned_model() {
        // A freshly-created job has no fine_tuned_model yet.
        let json = r#"{ "id": "ftjob-abc123", "status": "pending", "object": "fine_tuning.job" }"#;
        let job: FineTuningJob = serde_json::from_str(json).expect("should deserialize");
        assert_eq!(job.status, "pending");
        assert!(job.fine_tuned_model.is_none());
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod http_tests {
    use super::*;
    use crate::azure::{AzureClientConfig, AzureClientConfigExt};
    use httpmock::{Method::GET, Method::POST, MockServer};
    use serde_json::json;

    fn test_client() -> AzureFoundryFineTuningClient {
        // The account endpoint is passed per call, so the client itself needs no
        // endpoint override — the MockServer URL is the `endpoint` argument.
        AzureFoundryFineTuningClient::new(
            Client::new(),
            AzureTokenCache::new(AzureClientConfig::mock()),
        )
    }

    /// The POST must hit the data-plane fine-tuning path with the pinned
    /// api-version, carry a bearer token, and send the snake_case body. This is
    /// the request-shape contract the gateway/controller relies on.
    #[tokio::test]
    async fn create_job_builds_correct_request() {
        let server = MockServer::start_async().await;

        let request_mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/openai/fine_tuning/jobs")
                    .query_param("api-version", OPENAI_FINE_TUNING_API_VERSION)
                    .header_exists("authorization")
                    .json_body(json!({
                        "model": "gpt-4o-mini",
                        "training_file": "https://acct.blob.core.windows.net/data/training.jsonl"
                    }));
                then.status(201).json_body(json!({
                    "id": "ftjob-xyz",
                    "status": "pending",
                    "object": "fine_tuning.job"
                }));
            })
            .await;

        let job = test_client()
            .create_fine_tuning_job(
                &server.base_url(),
                "gpt-4o-mini",
                "https://acct.blob.core.windows.net/data/training.jsonl",
            )
            .await
            .expect("create fine-tuning job should succeed");

        request_mock.assert_async().await;
        assert_eq!(job.id, "ftjob-xyz");
        assert_eq!(job.status, "pending");
        assert!(job.fine_tuned_model.is_none());
    }

    /// The GET must hit `/openai/fine_tuning/jobs/{job_id}` and parse the tuned
    /// model name out of a succeeded response.
    #[tokio::test]
    async fn get_job_builds_correct_request_and_parses_success() {
        let server = MockServer::start_async().await;

        let request_mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/openai/fine_tuning/jobs/ftjob-xyz")
                    .query_param("api-version", OPENAI_FINE_TUNING_API_VERSION)
                    .header_exists("authorization");
                then.status(200).json_body(json!({
                    "id": "ftjob-xyz",
                    "status": "succeeded",
                    "fine_tuned_model": "gpt-4o-mini.ft-xyz",
                    "object": "fine_tuning.job"
                }));
            })
            .await;

        let job = test_client()
            .get_fine_tuning_job(&server.base_url(), "ftjob-xyz")
            .await
            .expect("get fine-tuning job should succeed");

        request_mock.assert_async().await;
        assert!(job.is_succeeded());
        assert_eq!(job.fine_tuned_model.as_deref(), Some("gpt-4o-mini.ft-xyz"));
    }
}
