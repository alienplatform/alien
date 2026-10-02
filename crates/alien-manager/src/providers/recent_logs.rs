//! Keeps recent deployment logs readable from the manager.
//!
//! Wraps the configured telemetry backend: every OTLP log batch is still
//! passed on unchanged (to your observability backend, or dropped when none is
//! configured), and its records are also decoded into a bounded in-memory
//! buffer that `GET /v1/deployments/{id}/logs` and `alien logs` read. The
//! buffer is a convenience for recent activity; your OTLP backend is the
//! long-term store.

use std::sync::Arc;

use alien_error::AlienError;
use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use opentelemetry_proto::tonic::{
    collector::logs::v1::ExportLogsServiceRequest,
    common::v1::{any_value::Value, AnyValue, KeyValue},
};
use prost::Message;
use tracing::debug;

use crate::dev::{LogBuffer, LogEntry};
use crate::traits::telemetry_backend::{TelemetryBackend, TelemetryCaller, TelemetrySignal};

pub struct RecentLogsBackend {
    inner: Arc<dyn TelemetryBackend>,
    buffer: Arc<LogBuffer>,
}

impl RecentLogsBackend {
    pub fn new(inner: Arc<dyn TelemetryBackend>, buffer: Arc<LogBuffer>) -> Self {
        Self { inner, buffer }
    }
}

#[async_trait]
impl TelemetryBackend for RecentLogsBackend {
    async fn ingest(
        &self,
        signal: TelemetrySignal,
        caller: &TelemetryCaller,
        data: bytes::Bytes,
    ) -> Result<(), AlienError> {
        if signal == TelemetrySignal::Logs {
            if let Some(deployment_id) = &caller.deployment_id {
                match decode_log_entries(deployment_id, &data) {
                    Ok(entries) => {
                        for entry in entries {
                            self.buffer.push(entry).await;
                        }
                    }
                    // Forwarding is the contract; the buffer is best effort.
                    Err(e) => debug!(error = %e, "Recent logs: OTLP batch not decodable"),
                }
            }
        }
        self.inner.ingest(signal, caller, data).await
    }
}

/// Decode an OTLP/protobuf logs export into buffer entries.
pub fn decode_log_entries(
    deployment_id: &str,
    data: &[u8],
) -> Result<Vec<LogEntry>, prost::DecodeError> {
    let request = ExportLogsServiceRequest::decode(data)?;
    let mut entries = Vec::new();
    for resource_logs in request.resource_logs {
        let resource_attributes = resource_logs
            .resource
            .map(|resource| resource.attributes)
            .unwrap_or_default();
        let resource_name = attribute(&resource_attributes, "service.name")
            .or_else(|| attribute(&resource_attributes, "k8s.container.name"));
        for scope_logs in resource_logs.scope_logs {
            for record in scope_logs.log_records {
                let nanos = if record.time_unix_nano > 0 {
                    record.time_unix_nano
                } else {
                    record.observed_time_unix_nano
                };
                let mut attributes: Vec<(String, String)> = resource_attributes
                    .iter()
                    .chain(record.attributes.iter())
                    .filter(|kv| kv.key != "log.record.original")
                    .filter_map(|kv| Some((kv.key.clone(), any_value_string(kv.value.as_ref()?))))
                    .collect();
                attributes.sort();
                attributes.dedup();
                entries.push(LogEntry {
                    timestamp: timestamp(nanos),
                    deployment_id: deployment_id.to_string(),
                    body: record
                        .body
                        .as_ref()
                        .map(any_value_string)
                        .unwrap_or_default(),
                    severity: if record.severity_text.is_empty() {
                        severity_name(record.severity_number).to_string()
                    } else {
                        record.severity_text
                    },
                    resource_name: resource_name.clone(),
                    attributes,
                });
            }
        }
    }
    Ok(entries)
}

fn attribute(attributes: &[KeyValue], key: &str) -> Option<String> {
    attributes
        .iter()
        .find(|kv| kv.key == key)
        .and_then(|kv| kv.value.as_ref())
        .map(any_value_string)
}

fn any_value_string(value: &AnyValue) -> String {
    match &value.value {
        Some(Value::StringValue(v)) => v.clone(),
        Some(Value::BoolValue(v)) => v.to_string(),
        Some(Value::IntValue(v)) => v.to_string(),
        Some(Value::DoubleValue(v)) => v.to_string(),
        Some(Value::BytesValue(v)) => format!("<{} bytes>", v.len()),
        Some(Value::ArrayValue(v)) => v
            .values
            .iter()
            .map(any_value_string)
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::KvlistValue(v)) => v
            .values
            .iter()
            .filter_map(|kv| {
                Some(format!(
                    "{}={}",
                    kv.key,
                    any_value_string(kv.value.as_ref()?)
                ))
            })
            .collect::<Vec<_>>()
            .join(" "),
        None => String::new(),
    }
}

fn timestamp(nanos: u64) -> DateTime<Utc> {
    if nanos == 0 {
        return Utc::now();
    }
    Utc.timestamp_nanos(i64::try_from(nanos).unwrap_or(i64::MAX))
}

/// OTLP severity numbers to their short names.
fn severity_name(number: i32) -> &'static str {
    match number {
        1..=4 => "TRACE",
        5..=8 => "DEBUG",
        9..=12 => "INFO",
        13..=16 => "WARN",
        17..=20 => "ERROR",
        21..=24 => "FATAL",
        _ => "INFO",
    }
}

#[cfg(test)]
mod tests {
    use opentelemetry_proto::tonic::{
        logs::v1::{LogRecord, ResourceLogs, ScopeLogs},
        resource::v1::Resource,
    };

    use super::*;

    fn string(value: &str) -> Option<AnyValue> {
        Some(AnyValue {
            value: Some(Value::StringValue(value.to_string())),
        })
    }

    #[test]
    fn decodes_records_with_resource_and_severity() {
        let request = ExportLogsServiceRequest {
            resource_logs: vec![ResourceLogs {
                resource: Some(Resource {
                    attributes: vec![KeyValue {
                        key: "service.name".to_string(),
                        value: string("api"),
                    }],
                    ..Default::default()
                }),
                scope_logs: vec![ScopeLogs {
                    log_records: vec![
                        LogRecord {
                            time_unix_nano: 1_790_000_000_000_000_000,
                            severity_number: 17,
                            body: string("disk full"),
                            attributes: vec![KeyValue {
                                key: "k8s.pod.name".to_string(),
                                value: string("api-1"),
                            }],
                            ..Default::default()
                        },
                        LogRecord {
                            observed_time_unix_nano: 1_790_000_000_500_000_000,
                            severity_text: "WARN".to_string(),
                            body: string("slow"),
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };

        let entries = decode_log_entries("dep_1", &request.encode_to_vec()).unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].body, "disk full");
        assert_eq!(entries[0].severity, "ERROR");
        assert_eq!(entries[0].resource_name.as_deref(), Some("api"));
        assert!(entries[0]
            .attributes
            .contains(&("k8s.pod.name".to_string(), "api-1".to_string())));
        assert_eq!(entries[0].timestamp.timestamp(), 1_790_000_000);
        assert_eq!(entries[1].severity, "WARN");
        assert_eq!(entries[1].timestamp.timestamp_subsec_millis(), 500);
        assert!(entries.iter().all(|entry| entry.deployment_id == "dep_1"));
    }

    #[test]
    fn rejects_bytes_that_are_not_otlp() {
        assert!(decode_log_entries("dep_1", b"\xff\xff not protobuf").is_err());
    }
}
