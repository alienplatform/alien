use async_trait::async_trait;

use alien_error::AlienError;

use crate::auth::GatewayLogSource;

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
    /// Deployment ID for ordinary deployment telemetry.
    pub deployment_id: Option<String>,
    /// Project ID if known from the auth token. Avoids an extra API call to resolve it.
    pub project_id: Option<String>,
    /// Workspace ID of the deployment. Used for scoping telemetry data.
    pub workspace_id: Option<String>,
    /// Trusted gateway source for capability-authorized request logs.
    pub gateway_log_source: Option<GatewayLogSource>,
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

    /// Ingest OTLP metrics this manager pulled itself from a cloud provider's API (for example
    /// CloudWatch) for the deployment in `caller`.
    ///
    /// Only the manager's own loops call this; telemetry a deployment sends never reaches it.
    /// So a backend may keep these metrics apart from deployment-supplied metrics and trust
    /// their resource attributes. Wrappers must forward it to the backend they wrap.
    async fn ingest_cloud_metrics(
        &self,
        caller: &TelemetryCaller,
        data: bytes::Bytes,
    ) -> Result<(), AlienError> {
        self.ingest(TelemetrySignal::Metrics, caller, data).await
    }
}
