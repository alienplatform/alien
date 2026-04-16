use crate::error::{ErrorData, Result};
use crate::traits::{Binding, Function, FunctionInvokeRequest, FunctionInvokeResponse};
use alien_core::bindings::LocalFunctionBinding;
use alien_error::{Context, IntoAlienError};
use async_trait::async_trait;
use std::collections::BTreeMap;

/// Local function binding implementation for development and testing.
///
/// This provides a simple HTTP client for calling local functions
/// running on HTTP endpoints (e.g., during local development).
#[derive(Debug)]
pub struct LocalFunction {
    binding: LocalFunctionBinding,
}

impl LocalFunction {
    /// Create a new local function binding.
    pub fn new(binding: LocalFunctionBinding) -> Self {
        Self { binding }
    }

    /// Get the function URL from the binding, resolving template expressions if needed
    fn get_function_url(&self) -> Result<String> {
        self.binding
            .function_url
            .clone()
            .into_value("function", "function_url")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "function".to_string(),
                reason: "Failed to resolve function_url from binding".to_string(),
            })
    }
}

impl Binding for LocalFunction {}

#[async_trait]
impl Function for LocalFunction {
    async fn invoke(&self, request: FunctionInvokeRequest) -> Result<FunctionInvokeResponse> {
        let function_url = self.get_function_url()?;

        // Build the target URL
        let target_url = if !request.target_function.is_empty() {
            format!(
                "{}/{}",
                function_url.trim_end_matches('/'),
                request.target_function
            )
        } else {
            function_url
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
                    binding_name: "function".to_string(),
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
                message: format!("Failed to invoke local function at: {}", full_url),
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
                message: "Failed to read response body from local function".to_string(),
                resource_id: None,
            })?
            .to_vec();

        Ok(FunctionInvokeResponse {
            status,
            headers,
            body,
        })
    }

    async fn get_function_url(&self) -> Result<Option<String>> {
        Ok(Some(self.get_function_url()?))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
