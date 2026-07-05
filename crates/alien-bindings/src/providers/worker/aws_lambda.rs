use crate::error::{binding_env_var, ErrorData, Result};
use crate::traits::{Binding, Worker, WorkerInvokeRequest, WorkerInvokeResponse};
use alien_aws_clients::lambda::{InvocationType, InvokeRequest, LambdaApi, LambdaClient};
use alien_aws_clients::AwsCredentialProvider;
use alien_core::bindings::LambdaWorkerBinding;
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use base64::Engine;
use reqwest::Client;
use std::collections::BTreeMap;

/// AWS Lambda worker binding implementation
#[derive(Debug)]
pub struct LambdaWorker {
    client: LambdaClient,
    binding: LambdaWorkerBinding,
}

impl LambdaWorker {
    pub fn new(
        client: Client,
        credentials: AwsCredentialProvider,
        binding: LambdaWorkerBinding,
    ) -> Self {
        let lambda_client = LambdaClient::new(client, credentials);
        Self {
            client: lambda_client,
            binding,
        }
    }

    /// Get the worker name from the binding, resolving template expressions if needed
    fn get_worker_name(&self) -> Result<String> {
        self.binding
            .worker_name
            .clone()
            .into_value("worker", "worker_name")
            .context(ErrorData::BindingConfigInvalid {
                env_var: binding_env_var("worker"),
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

        // Create the invoke request payload
        // For Lambda, we need to construct an HTTP-like payload that the runtime can understand
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

        // Use the target_worker if provided, otherwise use the bound worker
        let target_worker = if !request.target_worker.is_empty() {
            request.target_worker.clone()
        } else {
            worker_name
        };

        let invoke_request = InvokeRequest::builder()
            .function_name(target_worker.clone())
            .invocation_type(InvocationType::RequestResponse)
            .payload(payload_bytes)
            .build();

        let response = self
            .client
            .invoke(invoke_request)
            .await
            .context(ErrorData::Other {
                message: format!("Failed to invoke Lambda worker '{}'", target_worker),
            })?;

        // Check for worker error
        if let Some(function_error) = response.function_error {
            return Err(AlienError::new(ErrorData::Other {
                message: format!(
                    "Lambda worker '{}' returned error: {}",
                    target_worker, function_error
                ),
            }));
        }

        // Parse the response payload
        let lambda_response: serde_json::Value = serde_json::from_slice(&response.payload)
            .into_alien_error()
            .context(ErrorData::Other {
                message: "Failed to parse Lambda response payload".to_string(),
            })?;

        // Extract HTTP response components
        let status = lambda_response
            .get("statusCode")
            .and_then(|s| s.as_u64())
            .unwrap_or(200) as u16;

        let headers = lambda_response
            .get("headers")
            .and_then(|h| h.as_object())
            .map(|obj| {
                obj.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                    .collect::<BTreeMap<String, String>>()
            })
            .unwrap_or_default();

        let body = if let Some(body_str) = lambda_response.get("body").and_then(|b| b.as_str()) {
            // Check if body is base64 encoded
            let is_base64 = lambda_response
                .get("isBase64Encoded")
                .and_then(|b| b.as_bool())
                .unwrap_or(false);

            if is_base64 {
                base64::engine::general_purpose::STANDARD
                    .decode(body_str)
                    .into_alien_error()
                    .context(ErrorData::Other {
                        message: "Failed to decode base64 response body".to_string(),
                    })?
            } else {
                body_str.as_bytes().to_vec()
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
        // First check if we have it in the binding
        if let Some(url_binding) = &self.binding.url {
            let url = url_binding.clone().into_value("worker", "url").context(
                ErrorData::BindingConfigInvalid {
                    env_var: binding_env_var("worker"),
                    binding_name: "worker".to_string(),
                    reason: "Failed to resolve url from binding".to_string(),
                },
            )?;
            return Ok(Some(url));
        }

        // If not in binding, try to fetch it from AWS
        let worker_name = self.get_worker_name()?;
        match self
            .client
            .get_function_url_config(&worker_name, None)
            .await
        {
            Ok(url_config) => Ok(Some(url_config.function_url)),
            Err(_) => Ok(None), // Worker URL doesn't exist
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
