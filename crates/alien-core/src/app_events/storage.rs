use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// Represents the type of storage event that occurred.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum StorageEventType {
    /// An object was created (e.g., uploaded, put).
    Created,
    /// An object was deleted.
    Deleted,
    /// An object was copied.
    Copied,
    /// An object's metadata was updated.
    MetadataUpdated,
    /// An object was restored from an archive tier.
    Restored,
    /// An object's storage tier was changed.
    TierChanged,
    /// An unknown or unsupported storage event type.
    Unknown,
}

/// Represents an event triggered by an action in an object storage service.
///
/// This struct provides a generic representation for events from services like
/// AWS S3, Google Cloud Storage (GCS), and Azure Blob Storage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StorageEvent {
    /// The type of storage event.
    pub event_type: StorageEventType,

    /// The name of the bucket or container where the event occurred.
    pub bucket_name: String,

    /// The key or path of the object involved in the event.
    pub object_key: String,

    /// The timestamp when the event occurred.
    pub timestamp: DateTime<Utc>,

    /// Optional size of the object in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,

    /// Optional ETag or hash of the object content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,

    /// Optional content type (MIME type) of the object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,

    /// Optional metadata associated with the object.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,

    /// Optional information about the source object for copy events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copy_source: Option<String>,

    /// Optional previous storage tier for TierChanged or Restored events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_tier: Option<String>,

    /// Optional current storage tier for TierChanged or Restored events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_tier: Option<String>,

    /// Optional region where the event originated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,

    /// Optional version or sequencer identifier for the event or object state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
}

/// A wrapper type for a list of storage events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StorageEvents(pub Vec<StorageEvent>);
