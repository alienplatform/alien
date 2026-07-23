//! Runtime fine-tuning control plane for the AI gateway.
//!
//! Inference is a stateless proxy (see [`crate::router`]); fine-tuning adds a small
//! *control-plane* surface on the same gateway, reusing the same ambient credential.
//! The app triggers a job at runtime:
//!
//!   - `POST /<binding>/v1/finetune`       → submit a job, returns `{ jobId, servedModel }`
//!   - `GET  /<binding>/v1/finetune/<job>` → poll it, returns `{ status, model? }`
//!
//! The gateway is per-process and stateless: a job started by one worker completes on
//! the cloud's side hours later, possibly after that worker is gone. So nothing about
//! job state is persisted. Instead the tuned model's cloud name is **deterministic**
//! from the binding's [`FinetuneCapability`], and the router rediscovers the completed
//! model by that name on the next inference request (see [`FineTuneProvider::resolve_served_model`]).
//!
//! Each cloud implements [`FineTuneProvider`]; the router dispatches on the route's cloud.

use alien_core::bindings::FinetuneCapability;
use alien_error::AlienError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::creds::AmbientCred;
use crate::error::{ErrorData, Result};

pub mod bedrock;
pub mod foundry;
pub mod vertex;

/// The lifecycle status of a fine-tuning job, normalized across providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    /// The job is queued or running.
    Running,
    /// The job completed and the tuned model is ready to serve.
    Succeeded,
    /// The job failed; `message` on [`JobState`] carries the reason.
    Failed,
}

/// A submitted job's identity: the provider job id the app polls with.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobHandle {
    /// Provider job id (a Bedrock job ARN, Vertex tuning-job name, or Foundry job id).
    pub job_id: String,
    /// The public model id the tuned model will be served under once complete.
    pub served_model: String,
}

/// The observed state of a job, plus the tuned upstream id once it succeeds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobState {
    pub status: JobStatus,
    /// The tuned model's provider-native upstream id (custom-model ARN / tuned
    /// endpoint / deployment name), set only when `status == Succeeded`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// A human-readable failure reason, set only when `status == Failed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Everything a provider needs to submit one job: the declared capability plus an
/// optional per-request override of the training key (so an app can retrain on a
/// freshly-uploaded file without redeploying).
pub struct SubmitRequest<'a> {
    pub capability: &'a FinetuneCapability,
    /// Override for `capability.training_key`; `None` uses the declared key.
    pub training_key: Option<String>,
}

impl SubmitRequest<'_> {
    /// The training key this request uses (override or the capability's default).
    pub fn training_key(&self) -> &str {
        self.training_key
            .as_deref()
            .unwrap_or(&self.capability.training_key)
    }
}

/// A cloud's fine-tuning control plane. Implementations issue signed control-plane
/// calls via the ambient credential and translate provider responses into the
/// normalized [`JobStatus`]/[`JobState`] above.
#[async_trait]
pub trait FineTuneProvider: Send + Sync {
    /// Submit a tuning job. Returns the job id to poll and the public served model id.
    async fn submit(&self, request: &SubmitRequest<'_>) -> Result<JobHandle>;

    /// Poll a previously-submitted job by its provider job id.
    async fn status(&self, job_id: &str) -> Result<JobState>;

    /// Rediscover the tuned model by its deterministic name, without a job id.
    /// Returns the tuned upstream id if the model exists and is ready to serve,
    /// `None` if it doesn't exist yet or is still being created. This is how a
    /// stateless gateway routes `served_model_id` after the worker that ran the
    /// job is gone.
    async fn resolve_served_model(&self, capability: &FinetuneCapability) -> Result<Option<String>>;
}

/// Build a per-request [`FineTuneProvider`] for a route's cloud. The provider borrows
/// the route's ambient credential and the shared HTTP client, so it is cheap to build
/// on each control-plane request. `None` for clouds/bindings the gateway does not
/// fine-tune (currently only Bedrock is wired; Vertex/Foundry follow the same trait).
/// The route location fields a provider needs to build control-plane URLs, plus the
/// optional test base-URL override. Borrowed from the router's `GatewayRoute`.
pub struct ProviderCtx<'a> {
    pub cloud: alien_core::Platform,
    /// AWS region / Vertex location.
    pub region: Option<&'a str>,
    /// GCP project id.
    pub project: Option<&'a str>,
    /// Azure Foundry account endpoint.
    pub azure_endpoint: Option<&'a str>,
    pub cred: &'a AmbientCred,
    pub client: &'a reqwest::Client,
    /// Test-only upstream base override (mirrors the inference proxy).
    pub base_override: Option<&'a str>,
}

pub fn provider_for<'a>(ctx: &ProviderCtx<'a>) -> Option<Box<dyn FineTuneProvider + 'a>> {
    match ctx.cloud {
        alien_core::Platform::Aws => ctx.region.map(|region| {
            let provider = match ctx.base_override {
                Some(base) => bedrock::BedrockFineTune::with_base_override(
                    region.to_string(),
                    ctx.cred,
                    ctx.client,
                    base.to_string(),
                ),
                None => bedrock::BedrockFineTune::new(region.to_string(), ctx.cred, ctx.client),
            };
            Box::new(provider) as Box<dyn FineTuneProvider + 'a>
        }),
        alien_core::Platform::Gcp => match (ctx.region, ctx.project) {
            (Some(location), Some(project)) => Some(Box::new(vertex::VertexFineTune::new(
                location.to_string(),
                project.to_string(),
                ctx.cred,
                ctx.client,
                ctx.base_override.map(str::to_string),
            )) as Box<dyn FineTuneProvider + 'a>),
            _ => None,
        },
        alien_core::Platform::Azure => ctx.azure_endpoint.map(|endpoint| {
            Box::new(foundry::FoundryFineTune::new(
                endpoint.to_string(),
                ctx.cred,
                ctx.client,
                ctx.base_override.map(str::to_string),
            )) as Box<dyn FineTuneProvider + 'a>
        }),
        _ => None,
    }
}

/// Map a "no fine-tuning capability on this binding" condition to a gateway error.
pub(crate) fn no_capability(binding: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::InvalidRequest {
        message: format!("binding '{binding}' has no fine-tuning capability declared"),
    })
}
