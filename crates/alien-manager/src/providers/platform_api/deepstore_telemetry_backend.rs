use crate::traits::{TelemetryBackend, TelemetrySignal};
use alien_error::{AlienError, GenericError};
use alien_platform_api::SdkResultExt;
use async_trait::async_trait;
use tracing::debug;

/// Forwards OTLP telemetry data to DeepStore, scoping it to the deployment's project.
pub struct DeepStoreTelemetryBackend {
    otlp_url: String,
    database_id: String,
    workspace_name: String,
    platform_client: alien_platform_api::Client,
    http_client: reqwest::Client,
}

impl DeepStoreTelemetryBackend {
    pub fn new(
        otlp_url: String,
        database_id: String,
        workspace_name: String,
        platform_client: alien_platform_api::Client,
    ) -> Self {
        Self {
            otlp_url,
            database_id,
            workspace_name,
            platform_client,
            http_client: reqwest::Client::new(),
        }
    }

    async fn resolve_scope(&self, deployment_id: &str) -> String {
        if deployment_id == "admin" {
            return format!("{}/*", self.workspace_name);
        }

        match self
            .platform_client
            .get_deployment()
            .id(deployment_id)
            .send()
            .await
            .into_sdk_error()
        {
            Ok(deployment) => format!("{}/{}", self.workspace_name, deployment.project_id.as_str()),
            Err(e) => {
                tracing::warn!(
                    deployment_id = deployment_id,
                    error = %e,
                    "Failed to resolve deployment project for telemetry scope, falling back to workspace scope"
                );
                format!("{}/*", self.workspace_name)
            }
        }
    }
}

#[async_trait]
impl TelemetryBackend for DeepStoreTelemetryBackend {
    async fn ingest(
        &self,
        signal: TelemetrySignal,
        deployment_id: &str,
        data: bytes::Bytes,
    ) -> Result<(), AlienError> {
        let signal_path = match signal {
            TelemetrySignal::Logs => "logs",
            TelemetrySignal::Traces => "traces",
            TelemetrySignal::Metrics => {
                return Err(AlienError::new(GenericError {
                    message:
                        "Metrics proxying is not supported; sync metrics directly to the Alien API"
                            .to_string(),
                }));
            }
        };

        let scope = self.resolve_scope(deployment_id).await;

        debug!(
            signal = signal_path,
            deployment_id = deployment_id,
            scope = scope.as_str(),
            bytes = data.len(),
            "Proxying telemetry to DeepStore"
        );

        let endpoint = format!("{}/v1/{}", self.otlp_url, signal_path);

        let response = self
            .http_client
            .post(&endpoint)
            .header("X-Scope", &scope)
            .header("X-Database-Id", &self.database_id)
            .header("Content-Type", "application/x-protobuf")
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| {
                AlienError::new(GenericError {
                    message: format!("Failed to forward {} to DeepStore: {}", signal_path, e),
                })
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AlienError::new(GenericError {
                message: format!(
                    "DeepStore returned {} for {}: {}",
                    status, signal_path, body
                ),
            }));
        }

        Ok(())
    }
}
