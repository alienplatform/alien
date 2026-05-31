use crate::{
    error::{ErrorData, Result},
    traits::{Binding, Worker, WorkerInvokeRequest, WorkerInvokeResponse},
};
use alien_core::bindings::KubernetesWorkerBinding;
use alien_error::{Context, IntoAlienError};
use async_trait::async_trait;
use std::collections::BTreeMap;

/// Kubernetes Worker implementation that calls workers via internal Kubernetes Services
#[derive(Debug)]
pub struct KubernetesWorker {
    namespace: String,
    service_name: String,
    service_port: u16,
    public_url: Option<String>,
    http_client: reqwest::Client,
}

impl KubernetesWorker {
    pub fn new(binding_name: String, binding: KubernetesWorkerBinding) -> Result<Self> {
        let namespace = binding
            .namespace
            .into_value(&binding_name, "namespace")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract namespace from Kubernetes worker binding".to_string(),
            })?;

        let service_name = binding
            .service_name
            .into_value(&binding_name, "service_name")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract service_name from Kubernetes worker binding".to_string(),
            })?;

        let service_port = binding
            .service_port
            .into_value(&binding_name, "service_port")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract service_port from Kubernetes worker binding".to_string(),
            })?;

        let public_url = binding
            .public_url
            .map(|v| v.into_value(&binding_name, "public_url"))
            .transpose()
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract public_url from Kubernetes worker binding".to_string(),
            })?;

        Ok(Self {
            namespace,
            service_name,
            service_port,
            public_url,
            http_client: reqwest::Client::new(),
        })
    }

    /// Constructs the internal service URL for the worker
    fn get_internal_service_url(&self) -> String {
        format!(
            "http://{}.{}.svc.cluster.local:{}",
            self.service_name, self.namespace, self.service_port
        )
    }
}

#[async_trait]
impl Binding for KubernetesWorker {}

#[async_trait]
impl Worker for KubernetesWorker {
    async fn invoke(&self, request: WorkerInvokeRequest) -> Result<WorkerInvokeResponse> {
        // Construct the full URL
        let base_url = self.get_internal_service_url();
        let url = format!("{}{}", base_url, request.path);

        // Build the HTTP request
        let mut req_builder = self
            .http_client
            .request(
                reqwest::Method::from_bytes(request.method.as_bytes())
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to invoke Kubernetes worker '{}': Invalid HTTP method",
                            self.service_name
                        ),
                        resource_id: Some(self.service_name.clone()),
                    })?,
                &url,
            )
            .body(request.body);

        // Add headers
        for (key, value) in request.headers {
            req_builder = req_builder.header(&key, &value);
        }

        // Set timeout if specified
        if let Some(timeout) = request.timeout {
            req_builder = req_builder.timeout(timeout);
        }

        // Execute the request
        let response =
            req_builder
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to invoke Kubernetes worker '{}': HTTP request failed",
                        self.service_name
                    ),
                    resource_id: Some(self.service_name.clone()),
                })?;

        // Extract response data
        let status = response.status().as_u16();
        let headers: BTreeMap<String, String> = response
            .headers()
            .iter()
            .filter_map(|(k, v)| {
                v.to_str()
                    .ok()
                    .map(|v| (k.as_str().to_string(), v.to_string()))
            })
            .collect();

        let body = response
            .bytes()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to invoke Kubernetes worker '{}': Failed to read response body",
                    self.service_name
                ),
                resource_id: Some(self.service_name.clone()),
            })?
            .to_vec();

        Ok(WorkerInvokeResponse {
            status,
            headers,
            body,
        })
    }

    async fn get_worker_url(&self) -> Result<Option<String>> {
        // Return the public URL if configured in the binding
        Ok(self.public_url.clone())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
