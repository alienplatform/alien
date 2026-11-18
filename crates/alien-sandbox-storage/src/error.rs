use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Why a persistent storage operation failed.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// A storage key is not one path segment of the allowed characters.
    #[error(
        code = "SANDBOX_STORAGE_KEY_INVALID",
        message = "Storage key '{key}' is invalid: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    KeyInvalid {
        /// The refused key.
        key: String,
        /// What is wrong with it.
        reason: String,
    },

    /// A path inside the stored tree is not a clean relative path.
    #[error(
        code = "SANDBOX_STORAGE_PATH_INVALID",
        message = "Path '{path}' is invalid: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    PathInvalid {
        /// The refused path.
        path: String,
        /// What is wrong with it.
        reason: String,
    },

    /// Another sandbox moved the key's HEAD since this writer last did, so this writer was
    /// superseded and its commit was refused whole.
    #[error(
        code = "SANDBOX_STORAGE_SUPERSEDED",
        message = "Storage key '{key}' has a newer writer; this sandbox's commit was refused",
        retryable = "false",
        internal = "false",
        http_status_code = 409
    )]
    Superseded {
        /// The key whose HEAD moved.
        key: String,
    },

    /// A commit would make the tree larger than the declared `maxSize`; nothing was written.
    #[error(
        code = "SANDBOX_STORAGE_MAX_SIZE_EXCEEDED",
        message = "The tree is {size} bytes, above the declared maxSize of {max_size} bytes; the commit was refused whole",
        retryable = "false",
        internal = "false",
        http_status_code = 413
    )]
    MaxSizeExceeded {
        /// Bytes the tree would hold.
        size: u64,
        /// The declared ceiling in bytes.
        max_size: u64,
    },

    /// A stored object is missing or does not parse; the key's data is not readable as written.
    #[error(
        code = "SANDBOX_STORAGE_CORRUPT",
        message = "Stored object '{object}' is unreadable: {reason}",
        retryable = "false",
        internal = "true"
    )]
    Corrupt {
        /// The object, relative to the key.
        object: String,
        /// What is wrong with it.
        reason: String,
    },

    /// A file was not found in the key's current tree.
    #[error(
        code = "SANDBOX_STORAGE_FILE_NOT_FOUND",
        message = "'{path}' is not a file in storage key '{key}'",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    FileNotFound {
        /// The key that was read.
        key: String,
        /// The path that was asked for.
        path: String,
    },

    /// The object store failed an operation.
    #[error(
        code = "SANDBOX_STORAGE_STORE_FAILED",
        message = "Object store {operation} on '{object}' failed: {reason}",
        retryable = "true",
        internal = "true"
    )]
    StoreFailed {
        /// get, put, list or delete.
        operation: String,
        /// The object, relative to the key.
        object: String,
        /// What the store said.
        reason: String,
    },

    /// Reading the tree to commit, or writing a restored tree, failed.
    #[error(
        code = "SANDBOX_STORAGE_TREE_IO_FAILED",
        message = "Could not {operation} '{path}': {reason}",
        retryable = "true",
        internal = "true"
    )]
    TreeIo {
        /// read or write.
        operation: String,
        /// The path inside the tree.
        path: String,
        /// The I/O error.
        reason: String,
    },
}

pub type Result<T> = alien_error::Result<T, ErrorData>;
