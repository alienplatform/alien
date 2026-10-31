//! Errors returned by application bindings, Worker APIs and storage event parsing.

use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

pub use alien_bindings::error::{Error, ErrorData, Result};

/// Errors decoding portable storage events from queue messages.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageEventErrorData {
    /// The queue payload does not conform to the portable storage event schema.
    #[error(
        code = "INVALID_STORAGE_EVENT_MESSAGE",
        message = "Queue payload is not a valid StorageEvent: {message}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    InvalidStorageEventMessage {
        /// Description of the decoding operation, without including the payload.
        message: String,
    },
}
