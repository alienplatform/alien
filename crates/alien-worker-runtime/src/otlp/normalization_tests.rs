use std::collections::HashMap;
use std::time::{Duration, UNIX_EPOCH};

use opentelemetry::logs::{AnyValue, Severity};
use opentelemetry_sdk::logs::{InMemoryLogExporter, SdkLoggerProvider};

fn value_json(value: &AnyValue) -> serde_json::Value {
    match value {
        AnyValue::Int(value) => serde_json::json!(value),
        AnyValue::Double(value) => serde_json::json!(value),
        AnyValue::Boolean(value) => serde_json::json!(value),
        AnyValue::String(value) => serde_json::json!(value.as_str()),
        AnyValue::ListAny(values) => values.iter().map(value_json).collect(),
        AnyValue::Map(values) => serde_json::Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.as_str().to_owned(), value_json(value)))
                .collect(),
        ),
        _ => panic!("unexpected SDK value"),
    }
}

const STRUCTURED: &str = r#"{"level":40,"time":1700000000123,"msg":"request rejected","payload":{"status":401,"enabled":true,"items":[7,false,null]},"container.name":"spoofed","stream":"spoofed","alien.system":"true","huge":18446744073709551615,"precise":0.123456789012345678901}"#;

use super::emit_to_provider;

#[test]
fn exports_structured_fields_without_overwriting_capture_context() {
    let exporter = InMemoryLogExporter::default();
    let provider = SdkLoggerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    emit_to_provider(
        &provider,
        "stdout",
        STRUCTURED,
        1_800_000_000_000_000_000,
        false,
    );
    let logs = exporter.get_emitted_logs().unwrap();
    assert_eq!(logs.len(), 1);
    let record = &logs[0].record;
    assert_eq!(record.severity_number(), Some(Severity::Warn));
    assert_eq!(
        record.timestamp(),
        Some(UNIX_EPOCH + Duration::from_millis(1700000000123))
    );
    assert_eq!(
        record.observed_timestamp(),
        Some(UNIX_EPOCH + Duration::from_secs(1_800_000_000))
    );
    assert!(
        matches!(record.body(), Some(AnyValue::String(body)) if body.as_str() == "request rejected")
    );
    let attrs: HashMap<_, _> = record
        .attributes_iter()
        .map(|(key, value)| (key.as_str(), value_json(value)))
        .collect();
    assert_eq!(attrs["stream"], "stdout");
    assert!(!attrs.contains_key("alien.system"));
    assert_eq!(attrs["log.record.original"], STRUCTURED);
    assert_eq!(
        attrs["app"]["payload"],
        serde_json::json!({
            "status":401,"enabled":true,"items":[7,false,"null"]
        })
    );
    assert_eq!(attrs["app"]["huge"], "18446744073709551615");
    assert_eq!(attrs["app"]["precise"], "0.123456789012345678901");
    assert_eq!(attrs["app"]["stream"], "spoofed");
}

#[test]
fn keeps_malformed_duplicate_and_system_records_recoverable() {
    let exporter = InMemoryLogExporter::default();
    let provider = SdkLoggerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let cases = [
        (r#"{"msg":"truncated""#, false),
        (r#"{"msg":"one","msg":"two","level":"INFO"}"#, false),
        (STRUCTURED, true),
    ];
    for (body, system) in cases {
        emit_to_provider(&provider, "stderr", body, -1, system);
    }
    let logs = exporter.get_emitted_logs().unwrap();
    assert_eq!(logs.len(), cases.len());
    for (log, (body, system)) in logs.iter().zip(cases) {
        assert!(
            matches!(log.record.body(), Some(AnyValue::String(value)) if value.as_str() == body)
        );
        assert_eq!(log.record.timestamp(), Some(UNIX_EPOCH));
        assert_eq!(log.record.severity_number(), Some(Severity::Error));
        assert!(log
            .record
            .attributes_iter()
            .all(|(key, _)| key.as_str() != "app"));
        assert_eq!(
            log.record
                .attributes_iter()
                .any(|(key, _)| key.as_str() == "alien.system"),
            system
        );
    }
}

#[test]
fn selects_messages_from_multiple_logger_conventions() {
    let exporter = InMemoryLogExporter::default();
    let provider = SdkLoggerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    for body in [
        r#"{"level":"WARN","target":"example","fields":{"message":"ready","count":3}}"#,
        r#"{"level":"warn","message":"ready","count":3}"#,
        r#"{"event":"ready","level":"warning","count":3}"#,
        r#"{"@message":"ready","@level":"warn","@timestamp":"2023-11-14T22:13:20Z"}"#,
    ] {
        emit_to_provider(&provider, "stdout", body, 0, false);
    }
    let logs = exporter.get_emitted_logs().unwrap();
    assert_eq!(logs.len(), 4);
    for log in logs {
        assert!(
            matches!(log.record.body(), Some(AnyValue::String(value)) if value.as_str() == "ready")
        );
        assert_eq!(log.record.severity_number(), Some(Severity::Warn));
        assert!(log
            .record
            .attributes_iter()
            .any(|(key, _)| key.as_str() == "app"));
    }
}
