use crate::traits::telemetry_backend::{TelemetryBackend, TelemetrySignal};
use alien_error::{AlienError, Context, GenericError, IntoAlienError};
use async_trait::async_trait;

pub struct OtlpForwardingBackend {
    client: reqwest::Client,
    endpoint: String,
}

impl OtlpForwardingBackend {
    pub fn new(endpoint: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            endpoint,
        }
    }
}

#[async_trait]
impl TelemetryBackend for OtlpForwardingBackend {
    async fn ingest(
        &self,
        signal: TelemetrySignal,
        _deployment_id: &str,
        data: bytes::Bytes,
    ) -> Result<(), AlienError> {
        let path = match signal {
            TelemetrySignal::Logs => "/v1/logs",
            TelemetrySignal::Traces => "/v1/traces",
            TelemetrySignal::Metrics => "/v1/metrics",
        };

        let url = format!("{}{}", self.endpoint.trim_end_matches('/'), path);

        self.client
            .post(&url)
            .header("content-type", "application/x-protobuf")
            .body(data)
            .send()
            .await
            .into_alien_error()
            .context(GenericError { message: "Failed to forward telemetry signal to OTLP backend".to_string() })?;

        Ok(())
    }
}
