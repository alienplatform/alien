use crate::error::{ErrorData, Result};
use crate::traits::{Binding, Function, FunctionInvokeRequest, FunctionInvokeResponse};
use alien_azure_clients::container_apps::{AzureContainerAppsClient, ContainerAppsApi};
use alien_azure_clients::{AzureClientConfig, AzureTokenCache};
use alien_core::bindings::ContainerAppFunctionBinding;
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use reqwest::Client;
use std::collections::BTreeMap;

/// Azure Container Apps function binding implementation
#[derive(Debug)]
pub struct ContainerAppFunction {
    client: Client,
    container_apps_client: AzureContainerAppsClient,
    binding: ContainerAppFunctionBinding,
}

impl ContainerAppFunction {
    pub fn new(
        client: Client,
        config: AzureClientConfig,
        binding: ContainerAppFunctionBinding,
    ) -> Self {
        let container_apps_client = AzureContainerAppsClient::new(client.clone(), AzureTokenCache::new(config));
        Self {
            client,
            container_apps_client,
            binding,
        }
    }

    /// Get the private URL from the binding, resolving template expressions if needed
    fn get_private_url(&self) -> Result<String> {
        self.binding
            .private_url
            .clone()
            .into_value("function", "private_url")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "function".to_string(),
                reason: "Failed to resolve private_url from binding".to_string(),
            })
    }

    /// Get the public URL from the binding if available
    pub async fn get_function_url(&self) -> Result<Option<String>> {
        // First check if we have it in the binding
        if let Some(url_binding) = &self.binding.public_url {
            let url = url_binding
                .clone()
                .into_value("function", "public_url")
                .context(ErrorData::BindingConfigInvalid {
                    binding_name: "function".to_string(),
                    reason: "Failed to resolve public_url from binding".to_string(),
                })?;
            return Ok(Some(url));
        }

        // If not in binding, try to fetch it from Azure
        let resource_group_name = self
            .binding
            .resource_group_name
            .clone()
            .into_value("function", "resource_group_name")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "function".to_string(),
                reason: "Failed to resolve resource_group_name from binding".to_string(),
            })?;

        let container_app_name = self
            .binding
            .container_app_name
            .clone()
            .into_value("function", "container_app_name")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "function".to_string(),
                reason: "Failed to resolve container_app_name from binding".to_string(),
            })?;

        match self
            .container_apps_client
            .get_container_app(&resource_group_name, &container_app_name)
            .await
        {
            Ok(container_app) => {
                // Check if there's a public ingress configuration
                if let Some(configuration) = &container_app.properties {
                    if let Some(ingress) = &configuration
                        .configuration
                        .as_ref()
                        .and_then(|c| c.ingress.as_ref())
                    {
                        if ingress.external {
                            // Return the FQDN if available
                            return Ok(ingress
                                .fqdn
                                .clone()
                                .map(|fqdn| format!("https://{}", fqdn)));
                        }
                    }
                }
                Ok(None)
            }
            Err(_) => Ok(None), // Container App doesn't exist or no public URL
        }
    }

    /// Resolve the target URL for invocation
    async fn resolve_target_url(&self, target_function: &str) -> Result<String> {
        if !target_function.is_empty() {
            // Check if target_function looks like a URL (starts with http)
            if target_function.starts_with("http://") || target_function.starts_with("https://") {
                // Use the provided target function as URL
                Ok(target_function.to_string())
            } else {
                // target_function is likely a path/identifier, use binding URL
                self.get_private_url()
            }
        } else {
            // Use the private URL from binding
            self.get_private_url()
        }
    }
}

impl Binding for ContainerAppFunction {}

#[async_trait]
impl Function for ContainerAppFunction {
    async fn invoke(&self, request: FunctionInvokeRequest) -> Result<FunctionInvokeResponse> {
        let target_url = self.resolve_target_url(&request.target_function).await?;

        // Construct the full URL with path
        let url = if request.path.starts_with('/') {
            format!("{}{}", target_url.trim_end_matches('/'), request.path)
        } else {
            format!("{}/{}", target_url.trim_end_matches('/'), request.path)
        };

        // Build the HTTP request
        let method = match request.method.to_uppercase().as_str() {
            "GET" => reqwest::Method::GET,
            "POST" => reqwest::Method::POST,
            "PUT" => reqwest::Method::PUT,
            "DELETE" => reqwest::Method::DELETE,
            "PATCH" => reqwest::Method::PATCH,
            "HEAD" => reqwest::Method::HEAD,
            "OPTIONS" => reqwest::Method::OPTIONS,
            _ => {
                return Err(AlienError::new(ErrorData::InvalidInput {
                    operation_context: "Function invocation".to_string(),
                    details: format!("Unsupported HTTP method: {}", request.method),
                    field_name: Some("method".to_string()),
                }));
            }
        };

        let mut req_builder = self.client.request(method, &url);

        // Add headers
        for (key, value) in &request.headers {
            req_builder = req_builder.header(key, value);
        }

        // Add body if present
        if !request.body.is_empty() {
            req_builder = req_builder.body(request.body.clone());
        }

        // Set timeout if specified
        if let Some(timeout) = request.timeout {
            req_builder = req_builder.timeout(timeout);
        }

        // Send the request
        let response =
            req_builder
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    url: url.clone(),
                    method: request.method.clone(),
                })?;

        // Extract response components
        let status = response.status().as_u16();

        let headers = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect::<BTreeMap<String, String>>();

        let body = response
            .bytes()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                url: url.clone(),
                method: "READ_BODY".to_string(),
            })?
            .to_vec();

        Ok(FunctionInvokeResponse {
            status,
            headers,
            body,
        })
    }

    async fn get_function_url(&self) -> Result<Option<String>> {
        // First check if we have it in the binding
        if let Some(url_binding) = &self.binding.public_url {
            let url = url_binding
                .clone()
                .into_value("function", "public_url")
                .context(ErrorData::BindingConfigInvalid {
                    binding_name: "function".to_string(),
                    reason: "Failed to resolve public_url from binding".to_string(),
                })?;
            return Ok(Some(url));
        }

        // If not in binding, try to fetch it from Azure
        let resource_group_name = self
            .binding
            .resource_group_name
            .clone()
            .into_value("function", "resource_group_name")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "function".to_string(),
                reason: "Failed to resolve resource_group_name from binding".to_string(),
            })?;

        let container_app_name = self
            .binding
            .container_app_name
            .clone()
            .into_value("function", "container_app_name")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "function".to_string(),
                reason: "Failed to resolve container_app_name from binding".to_string(),
            })?;

        match self
            .container_apps_client
            .get_container_app(&resource_group_name, &container_app_name)
            .await
        {
            Ok(container_app) => {
                // Extract the URL from the container app configuration
                if let Some(properties) = &container_app.properties {
                    if let Some(configuration) = &properties.configuration {
                        if let Some(ingress) = &configuration.ingress {
                            if let Some(fqdn) = &ingress.fqdn {
                                return Ok(Some(format!("https://{}", fqdn)));
                            }
                        }
                    }
                }
                Ok(None)
            }
            Err(_) => Ok(None), // Container app doesn't exist or no public URL
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
