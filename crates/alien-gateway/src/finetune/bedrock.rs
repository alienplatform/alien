//! AWS Bedrock fine-tuning provider.
//!
//! Submits `CreateModelCustomizationJob`, polls `GetModelCustomizationJob`, and
//! rediscovers the tuned model with `GetCustomModel` (which accepts the model
//! *name*, so the stateless gateway needs no stored ARN). All three are signed
//! with the workload's ambient SigV4 credential for the `bedrock` service against
//! the control-plane host `bedrock.{region}.amazonaws.com` (distinct from the
//! `bedrock-runtime` inference host).

use alien_core::bindings::FinetuneCapability;
use alien_error::{Context, IntoAlienError};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use crate::creds::AmbientCred;
use crate::error::{ErrorData, Result};

use super::{FineTuneProvider, JobHandle, JobState, JobStatus, SubmitRequest};

/// SigV4 service name for the Bedrock control plane.
const BEDROCK_SERVICE: &str = "bedrock";

pub struct BedrockFineTune<'a> {
    region: String,
    cred: &'a AmbientCred,
    client: &'a reqwest::Client,
    /// When set, control-plane requests target this base URL instead of the
    /// region-derived Bedrock host. Lets tests aim the provider at a mock upstream
    /// (mirrors the inference proxy's `upstream_base_override`).
    base_override: Option<String>,
}

impl<'a> BedrockFineTune<'a> {
    pub fn new(region: String, cred: &'a AmbientCred, client: &'a reqwest::Client) -> Self {
        Self { region, cred, client, base_override: None }
    }

    /// Build a provider aimed at an explicit base URL (test upstream).
    pub fn with_base_override(
        region: String,
        cred: &'a AmbientCred,
        client: &'a reqwest::Client,
        base: String,
    ) -> Self {
        Self { region, cred, client, base_override: Some(base) }
    }

    /// Bedrock control-plane host (not the `bedrock-runtime` inference host).
    fn host(&self) -> String {
        self.base_override
            .clone()
            .unwrap_or_else(|| format!("https://bedrock.{}.amazonaws.com", self.region))
    }

    /// Sign `req` with the ambient credential for the `bedrock` service and execute it,
    /// returning the parsed JSON body on 2xx or a contextual error otherwise.
    async fn send(&self, mut req: reqwest::Request) -> Result<serde_json::Value> {
        let url = req.url().to_string();
        self.cred.authorize(&mut req, BEDROCK_SERVICE).await?;
        let resp = self
            .client
            .execute(req)
            .await
            .into_alien_error()
            .context(ErrorData::UpstreamFailed {
                message: format!("Bedrock control-plane request to {url} failed"),
            })?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(alien_error::AlienError::new(ErrorData::UpstreamFailed {
                // Bedrock returns a JSON `{message, __type}`; surface it verbatim.
                message: format!("Bedrock {status} for {url}: {body}"),
            }));
        }
        if body.is_empty() {
            return Ok(serde_json::Value::Null);
        }
        serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::UpstreamFailed {
                message: format!("Bedrock returned non-JSON from {url}: {body}"),
            })
    }

    fn build_json_post(&self, path: &str, body: serde_json::Value) -> Result<reqwest::Request> {
        self.client
            .post(format!("{}{path}", self.host()))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(&body)
            .build()
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("could not build Bedrock POST {path}"),
            })
    }

    fn build_get(&self, path: &str) -> Result<reqwest::Request> {
        self.client
            .get(format!("{}{path}", self.host()))
            .build()
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("could not build Bedrock GET {path}"),
            })
    }
}

/// `CreateModelCustomizationJob` response — only the job ARN is needed.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateJobResponse {
    job_arn: String,
}

/// `GetModelCustomizationJob` response — status plus the produced custom-model ARN.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetJobResponse {
    status: String,
    #[serde(default)]
    output_model_arn: Option<String>,
    #[serde(default)]
    failure_message: Option<String>,
}

/// `GetCustomModel` response — status plus the model ARN for rediscovery.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetCustomModelResponse {
    model_arn: String,
    model_status: String,
}

#[async_trait]
impl FineTuneProvider for BedrockFineTune<'_> {
    async fn submit(&self, request: &SubmitRequest<'_>) -> Result<JobHandle> {
        let cap = request.capability;
        let training_uri = format!("s3://{}/{}", cap.training_bucket, request.training_key());
        let output_uri = format!("s3://{}/alien-finetune-output/{}/", cap.training_bucket, cap.job_name);

        // clientRequestToken makes the submit idempotent so a retried POST /finetune
        // (or a transport retry) doesn't create a second job for the same run.
        let body = json!({
            "jobName": cap.job_name,
            "customModelName": cap.job_name,
            "roleArn": cap.role_arn,
            "baseModelIdentifier": cap.base_model,
            "customizationType": "FINE_TUNING",
            "clientRequestToken": cap.job_name,
            "trainingDataConfig": { "s3Uri": training_uri },
            "outputDataConfig": { "s3Uri": output_uri },
        });

        let req = self.build_json_post("/model-customization-jobs", body)?;
        let value = self.send(req).await?;
        let parsed: CreateJobResponse = serde_json::from_value(value)
            .into_alien_error()
            .context(ErrorData::UpstreamFailed {
                message: "Bedrock CreateModelCustomizationJob returned no jobArn".to_string(),
            })?;

        Ok(JobHandle {
            job_id: parsed.job_arn,
            served_model: cap.served_model_id.clone(),
        })
    }

    async fn status(&self, job_id: &str) -> Result<JobState> {
        // jobIdentifier can be the job ARN; percent-encode it for the path.
        let encoded = urlencoding::encode(job_id);
        let req = self.build_get(&format!("/model-customization-jobs/{encoded}"))?;
        let value = self.send(req).await?;
        let parsed: GetJobResponse = serde_json::from_value(value)
            .into_alien_error()
            .context(ErrorData::UpstreamFailed {
                message: "Bedrock GetModelCustomizationJob returned an unexpected body".to_string(),
            })?;

        Ok(match parsed.status.as_str() {
            "Completed" => JobState {
                status: JobStatus::Succeeded,
                model: parsed.output_model_arn,
                message: None,
            },
            "Failed" | "Stopped" => JobState {
                status: JobStatus::Failed,
                model: None,
                message: parsed.failure_message,
            },
            // InProgress | Stopping | anything else -> still running.
            _ => JobState { status: JobStatus::Running, model: None, message: None },
        })
    }

    async fn resolve_served_model(&self, cap: &FinetuneCapability) -> Result<Option<String>> {
        // GetCustomModel accepts the model NAME, so the deterministic job_name (also the
        // custom-model name) rediscovers the model without any stored ARN. A 404 means
        // the job hasn't produced the model yet.
        let encoded = urlencoding::encode(&cap.job_name);
        let req = self.build_get(&format!("/custom-models/{encoded}"))?;
        match self.send(req).await {
            Ok(value) => {
                let parsed: GetCustomModelResponse = serde_json::from_value(value)
                    .into_alien_error()
                    .context(ErrorData::UpstreamFailed {
                        message: "Bedrock GetCustomModel returned an unexpected body".to_string(),
                    })?;
                Ok(match parsed.model_status.as_str() {
                    "Active" => Some(parsed.model_arn),
                    // Creating / Failed -> not servable yet.
                    _ => None,
                })
            }
            // A ResourceNotFound (model not created yet) is not an error for rediscovery.
            Err(e) if is_not_found(&e) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// True if the error is a Bedrock 404 / ResourceNotFound, which for rediscovery means
/// "the tuned model doesn't exist yet", not a real failure.
fn is_not_found(err: &alien_error::AlienError<ErrorData>) -> bool {
    match &err.error {
        Some(ErrorData::UpstreamFailed { message }) => {
            message.contains("404") || message.contains("ResourceNotFound")
        }
        _ => false,
    }
}
