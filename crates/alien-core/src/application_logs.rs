/// A recognized application-provided log level.
pub use lognorm::Severity as ApplicationLogLevel;

/// Reads an unambiguous severity using the shared normalization library.
pub fn parse_application_log_level(body: &str) -> Option<ApplicationLogLevel> {
    lognorm::parse(body).severity()
}

/// Projects a readable structured message while callers retain the original.
pub fn parse_application_log_message(body: &str) -> Option<String> {
    let log = lognorm::parse(body);
    (log.outcome() == lognorm::ParseOutcome::Structured && log.message() != body)
        .then(|| log.message().to_owned())
}
