//! Conversion at the OpenTelemetry SDK boundary; recognition belongs to lognorm.
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lognorm::{ParsedLog, Severity as LogSeverity};
use opentelemetry::{
    logs::{AnyValue, Severity},
    Key,
};
use serde_json::{Map, Value};

pub(super) fn timestamp(log: &ParsedLog<'_>) -> Option<SystemTime> {
    let time = log.timestamp()?;
    let seconds = u64::try_from(time.unix_seconds()).ok()?;
    UNIX_EPOCH.checked_add(Duration::new(seconds, time.nanoseconds()))
}

pub(super) fn severity(log: &ParsedLog<'_>) -> Option<Severity> {
    log.severity().map(|severity| match severity {
        LogSeverity::Trace => Severity::Trace,
        LogSeverity::Debug => Severity::Debug,
        LogSeverity::Info => Severity::Info,
        LogSeverity::Warn => Severity::Warn,
        LogSeverity::Error => Severity::Error,
        LogSeverity::Fatal => Severity::Fatal,
    })
}

pub(super) fn fields(fields: &Map<String, Value>) -> AnyValue {
    AnyValue::Map(Box::new(
        fields
            .iter()
            .map(|(key, value)| (Key::new(key.clone()), value_to_sdk(value)))
            .collect(),
    ))
}

fn value_to_sdk(value: &Value) -> AnyValue {
    match value {
        // SDK 0.30/0.31 cannot represent null. Retain its JSON spelling here
        // and retain the exact record in log.record.original for recovery.
        Value::Null => AnyValue::String("null".into()),
        Value::Bool(value) => AnyValue::Boolean(*value),
        Value::String(value) => AnyValue::String(value.clone().into()),
        Value::Array(values) => {
            AnyValue::ListAny(Box::new(values.iter().map(value_to_sdk).collect()))
        }
        Value::Object(values) => fields(values),
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                return AnyValue::Int(value);
            }
            // Integer overflow and decimals that change when rendered through
            // f64 remain exact strings rather than silently losing precision.
            let text = number.to_string();
            let integer = !text.bytes().any(|byte| matches!(byte, b'.' | b'e' | b'E'));
            let double = (!integer)
                .then(|| number.as_f64())
                .flatten()
                .filter(|value| {
                    serde_json::Number::from_f64(*value)
                        .is_some_and(|candidate| candidate.to_string() == text)
                });
            double
                .map(AnyValue::Double)
                .unwrap_or_else(|| AnyValue::String(text.into()))
        }
    }
}
