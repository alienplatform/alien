//! Error types for the launcher.
//!
//! The launcher is a local supervisor with no HTTP API of its own, so these
//! errors are for logs and for classification by the update state machine
//! (promote / rollback / retry decisions) — not for API responses.

use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Convenient type alias for this crate's Result type.
#[allow(dead_code)] // consumed once the version store and state machine land
pub type Result<T> = alien_error::Result<T, ErrorData>;

#[allow(dead_code)] // variants are constructed by upcoming core tasks
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// The on-disk version store is in a state the startup classification
    /// cannot map to a recovery action (missing pointers, unparseable markers).
    #[error(
        code = "STORE_CORRUPT",
        message = "Version store at '{path}' is corrupt: {message}",
        retryable = "false",
        internal = "true"
    )]
    StoreCorrupt { path: String, message: String },

    /// Spawning the operator binary failed (missing file, exec error).
    #[error(
        code = "SPAWN_FAILED",
        message = "Failed to spawn operator '{binary_path}': {message}",
        retryable = "false",
        internal = "true"
    )]
    SpawnFailed {
        binary_path: String,
        message: String,
    },

    /// Copying `state/` to (or restoring it from) a pre-swap snapshot failed.
    #[error(
        code = "SNAPSHOT_FAILED",
        message = "State snapshot operation failed: {message}",
        retryable = "true",
        internal = "true"
    )]
    SnapshotFailed { message: String },

    /// The health probe could not be issued (bad address, client error) —
    /// distinct from a healthy "not ready yet" 503, which is not an error.
    #[error(
        code = "PROBE_FAILED",
        message = "Health probe against '{url}' failed: {message}",
        retryable = "true",
        internal = "true"
    )]
    ProbeFailed { url: String, message: String },

    /// Not enough free disk space to proceed safely (snapshot / staging).
    /// An out-of-space condition must abort the attempt cleanly, never corrupt.
    #[error(
        code = "DISK_SPACE",
        message = "Insufficient disk space: need {required_bytes} bytes, {available_bytes} available. {message}",
        retryable = "false",
        internal = "true"
    )]
    DiskSpace {
        required_bytes: u64,
        available_bytes: u64,
        message: String,
    },

    /// Generic catch-all for uncommon launcher errors.
    #[error(
        code = "LAUNCHER_ERROR",
        message = "Launcher operation failed: {message}",
        retryable = "true",
        internal = "true"
    )]
    Other { message: String },
}
