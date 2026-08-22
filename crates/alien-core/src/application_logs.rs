use serde_json::{Map, Value};

/// A recognized application-provided log level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplicationLogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

/// Reads an unambiguous severity from a structured application log.
///
/// The supported fields follow common OpenTelemetry, ECS, Python, Pino,
/// Bunyan, Loguru, and structured-logger conventions. Message text, HTTP
/// status codes, embedded timestamps, and arbitrary fields are deliberately
/// ignored. Callers retain their existing stream-based fallback when parsing
/// returns `None`.
pub fn parse_application_log_level(body: &str) -> Option<ApplicationLogLevel> {
    let body = body.trim();
    if !body.starts_with('{') || !body.ends_with('}') {
        return None;
    }

    let Value::Object(record) = serde_json::from_str::<Value>(body).ok()? else {
        return None;
    };

    let mut resolved = None;
    let mut conflict = false;

    for key in [
        "level",
        "severity",
        "severityText",
        "severity_text",
        "levelname",
        "level_name",
        "logLevel",
        "log_level",
        "log.level",
    ] {
        add_candidate(
            &mut resolved,
            &mut conflict,
            record.get(key).and_then(Value::as_str).and_then(parse_name),
        );
    }

    add_candidate(
        &mut resolved,
        &mut conflict,
        nested_value(&record, &["log", "level"])
            .and_then(Value::as_str)
            .and_then(parse_name),
    );
    add_candidate(
        &mut resolved,
        &mut conflict,
        nested_value(&record, &["record", "level", "name"])
            .and_then(Value::as_str)
            .and_then(parse_name),
    );

    for key in ["severityNumber", "severity_number"] {
        add_candidate(
            &mut resolved,
            &mut conflict,
            record
                .get(key)
                .and_then(Value::as_i64)
                .and_then(parse_otel_number),
        );
    }

    for key in ["levelno", "level_number"] {
        add_candidate(
            &mut resolved,
            &mut conflict,
            record
                .get(key)
                .and_then(Value::as_i64)
                .and_then(parse_python_number),
        );
    }

    if is_pino_or_bunyan_record(&record) {
        add_candidate(
            &mut resolved,
            &mut conflict,
            record
                .get("level")
                .and_then(Value::as_i64)
                .and_then(parse_pino_number),
        );
    }

    (!conflict).then_some(resolved).flatten()
}

fn nested_value<'a>(record: &'a Map<String, Value>, path: &[&str]) -> Option<&'a Value> {
    let (first, rest) = path.split_first()?;
    let mut value = record.get(*first)?;
    for key in rest {
        value = value.as_object()?.get(*key)?;
    }
    Some(value)
}

fn add_candidate(
    resolved: &mut Option<ApplicationLogLevel>,
    conflict: &mut bool,
    candidate: Option<ApplicationLogLevel>,
) {
    let Some(candidate) = candidate else {
        return;
    };
    match resolved {
        Some(existing) if *existing != candidate => *conflict = true,
        Some(_) => {}
        None => *resolved = Some(candidate),
    }
}

fn parse_name(value: &str) -> Option<ApplicationLogLevel> {
    match value.trim().to_ascii_lowercase().as_str() {
        "trace" => Some(ApplicationLogLevel::Trace),
        "debug" | "verbose" | "silly" => Some(ApplicationLogLevel::Debug),
        "info" | "information" | "informational" | "notice" | "http" | "success" => {
            Some(ApplicationLogLevel::Info)
        }
        "warn" | "warning" => Some(ApplicationLogLevel::Warn),
        "error" | "err" | "exception" => Some(ApplicationLogLevel::Error),
        "fatal" | "critical" | "crit" | "alert" | "emergency" | "emerg" | "panic" | "dpanic" => {
            Some(ApplicationLogLevel::Fatal)
        }
        _ => None,
    }
}

fn parse_otel_number(value: i64) -> Option<ApplicationLogLevel> {
    match value {
        1..=4 => Some(ApplicationLogLevel::Trace),
        5..=8 => Some(ApplicationLogLevel::Debug),
        9..=12 => Some(ApplicationLogLevel::Info),
        13..=16 => Some(ApplicationLogLevel::Warn),
        17..=20 => Some(ApplicationLogLevel::Error),
        21..=24 => Some(ApplicationLogLevel::Fatal),
        _ => None,
    }
}

fn parse_python_number(value: i64) -> Option<ApplicationLogLevel> {
    match value {
        5 => Some(ApplicationLogLevel::Trace),
        10 => Some(ApplicationLogLevel::Debug),
        20 => Some(ApplicationLogLevel::Info),
        30 => Some(ApplicationLogLevel::Warn),
        40 => Some(ApplicationLogLevel::Error),
        50 => Some(ApplicationLogLevel::Fatal),
        _ => None,
    }
}

fn parse_pino_number(value: i64) -> Option<ApplicationLogLevel> {
    match value {
        10 => Some(ApplicationLogLevel::Trace),
        20 => Some(ApplicationLogLevel::Debug),
        30 => Some(ApplicationLogLevel::Info),
        40 => Some(ApplicationLogLevel::Warn),
        50 => Some(ApplicationLogLevel::Error),
        60 => Some(ApplicationLogLevel::Fatal),
        _ => None,
    }
}

fn is_pino_or_bunyan_record(record: &Map<String, Value>) -> bool {
    if !record.get("msg").is_some_and(Value::is_string) {
        return false;
    }

    let pino = record.get("pid").is_some_and(Value::is_i64)
        && record
            .get("time")
            .is_some_and(|value| value.is_number() || value.is_string());
    let bunyan = record.get("v").and_then(Value::as_i64) == Some(0)
        && record.get("name").is_some_and(Value::is_string)
        && record.get("hostname").is_some_and(Value::is_string);

    pino || bunyan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_string_fields_and_aliases() {
        let cases = [
            (r#"{"level":" TRACE "}"#, ApplicationLogLevel::Trace),
            (r#"{"severity":"verbose"}"#, ApplicationLogLevel::Debug),
            (r#"{"severityText":"NOTICE"}"#, ApplicationLogLevel::Info),
            (r#"{"severity_text":"success"}"#, ApplicationLogLevel::Info),
            (r#"{"levelname":"WARNING"}"#, ApplicationLogLevel::Warn),
            (r#"{"level_name":"err"}"#, ApplicationLogLevel::Error),
            (r#"{"logLevel":"exception"}"#, ApplicationLogLevel::Error),
            (r#"{"log_level":"critical"}"#, ApplicationLogLevel::Fatal),
            (r#"{"log.level":"panic"}"#, ApplicationLogLevel::Fatal),
            (r#"{"log":{"level":"debug"}}"#, ApplicationLogLevel::Debug),
            (
                r#"{"record":{"level":{"name":"INFO"}}}"#,
                ApplicationLogLevel::Info,
            ),
        ];

        for (body, expected) in cases {
            assert_eq!(parse_application_log_level(body), Some(expected), "{body}");
        }
    }

    #[test]
    fn parses_opentelemetry_severity_ranges() {
        let cases = [
            (1, ApplicationLogLevel::Trace),
            (4, ApplicationLogLevel::Trace),
            (5, ApplicationLogLevel::Debug),
            (8, ApplicationLogLevel::Debug),
            (9, ApplicationLogLevel::Info),
            (12, ApplicationLogLevel::Info),
            (13, ApplicationLogLevel::Warn),
            (16, ApplicationLogLevel::Warn),
            (17, ApplicationLogLevel::Error),
            (20, ApplicationLogLevel::Error),
            (21, ApplicationLogLevel::Fatal),
            (24, ApplicationLogLevel::Fatal),
        ];

        for (number, expected) in cases {
            let body = format!(r#"{{"severityNumber":{number}}}"#);
            assert_eq!(parse_application_log_level(&body), Some(expected), "{body}");
        }
        assert_eq!(parse_application_log_level(r#"{"severityNumber":0}"#), None);
        assert_eq!(
            parse_application_log_level(r#"{"severity_number":25}"#),
            None
        );
    }

    #[test]
    fn parses_python_logging_numbers() {
        let cases = [
            (5, ApplicationLogLevel::Trace),
            (10, ApplicationLogLevel::Debug),
            (20, ApplicationLogLevel::Info),
            (30, ApplicationLogLevel::Warn),
            (40, ApplicationLogLevel::Error),
            (50, ApplicationLogLevel::Fatal),
        ];

        for (number, expected) in cases {
            let body = format!(r#"{{"levelno":{number}}}"#);
            assert_eq!(parse_application_log_level(&body), Some(expected), "{body}");
        }
        assert_eq!(parse_application_log_level(r#"{"level_number":35}"#), None);
    }

    #[test]
    fn parses_pino_and_bunyan_numeric_levels_only_with_a_signature() {
        let pino_cases = [
            (10, ApplicationLogLevel::Trace),
            (20, ApplicationLogLevel::Debug),
            (30, ApplicationLogLevel::Info),
            (40, ApplicationLogLevel::Warn),
            (50, ApplicationLogLevel::Error),
            (60, ApplicationLogLevel::Fatal),
        ];
        for (number, expected) in pino_cases {
            let body = format!(
                r#"{{"level":{number},"time":1573664685466,"pid":78742,"hostname":"host","msg":"ready"}}"#
            );
            assert_eq!(parse_application_log_level(&body), Some(expected), "{body}");
        }

        assert_eq!(
            parse_application_log_level(
                r#"{"name":"api","hostname":"host","v":0,"level":40,"msg":"slow"}"#
            ),
            Some(ApplicationLogLevel::Warn)
        );
        assert_eq!(
            parse_application_log_level(r#"{"level":30,"msg":"ready"}"#),
            None
        );
        assert_eq!(
            parse_application_log_level(r#"{"level":30,"time":1,"msg":"ready"}"#),
            None
        );
    }

    #[test]
    fn accepts_agreeing_candidates_and_rejects_conflicts() {
        assert_eq!(
            parse_application_log_level(r#"{"level":"warn","severity":"warning"}"#),
            Some(ApplicationLogLevel::Warn)
        );
        assert_eq!(
            parse_application_log_level(r#"{"level":"custom","severity":"error"}"#),
            Some(ApplicationLogLevel::Error)
        );
        assert_eq!(
            parse_application_log_level(r#"{"level":"error","severity":"info"}"#),
            None
        );
        assert_eq!(
            parse_application_log_level(r#"{"severityText":"warn","severityNumber":17}"#),
            None
        );
    }

    #[test]
    fn ignores_unstructured_or_ambiguous_records() {
        for body in [
            r#"{"level":"LOUD"}"#,
            r#"{"level":"30"}"#,
            r#"{"status":500,"message":"request failed"}"#,
            r#"{"exception":{"message":"boom"}}"#,
            r#"{"fields":{"level":"ERROR"}}"#,
            r#"[{"level":"error"}]"#,
            r#""error""#,
            "INFO ready",
            "prefix {\"level\":\"error\"}",
            r#"{"level":"error""#,
        ] {
            assert_eq!(parse_application_log_level(body), None, "{body}");
        }
    }
}
