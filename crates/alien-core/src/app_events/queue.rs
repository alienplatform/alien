use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// JSON-first message payload that supports both structured JSON and UTF-8 text
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum MessagePayload {
    /// JSON-serializable value
    Json(serde_json::Value),
    /// UTF-8 text payload
    Text(String),
}

/// Standardized queue message structure used by alien-runtime
///
/// This structure contains commonly available metadata across all platforms
/// and a JSON-first payload that handles both structured data and plain text.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct QueueMessage {
    /// Unique message identifier (from messageId/ce-id)
    pub id: String,

    /// JSON-first message payload
    pub payload: MessagePayload,

    /// Platform-specific receipt handle for acknowledgment
    pub receipt_handle: String,

    /// Message timestamp (from SentTimestamp/ce-time/enqueuedTimeUtc)
    pub timestamp: DateTime<Utc>,

    /// Source queue/topic name (derived from ARN/source/topic)
    pub source: String,

    /// Message attributes/properties (flattened from messageAttributes/attributes/properties)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub attributes: HashMap<String, String>,

    /// Delivery attempt count (from ApproximateReceiveCount/deliveryCount, if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt_count: Option<u32>,
}
