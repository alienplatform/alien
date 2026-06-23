use std::collections::BTreeMap;

use alien_error::{AlienError, Context, IntoAlienError};
use axum::http::HeaderMap;
use chrono::{DateTime, Utc};
use opentelemetry_proto::tonic::{
    collector::logs::v1::ExportLogsServiceRequest,
    common::v1::{any_value, AnyValue, KeyValue},
    logs::v1::{LogRecord, ResourceLogs, ScopeLogs, SeverityNumber},
    resource::v1::Resource,
};
use prost::Message;
use serde_json::Value;

use crate::error::{ErrorData, Result};

const COLLECTOR_SOURCE: &str = "node-fluentbit";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedLogSource {
    pub namespace: String,
    pub pod: String,
    pub container: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CollectorLogRecord {
    namespace: String,
    pod: String,
    container: String,
    stream: String,
    timestamp_unix_nanos: u64,
    body: String,
    filename: Option<String>,
}

pub fn require_collector_auth(headers: &HeaderMap, expected_token: Option<&str>) -> Result<()> {
    let Some(expected_token) = expected_token else {
        return Err(AlienError::new(ErrorData::CollectorAuthorizationFailed {
            message: "collector token is not configured".to_string(),
        }));
    };

    let provided = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| {
            AlienError::new(ErrorData::CollectorAuthorizationFailed {
                message: "missing bearer token".to_string(),
            })
        })?;

    if provided != expected_token {
        return Err(AlienError::new(ErrorData::CollectorAuthorizationFailed {
            message: "invalid bearer token".to_string(),
        }));
    }

    Ok(())
}

pub fn collector_records_to_otlp(
    body: &[u8],
    default_namespace: &str,
    deployment_id: &str,
) -> Result<(usize, Vec<u8>)> {
    let values = parse_collector_records(body)?;
    let now = Utc::now();
    let mut records = Vec::new();

    for value in values {
        let Some(record) = collector_value_to_record(&value, default_namespace, now)? else {
            continue;
        };
        if record.namespace != default_namespace {
            continue;
        }
        records.push(record);
    }

    if records.is_empty() {
        return Err(AlienError::new(ErrorData::CollectorPayloadInvalid {
            message: "collector payload contained no in-scope log records".to_string(),
        }));
    }

    let request = otlp_request(records, deployment_id);
    let mut encoded = Vec::new();
    request.encode(&mut encoded).into_alien_error().context(
        ErrorData::CollectorTelemetryInvalid {
            message: "failed to encode OTLP log export request".to_string(),
        },
    )?;

    let count = request
        .resource_logs
        .iter()
        .flat_map(|resource_logs| &resource_logs.scope_logs)
        .map(|scope_logs| scope_logs.log_records.len())
        .sum();

    Ok((count, encoded))
}

fn parse_collector_records(body: &[u8]) -> Result<Vec<Value>> {
    match serde_json::from_slice::<Value>(body) {
        Ok(value) => {
            let records = flatten_collector_records(value);
            if records.is_empty() {
                return Err(AlienError::new(ErrorData::CollectorPayloadInvalid {
                    message: "collector JSON contained no log records".to_string(),
                }));
            }
            Ok(records)
        }
        Err(json_error) => {
            let text = std::str::from_utf8(body).into_alien_error().context(
                ErrorData::CollectorPayloadInvalid {
                    message: "body is not UTF-8 JSON".to_string(),
                },
            )?;
            let mut records = Vec::new();
            for line in text.lines().filter(|line| !line.trim().is_empty()) {
                let value = serde_json::from_str::<Value>(line)
                    .into_alien_error()
                    .context(ErrorData::CollectorPayloadInvalid {
                        message: format!("body is not valid JSON or NDJSON: {json_error}"),
                    })?;
                records.extend(flatten_collector_records(value));
            }
            if records.is_empty() {
                return Err(AlienError::new(ErrorData::CollectorPayloadInvalid {
                    message: "collector body contained no log records".to_string(),
                }));
            }
            Ok(records)
        }
    }
}

fn flatten_collector_records(value: Value) -> Vec<Value> {
    match value {
        Value::Array(items) => items
            .into_iter()
            .flat_map(|item| match item {
                Value::Array(mut tuple) if tuple.len() >= 2 => {
                    let record = tuple.remove(1);
                    flatten_collector_records(record)
                }
                other => flatten_collector_records(other),
            })
            .collect(),
        Value::Object(mut object) => {
            for key in ["records", "logs"] {
                if let Some(value) = object.remove(key) {
                    return flatten_collector_records(value);
                }
            }
            vec![Value::Object(object)]
        }
        _ => Vec::new(),
    }
}

fn collector_value_to_record(
    record: &Value,
    default_namespace: &str,
    now: DateTime<Utc>,
) -> Result<Option<CollectorLogRecord>> {
    let inner = record.get("record").unwrap_or(record);
    let kubernetes = inner.get("kubernetes").unwrap_or(&Value::Null);
    let filename = string_field(inner, &["filename", "path", "file"]);
    let parsed = filename.as_deref().and_then(parse_log_filename);

    let namespace = parsed
        .as_ref()
        .map(|source| source.namespace.as_str())
        .or_else(|| kubernetes.get("namespace_name").and_then(Value::as_str))
        .unwrap_or(default_namespace)
        .to_string();
    let pod = parsed
        .as_ref()
        .map(|source| source.pod.as_str())
        .or_else(|| kubernetes.get("pod_name").and_then(Value::as_str))
        .unwrap_or("unknown-pod")
        .to_string();
    let container = parsed
        .as_ref()
        .map(|source| source.container.as_str())
        .or_else(|| kubernetes.get("container_name").and_then(Value::as_str))
        .unwrap_or("unknown-container")
        .to_string();
    let body = string_field(inner, &["log", "message", "body"])
        .unwrap_or_else(|| inner.to_string())
        .trim_end()
        .to_string();

    if body.is_empty() {
        return Ok(None);
    }

    let timestamp_unix_nanos = timestamp_from_record(inner)
        .or_else(|| cri_timestamp_from_line(&body))
        .unwrap_or_else(|| nanos(now));
    let stream = string_field(inner, &["stream"]).unwrap_or_else(|| {
        if body.contains(" stderr ") {
            "stderr".to_string()
        } else {
            "stdout".to_string()
        }
    });

    Ok(Some(CollectorLogRecord {
        namespace,
        pod,
        container,
        stream,
        timestamp_unix_nanos,
        body,
        filename,
    }))
}

fn timestamp_from_record(record: &Value) -> Option<u64> {
    for key in ["time", "timestamp", "observed_at", "observedAt"] {
        let Some(value) = record.get(key) else {
            continue;
        };
        if let Some(text) = value.as_str() {
            if let Ok(timestamp) = DateTime::parse_from_rfc3339(text) {
                return Some(nanos(timestamp.with_timezone(&Utc)));
            }
        }
        if let Some(seconds) = value.as_f64() {
            if seconds.is_finite() && seconds >= 0.0 {
                return Some((seconds * 1_000_000_000.0) as u64);
            }
        }
    }
    None
}

fn cri_timestamp_from_line(line: &str) -> Option<u64> {
    let timestamp = line.split_once(' ')?.0;
    DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|value| nanos(value.with_timezone(&Utc)))
}

fn nanos(timestamp: DateTime<Utc>) -> u64 {
    timestamp.timestamp_nanos_opt().unwrap_or_default().max(0) as u64
}

fn otlp_request(records: Vec<CollectorLogRecord>, deployment_id: &str) -> ExportLogsServiceRequest {
    let mut grouped: BTreeMap<(String, String, String), Vec<CollectorLogRecord>> = BTreeMap::new();
    for record in records {
        grouped
            .entry((
                record.namespace.clone(),
                record.pod.clone(),
                record.container.clone(),
            ))
            .or_default()
            .push(record);
    }

    let resource_logs = grouped
        .into_iter()
        .map(|((_namespace, _pod, container), records)| {
            let log_records = records
                .into_iter()
                .map(|record| {
                    let (severity_text, severity_number) = if record.stream == "stderr" {
                        ("ERROR", SeverityNumber::Error as i32)
                    } else {
                        ("INFO", SeverityNumber::Info as i32)
                    };

                    let mut attributes = vec![
                        kv("alien.log.source", COLLECTOR_SOURCE),
                        kv("stream", &record.stream),
                        kv("k8s.namespace.name", &record.namespace),
                        kv("k8s.pod.name", &record.pod),
                        kv("k8s.container.name", &record.container),
                    ];
                    if let Some(filename) = record.filename.as_deref() {
                        attributes.push(kv("log.file.path", filename));
                    }

                    LogRecord {
                        time_unix_nano: record.timestamp_unix_nanos,
                        observed_time_unix_nano: record.timestamp_unix_nanos,
                        severity_number,
                        severity_text: severity_text.to_string(),
                        body: Some(string_value(record.body)),
                        attributes,
                        dropped_attributes_count: 0,
                        flags: 0,
                        trace_id: Vec::new(),
                        span_id: Vec::new(),
                        event_name: String::new(),
                    }
                })
                .collect();

            ResourceLogs {
                resource: Some(Resource {
                    attributes: vec![
                        kv("service.name", &container),
                        kv("alien.deployment_id", deployment_id),
                    ],
                    dropped_attributes_count: 0,
                    entity_refs: Vec::new(),
                }),
                scope_logs: vec![ScopeLogs {
                    scope: None,
                    log_records,
                    schema_url: String::new(),
                }],
                schema_url: String::new(),
            }
        })
        .collect();

    ExportLogsServiceRequest { resource_logs }
}

fn kv(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.to_string(),
        value: Some(string_value(value.to_string())),
    }
}

fn string_value(value: String) -> AnyValue {
    AnyValue {
        value: Some(any_value::Value::StringValue(value)),
    }
}

fn string_field(record: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| record.get(key).and_then(Value::as_str))
        .map(ToOwned::to_owned)
}

pub fn parse_log_filename(filename: &str) -> Option<ParsedLogSource> {
    if let Some(path) = filename.strip_prefix("/var/log/pods/") {
        let mut parts = path.split('/');
        let pod_dir = parts.next()?;
        let container = parts.next()?.to_string();
        let mut pod_parts = pod_dir.splitn(3, '_');
        return Some(ParsedLogSource {
            namespace: pod_parts.next()?.to_string(),
            pod: pod_parts.next()?.to_string(),
            container,
        });
    }

    let file = filename.rsplit('/').next()?.strip_suffix(".log")?;
    let mut parts = file.split('_');
    let pod = parts.next()?.to_string();
    let namespace = parts.next()?.to_string();
    let container = parts.next()?.rsplit_once('-')?.0.to_string();
    Some(ParsedLogSource {
        namespace,
        pod,
        container,
    })
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;
    use opentelemetry_proto::tonic::common::v1::any_value;

    use super::*;

    #[test]
    fn parses_var_log_pods_filename() {
        assert_eq!(
            parse_log_filename("/var/log/pods/demo_noisy-abc_uid/noisy/0.log"),
            Some(ParsedLogSource {
                namespace: "demo".to_string(),
                pod: "noisy-abc".to_string(),
                container: "noisy".to_string(),
            })
        );
    }

    #[test]
    fn parses_var_log_containers_filename() {
        assert_eq!(
            parse_log_filename("/var/log/containers/noisy-abc_demo_noisy-123.log"),
            Some(ParsedLogSource {
                namespace: "demo".to_string(),
                pod: "noisy-abc".to_string(),
                container: "noisy".to_string(),
            })
        );
    }

    #[test]
    fn rejects_missing_or_wrong_collector_token() {
        let mut headers = HeaderMap::new();
        assert!(require_collector_auth(&headers, Some("secret")).is_err());

        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer wrong"),
        );
        assert!(require_collector_auth(&headers, Some("secret")).is_err());

        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer secret"),
        );
        require_collector_auth(&headers, Some("secret")).expect("correct token should pass");
    }

    #[test]
    fn collector_records_to_otlp_sets_viewer_fields() {
        let body = br#"{
          "records": [
            [
              1710000000.0,
              {
                "filename": "/var/log/pods/demo_noisy-abc_uid/noisy/0.log",
                "stream": "stderr",
                "time": "2026-06-22T01:00:00.123456789Z",
                "log": "raw-pod log 7\n"
              }
            ]
          ]
        }"#;

        let (count, encoded) =
            collector_records_to_otlp(body, "demo", "dep_test").expect("body should convert");
        assert_eq!(count, 1);

        let request =
            ExportLogsServiceRequest::decode(encoded.as_slice()).expect("OTLP should decode");
        let resource = request.resource_logs[0].resource.as_ref().unwrap();
        assert!(resource.attributes.iter().any(|attribute| {
            attribute.key == "service.name"
                && attribute
                    .value
                    .as_ref()
                    .and_then(|value| value.value.as_ref())
                    == Some(&any_value::Value::StringValue("noisy".to_string()))
        }));
        assert!(resource.attributes.iter().any(|attribute| {
            attribute.key == "alien.deployment_id"
                && attribute
                    .value
                    .as_ref()
                    .and_then(|value| value.value.as_ref())
                    == Some(&any_value::Value::StringValue("dep_test".to_string()))
        }));

        let record = &request.resource_logs[0].scope_logs[0].log_records[0];
        assert_eq!(record.severity_text, "ERROR");
        assert_eq!(record.severity_number, SeverityNumber::Error as i32);
        assert_eq!(record.time_unix_nano, 1_782_090_000_123_456_789);
        assert_eq!(
            record.body.as_ref().and_then(|body| body.value.as_ref()),
            Some(&any_value::Value::StringValue("raw-pod log 7".to_string()))
        );
        assert!(record.attributes.iter().any(|attribute| {
            attribute.key == "alien.log.source"
                && attribute
                    .value
                    .as_ref()
                    .and_then(|value| value.value.as_ref())
                    == Some(&any_value::Value::StringValue(COLLECTOR_SOURCE.to_string()))
        }));
    }

    #[test]
    fn drops_out_of_scope_namespace_records() {
        let body = br#"{"filename":"/var/log/pods/other_noisy_uid/noisy/0.log","log":"drop me"}"#;
        let error = collector_records_to_otlp(body, "demo", "dep_test")
            .expect_err("out-of-scope records should be rejected");
        assert!(error.to_string().contains("no in-scope log records"));
    }

    #[test]
    fn parses_ndjson_records() {
        let body = br#"{"filename":"/var/log/pods/demo_pod_uid/api/0.log","log":"one"}
{"filename":"/var/log/pods/demo_pod_uid/api/1.log","log":"two"}"#;
        let (count, _encoded) =
            collector_records_to_otlp(body, "demo", "dep_test").expect("NDJSON should convert");
        assert_eq!(count, 2);
    }
}
