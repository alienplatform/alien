//! Typed parsing for queue bindings and single or batched Worker deliveries.

use alien_error::{Context, IntoAlienError};

use crate::{
    error::StorageEventErrorData,
    traits::{MessagePayload, QueueMessage},
};

pub use alien_core::{StorageEvent, StorageEventType};

/// Result of parsing a portable storage notification.
pub type Result<T> = alien_error::Result<T, StorageEventErrorData>;

/// A queue message whose payload can be decoded as a portable storage event.
pub trait StorageEventMessage {
    /// Validate and decode the payload without acknowledging the delivery.
    fn storage_event(&self) -> Result<StorageEvent>;
}

/// Decode a queue notification, preserving its timestamp, key and optional metadata.
///
/// Accepts `queue.receive()` messages, Worker single messages, and messages in Worker
/// batches. Invalid JSON or invalid event fields return `INVALID_STORAGE_EVENT_MESSAGE`.
/// This does not acknowledge messages; acknowledge only after processing succeeds.
pub fn storage_event_from_message(message: &impl StorageEventMessage) -> Result<StorageEvent> {
    message.storage_event()
}

impl StorageEventMessage for QueueMessage {
    fn storage_event(&self) -> Result<StorageEvent> {
        match &self.payload {
            MessagePayload::Json(value) => serde_json::from_value(value.clone()),
            MessagePayload::Text(value) => serde_json::from_str(value),
        }
        .into_alien_error()
        .context(StorageEventErrorData::InvalidStorageEventMessage {
            message: "decoding queue binding payload".into(),
        })
    }
}

#[cfg(feature = "worker")]
fn decode_worker_payload(payload: &[u8], id: &str) -> Result<StorageEvent> {
    serde_json::from_slice(payload).into_alien_error().context(
        StorageEventErrorData::InvalidStorageEventMessage {
            message: format!("decoding Worker message '{id}'"),
        },
    )
}

#[cfg(feature = "worker")]
impl StorageEventMessage for crate::worker::QueueMessage {
    fn storage_event(&self) -> Result<StorageEvent> {
        decode_worker_payload(&self.payload, &self.id)
    }
}

#[cfg(feature = "worker")]
impl StorageEventMessage for alien_worker_protocol::control::QueueMessage {
    fn storage_event(&self) -> Result<StorageEvent> {
        decode_worker_payload(&self.payload, &self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn payload() -> serde_json::Value {
        json!({"eventType":"created", "bucketName":"uploads", "objectKey":"quote\"é*.json", "timestamp":"2026-10-30T09:00:00Z", "size":7, "etag":"version-1", "metadata":{"kind":"example"}})
    }

    #[test]
    fn binding_payloads_preserve_the_complete_event_and_receipt() {
        let value = payload();
        for payload in [
            MessagePayload::Json(value.clone()),
            MessagePayload::Text(value.to_string()),
        ] {
            let message = QueueMessage {
                payload,
                receipt_handle: "receipt".into(),
                attempt: 2,
            };
            let event = storage_event_from_message(&message).unwrap();
            assert_eq!(serde_json::to_value(event).unwrap(), value);
            assert_eq!(message.receipt_handle, "receipt");
            assert_eq!(message.attempt, 2);
        }
    }

    #[test]
    fn malformed_notifications_fail_with_structured_errors() {
        let mut invalid = vec!["not json".into(), "null".into(), "{}".into()];
        for (field, value) in [
            ("eventType", json!("typo")),
            ("timestamp", json!("today")),
            ("size", json!(-1)),
            ("size", json!(true)),
            ("objectKey", json!(42)),
            ("metadata", json!({"key":1})),
        ] {
            let mut message = payload();
            message[field] = value;
            invalid.push(message.to_string());
        }
        for value in invalid {
            let message = QueueMessage {
                payload: MessagePayload::Text(value),
                receipt_handle: "unacknowledged".into(),
                attempt: 1,
            };
            let error = storage_event_from_message(&message).unwrap_err();
            assert_eq!(error.code, "INVALID_STORAGE_EVENT_MESSAGE");
            assert!(!error.retryable);
            assert!(!error.internal);
            assert!(error.source.is_some());
            assert_eq!(message.receipt_handle, "unacknowledged");
        }
    }

    #[cfg(feature = "worker")]
    #[test]
    fn single_and_batched_worker_messages_parse_the_same_payload() {
        let value = payload();
        let single = crate::worker::QueueMessage {
            id: "single".into(),
            payload: value.to_string().into_bytes(),
            receipt_handle: "receipt".into(),
            source: "jobs".into(),
            attempt_count: 1,
        };
        let batch = alien_worker_protocol::control::QueueMessage {
            id: "batch".into(),
            payload: single.payload.clone(),
            ..Default::default()
        };
        assert_eq!(
            storage_event_from_message(&single).unwrap(),
            storage_event_from_message(&batch).unwrap()
        );
        assert_eq!(
            serde_json::to_value(storage_event_from_message(&batch).unwrap()).unwrap(),
            value
        );
    }
}
