//! Post-write verification — confirming a mutating or destructive operation
//! actually took effect, by polling a read-only operation on the same
//! plugin until a success condition is met.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::manifest::RetryPolicy;

/// How to confirm an operation's effect after it reports success.
///
/// The runtime calls `pollOperation` (a read-only operation on the same
/// plugin), extracts `successField` from its JSON result, and compares it
/// against `successValue`. It retries per `retry` until `timeoutSeconds`
/// elapses, at which point verification is reported as failed (the write
/// itself is not undone — verification only confirms, it never rolls back).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Verification {
    /// Human-readable description of what changes as a result of this
    /// operation, surfaced in access-request prompts.
    pub changes: String,
    /// The read-only operation (declared by the same plugin) to poll for
    /// confirmation.
    pub poll_operation: String,
    /// Parameters to pass to `pollOperation`, keyed by the poll operation's
    /// param name and valued by a dot-path into the triggering operation's
    /// own result (e.g. `{"podName": "pod.name"}`).
    #[serde(default)]
    pub poll_params_from_result: BTreeMap<String, String>,
    /// Dot-path into the poll operation's result to compare.
    pub success_field: String,
    /// The value `successField` must equal for verification to pass.
    pub success_value: String,
    /// How to retry the poll while waiting for the success condition.
    #[serde(default)]
    pub retry: Option<RetryPolicy>,
    /// Give up and report verification as failed after this many seconds.
    pub timeout_seconds: u32,
}
