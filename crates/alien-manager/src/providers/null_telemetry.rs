//! No-op telemetry backend that silently discards all signals.
//!
//! Useful when embedding alien-manager in a context where telemetry
//! is not configured or handled externally.

use alien_error::AlienError;
use async_trait::async_trait;
use bytes::Bytes;
use tracing::debug;

use crate::traits::{TelemetryBackend, TelemetrySignal};

pub struct NullTelemetryBackend;

#[async_trait]
impl TelemetryBackend for NullTelemetryBackend {
    async fn ingest(
        &self,
        signal: TelemetrySignal,
        deployment_id: &str,
        data: Bytes,
    ) -> Result<(), AlienError> {
        debug!(
            signal = ?signal,
            deployment_id = deployment_id,
            bytes = data.len(),
            "NullTelemetryBackend: discarding telemetry signal"
        );
        Ok(())
    }
}
