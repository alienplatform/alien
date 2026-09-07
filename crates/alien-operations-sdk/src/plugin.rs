//! The [`Plugin`] trait and [`run_plugin`] entry point.
//!
//! A plugin author implements [`Plugin`] and calls [`run_plugin`] from
//! `main`. This replaces the hand-copied stdin-read / dispatch /
//! stdout-write boilerplate every `alien-op-*` plugin previously wrote by
//! hand — reading the invocation, checking the protocol version, and
//! writing the result back are all handled here.
//!
//! ```no_run
//! use alien_operations_sdk::plugin::{run_plugin, Plugin};
//! use alien_operations_sdk::protocol::{PluginInvocation, PluginResult};
//! use async_trait::async_trait;
//!
//! struct Postgres;
//!
//! #[async_trait]
//! impl Plugin for Postgres {
//!     async fn handle(&self, invocation: &PluginInvocation) -> PluginResult {
//!         PluginResult::error("NOT_IMPLEMENTED", "example plugin")
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> std::process::ExitCode {
//!     run_plugin(Postgres).await
//! }
//! ```

use std::process::ExitCode;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::protocol::{PluginInvocation, PluginResult, PROTOCOL_VERSION};

/// Maximum invocation size read from stdin before it is rejected as
/// malformed, rather than buffered without bound. Matches the cap several
/// plugins enforced by hand before this became the SDK's responsibility;
/// large params belong in a storage-backed [`crate::protocol::BodySpec`],
/// not inline.
const MAX_INVOCATION_BYTES: usize = 64 * 1024;

/// Implemented by a plugin binary. `handle` is called once per invocation;
/// [`run_plugin`] owns everything around it (stdin/stdout, protocol version
/// checking, result encoding).
#[async_trait]
pub trait Plugin: Send + Sync {
    /// Run one operation and return its result. Implementations should
    /// return `PluginResult::error(...)` rather than panicking on any
    /// expected failure (unknown operation, invalid params, upstream
    /// failure) — a panic is reported to the caller as an opaque crash.
    async fn handle(&self, invocation: &PluginInvocation) -> PluginResult;
}

/// Read one [`PluginInvocation`] from stdin, run it through `plugin`, write
/// the [`PluginResult`] to stdout, and return the process exit code.
///
/// Call this from `main`. It never panics: a malformed invocation or an I/O
/// failure writing the result is reported as `ExitCode::FAILURE` rather
/// than propagated.
pub async fn run_plugin(plugin: impl Plugin) -> ExitCode {
    let result = match read_invocation(tokio::io::stdin()).await {
        Ok(invocation) => dispatch(&plugin, invocation).await,
        Err(message) => PluginResult::error("PLUGIN_PROTOCOL_VIOLATION", message),
    };

    match write_result(&result).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

/// Check the invocation's protocol version and, if supported, hand it to
/// `plugin.handle`. Exposed so a plugin crate can assert its
/// protocol-version handling in tests without spawning a real subprocess
/// wired to stdin/stdout.
pub async fn dispatch(plugin: &impl Plugin, invocation: PluginInvocation) -> PluginResult {
    if invocation.protocol_version != PROTOCOL_VERSION {
        return PluginResult::error(
            "PLUGIN_PROTOCOL_VERSION_UNSUPPORTED",
            format!(
                "plugin supports protocol version {PROTOCOL_VERSION}, received {}",
                invocation.protocol_version
            ),
        );
    }
    plugin.handle(&invocation).await
}

async fn read_invocation(
    reader: impl tokio::io::AsyncRead + Unpin,
) -> Result<PluginInvocation, String> {
    let mut bytes = Vec::new();
    reader
        // Bound the read itself, not just the resulting buffer's length —
        // otherwise an oversized or unbounded stdin stream is fully buffered
        // in memory before the size is ever checked.
        .take((MAX_INVOCATION_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| "failed to read invocation from stdin".to_string())?;
    if bytes.len() > MAX_INVOCATION_BYTES {
        return Err(format!(
            "invocation exceeded the maximum size of {MAX_INVOCATION_BYTES} bytes"
        ));
    }
    serde_json::from_slice(&bytes).map_err(|_| "invocation was not valid JSON".to_string())
}

async fn write_result(result: &PluginResult) -> std::io::Result<()> {
    let bytes =
        serde_json::to_vec(result).expect("PluginResult contains only serializable protocol data");
    let mut stdout = tokio::io::stdout();
    stdout.write_all(&bytes).await?;
    stdout.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;

    #[async_trait]
    impl Plugin for Echo {
        async fn handle(&self, invocation: &PluginInvocation) -> PluginResult {
            PluginResult::success_json(&serde_json::json!({ "operation": invocation.operation }))
                .expect("json result should encode")
        }
    }

    struct AlwaysErrors;

    #[async_trait]
    impl Plugin for AlwaysErrors {
        async fn handle(&self, _invocation: &PluginInvocation) -> PluginResult {
            PluginResult::error("BOOM", "always fails")
        }
    }

    #[tokio::test]
    async fn dispatches_to_handle() {
        let invocation = PluginInvocation::inline_json("health", b"{}");
        let result = Echo.handle(&invocation).await;
        assert!(result.is_success());
    }

    #[tokio::test]
    async fn plugin_error_is_reported_as_error_result() {
        let invocation = PluginInvocation::inline_json("health", b"{}");
        let result = AlwaysErrors.handle(&invocation).await;
        assert!(result.is_error());
    }

    #[tokio::test]
    async fn dispatch_rejects_unsupported_protocol_version_before_calling_handle() {
        let mut invocation = PluginInvocation::inline_json("health", b"{}");
        invocation.protocol_version = u32::MAX;

        let result = dispatch(&AlwaysErrors, invocation).await;
        match result {
            PluginResult::Error { code, .. } => {
                assert_eq!(code, "PLUGIN_PROTOCOL_VERSION_UNSUPPORTED")
            }
            PluginResult::Success { .. } => panic!("unsupported protocol unexpectedly succeeded"),
        }
    }

    #[tokio::test]
    async fn dispatch_calls_handle_for_supported_protocol_version() {
        let invocation = PluginInvocation::inline_json("health", b"{}");
        let result = dispatch(&Echo, invocation).await;
        assert!(result.is_success());
    }

    #[tokio::test]
    async fn read_invocation_parses_a_well_formed_invocation() {
        let invocation = PluginInvocation::inline_json("health", b"{}");
        let bytes = serde_json::to_vec(&invocation).expect("invocation should serialize");

        let parsed = read_invocation(bytes.as_slice())
            .await
            .expect("well-formed invocation should parse");
        assert_eq!(parsed, invocation);
    }

    #[tokio::test]
    async fn read_invocation_rejects_input_over_the_size_cap() {
        // Oversized but otherwise well-formed JSON, so the failure can only
        // be attributed to the size cap, not a parse error.
        let padding = "a".repeat(MAX_INVOCATION_BYTES + 1);
        let oversized = format!(r#"{{"padding":"{padding}"}}"#);

        let err = read_invocation(oversized.as_bytes())
            .await
            .expect_err("oversized invocation must be rejected");
        assert!(err.contains("maximum size"));
    }

    #[tokio::test]
    async fn read_invocation_accepts_input_at_exactly_the_size_cap() {
        // The cap is inclusive: exactly MAX_INVOCATION_BYTES of valid JSON
        // must still parse, only content that exceeds it is rejected. Pad
        // inside a string field's value so the result stays valid JSON.
        let prefix = r#"{"protocolVersion":1,"operation":"health","params":{"inline":""#;
        let suffix = r#""}}"#;
        let padding_len = MAX_INVOCATION_BYTES - prefix.len() - suffix.len();
        let padded = format!("{prefix}{}{suffix}", "a".repeat(padding_len));
        assert_eq!(padded.len(), MAX_INVOCATION_BYTES);

        // The padded JSON doesn't match PluginInvocation's real shape
        // (params.inline expects base64, not arbitrary text), so parsing it
        // fails on content, not on size — proving the cap itself let it
        // through.
        let err = read_invocation(padded.as_bytes())
            .await
            .expect_err("malformed params should fail to parse");
        assert!(!err.contains("maximum size"));
    }
}
