//! Vertex AI Platform client covering the tuning-job surface used for
//! fine-tuning.
//!
//! Only the two calls the fine-tuning controller needs are exposed:
//! `create_tuning_job` (POST `.../tuningJobs`) and `get_tuning_job`
//! (GET `.../tuningJobs/{id}`). See:
//! <https://cloud.google.com/vertex-ai/docs/reference/rest/v1/projects.locations.tuningJobs>
//!
//! Unlike the other GCP services, Vertex AI is *regional*: the host is
//! `{location}-aiplatform.googleapis.com`. Because [`GcpServiceConfig::base_url`]
//! must return a `&'static str`, the location-specific base URL is built in the
//! client constructor and injected as a service override, so [`GcpClientBase`]'s
//! standard override lookup resolves it.

use crate::gcp::api_client::{GcpClientBase, GcpServiceConfig};
use crate::gcp::longrunning::Status;
use crate::gcp::GcpClientConfig;
use crate::gcp::GcpClientConfigExt;
use crate::gcp::ServiceOverrides;
use alien_client_core::Result;
use bon::Builder;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

#[cfg(feature = "test-utils")]
use mockall::automock;

/// Service key used to inject the regional base-URL override for Vertex AI.
const AIPLATFORM_SERVICE_KEY: &str = "aiplatform";

/// Vertex AI (aiplatform) service configuration.
///
/// The `base_url` returned here is a placeholder for the global host; the real,
/// region-scoped host is always supplied via a service override installed by
/// [`AiPlatformClient::new`], so this default is never used in practice.
#[derive(Debug)]
pub struct AiPlatformServiceConfig;

impl GcpServiceConfig for AiPlatformServiceConfig {
    fn base_url(&self) -> &'static str {
        // Overridden per-region in the constructor; kept valid as a fallback.
        "https://aiplatform.googleapis.com/v1"
    }

    fn default_audience(&self) -> &'static str {
        "https://aiplatform.googleapis.com/"
    }

    fn service_name(&self) -> &'static str {
        "Vertex AI"
    }

    fn service_key(&self) -> &'static str {
        AIPLATFORM_SERVICE_KEY
    }
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait AiPlatformApi: Send + Sync + Debug {
    /// Submits a supervised tuning job and returns the created [`TuningJob`],
    /// whose `name` (`projects/{p}/locations/{l}/tuningJobs/{id}`) is the handle
    /// to poll.
    async fn create_tuning_job(&self, request: CreateTuningJobRequest) -> Result<TuningJob>;

    /// Fetches the current state of a tuning job by its full resource name
    /// (`projects/{p}/locations/{l}/tuningJobs/{id}`) or bare id.
    async fn get_tuning_job(&self, name: String) -> Result<TuningJob>;
}

/// Vertex AI tuning client.
#[derive(Debug)]
pub struct AiPlatformClient {
    base: GcpClientBase,
    project_id: String,
    location: String,
}

impl AiPlatformClient {
    /// Builds a client pinned to the config's region. Installs the regional
    /// `aiplatform` base-URL override (unless the caller already set one, e.g.
    /// a test endpoint) so all requests hit `{location}-aiplatform...`.
    pub fn new(client: Client, config: GcpClientConfig) -> Self {
        let project_id = config.project_id.clone();
        let location = config.region.clone();

        // Only synthesize the regional host when no override is present; this
        // lets tests point the client at a mock endpoint via service overrides.
        let mut config = config;
        if config.get_service_endpoint_option(AIPLATFORM_SERVICE_KEY).is_none() {
            let regional = format!("https://{location}-aiplatform.googleapis.com/v1");
            let mut overrides = config.service_overrides.unwrap_or(ServiceOverrides {
                endpoints: std::collections::HashMap::new(),
            });
            overrides
                .endpoints
                .insert(AIPLATFORM_SERVICE_KEY.to_string(), regional);
            config.service_overrides = Some(overrides);
        }

        Self {
            base: GcpClientBase::new(client, config, Box::new(AiPlatformServiceConfig)),
            project_id,
            location,
        }
    }

    /// The collection path `projects/{p}/locations/{l}/tuningJobs`.
    fn tuning_jobs_path(&self) -> String {
        format!(
            "projects/{}/locations/{}/tuningJobs",
            self.project_id, self.location
        )
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl AiPlatformApi for AiPlatformClient {
    async fn create_tuning_job(&self, request: CreateTuningJobRequest) -> Result<TuningJob> {
        let path = self.tuning_jobs_path();
        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Some(request.clone()),
                &request.base_model,
            )
            .await
    }

    async fn get_tuning_job(&self, name: String) -> Result<TuningJob> {
        // Vertex returns the full resource name; accept either that or a bare id
        // and build the region-scoped collection path either way.
        let path = if name.contains("/tuningJobs/") {
            name.clone()
        } else {
            format!("{}/{}", self.tuning_jobs_path(), name)
        };

        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &name)
            .await
    }
}

// --- Data Structures ---

/// Request body for `tuningJobs.create`.
///
/// Only the supervised-tuning surface is modelled: `baseModel`, the training
/// dataset URI (a `gs://` path to JSONL), and an optional display name. See
/// <https://cloud.google.com/vertex-ai/docs/reference/rest/v1/projects.locations.tuningJobs/create>.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateTuningJobRequest {
    /// Provider-native base model to tune (e.g. a Gemini model id).
    pub base_model: String,

    /// Supervised tuning parameters, including the training dataset URI.
    pub supervised_tuning_spec: SupervisedTuningSpec,

    /// Optional human-readable name for the resulting tuned model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tuned_model_display_name: Option<String>,
}

/// Supervised tuning parameters.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SupervisedTuningSpec {
    /// `gs://` URI of the JSONL training dataset in the customer bucket.
    pub training_dataset_uri: String,

    /// Optional `gs://` URI of a JSONL validation dataset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validation_dataset_uri: Option<String>,

    /// Optional supervised-tuning hyperparameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hyper_parameters: Option<SupervisedHyperParameters>,
}

/// Supervised tuning hyperparameters (all optional; Vertex picks defaults).
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SupervisedHyperParameters {
    /// Number of complete passes over the training dataset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epoch_count: Option<String>,

    /// Multiplier applied to the recommended learning rate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub learning_rate_multiplier: Option<f64>,
}

/// A Vertex AI tuning job.
///
/// Mirrors the fields the controller needs from
/// <https://cloud.google.com/vertex-ai/docs/reference/rest/v1/projects.locations.tuningJobs#TuningJob>.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TuningJob {
    /// Server-assigned resource name:
    /// `projects/{p}/locations/{l}/tuningJobs/{id}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Current lifecycle state of the job.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<JobState>,

    /// The tuned model produced once the job reaches `JOB_STATE_SUCCEEDED`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tuned_model: Option<TunedModelRef>,

    /// Populated when the job fails; carries the gRPC-style status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Status>,
}

/// Reference to the artifacts a completed tuning job produced.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TunedModelRef {
    /// Resource name of the tuned Model:
    /// `projects/{p}/locations/{l}/models/{model}@{version}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Resource name of the Endpoint serving the tuned model:
    /// `projects/{p}/locations/{l}/endpoints/{endpoint}`. This is the id the
    /// Vertex OpenAI-compat chat path routes to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

/// Lifecycle state of a Vertex AI job (subset of `google.cloud.aiplatform.v1.JobState`).
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum JobState {
    /// The job state is unspecified.
    JobStateUnspecified,
    /// The job has been created and is awaiting resources.
    JobStateQueued,
    /// The job is pending; not yet running.
    JobStatePending,
    /// The job is currently running.
    JobStateRunning,
    /// The job completed successfully.
    JobStateSucceeded,
    /// The job failed.
    JobStateFailed,
    /// The job is being cancelled.
    JobStateCancelling,
    /// The job was cancelled.
    JobStateCancelled,
    /// The job was paused.
    JobStatePaused,
    /// The job expired.
    JobStateExpired,
    /// The job is being updated.
    JobStateUpdating,
    /// The job partially failed (some outputs missing).
    JobStatePartiallySucceeded,
}

impl JobState {
    /// Whether the job is still making progress and should be polled again.
    pub fn is_in_progress(self) -> bool {
        matches!(
            self,
            JobState::JobStateUnspecified
                | JobState::JobStateQueued
                | JobState::JobStatePending
                | JobState::JobStateRunning
                | JobState::JobStateCancelling
                | JobState::JobStatePaused
                | JobState::JobStateUpdating
        )
    }

    /// Whether the job reached a terminal *failure* state (never produces a
    /// usable tuned model).
    pub fn is_terminal_failure(self) -> bool {
        matches!(
            self,
            JobState::JobStateFailed
                | JobState::JobStateCancelled
                | JobState::JobStateExpired
                | JobState::JobStatePartiallySucceeded
        )
    }
}

impl TunedModelRef {
    /// The id the Vertex OpenAI-compat chat endpoint accepts for a tuned model:
    /// the serving `endpoint` resource name, falling back to the `model`
    /// resource name if the endpoint is absent.
    pub fn upstream_id(&self) -> Option<&str> {
        self.endpoint
            .as_deref()
            .or(self.model.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_request_serializes_supervised_shape() {
        let request = CreateTuningJobRequest::builder()
            .base_model("gemini-2.0-flash-001".to_string())
            .supervised_tuning_spec(
                SupervisedTuningSpec::builder()
                    .training_dataset_uri("gs://my-bucket/training.jsonl".to_string())
                    .build(),
            )
            .tuned_model_display_name("my-ai-tuned".to_string())
            .build();

        let json = serde_json::to_value(&request).expect("request should serialize");

        assert_eq!(json["baseModel"], "gemini-2.0-flash-001");
        assert_eq!(
            json["supervisedTuningSpec"]["trainingDatasetUri"],
            "gs://my-bucket/training.jsonl"
        );
        assert_eq!(json["tunedModelDisplayName"], "my-ai-tuned");
        // Optional fields must be omitted, not sent as null (Vertex rejects nulls).
        assert!(
            json["supervisedTuningSpec"].get("validationDatasetUri").is_none(),
            "unset validationDatasetUri must be omitted, got {json:?}"
        );
        assert!(
            json["supervisedTuningSpec"].get("hyperParameters").is_none(),
            "unset hyperParameters must be omitted, got {json:?}"
        );
    }

    #[test]
    fn tuning_job_deserializes_succeeded_with_endpoint() {
        let body = r#"{
            "name": "projects/p/locations/us-central1/tuningJobs/123",
            "state": "JOB_STATE_SUCCEEDED",
            "tunedModel": {
                "model": "projects/p/locations/us-central1/models/456@1",
                "endpoint": "projects/p/locations/us-central1/endpoints/789"
            }
        }"#;

        let job: TuningJob = serde_json::from_str(body).expect("job should deserialize");

        assert_eq!(job.state, Some(JobState::JobStateSucceeded));
        assert!(job.state.unwrap().is_terminal_failure() == false);
        let tuned = job.tuned_model.expect("tuned model present on success");
        // The OpenAI-compat chat path routes to the serving endpoint.
        assert_eq!(
            tuned.upstream_id(),
            Some("projects/p/locations/us-central1/endpoints/789")
        );
    }

    #[test]
    fn tuning_job_upstream_falls_back_to_model_without_endpoint() {
        let tuned = TunedModelRef::builder()
            .model("projects/p/locations/us-central1/models/456@1".to_string())
            .build();
        assert_eq!(
            tuned.upstream_id(),
            Some("projects/p/locations/us-central1/models/456@1")
        );
    }

    #[test]
    fn tuning_job_deserializes_failed_with_error() {
        let body = r#"{
            "name": "projects/p/locations/us-central1/tuningJobs/123",
            "state": "JOB_STATE_FAILED",
            "error": { "code": 9, "message": "training data invalid" }
        }"#;

        let job: TuningJob = serde_json::from_str(body).expect("job should deserialize");

        let state = job.state.expect("state present");
        assert!(state.is_terminal_failure(), "FAILED must be terminal failure");
        assert!(!state.is_in_progress());
        assert_eq!(job.error.expect("error present").message, "training data invalid");
    }

    #[test]
    fn in_progress_states_are_polled_not_terminal() {
        for state in [
            JobState::JobStatePending,
            JobState::JobStateRunning,
            JobState::JobStateQueued,
        ] {
            assert!(state.is_in_progress(), "{state:?} should be in-progress");
            assert!(!state.is_terminal_failure());
        }
    }
}
