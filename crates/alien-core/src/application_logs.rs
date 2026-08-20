use serde_json::Value;

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

/// Reads a recognized top-level JSON `level` field from an application log.
///
/// This deliberately does not inspect message text or nested fields. Callers
/// retain their existing stream-based fallback when parsing fails.
pub fn parse_application_log_level(body: &str) -> Option<ApplicationLogLevel> {
    let value = serde_json::from_str::<Value>(body).ok()?;
    let level = value.get("level")?.as_str()?;
    match level.to_ascii_uppercase().as_str() {
        "TRACE" => Some(ApplicationLogLevel::Trace),
        "DEBUG" => Some(ApplicationLogLevel::Debug),
        "INFO" => Some(ApplicationLogLevel::Info),
        "WARN" | "WARNING" => Some(ApplicationLogLevel::Warn),
        "ERROR" => Some(ApplicationLogLevel::Error),
        "FATAL" => Some(ApplicationLogLevel::Fatal),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_recognized_top_level_json_levels() {
        assert_eq!(
            parse_application_log_level(r#"{"level":"info","message":"ready"}"#),
            Some(ApplicationLogLevel::Info)
        );
        assert_eq!(
            parse_application_log_level(r#"{"level":"WARNING"}"#),
            Some(ApplicationLogLevel::Warn)
        );
        assert_eq!(
            parse_application_log_level(r#"{"fields":{"level":"ERROR"}}"#),
            None
        );
        assert_eq!(parse_application_log_level(r#"{"level":"LOUD"}"#), None);
        assert_eq!(parse_application_log_level("INFO ready"), None);
    }
}
