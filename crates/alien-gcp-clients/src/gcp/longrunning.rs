//! Shared types for Google Cloud long-running operations.
//! These structs mirror the canonical Google APIs definitions and are re-exported
//! by individual service clients (Cloud Run, Service Usage, etc.).

use bon::Builder;
use serde::{Deserialize, Serialize};

/// Represents a long-running operation returned from many GCP APIs.
/// See https://cloud.google.com/apis/design/design_patterns#long_running_operations
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    /// Server-assigned name unique within the service that returns it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Service-specific metadata (progress information, timestamps, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,

    /// Whether the operation has completed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done: Option<bool>,

    /// The operation result – **either** `error` **or** `response` is set when `done == true`.
    #[serde(flatten)]
    pub result: Option<OperationResult>,
}

/// Result payload for an [`Operation`]. Exactly one variant is set when `done == true`.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum OperationResult {
    /// The operation failed and returned a structured error.
    Error { error: Status },
    /// The operation succeeded and returned a JSON response.
    Response { response: serde_json::Value },
}

/// Wire-compatible error type used by Google APIs (mirrors `google.rpc.Status`).
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// gRPC-style error code.
    pub code: i32,
    /// Developer-visible, English error message.
    pub message: String,
    /// Additional structured details (service-dependent).
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<serde_json::Value>,
}
