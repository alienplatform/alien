use async_trait::async_trait;

use alien_error::AlienError;

/// Signal type for OTLP data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetrySignal {
    Logs,
    Traces,
    Metrics,
}

/// Receives OTLP telemetry data from deployments.
///
/// Default: `OtlpForwardingBackend` — forwards raw protobuf to an external endpoint.
/// Dev mode: `InMemoryTelemetryBackend` — parses and stores in a ring buffer.
#[async_trait]
pub trait TelemetryBackend: Send + Sync {
    /// Ingest OTLP protobuf data.
    async fn ingest(
        &self,
        signal: TelemetrySignal,
        deployment_id: &str,
        data: bytes::Bytes,
    ) -> Result<(), AlienError>;
}
