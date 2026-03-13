use crate::error::{Error, ErrorData};
use alien_core::{MessagePayload, QueueMessage, StorageEvent, StorageEventType, StorageEvents};
use alien_error::{AlienError, Context, IntoAlienError};
use base64::{engine::general_purpose, Engine};
use chrono::{DateTime, Utc};
use cloudevents::AttributesReader;
use cloudevents::{event::ExtensionValue, Data, Event};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::fmt;

// Visitor to handle string or number deserialization
struct StringOrNumberVisitor;

impl<'de> Visitor<'de> for StringOrNumberVisitor {
    type Value = String;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a string or a number")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(value.to_string())
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(value)
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(value.to_string())
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(value.to_string())
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Err(de::Error::custom(
            "Expected string or number, found null/None",
        ))
    }
}

// Deserializer function for String
fn deserialize_string_or_number<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(StringOrNumberVisitor)
}

// Deserializer function for Option<String>
fn deserialize_optional_string_or_number<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    struct Wrapper(#[serde(deserialize_with = "deserialize_string_or_number")] String);

    Option::<Wrapper>::deserialize(deserializer).map(|opt_wrapped| opt_wrapped.map(|w| w.0))
}

/// GCP Pub/Sub message structure from CloudEvents
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PubSubMessage {
    /// Base64-encoded message data
    pub data: String,
    /// Message ID
    pub message_id: String,
    /// Message publish time
    pub publish_time: Option<DateTime<Utc>>,
    /// Message attributes
    #[serde(default)]
    pub attributes: HashMap<String, String>,
}

/// GCP Pub/Sub CloudEvent data structure
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PubSubCloudEventData {
    /// The message that was published
    pub message: PubSubMessage,
    /// The subscription that received the message
    pub subscription: String,
}

/// An object within Google Cloud Storage.
/// Based on google.events.cloud.storage.v1.StorageObjectData
/// Using serde instead of prost for JSON handling.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct StorageObjectData {
    pub content_encoding: Option<String>,
    pub content_disposition: Option<String>,
    pub cache_control: Option<String>,
    pub content_language: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub metageneration: Option<String>,
    pub time_deleted: Option<DateTime<Utc>>,
    pub content_type: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub size: Option<String>,
    pub time_created: Option<DateTime<Utc>>,
    pub crc32c: Option<String>,
    pub component_count: Option<i32>,
    pub md5_hash: Option<String>,
    pub etag: Option<String>,
    pub updated: Option<DateTime<Utc>>,
    pub storage_class: Option<String>,
    pub kms_key_name: Option<String>,
    pub time_storage_class_updated: Option<DateTime<Utc>>,
    pub temporary_hold: Option<bool>,
    pub retention_expiration_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    pub event_based_hold: Option<bool>,
    pub name: Option<String>,
    pub id: Option<String>,
    pub bucket: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_number")]
    pub generation: Option<String>,
    pub customer_encryption: Option<CustomerEncryption>,
    pub media_link: Option<String>,
    pub self_link: Option<String>,
    pub kind: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct CustomerEncryption {
    pub encryption_algorithm: Option<String>,
    pub key_sha256: Option<String>,
}

// Helper to parse size string (which might have originated as a number)
fn parse_gcp_size(size_str: Option<String>) -> Result<Option<u64>, Error> {
    match size_str {
        Some(s) => s.parse::<u64>().map(Some).into_alien_error().context(
            ErrorData::EventProcessingFailed {
                event_type: "GCP Storage Event".to_string(),
                reason: format!("Failed to parse size '{}'", s),
            },
        ),
        None => Ok(None),
    }
}

/// Extract topic/queue name from GCP subscription path
fn extract_topic_name_from_subscription(subscription: &str) -> String {
    // Extract topic name from subscription path like:
    // "projects/my-project/subscriptions/my-queue-sub" -> "my-queue"
    // We'll use a simple heuristic: remove "-sub" suffix if present
    let parts: Vec<&str> = subscription.split('/').collect();
    if let Some(sub_name) = parts.last() {
        if sub_name.ends_with("-sub") {
            sub_name.trim_end_matches("-sub").to_string()
        } else {
            sub_name.to_string()
        }
    } else {
        subscription.to_string()
    }
}

/// Extract topic name from CloudEvent source
fn extract_topic_name_from_source(source: &str) -> String {
    // Extract from: "//pubsub.googleapis.com/projects/my-project/topics/my-queue"
    let parts: Vec<&str> = source.split('/').collect();
    if let Some(topic_name) = parts.last() {
        topic_name.to_string()
    } else {
        source.to_string()
    }
}

/// Extract topic name from CloudEvent source (handling the actual CloudEvent source type)
fn extract_topic_name_from_ce_source<T: AsRef<str>>(source: T) -> String {
    extract_topic_name_from_source(source.as_ref())
}

// Convert Pub/Sub CloudEvents to QueueMessage
pub fn pubsub_cloudevent_to_queue_messages(event: Event) -> Result<Vec<QueueMessage>, Error> {
    let event_type_str = event.ty();

    // Only handle Pub/Sub events
    if event_type_str != "google.cloud.pubsub.topic.v1.messagePublished" {
        return Err(AlienError::new(ErrorData::EventProcessingFailed {
            event_type: event_type_str.to_string(),
            reason: "Not a Pub/Sub message published event".to_string(),
        }));
    }

    let timestamp = event.time().cloned().ok_or_else(|| {
        AlienError::new(ErrorData::EventProcessingFailed {
            event_type: "GCP Pub/Sub Event".to_string(),
            reason: "CloudEvent missing timestamp".to_string(),
        })
    })?;

    let data = event.data().ok_or_else(|| {
        AlienError::new(ErrorData::EventProcessingFailed {
            event_type: "GCP Pub/Sub Event".to_string(),
            reason: "CloudEvent missing data payload".to_string(),
        })
    })?;

    let expected_content_type = event.datacontenttype();

    let pubsub_data: PubSubCloudEventData = match data {
        Data::Json(value) => serde_json::from_value(value.clone())
            .into_alien_error()
            .context(ErrorData::EventProcessingFailed {
                event_type: event_type_str.to_string(),
                reason: "Failed to decode JSON CloudEvent data".to_string(),
            })?,
        Data::Binary(bytes) => {
            if expected_content_type == Some("application/json") {
                serde_json::from_slice(bytes.as_slice())
                    .into_alien_error()
                    .context(ErrorData::EventProcessingFailed {
                        event_type: event_type_str.to_string(),
                        reason: "Failed to parse JSON from binary CloudEvent data".to_string(),
                    })?
            } else {
                return Err(AlienError::new(ErrorData::EventProcessingFailed {
                    event_type: event_type_str.to_string(),
                    reason: format!(
                        "Unsupported binary CloudEvent data content type: {:?}",
                        expected_content_type
                    ),
                }));
            }
        }
        Data::String(s) => {
            if expected_content_type == Some("application/json") {
                serde_json::from_str(s.as_str())
                    .into_alien_error()
                    .context(ErrorData::EventProcessingFailed {
                        event_type: event_type_str.to_string(),
                        reason: "Failed to parse JSON from string CloudEvent data".to_string(),
                    })?
            } else {
                return Err(AlienError::new(ErrorData::EventProcessingFailed {
                    event_type: event_type_str.to_string(),
                    reason: format!(
                        "Unsupported string CloudEvent data content type: {:?}",
                        expected_content_type
                    ),
                }));
            }
        }
    };

    // Decode base64 message data
    let message_bytes = general_purpose::STANDARD
        .decode(&pubsub_data.message.data)
        .into_alien_error()
        .context(ErrorData::EventProcessingFailed {
            event_type: event_type_str.to_string(),
            reason: "Failed to decode base64 message data".to_string(),
        })?;

    let message_text = String::from_utf8(message_bytes)
        .into_alien_error()
        .context(ErrorData::EventProcessingFailed {
            event_type: event_type_str.to_string(),
            reason: "Message data is not valid UTF-8".to_string(),
        })?;

    // Try to parse as JSON, fall back to Text
    let payload = match serde_json::from_str::<serde_json::Value>(&message_text) {
        Ok(json_value) => MessagePayload::Json(json_value),
        Err(_) => MessagePayload::Text(message_text),
    };

    // Extract source queue name from subscription or CloudEvent source
    let source = if !pubsub_data.subscription.is_empty() {
        extract_topic_name_from_subscription(&pubsub_data.subscription)
    } else {
        extract_topic_name_from_ce_source(event.source())
    };

    // Use message publish time if available, otherwise CloudEvent time
    let msg_timestamp = pubsub_data.message.publish_time.unwrap_or(timestamp);

    let queue_message = QueueMessage {
        id: pubsub_data.message.message_id,
        payload,
        receipt_handle: event.id().to_string(), // Use CloudEvent ID as receipt handle
        timestamp: msg_timestamp,
        source,
        attributes: pubsub_data.message.attributes,
        attempt_count: None, // Pub/Sub doesn't provide attempt count in push mode
    };

    Ok(vec![queue_message])
}

// Convert Storage CloudEvents to StorageEvents
pub fn storage_cloudevent_to_storage_events(event: Event) -> Result<StorageEvents, Error> {
    let event_type_str = event.ty();

    // Only handle storage events
    if !event_type_str.starts_with("google.cloud.storage.object.v1.") {
        return Err(AlienError::new(ErrorData::EventProcessingFailed {
            event_type: event_type_str.to_string(),
            reason: "Not a GCP Storage event".to_string(),
        }));
    }

    let timestamp = event.time().cloned().ok_or_else(|| {
        AlienError::new(ErrorData::EventProcessingFailed {
            event_type: "GCP Storage Event".to_string(),
            reason: "CloudEvent missing timestamp".to_string(),
        })
    })?;

    let data = event.data().ok_or_else(|| {
        AlienError::new(ErrorData::EventProcessingFailed {
            event_type: "GCP Storage Event".to_string(),
            reason: "CloudEvent missing data payload".to_string(),
        })
    })?;

    let expected_content_type = event.datacontenttype();

    let storage_object_data: StorageObjectData = match data {
        Data::Json(value) => serde_json::from_value(value.clone())
            .into_alien_error()
            .context(ErrorData::EventProcessingFailed {
                event_type: event_type_str.to_string(),
                reason: "Failed to decode JSON CloudEvent data".to_string(),
            })?,
        Data::Binary(bytes) => {
            if expected_content_type == Some("application/json") {
                serde_json::from_slice(bytes.as_slice())
                    .into_alien_error()
                    .context(ErrorData::EventProcessingFailed {
                        event_type: event_type_str.to_string(),
                        reason: "Failed to parse JSON from binary CloudEvent data".to_string(),
                    })?
            } else {
                return Err(AlienError::new(ErrorData::EventProcessingFailed {
                    event_type: event_type_str.to_string(),
                    reason: format!(
                        "Unsupported binary CloudEvent data content type: {:?}",
                        expected_content_type
                    ),
                }));
            }
        }
        Data::String(s) => {
            if expected_content_type == Some("application/json") {
                serde_json::from_str(s.as_str())
                    .into_alien_error()
                    .context(ErrorData::EventProcessingFailed {
                        event_type: event_type_str.to_string(),
                        reason: "Failed to parse JSON from string CloudEvent data".to_string(),
                    })?
            } else {
                return Err(AlienError::new(ErrorData::EventProcessingFailed {
                    event_type: event_type_str.to_string(),
                    reason: format!(
                        "Unsupported string CloudEvent data content type: {:?}",
                        expected_content_type
                    ),
                }));
            }
        }
    };

    let alien_event_type = match event_type_str {
        "google.cloud.storage.object.v1.finalized" => StorageEventType::Created,
        "google.cloud.storage.object.v1.archived" => StorageEventType::TierChanged,
        "google.cloud.storage.object.v1.deleted" => StorageEventType::Deleted,
        "google.cloud.storage.object.v1.metadataUpdated" => StorageEventType::MetadataUpdated,
        _ => {
            return Err(AlienError::new(ErrorData::EventProcessingFailed {
                event_type: event_type_str.to_string(),
                reason: "Unsupported event type".to_string(),
            }));
        }
    };

    let bucket_name = storage_object_data.bucket.clone().ok_or_else(|| {
        AlienError::new(ErrorData::EventProcessingFailed {
            event_type: event_type_str.to_string(),
            reason: "Missing field: data.bucket".to_string(),
        })
    })?;
    let object_key = storage_object_data.name.clone().ok_or_else(|| {
        AlienError::new(ErrorData::EventProcessingFailed {
            event_type: event_type_str.to_string(),
            reason: "Missing field: data.name".to_string(),
        })
    })?;

    let size = parse_gcp_size(storage_object_data.size.clone())?;

    // Extract region if available (might be in extensions)
    let region = event.extension("datacontenttype").and_then(|v| match v {
        ExtensionValue::String(s) => Some(s.clone()),
        _ => None,
    });

    let storage_event = StorageEvent {
        event_type: alien_event_type,
        bucket_name,
        object_key,
        timestamp,
        size,
        etag: storage_object_data.etag.clone(),
        content_type: storage_object_data.content_type.clone(),
        metadata: storage_object_data.metadata.clone(),
        copy_source: None,
        previous_tier: None,
        current_tier: storage_object_data.storage_class.clone(),
        region,
        version_id: storage_object_data.generation.clone(),
    };

    Ok(StorageEvents(vec![storage_event]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose, Engine};
    use cloudevents::{EventBuilder as _, EventBuilderV10};
    use serde_json::json;

    fn parse_datetime(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn test_try_from_pubsub_cloudevent() {
        let event_time_str = "2023-01-01T12:00:00Z";
        let event_time = parse_datetime(event_time_str);
        let publish_time_str = "2023-01-01T11:55:00Z";
        let publish_time = parse_datetime(publish_time_str);

        let message_data = "Hello World!";
        let encoded_data = general_purpose::STANDARD.encode(message_data);

        let event_data = json!({
            "message": {
                "data": encoded_data,
                "messageId": "msg-12345",
                "publishTime": publish_time_str,
                "attributes": {
                    "source": "order-service",
                    "priority": "high"
                }
            },
            "subscription": "projects/my-project/subscriptions/my-queue-sub"
        });

        let cloud_event = EventBuilderV10::new()
            .id("ce-id-12345")
            .ty("google.cloud.pubsub.topic.v1.messagePublished")
            .source("//pubsub.googleapis.com/projects/my-project/topics/my-queue")
            .time(event_time)
            .data("application/json", event_data)
            .build()
            .unwrap();

        let queue_messages: Vec<QueueMessage> =
            pubsub_cloudevent_to_queue_messages(cloud_event).unwrap();

        assert_eq!(queue_messages.len(), 1);
        let msg = &queue_messages[0];

        assert_eq!(msg.id, "msg-12345");
        assert_eq!(msg.receipt_handle, "ce-id-12345");
        assert_eq!(msg.source, "my-queue"); // Extracted from subscription
        assert_eq!(msg.timestamp, publish_time); // Uses message publish time
        assert!(msg.attempt_count.is_none()); // Not available in push mode

        // Verify payload
        match &msg.payload {
            MessagePayload::Text(text) => {
                assert_eq!(text, message_data);
            }
            _ => panic!("Expected Text payload, got {:?}", msg.payload),
        }

        // Verify attributes
        assert_eq!(
            msg.attributes.get("source"),
            Some(&"order-service".to_string())
        );
        assert_eq!(msg.attributes.get("priority"), Some(&"high".to_string()));
    }

    #[test]
    fn test_try_from_pubsub_json_payload() {
        let event_time_str = "2023-01-01T12:00:00Z";
        let event_time = parse_datetime(event_time_str);

        let json_payload = json!({"orderId": "order-123", "amount": 50.0});
        let message_data = serde_json::to_string(&json_payload).unwrap();
        let encoded_data = general_purpose::STANDARD.encode(&message_data);

        let event_data = json!({
            "message": {
                "data": encoded_data,
                "messageId": "msg-json-456",
                "publishTime": event_time_str,
                "attributes": {}
            },
            "subscription": "projects/my-project/subscriptions/orders-sub"
        });

        let cloud_event = EventBuilderV10::new()
            .id("ce-json-456")
            .ty("google.cloud.pubsub.topic.v1.messagePublished")
            .source("//pubsub.googleapis.com/projects/my-project/topics/orders")
            .time(event_time)
            .data("application/json", event_data)
            .build()
            .unwrap();

        let queue_messages: Vec<QueueMessage> =
            pubsub_cloudevent_to_queue_messages(cloud_event).unwrap();
        let msg = &queue_messages[0];

        // Verify JSON payload
        match &msg.payload {
            MessagePayload::Json(json_value) => {
                assert_eq!(json_value["orderId"], "order-123");
                assert_eq!(json_value["amount"], 50.0);
            }
            _ => panic!("Expected Json payload, got {:?}", msg.payload),
        }

        assert_eq!(msg.source, "orders"); // Extracted from subscription
    }

    #[test]
    fn test_try_from_storage_cloudevent() {
        let event_time_str = "2020-04-23T07:38:57.230Z";
        let event_time = parse_datetime(event_time_str);
        let event_data = json!({
          "bucket": "sample-bucket",
          "contentType": "text/plain",
          "crc32c": "rTVTeQ==",
          "etag": "CNHZkbuF/ugCEAE=",
          "generation": "1587627537231057",
          "id": "sample-bucket/folder/Test.cs/1587627537231057",
          "kind": "storage#object",
          "md5Hash": "kF8MuJ5+CTJxvyhHS1xzRg==",
          "name": "folder/Test.cs",
          "size": "352",
          "storageClass": "MULTI_REGIONAL",
          "timeCreated": event_time_str,
          "updated": event_time_str
        });

        let cloud_event = EventBuilderV10::new()
            .id("test-storage-id")
            .ty("google.cloud.storage.object.v1.finalized")
            .source("//storage.googleapis.com/projects/_/buckets/sample-bucket")
            .time(event_time)
            .data("application/json", event_data)
            .build()
            .unwrap();

        let storage_events: StorageEvents =
            storage_cloudevent_to_storage_events(cloud_event).unwrap();

        assert_eq!(storage_events.0.len(), 1);
        let event = &storage_events.0[0];

        assert_eq!(event.event_type, StorageEventType::Created);
        assert_eq!(event.bucket_name, "sample-bucket");
        assert_eq!(event.object_key, "folder/Test.cs");
        assert_eq!(event.timestamp, event_time);
        assert_eq!(event.size, Some(352));
        assert_eq!(event.etag, Some("CNHZkbuF/ugCEAE=".to_string()));
        assert_eq!(event.content_type, Some("text/plain".to_string()));
        assert_eq!(event.current_tier, Some("MULTI_REGIONAL".to_string()));
        assert_eq!(event.version_id, Some("1587627537231057".to_string()));
    }

    #[test]
    fn test_extract_topic_name_from_subscription() {
        assert_eq!(
            extract_topic_name_from_subscription("projects/my-project/subscriptions/my-queue-sub"),
            "my-queue"
        );
        assert_eq!(
            extract_topic_name_from_subscription("projects/my-project/subscriptions/orders-sub"),
            "orders"
        );
        assert_eq!(
            extract_topic_name_from_subscription("my-queue-sub"),
            "my-queue"
        );
        assert_eq!(
            extract_topic_name_from_subscription("no-sub-suffix"),
            "no-sub-suffix"
        );
    }

    #[test]
    fn test_extract_topic_name_from_source() {
        assert_eq!(
            extract_topic_name_from_source(
                "//pubsub.googleapis.com/projects/my-project/topics/my-queue"
            ),
            "my-queue"
        );
        assert_eq!(
            extract_topic_name_from_source(
                "//pubsub.googleapis.com/projects/my-project/topics/orders"
            ),
            "orders"
        );
    }

    #[test]
    fn test_try_from_unsupported_event_type() {
        let event_time = Utc::now();
        let cloud_event = EventBuilderV10::new()
            .id("test-unsupported")
            .ty("google.cloud.firestore.document.v1.created") // Not supported
            .source("//firestore.googleapis.com/projects/my-project")
            .time(event_time)
            .data("application/json", json!({}))
            .build()
            .unwrap();

        // Try both conversions - both should fail
        let queue_result = pubsub_cloudevent_to_queue_messages(cloud_event.clone());
        assert!(queue_result.is_err());

        let storage_result = storage_cloudevent_to_storage_events(cloud_event);
        assert!(storage_result.is_err());
    }
}
