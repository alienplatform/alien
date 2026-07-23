//! Azure AI Foundry fine-tuning provider (data plane).
//!
//! Submits `POST {endpoint}/openai/fine_tuning/jobs`, polls `GET .../jobs/{id}`, and
//! rediscovers the tuned model as the deployment named after `served_model_id`. All
//! calls carry the workload's ambient bearer token for the cognitive-services data
//! plane; the SigV4 service name is unused.
//!
//! Note: Foundry can import training data from Blob, but that requires the storage
//! account to allow public network access — a gotcha documented in the example README.

use alien_core::bindings::FinetuneCapability;
use alien_error::{Context, IntoAlienError};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use crate::creds::AmbientCred;
use crate::error::{ErrorData, Result};

use super::{FineTuneProvider, JobHandle, JobState, JobStatus, SubmitRequest};

/// Bearer auth ignores the SigV4 service name.
const UNUSED_SERVICE: &str = "cognitiveservices";
/// Data-plane API version for fine-tuning + deployments.
const API_VERSION: &str = "2024-10-21";

pub struct FoundryFineTune<'a> {
    endpoint: String,
    cred: &'a AmbientCred,
    client: &'a reqwest::Client,
    base_override: Option<String>,
}

impl<'a> FoundryFineTune<'a> {
    pub fn new(
        endpoint: String,
        cred: &'a AmbientCred,
        client: &'a reqwest::Client,
        base_override: Option<String>,
    ) -> Self {
        Self { endpoint, cred, client, base_override }
    }

    /// The account data-plane base (or the test override), without a trailing slash.
    fn base(&self) -> String {
        self.base_override
            .clone()
            .unwrap_or_else(|| self.endpoint.clone())
            .trim_end_matches('/')
            .to_string()
    }

    async fn send(&self, mut req: reqwest::Request) -> Result<serde_json::Value> {
        let url = req.url().to_string();
        self.cred.authorize(&mut req, UNUSED_SERVICE).await?;
        let resp = self
            .client
            .execute(req)
            .await
            .into_alien_error()
            .context(ErrorData::UpstreamFailed {
                message: format!("Foundry fine-tuning request to {url} failed"),
            })?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(alien_error::AlienError::new(ErrorData::UpstreamFailed {
                message: format!("Foundry {status} for {url}: {body}"),
            }));
        }
        if body.is_empty() {
            return Ok(serde_json::Value::Null);
        }
        serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::UpstreamFailed {
                message: format!("Foundry returned non-JSON from {url}: {body}"),
            })
    }
}

/// A fine-tuning job — `id` to poll, `status`, and `fine_tuned_model` once done.
#[derive(Deserialize)]
struct FineTuningJob {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    fine_tuned_model: Option<String>,
    #[serde(default)]
    error: Option<JobError>,
}

#[derive(Deserialize)]
struct JobError {
    #[serde(default)]
    message: Option<String>,
}

/// A deployment resource — `provisioningState` tells us if the tuned model is servable.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Deployment {
    #[serde(default)]
    provisioning_state: Option<String>,
}

fn map_status(status: Option<&str>, model: Option<String>, err: Option<String>) -> JobState {
    match status {
        Some("succeeded") => JobState {
            status: JobStatus::Succeeded,
            model,
            message: None,
        },
        Some("failed") | Some("cancelled") => {
            JobState { status: JobStatus::Failed, model: None, message: err }
        }
        _ => JobState { status: JobStatus::Running, model: None, message: None },
    }
}

#[async_trait]
impl FineTuneProvider for FoundryFineTune<'_> {
    async fn submit(&self, request: &SubmitRequest<'_>) -> Result<JobHandle> {
        let cap = request.capability;
        // The training file is a Blob URL under the training bucket/container. Foundry
        // requires the storage account to allow public network access for Blob import.
        let training_file = format!(
            "https://{}.blob.core.windows.net/{}",
            cap.training_bucket,
            request.training_key()
        );
        let body = json!({
            "model": cap.base_model,
            "training_file": training_file,
            "suffix": cap.served_model_id,
        });
        let url = format!("{}/openai/fine_tuning/jobs?api-version={API_VERSION}", self.base());
        let req = self
            .client
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(&body)
            .build()
            .into_alien_error()
            .context(ErrorData::Other { message: "could not build Foundry fine_tuning POST".to_string() })?;
        let value = self.send(req).await?;
        let parsed: FineTuningJob = serde_json::from_value(value)
            .into_alien_error()
            .context(ErrorData::UpstreamFailed {
                message: "Foundry CreateFineTuningJob returned no id".to_string(),
            })?;
        let job_id = parsed.id.ok_or_else(|| {
            alien_error::AlienError::new(ErrorData::UpstreamFailed {
                message: "Foundry fine-tuning job response had no id".to_string(),
            })
        })?;
        Ok(JobHandle { job_id, served_model: cap.served_model_id.clone() })
    }

    async fn status(&self, job_id: &str) -> Result<JobState> {
        let encoded = urlencoding::encode(job_id);
        let url = format!(
            "{}/openai/fine_tuning/jobs/{encoded}?api-version={API_VERSION}",
            self.base()
        );
        let req = self
            .client
            .get(url)
            .build()
            .into_alien_error()
            .context(ErrorData::Other { message: "could not build Foundry GET job".to_string() })?;
        let value = self.send(req).await?;
        let parsed: FineTuningJob = serde_json::from_value(value)
            .into_alien_error()
            .context(ErrorData::UpstreamFailed {
                message: "Foundry GetFineTuningJob returned an unexpected body".to_string(),
            })?;
        Ok(map_status(
            parsed.status.as_deref(),
            parsed.fine_tuned_model,
            parsed.error.and_then(|e| e.message),
        ))
    }

    async fn resolve_served_model(&self, cap: &FinetuneCapability) -> Result<Option<String>> {
        // The tuned model is served as a deployment named `served_model_id`; if it
        // exists and is Succeeded, that deployment name is the OpenAI-path `model`.
        let encoded = urlencoding::encode(&cap.served_model_id);
        let url = format!(
            "{}/openai/deployments/{encoded}?api-version={API_VERSION}",
            self.base()
        );
        let req = self
            .client
            .get(url)
            .build()
            .into_alien_error()
            .context(ErrorData::Other { message: "could not build Foundry GET deployment".to_string() })?;
        match self.send(req).await {
            Ok(value) => {
                let dep: Deployment = serde_json::from_value(value)
                    .into_alien_error()
                    .context(ErrorData::UpstreamFailed {
                        message: "Foundry GetDeployment returned an unexpected body".to_string(),
                    })?;
                Ok(match dep.provisioning_state.as_deref() {
                    Some("Succeeded") => Some(cap.served_model_id.clone()),
                    _ => None,
                })
            }
            Err(e) if is_not_found(&e) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

fn is_not_found(err: &alien_error::AlienError<ErrorData>) -> bool {
    matches!(
        &err.error,
        Some(ErrorData::UpstreamFailed { message }) if message.contains("404")
    )
}
