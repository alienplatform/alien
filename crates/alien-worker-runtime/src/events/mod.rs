//! Platform-specific event parsing for alien-worker-runtime
//!
//! This module contains event parsing logic that converts platform-specific
//! event formats (AWS Lambda events, GCP CloudEvents, Azure Dapr events) into
//! standardized alien-core event types.

#[cfg(any(feature = "gcp", feature = "azure"))]
use alien_error::{AlienError, Context, IntoAlienError};
#[cfg(any(feature = "gcp", feature = "azure"))]
use cloudevents::Data;

#[cfg(any(feature = "gcp", feature = "azure"))]
use crate::error::{Error, ErrorData};

#[cfg(feature = "aws")]
pub mod aws;
#[cfg(feature = "aws")]
pub use aws::*;

#[cfg(feature = "gcp")]
pub mod gcp;
#[cfg(feature = "gcp")]
pub use gcp::*;

#[cfg(feature = "azure")]
pub mod azure;
#[cfg(feature = "azure")]
pub use azure::*;

/// Decode a CloudEvent's `data` payload into `T`.
///
/// JSON data is decoded directly; binary and string data are accepted only
/// when the event's data content type is `application/json`. All CloudEvent
/// sources (GCP Pub/Sub, GCS, Azure Blob Storage) share this decode step.
#[cfg(any(feature = "gcp", feature = "azure"))]
pub(crate) fn decode_cloudevent_data<T: serde::de::DeserializeOwned>(
    data: &Data,
    content_type: Option<&str>,
    event_type: &str,
) -> Result<T, Error> {
    match data {
        Data::Json(value) => serde_json::from_value(value.clone())
            .into_alien_error()
            .context(ErrorData::EventProcessingFailed {
                event_type: event_type.to_string(),
                reason: "Failed to decode JSON CloudEvent data".to_string(),
            }),
        Data::Binary(bytes) => {
            if content_type == Some("application/json") {
                serde_json::from_slice(bytes.as_slice())
                    .into_alien_error()
                    .context(ErrorData::EventProcessingFailed {
                        event_type: event_type.to_string(),
                        reason: "Failed to parse JSON from binary CloudEvent data".to_string(),
                    })
            } else {
                Err(AlienError::new(ErrorData::EventProcessingFailed {
                    event_type: event_type.to_string(),
                    reason: format!(
                        "Unsupported binary CloudEvent data content type: {:?}",
                        content_type
                    ),
                }))
            }
        }
        Data::String(s) => {
            if content_type == Some("application/json") {
                serde_json::from_str(s.as_str())
                    .into_alien_error()
                    .context(ErrorData::EventProcessingFailed {
                        event_type: event_type.to_string(),
                        reason: "Failed to parse JSON from string CloudEvent data".to_string(),
                    })
            } else {
                Err(AlienError::new(ErrorData::EventProcessingFailed {
                    event_type: event_type.to_string(),
                    reason: format!(
                        "Unsupported string CloudEvent data content type: {:?}",
                        content_type
                    ),
                }))
            }
        }
    }
}
