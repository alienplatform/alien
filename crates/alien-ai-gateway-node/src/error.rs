//! Error translation from `alien-gateway` into `napi::Error`.
//!
//! Mirrors the envelope contract used by `alien-bindings-node`: async `#[napi]`
//! methods can only carry data to JS through the error `reason` (napi's `Status`
//! is a closed enum), so a stable JSON envelope `{ code, message, context?,
//! retryable, internal }` is serialized into `reason` and the TS layer recovers
//! it with a single `JSON.parse`. `internal` crosses the boundary so the JS side
//! can honor the Rust error's redaction posture rather than defaulting it open.

use alien_error::AlienError;
use alien_gateway::ErrorData;
use serde::Serialize;

#[derive(Debug, Serialize)]
struct ErrorEnvelope<'a> {
    code: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<&'a serde_json::Value>,
    retryable: bool,
    internal: bool,
    // Carry the status code and hint too: a startup failure like
    // `BindingConfigInvalid` declares `http_status_code = 400`, and without this
    // the TS side would default it to 500 — a user-fixable config error rendered
    // as a server fault.
    #[serde(rename = "httpStatusCode", skip_serializing_if = "Option::is_none")]
    http_status_code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<&'a str>,
}

/// Serialize an `AlienError` into a `napi::Error` carrying the structured envelope
/// in its `reason`. The napi `Status` is always `GenericFailure`; the real code
/// lives in the envelope.
pub fn map_gateway_error(err: AlienError<ErrorData>) -> napi::Error {
    let envelope = ErrorEnvelope {
        code: &err.code,
        message: &err.message,
        context: err.context.as_ref(),
        retryable: err.retryable,
        internal: err.internal,
        http_status_code: err.http_status_code,
        hint: err.hint.as_deref(),
    };
    let reason = serde_json::to_string(&envelope).unwrap_or_else(|_| err.message.clone());
    napi::Error::new(napi::Status::GenericFailure, reason)
}
