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
                if let Some(deployment_id) = &caller.deployment_id {
                    if let Ok(entries) =
                        crate::providers::recent_logs::decode_log_entries(deployment_id, &data)
                    {
                        for entry in entries {
                            self.log_buffer.push(entry).await;
                        }
                        return Ok(());
                    }
                }
                self.log_buffer
                    .push(LogEntry {
                        timestamp: chrono::Utc::now(),
                        deployment_id: caller.deployment_id.clone().unwrap_or_else(|| {
                            caller
                                .gateway_log_source
                                .map(|source| source.as_str().to_string())
                                .unwrap_or_else(|| "unknown-telemetry-origin".to_string())
                        }),
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
