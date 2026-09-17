//! The exec contract between the operations runtime and a plugin binary.
//!
//! A plugin is invoked by running its extracted binary as a subprocess. The
//! invocation is written to the child's stdin as a single JSON
//! [`PluginInvocation`] object; the child writes a single JSON
//! [`PluginResult`] to stdout and exits. Params and results reuse the
//! command protocol's [`BodySpec`] so a plugin invocation is representable
//! as (and can be carried by) an `alien-commands` envelope on either the
//! push or pull transport.
//!
//! Secrets are never passed on the command line — the invocation goes over
//! stdin and any credentials the plugin needs are delivered via the process
//! environment by the runtime. Plugin stdout is data, never instructions.

use alien_core::commands_types::{BodySpec, CommandResponse};
use serde::{Deserialize, Serialize};

/// What the runtime hands to a plugin: which operation to run and its
/// params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInvocation {
    /// Protocol version, so a plugin can reject an envelope it doesn't
    /// understand. Bumped only on a breaking change to this contract.
    pub protocol_version: u32,
    /// The operation name to run (must be one the plugin's manifest
    /// declares).
    pub operation: String,
    /// The invocation params, inline (≤150KB, base64) or storage-backed —
    /// the same body shape a command envelope carries.
    pub params: BodySpec,
}

/// The current plugin protocol version.
pub const PROTOCOL_VERSION: u32 = 1;

impl PluginInvocation {
    /// Build an invocation with inline JSON params at the current protocol
    /// version.
    pub fn inline_json(operation: impl Into<String>, params_json: &[u8]) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            operation: operation.into(),
            params: BodySpec::inline(params_json),
        }
    }
}

/// What a plugin returns. This is exactly the command protocol's
/// [`CommandResponse`] (success with an inline/storage body, or a
/// structured error), so a plugin result can flow straight back over the
/// command response path without translation.
pub type PluginResult = CommandResponse;

/// Exact protocol marker used when a plugin explicitly declares that an
/// operation failure is safe to retry. Absence means non-retryable.
pub const RETRYABLE_ERROR_DETAILS: &str = r#"{"retryable":true}"#;

/// Build an explicitly retryable plugin error.
pub fn retryable_error(code: impl Into<String>, message: impl Into<String>) -> PluginResult {
    PluginResult::error_with_details(code, message, RETRYABLE_ERROR_DETAILS)
}

/// Return whether a plugin result carries the exact, versioned retry marker.
/// Free-form details and error-code guesses never authorize retrying a write.
pub fn is_explicitly_retryable(result: &PluginResult) -> bool {
    matches!(
        result,
        PluginResult::Error {
            details: Some(details),
            ..
        } if details == RETRYABLE_ERROR_DETAILS
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invocation_round_trips_as_json() {
        let inv = PluginInvocation::inline_json("vacuum", br#"{"table":"orders"}"#);
        let json = serde_json::to_vec(&inv).unwrap();
        let back: PluginInvocation = serde_json::from_slice(&json).unwrap();
        assert_eq!(back, inv);
        assert_eq!(back.protocol_version, PROTOCOL_VERSION);
        assert_eq!(back.operation, "vacuum");
    }

    #[test]
    fn result_success_and_error_round_trip() {
        let ok = PluginResult::success_json(&serde_json::json!({ "rows": 3 })).unwrap();
        let json = serde_json::to_vec(&ok).unwrap();
        let back: PluginResult = serde_json::from_slice(&json).unwrap();
        assert!(back.is_success());

        let err = PluginResult::error("PLUGIN_FAILED", "connection refused");
        let json = serde_json::to_vec(&err).unwrap();
        let back: PluginResult = serde_json::from_slice(&json).unwrap();
        assert!(back.is_error());
    }

    #[test]
    fn only_the_exact_protocol_marker_is_retryable() {
        let retryable = retryable_error("THROTTLED", "try later");
        assert!(is_explicitly_retryable(&retryable));
        assert!(!is_explicitly_retryable(&PluginResult::error(
            "THROTTLED",
            "try later"
        )));
        assert!(!is_explicitly_retryable(&PluginResult::error_with_details(
            "THROTTLED",
            "try later",
            r#"{"retryable":false}"#,
        )));
    }
}
