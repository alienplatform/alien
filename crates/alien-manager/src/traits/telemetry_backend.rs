use async_trait::async_trait;

use alien_error::AlienError;

/// Signal type for OTLP data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetrySignal {
    Logs,
    Traces,
    Metrics,
}

/// Identity of the caller ingesting telemetry.
#[derive(Debug, Clone)]
pub struct TelemetryCaller {
    pub deployment_id: String,
    /// Project ID if known from the auth token. Avoids an extra API call to resolve it.
    pub project_id: Option<String>,
    /// Workspace ID of the deployment. Used for scoping telemetry data.
    pub workspace_id: Option<String>,
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
        caller: &TelemetryCaller,
        data: bytes::Bytes,
    ) -> Result<(), AlienError>;
}
