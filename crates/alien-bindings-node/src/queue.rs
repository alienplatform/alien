//! Queue binding handle. Thin argument/error translation over the `Queue`
//! trait, including payload (JSON/text) marshalling.

use crate::error::map_alien_error;
use alien_bindings::error::ErrorData;
use alien_bindings::traits::{MessagePayload, QueueMessage};
use alien_bindings::BoundQueue;
use alien_error::AlienError;
use napi_derive::napi;

/// A message received from a queue.
///
/// The payload is a single string; `payload_type` (`"json"` | `"text"`) says how
/// to read it — serialized JSON for `"json"`, raw text for `"text"`.
#[napi(object)]
pub struct QueueMessageJs {
    /// Payload discriminant: `"json"` or `"text"`.
    pub payload_type: String,
    /// The payload string: serialized JSON when `payload_type == "json"`, raw
    /// text when `payload_type == "text"`.
    pub payload: String,
    /// Opaque receipt handle for ack/nack.
    pub receipt_handle: String,
    /// Delivery attempt for this message, 1-based (1 = first delivery), so
    /// handlers can enforce retry limits.
    pub attempt: u32,
}

/// Parse a JSON string argument into a `MessagePayload::Json`. Invalid JSON maps
/// to a `SERIALIZATION_FAILED` error.
fn parse_json_payload(json_string: &str) -> napi::Result<MessagePayload> {
    let value: serde_json::Value = serde_json::from_str(json_string).map_err(|e| {
        map_alien_error(AlienError::new(ErrorData::SerializationFailed {
            message: e.to_string(),
        }))
    })?;
    Ok(MessagePayload::Json(value))
}

/// Translate a received `QueueMessage` into its JS shape. Re-serializing the
/// JSON payload can fail (mapped to `SERIALIZATION_FAILED`).
fn message_to_js(message: QueueMessage) -> napi::Result<QueueMessageJs> {
    let QueueMessage {
        payload,
        receipt_handle,
        attempt,
    } = message;
    match payload {
        MessagePayload::Json(value) => {
            let payload = serde_json::to_string(&value).map_err(|e| {
                map_alien_error(AlienError::new(ErrorData::SerializationFailed {
                    message: e.to_string(),
                }))
            })?;
            Ok(QueueMessageJs {
                payload_type: "json".to_string(),
                payload,
                receipt_handle,
                attempt,
            })
        }
        MessagePayload::Text(text) => Ok(QueueMessageJs {
            payload_type: "text".to_string(),
            payload: text,
            receipt_handle,
            attempt,
        }),
    }
}

/// Handle to a resolved queue binding.
#[napi]
pub struct QueueHandle {
    inner: BoundQueue,
}

impl QueueHandle {
    pub(crate) fn new(inner: BoundQueue) -> Self {
        Self { inner }
    }
}

#[napi]
impl QueueHandle {
    /// Send a JSON message. `json_string` must be valid JSON.
    #[napi]
    pub async fn send_json(&self, json_string: String) -> napi::Result<()> {
        let payload = parse_json_payload(&json_string)?;
        let inner = self.inner.clone();
        inner.send(payload).await.map_err(map_alien_error)
    }

    /// Send a text message.
    #[napi]
    pub async fn send_text(&self, text: String) -> napi::Result<()> {
        let inner = self.inner.clone();
        inner
            .send(MessagePayload::Text(text))
            .await
            .map_err(map_alien_error)
    }

    /// Receive up to `max` messages.
    #[napi]
    pub async fn receive(&self, max: u32) -> napi::Result<Vec<QueueMessageJs>> {
        let inner = self.inner.clone();
        let messages = inner.receive(max as usize).await.map_err(map_alien_error)?;
        messages.into_iter().map(message_to_js).collect()
    }

    /// Acknowledge a message by its receipt handle.
    #[napi]
    pub async fn ack(&self, receipt: String) -> napi::Result<()> {
        let inner = self.inner.clone();
        inner.ack(&receipt).await.map_err(map_alien_error)
    }

    /// Negative-acknowledge a message, making it immediately redeliverable.
    #[napi]
    pub async fn nack(&self, receipt: String) -> napi::Result<()> {
        let inner = self.inner.clone();
        inner.nack(&receipt).await.map_err(map_alien_error)
    }

    /// Delete every message in the queue.
    #[napi]
    pub async fn purge(&self) -> napi::Result<()> {
        let inner = self.inner.clone();
        inner.purge().await.map_err(map_alien_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_json_payload_accepts_valid_json() {
        let payload = parse_json_payload(r#"{"a":1}"#).expect("valid json should parse");
        match payload {
            MessagePayload::Json(value) => assert_eq!(value["a"], 1),
            MessagePayload::Text(_) => panic!("expected json payload"),
        }
    }

    #[test]
    fn parse_json_payload_rejects_invalid_json_as_serialization_error() {
        let err = parse_json_payload("not json").expect_err("invalid json should error");
        let envelope: serde_json::Value =
            serde_json::from_str(err.reason.as_str()).expect("reason is a JSON envelope");
        assert_eq!(envelope["code"], "SERIALIZATION_FAILED");
    }

    #[test]
    fn message_to_js_maps_json_payload() {
        let message = QueueMessage {
            payload: MessagePayload::Json(serde_json::json!({"k": "v"})),
            receipt_handle: "r1".to_string(),
            attempt: 1,
        };

        let js = message_to_js(message).expect("json message should translate");

        assert_eq!(js.payload_type, "json");
        assert_eq!(js.receipt_handle, "r1");
        assert_eq!(js.attempt, 1);
        let parsed: serde_json::Value =
            serde_json::from_str(&js.payload).expect("payload is valid json");
        assert_eq!(parsed["k"], "v");
    }

    #[test]
    fn message_to_js_maps_text_payload() {
        let message = QueueMessage {
            payload: MessagePayload::Text("hello".to_string()),
            receipt_handle: "r2".to_string(),
            attempt: 3,
        };

        let js = message_to_js(message).expect("text message should translate");

        assert_eq!(js.payload_type, "text");
        assert_eq!(js.payload, "hello");
        assert_eq!(js.receipt_handle, "r2");
        assert_eq!(js.attempt, 3);
    }
}
