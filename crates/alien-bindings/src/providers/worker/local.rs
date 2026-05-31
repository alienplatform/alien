use crate::error::{ErrorData, Result};
use crate::traits::{Binding, Worker, WorkerInvokeRequest, WorkerInvokeResponse};
use alien_core::bindings::LocalWorkerBinding;
use alien_error::{Context, IntoAlienError};
use async_trait::async_trait;
use std::collections::BTreeMap;

/// Local worker binding implementation for development and testing.
///
/// This provides a simple HTTP client for calling local workers
/// running on HTTP endpoints (e.g., during local development).
#[derive(Debug)]
pub struct LocalWorker {
    binding: LocalWorkerBinding,
}

impl LocalWorker {
    /// Create a new local worker binding.
    pub fn new(binding: LocalWorkerBinding) -> Self {
        Self { binding }
    }

    /// Get the worker URL from the binding, resolving template expressions if needed
    fn get_worker_url(&self) -> Result<String> {
        self.binding
            .worker_url
            .clone()
            .into_value("worker", "worker_url")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "worker".to_string(),
                reason: "Failed to resolve worker_url from binding".to_string(),
            })
    }
}

impl Binding for LocalWorker {}

#[async_trait]
impl Worker for LocalWorker {
    async fn invoke(&self, request: WorkerInvokeRequest) -> Result<WorkerInvokeResponse> {
        let worker_url = self.get_worker_url()?;

        // Build the target URL
        let target_url = if !request.target_worker.is_empty() {
            format!(
                "{}/{}",
                worker_url.trim_end_matches('/'),
                request.target_worker
            )
        } else {
            worker_url
        };

        // Add path if provided
        let full_url = if !request.path.is_empty() {
            format!(
                "{}/{}",
                target_url.trim_end_matches('/'),
                request.path.trim_start_matches('/')
            )
        } else {
            target_url
        };

        // Create HTTP client
        let client = reqwest::Client::new();

        // Build HTTP request
        let mut http_request = client.request(
            reqwest::Method::from_bytes(request.method.as_bytes())
                .into_alien_error()
                .context(ErrorData::BindingConfigInvalid {
                    binding_name: "worker".to_string(),
                    reason: format!("Invalid HTTP method: {}", request.method),
                })?,
            &full_url,
        );

        // Add headers
        for (key, value) in request.headers {
            http_request = http_request.header(key, value);
        }

        // Add body if provided
        if !request.body.is_empty() {
            http_request = http_request.body(request.body);
        }

        // Set timeout if provided
        if let Some(timeout) = request.timeout {
            http_request = http_request.timeout(timeout);
        }

        // Send request
        let response = http_request.send().await.into_alien_error().context(
            ErrorData::CloudPlatformError {
                message: format!("Failed to invoke local worker at: {}", full_url),
                resource_id: None,
            },
        )?;

        // Extract response
        let status = response.status().as_u16();
        let mut headers = BTreeMap::new();

        for (key, value) in response.headers() {
            if let Ok(value_str) = value.to_str() {
                headers.insert(key.to_string(), value_str.to_string());
            }
        }

        let body = response
            .bytes()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Failed to read response body from local worker".to_string(),
                resource_id: None,
            })?
            .to_vec();

        Ok(WorkerInvokeResponse {
            status,
            headers,
            body,
        })
    }

    async fn get_worker_url(&self) -> Result<Option<String>> {
        Ok(Some(self.get_worker_url()?))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
