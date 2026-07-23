//! GCP Vertex AI fine-tuning provider.
//!
//! Submits `POST tuningJobs`, polls `GET tuningJobs/{id}`, and rediscovers the tuned
//! model by listing tuning jobs filtered on the deterministic display name (the
//! stateless gateway keeps no job id). All calls carry the workload's ambient bearer
//! token; the SigV4 service name is unused for bearer auth.
//!
//! Vertex supports only supervised tuning for Gemini (no user-selectable LoRA/DPO),
//! so `method` is not forwarded; the job is always supervised.

use alien_core::bindings::FinetuneCapability;
use alien_error::{Context, IntoAlienError};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use crate::creds::AmbientCred;
use crate::error::{ErrorData, Result};

use super::{FineTuneProvider, JobHandle, JobState, JobStatus, SubmitRequest};

/// Bearer auth ignores the SigV4 service name.
const UNUSED_SERVICE: &str = "aiplatform";

pub struct VertexFineTune<'a> {
    location: String,
    project: String,
    cred: &'a AmbientCred,
    client: &'a reqwest::Client,
    base_override: Option<String>,
}

impl<'a> VertexFineTune<'a> {
    pub fn new(
        location: String,
        project: String,
        cred: &'a AmbientCred,
        client: &'a reqwest::Client,
        base_override: Option<String>,
    ) -> Self {
        Self { location, project, cred, client, base_override }
    }

    /// Regional Vertex host (`global` uses the un-prefixed host), or the test override.
    fn host(&self) -> String {
        if let Some(base) = &self.base_override {
            return base.clone();
        }
        if self.location == "global" {
            "https://aiplatform.googleapis.com".to_string()
        } else {
            format!("https://{}-aiplatform.googleapis.com", self.location)
        }
    }

    fn jobs_path(&self) -> String {
        format!(
            "/v1/projects/{}/locations/{}/tuningJobs",
            self.project, self.location
        )
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
                message: format!("Vertex tuning request to {url} failed"),
            })?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(alien_error::AlienError::new(ErrorData::UpstreamFailed {
                message: format!("Vertex {status} for {url}: {body}"),
            }));
        }
        if body.is_empty() {
            return Ok(serde_json::Value::Null);
        }
        serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::UpstreamFailed {
                message: format!("Vertex returned non-JSON from {url}: {body}"),
            })
    }
}

/// A tuning job resource — `name` is the job id, `state` the lifecycle, `tunedModel`
/// the result once succeeded.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TuningJob {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    tuned_model: Option<TunedModelRef>,
    #[serde(default)]
    error: Option<StatusError>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TunedModelRef {
    /// The deployed tuned-model endpoint the OpenAI-compat path targets.
    #[serde(default)]
    endpoint: Option<String>,
    /// The tuned model resource name (fallback if no endpoint).
    #[serde(default)]
    model: Option<String>,
}

impl TunedModelRef {
    fn upstream_id(&self) -> Option<String> {
        self.endpoint.clone().or_else(|| self.model.clone())
    }
}

#[derive(Deserialize)]
struct StatusError {
    #[serde(default)]
    message: Option<String>,
}

/// List response for rediscovery.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListTuningJobs {
    #[serde(default)]
    tuning_jobs: Vec<TuningJobWithDisplay>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TuningJobWithDisplay {
    #[serde(default)]
    tuned_model_display_name: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    tuned_model: Option<TunedModelRef>,
}

fn map_state(state: Option<&str>, tuned: Option<&TunedModelRef>, err: Option<String>) -> JobState {
    match state {
        Some("JOB_STATE_SUCCEEDED") => JobState {
            status: JobStatus::Succeeded,
            model: tuned.and_then(TunedModelRef::upstream_id),
            message: None,
        },
        Some("JOB_STATE_FAILED")
        | Some("JOB_STATE_CANCELLED")
        | Some("JOB_STATE_EXPIRED")
        | Some("JOB_STATE_PARTIALLY_SUCCEEDED") => {
            JobState { status: JobStatus::Failed, model: None, message: err }
        }
        _ => JobState { status: JobStatus::Running, model: None, message: None },
    }
}

#[async_trait]
impl FineTuneProvider for VertexFineTune<'_> {
    async fn submit(&self, request: &SubmitRequest<'_>) -> Result<JobHandle> {
        let cap = request.capability;
        let training_uri = format!("gs://{}/{}", cap.training_bucket, request.training_key());
        let body = json!({
            "baseModel": cap.base_model,
            "supervisedTuningSpec": { "trainingDatasetUri": training_uri },
            "tunedModelDisplayName": cap.job_name,
        });
        let req = self
            .client
            .post(format!("{}{}", self.host(), self.jobs_path()))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(&body)
            .build()
            .into_alien_error()
            .context(ErrorData::Other { message: "could not build Vertex tuningJobs POST".to_string() })?;
        let value = self.send(req).await?;
        let parsed: TuningJob = serde_json::from_value(value)
            .into_alien_error()
            .context(ErrorData::UpstreamFailed {
                message: "Vertex CreateTuningJob returned no job name".to_string(),
            })?;
        let job_id = parsed.name.ok_or_else(|| {
            alien_error::AlienError::new(ErrorData::UpstreamFailed {
                message: "Vertex tuning job response had no name".to_string(),
            })
        })?;
        Ok(JobHandle { job_id, served_model: cap.served_model_id.clone() })
    }

    async fn status(&self, job_id: &str) -> Result<JobState> {
        // job_id is the full resource name; GET the host + "/v1/{name}".
        let req = self
            .client
            .get(format!("{}/v1/{}", self.host(), job_id))
            .build()
            .into_alien_error()
            .context(ErrorData::Other { message: "could not build Vertex GET tuningJob".to_string() })?;
        let value = self.send(req).await?;
        let parsed: TuningJob = serde_json::from_value(value)
            .into_alien_error()
            .context(ErrorData::UpstreamFailed {
                message: "Vertex GetTuningJob returned an unexpected body".to_string(),
            })?;
        Ok(map_state(
            parsed.state.as_deref(),
            parsed.tuned_model.as_ref(),
            parsed.error.and_then(|e| e.message),
        ))
    }

    async fn resolve_served_model(&self, cap: &FinetuneCapability) -> Result<Option<String>> {
        // No stored job id: list tuning jobs and find the succeeded one whose display
        // name matches the deterministic job_name, then take its tuned-model endpoint.
        let req = self
            .client
            .get(format!("{}{}", self.host(), self.jobs_path()))
            .build()
            .into_alien_error()
            .context(ErrorData::Other { message: "could not build Vertex ListTuningJobs".to_string() })?;
        let value = self.send(req).await?;
        let list: ListTuningJobs = serde_json::from_value(value)
            .into_alien_error()
            .context(ErrorData::UpstreamFailed {
                message: "Vertex ListTuningJobs returned an unexpected body".to_string(),
            })?;
        Ok(list
            .tuning_jobs
            .into_iter()
            .find(|j| {
                j.tuned_model_display_name.as_deref() == Some(&cap.job_name)
                    && j.state.as_deref() == Some("JOB_STATE_SUCCEEDED")
            })
            .and_then(|j| j.tuned_model.and_then(|t| t.upstream_id())))
    }
}
