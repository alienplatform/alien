use crate::error::{ErrorData, Result};
use crate::traits::{Binding, Worker, WorkerInvokeRequest, WorkerInvokeResponse};
use alien_core::bindings::LambdaWorkerBinding;
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use aws_sdk_lambda::{primitives::Blob, types::InvocationType};
use base64::Engine;
use std::{collections::BTreeMap, fmt::Debug, sync::Arc};

/// Response data returned from invoking a Lambda worker.
#[derive(Debug, Clone)]
pub struct LambdaInvokeOutput {
    /// Optional Lambda function error marker.
    pub function_error: Option<String>,
    /// Raw Lambda response payload.
    pub payload: Vec<u8>,
}

/// Minimal Lambda operations required by the worker binding.
#[async_trait]
pub trait LambdaWorkerClient: Debug + Send + Sync {
    /// Invoke a Lambda function and return its raw payload.
    async fn invoke_worker(
        &self,
        function_name: &str,
        payload: Vec<u8>,
    ) -> Result<LambdaInvokeOutput>;

    /// Fetch a Lambda function URL, if one is configured.
    async fn get_worker_url(&self, function_name: &str) -> Result<String>;
}

#[async_trait]
impl LambdaWorkerClient for aws_sdk_lambda::Client {
    async fn invoke_worker(
        &self,
        function_name: &str,
        payload: Vec<u8>,
    ) -> Result<LambdaInvokeOutput> {
        let response = self
            .invoke()
            .function_name(function_name)
            .invocation_type(InvocationType::RequestResponse)
            .payload(Blob::new(payload))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("Failed to invoke Lambda worker '{}'", function_name),
            })?;

        Ok(LambdaInvokeOutput {
            function_error: response.function_error().map(ToString::to_string),
            payload: response
                .payload()
                .map(|payload| payload.as_ref().to_vec())
                .unwrap_or_default(),
        })
    }

    async fn get_worker_url(&self, function_name: &str) -> Result<String> {
        let response = self
            .get_function_url_config()
            .function_name(function_name)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("Failed to get Lambda worker URL for '{}'", function_name),
            })?;

        Ok(response.function_url().to_string())
    }
}

/// AWS Lambda worker binding implementation.
#[derive(Debug)]
pub struct LambdaWorker {
    client: Arc<dyn LambdaWorkerClient>,
    binding: LambdaWorkerBinding,
}

impl LambdaWorker {
    pub fn new(client: Arc<dyn LambdaWorkerClient>, binding: LambdaWorkerBinding) -> Self {
        Self { client, binding }
    }

    /// Get the worker name from the binding, resolving template expressions if needed.
    fn get_worker_name(&self) -> Result<String> {
        self.binding
            .worker_name
            .clone()
            .into_value("worker", "worker_name")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "worker".to_string(),
                reason: "Failed to resolve worker_name from binding".to_string(),
            })
    }
}

impl Binding for LambdaWorker {}

#[async_trait]
impl Worker for LambdaWorker {
    async fn invoke(&self, request: WorkerInvokeRequest) -> Result<WorkerInvokeResponse> {
        let worker_name = self.get_worker_name()?;

        let payload = serde_json::json!({
            "httpMethod": request.method.to_uppercase(),
            "path": request.path,
            "headers": request.headers,
            "body": base64::engine::general_purpose::STANDARD.encode(&request.body),
            "isBase64Encoded": true
        });

        let payload_bytes =
            serde_json::to_vec(&payload)
                .into_alien_error()
                .context(ErrorData::Other {
                    message: "Failed to serialize Lambda invoke payload".to_string(),
                })?;

        let target_worker = if request.target_worker.is_empty() {
            worker_name
        } else {
            request.target_worker.clone()
        };

        let response = self
            .client
            .invoke_worker(&target_worker, payload_bytes)
            .await
            .context(ErrorData::Other {
                message: format!("Failed to invoke Lambda worker '{}'", target_worker),
            })?;

        if let Some(function_error) = response.function_error {
            return Err(AlienError::new(ErrorData::Other {
                message: format!(
                    "Lambda worker '{}' returned error: {}",
                    target_worker, function_error
                ),
            }));
        }

        let lambda_response: serde_json::Value = serde_json::from_slice(&response.payload)
            .into_alien_error()
            .context(ErrorData::Other {
                message: "Failed to parse Lambda response payload".to_string(),
            })?;

        let status = lambda_response
            .get("statusCode")
            .and_then(|status| status.as_u64())
            .unwrap_or(200) as u16;

        let headers = lambda_response
            .get("headers")
            .and_then(|headers| headers.as_object())
            .map(|headers| {
                headers
                    .iter()
                    .map(|(key, value)| (key.clone(), value.as_str().unwrap_or("").to_string()))
                    .collect::<BTreeMap<String, String>>()
            })
            .unwrap_or_default();

        let body = if let Some(body) = lambda_response.get("body").and_then(|body| body.as_str()) {
            let is_base64 = lambda_response
                .get("isBase64Encoded")
                .and_then(|is_base64| is_base64.as_bool())
                .unwrap_or(false);

            if is_base64 {
                base64::engine::general_purpose::STANDARD
                    .decode(body)
                    .into_alien_error()
                    .context(ErrorData::Other {
                        message: "Failed to decode base64 response body".to_string(),
                    })?
            } else {
                body.as_bytes().to_vec()
            }
        } else {
            Vec::new()
        };

        Ok(WorkerInvokeResponse {
            status,
            headers,
            body,
        })
    }

    async fn get_worker_url(&self) -> Result<Option<String>> {
        if let Some(url_binding) = &self.binding.url {
            let url = url_binding.clone().into_value("worker", "url").context(
                ErrorData::BindingConfigInvalid {
                    binding_name: "worker".to_string(),
                    reason: "Failed to resolve url from binding".to_string(),
                },
            )?;
            return Ok(Some(url));
        }

        let worker_name = self.get_worker_name()?;
        match self.client.get_worker_url(&worker_name).await {
            Ok(url) => Ok(Some(url)),
            Err(_) => Ok(None),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
