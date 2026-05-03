use crate::dev::{LogBuffer, LogEntry};
use crate::traits::telemetry_backend::{TelemetryBackend, TelemetryCaller, TelemetrySignal};
use alien_error::AlienError;
use async_trait::async_trait;

use std::sync::Arc;

pub struct InMemoryTelemetryBackend {
    log_buffer: Arc<LogBuffer>,
}

impl InMemoryTelemetryBackend {
    pub fn new(log_buffer: Arc<LogBuffer>) -> Self {
        Self { log_buffer }
    }
}

#[async_trait]
impl TelemetryBackend for InMemoryTelemetryBackend {
    async fn ingest(
        &self,
        signal: TelemetrySignal,
        caller: &TelemetryCaller,
        data: bytes::Bytes,
    ) -> Result<(), AlienError> {
        match signal {
            TelemetrySignal::Logs => {
                // For now, store as raw entry. Full OTLP protobuf parsing can be added later.
                self.log_buffer
                    .push(LogEntry {
                        timestamp: chrono::Utc::now(),
                        deployment_id: caller.deployment_id.clone(),
                        body: format!("[OTLP log data: {} bytes]", data.len()),
                        severity: "INFO".to_string(),
                        resource_name: None,
                        attributes: vec![],
                    })
                    .await;
            }
            _ => {
                // Traces and metrics are silently accepted in dev mode
            }
        }
        Ok(())
    }
}
