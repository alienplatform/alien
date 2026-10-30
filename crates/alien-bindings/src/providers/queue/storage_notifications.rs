//! Provider notification decoding shared by pull bindings and Worker queue routes.

use crate::error::{ErrorData, Result};
use alien_core::{
    storage_notifications::{GCS_RULE_ATTRIBUTE, GCS_SUFFIX_ATTRIBUTE},
    MessagePayload, StorageEvent, StorageEventType,
};
use alien_error::{AlienError, Context, IntoAlienError};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::HashMap;
use tracing::warn;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GcsObject {
    bucket: String,
    name: String,
    size: Option<String>,
    etag: Option<String>,
    content_type: Option<String>,
    generation: Option<String>,
    storage_class: Option<String>,
    #[serde(default)]
    metadata: HashMap<String, String>,
}

fn invalid(reason: impl Into<String>) -> ErrorData {
    ErrorData::QueueProviderResponseInvalid {
        reason: reason.into(),
    }
}

/// How a consumer handles one Pub/Sub message read from a Queue.
#[derive(Debug, PartialEq)]
pub enum GcpQueueDelivery {
    /// Hand this payload to the consumer.
    Deliver(MessagePayload),
    /// A storage notification whose key misses its rule's suffix: acknowledge it unseen.
    Filtered,
}

/// Normalize GCS notifications created for a Storage's rules; every other message keeps
/// its body. Each message is decided on its own, so one bad message never fails a batch:
/// a notification that doesn't parse is delivered unchanged, where the consumer's
/// `StorageEvent` parsing reports it, instead of being dropped or redelivered forever.
pub fn gcp_queue_payload(data: &[u8], attributes: &HashMap<String, String>) -> GcpQueueDelivery {
    if !attributes.contains_key(GCS_RULE_ATTRIBUTE) {
        return GcpQueueDelivery::Deliver(ordinary_payload(data));
    }
    match gcs_storage_event(data, attributes) {
        Ok(Some(event)) => GcpQueueDelivery::Deliver(MessagePayload::Json(event)),
        Ok(None) => GcpQueueDelivery::Filtered,
        Err(error) => {
            warn!(%error, "Delivering an unparseable storage notification unchanged");
            GcpQueueDelivery::Deliver(ordinary_payload(data))
        }
    }
}

fn ordinary_payload(data: &[u8]) -> MessagePayload {
    let text = String::from_utf8_lossy(data);
    serde_json::from_str(&text)
        .map(MessagePayload::Json)
        .unwrap_or_else(|_| MessagePayload::Text(text.into_owned()))
}

/// `None` means the notification failed its rule's suffix filter.
fn gcs_storage_event(
    data: &[u8],
    attributes: &HashMap<String, String>,
) -> Result<Option<serde_json::Value>> {
    for (name, expected) in [
        ("eventType", "OBJECT_FINALIZE"),
        ("payloadFormat", "JSON_API_V1"),
    ] {
        if attributes.get(name).map(String::as_str) != Some(expected) {
            return Err(AlienError::new(invalid(format!(
                "GCS queue notification requires {name}={expected}"
            ))));
        }
    }
    let time = attributes
        .get("eventTime")
        .ok_or_else(|| AlienError::new(invalid("GCS notification is missing eventTime")))?;
    let timestamp = DateTime::parse_from_rfc3339(time)
        .into_alien_error()
        .context(invalid("GCS notification eventTime is invalid"))?
        .with_timezone(&Utc);
    let object: GcsObject = serde_json::from_slice(data)
        .into_alien_error()
        .context(invalid("GCS notification object metadata is invalid"))?;
    if object.bucket.is_empty() || object.name.is_empty() {
        return Err(AlienError::new(invalid(
            "GCS notification bucket/name is empty",
        )));
    }
    let size = object
        .size
        .map(|size| size.parse::<u64>())
        .transpose()
        .into_alien_error()
        .context(invalid("GCS notification size is invalid"))?;
    if attributes
        .get(GCS_SUFFIX_ATTRIBUTE)
        .is_some_and(|suffix| !object.name.ends_with(suffix))
    {
        return Ok(None);
    }
    let event = StorageEvent {
        event_type: StorageEventType::Created,
        bucket_name: object.bucket,
        object_key: object.name,
        timestamp,
        size,
        etag: object.etag,
        content_type: object.content_type,
        metadata: object.metadata,
        version_id: object.generation,
        current_tier: object.storage_class,
        copy_source: None,
        previous_tier: None,
        region: None,
    };
    serde_json::to_value(event)
        .map(Some)
        .into_alien_error()
        .context(ErrorData::SerializationFailed {
            message: "Storage notification queue payload".to_string(),
        })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlobCreatedEvent {
    specversion: String,
    subject: String,
    time: DateTime<Utc>,
    data: BlobCreatedData,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlobCreatedData {
    content_length: Option<u64>,
    e_tag: Option<String>,
    content_type: Option<String>,
}

/// Normalize the singleton CloudEvent Event Grid delivers to Service Bus for a blob
/// creation; every other message keeps its body. As on GCP, an event that doesn't
/// parse is delivered unchanged rather than failing the batch it arrived in.
pub fn azure_queue_payload(payload: MessagePayload) -> MessagePayload {
    let MessagePayload::Json(value) = &payload else {
        return payload;
    };
    if value.get("type").and_then(serde_json::Value::as_str)
        != Some("Microsoft.Storage.BlobCreated")
    {
        return payload;
    }
    match blob_created_storage_event(value) {
        Ok(event) => MessagePayload::Json(event),
        Err(error) => {
            warn!(%error, "Delivering an unparseable storage notification unchanged");
            payload
        }
    }
}

fn blob_created_storage_event(value: &serde_json::Value) -> Result<serde_json::Value> {
    let event: BlobCreatedEvent = serde_json::from_value(value.clone())
        .into_alien_error()
        .context(invalid("Azure BlobCreated CloudEvent is invalid"))?;
    if event.specversion != "1.0" {
        return Err(AlienError::new(invalid(
            "Azure storage notification requires CloudEvents 1.0",
        )));
    }
    // Event Grid's subject contains the literal blob name. Do not URL-decode it:
    // a key containing `%2F` must not become a different object containing `/`.
    let (container, key) = event
        .subject
        .strip_prefix("/blobServices/default/containers/")
        .and_then(|subject| subject.split_once("/blobs/"))
        .filter(|(container, key)| {
            !container.is_empty() && !container.contains('/') && !key.is_empty()
        })
        .ok_or_else(|| AlienError::new(invalid("Azure storage notification subject is invalid")))?;
    let storage_event = StorageEvent {
        event_type: StorageEventType::Created,
        bucket_name: container.to_string(),
        object_key: key.to_string(),
        timestamp: event.time,
        size: event.data.content_length,
        etag: event.data.e_tag,
        content_type: event.data.content_type,
        metadata: HashMap::new(),
        version_id: None,
        current_tier: None,
        copy_source: None,
        previous_tier: None,
        region: None,
    };
    serde_json::to_value(storage_event)
        .into_alien_error()
        .context(ErrorData::SerializationFailed {
            message: "Storage notification queue payload".to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn attributes() -> HashMap<String, String> {
        [
            (
                "notificationConfig",
                "projects/_/buckets/source/notificationConfigs/1",
            ),
            ("eventType", "OBJECT_FINALIZE"),
            ("payloadFormat", "JSON_API_V1"),
            ("eventTime", "2026-10-30T08:00:00.123Z"),
            (GCS_RULE_ATTRIBUTE, "source/0"),
            (GCS_SUFFIX_ATTRIBUTE, ".json"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn gcs_notification_preserves_metadata_and_literal_keys() {
        let body = json!({"bucket":"source", "name":"in/quote\"é*.json", "size":"7", "etag":"etag", "generation":"123", "contentType":"application/json", "storageClass":"STANDARD", "metadata":{"label":"value"}});
        let result = gcp_queue_payload(&serde_json::to_vec(&body).unwrap(), &attributes());
        let GcpQueueDelivery::Deliver(MessagePayload::Json(value)) = result else {
            panic!("expected JSON")
        };
        let event: StorageEvent = serde_json::from_value(value).unwrap();
        assert_eq!(event.event_type, StorageEventType::Created);
        assert_eq!(event.bucket_name, "source");
        assert_eq!(event.object_key, "in/quote\"é*.json");
        assert_eq!(
            event.timestamp.to_rfc3339(),
            "2026-10-30T08:00:00.123+00:00"
        );
        assert_eq!(event.size, Some(7));
        assert_eq!(event.etag.as_deref(), Some("etag"));
        assert_eq!(event.version_id.as_deref(), Some("123"));
        assert_eq!(event.content_type.as_deref(), Some("application/json"));
        assert_eq!(event.current_tier.as_deref(), Some("STANDARD"));
        assert_eq!(
            event.metadata,
            HashMap::from([("label".into(), "value".into())])
        );
        assert!(
            event.region.is_none() && event.copy_source.is_none() && event.previous_tier.is_none()
        );
    }

    #[test]
    fn gcs_suffix_is_literal_and_case_sensitive_and_optional() {
        let mut attrs = attributes();
        attrs.insert(GCS_SUFFIX_ATTRIBUTE.into(), "*.json".into());
        for (key, matches) in [
            ("in/a*.json", true),
            ("in/a.json", false),
            ("in/a*.JSON", false),
        ] {
            let body = serde_json::to_vec(&json!({"bucket":"source", "name":key})).unwrap();
            assert_eq!(
                gcp_queue_payload(&body, &attrs) != GcpQueueDelivery::Filtered,
                matches,
                "{key}"
            );
        }
        attrs.remove(GCS_SUFFIX_ATTRIBUTE);
        assert!(matches!(
            gcp_queue_payload(br#"{"bucket":"source","name":"any"}"#, &attrs),
            GcpQueueDelivery::Deliver(MessagePayload::Json(_))
        ));
    }

    #[test]
    fn malformed_notifications_are_delivered_unchanged_instead_of_failing_the_batch() {
        let attrs = attributes();
        for (body, expected) in [
            (
                b"not json".as_slice(),
                MessagePayload::Text("not json".into()),
            ),
            (
                br#"{"bucket":"source"}"#.as_slice(),
                MessagePayload::Json(json!({"bucket":"source"})),
            ),
            (
                br#"{"bucket":"source","name":"excluded.txt","size":"bad"}"#.as_slice(),
                MessagePayload::Json(json!({"bucket":"source","name":"excluded.txt","size":"bad"})),
            ),
        ] {
            assert_eq!(
                gcp_queue_payload(body, &attrs),
                GcpQueueDelivery::Deliver(expected)
            );
        }
        let body = br#"{"bucket":"source","name":"file.txt"}"#;
        for field in ["eventType", "payloadFormat", "eventTime"] {
            let mut broken = attrs.clone();
            broken.remove(field);
            assert_eq!(
                gcp_queue_payload(body, &broken),
                GcpQueueDelivery::Deliver(MessagePayload::Json(
                    json!({"bucket":"source","name":"file.txt"})
                )),
                "a notification missing {field} must not be suffix-filtered or converted"
            );
        }
    }

    #[test]
    fn messages_without_a_rule_attribute_keep_their_payload() {
        let mut foreign = attributes();
        foreign.remove(GCS_RULE_ATTRIBUTE);
        for attrs in [HashMap::new(), foreign] {
            assert_eq!(
                gcp_queue_payload(b"text", &attrs),
                GcpQueueDelivery::Deliver(MessagePayload::Text("text".into()))
            );
            assert_eq!(
                gcp_queue_payload(br#"{"bucket":"source","name":"regular.txt"}"#, &attrs),
                GcpQueueDelivery::Deliver(MessagePayload::Json(
                    json!({"bucket":"source","name":"regular.txt"})
                ))
            );
            assert_eq!(
                gcp_queue_payload(&[b'a', 0xff], &attrs),
                GcpQueueDelivery::Deliver(MessagePayload::Text("a\u{fffd}".into()))
            );
        }
    }

    #[test]
    fn azure_notification_preserves_literal_subject_and_metadata() {
        let payload = json!({
            "specversion":"1.0", "type":"Microsoft.Storage.BlobCreated",
            "subject":"/blobServices/default/containers/source/blobs/in/quote\"é*%2F.json",
            "time":"2026-10-30T08:00:00.1234567Z",
            "data":{"contentLength":7,"eTag":"etag","contentType":"application/json"}
        });
        let MessagePayload::Json(result) = azure_queue_payload(MessagePayload::Json(payload))
        else {
            panic!("expected JSON")
        };
        let event: StorageEvent = serde_json::from_value(result).unwrap();
        assert_eq!(event.event_type, StorageEventType::Created);
        assert_eq!(event.bucket_name, "source");
        assert_eq!(event.object_key, "in/quote\"é*%2F.json");
        assert_eq!(event.timestamp.timestamp_subsec_nanos(), 123456700);
        assert_eq!(event.size, Some(7));
        assert_eq!(event.etag.as_deref(), Some("etag"));
        assert_eq!(event.content_type.as_deref(), Some("application/json"));
        assert!(event.metadata.is_empty());
        assert!(
            event.version_id.is_none()
                && event.current_tier.is_none()
                && event.region.is_none()
                && event.copy_source.is_none()
                && event.previous_tier.is_none()
        );
    }

    #[test]
    fn azure_ordinary_and_malformed_messages_pass_through_unchanged() {
        let mut payloads = vec![
            MessagePayload::Text("ordinary".into()),
            MessagePayload::Json(json!({"data":"ordinary"})),
            MessagePayload::Json(json!({"type":"Microsoft.Storage.BlobCreated"})),
        ];
        for subject in [
            "",
            "/blobServices/default/containers/source/blobs/",
            "/blobServices/default/containers//blobs/key",
        ] {
            payloads.push(MessagePayload::Json(json!({"type":"Microsoft.Storage.BlobCreated", "specversion":"1.0", "time":"2026-10-30T08:00:00Z", "subject":subject, "data":{}})));
        }
        for payload in payloads {
            assert_eq!(azure_queue_payload(payload.clone()), payload);
        }
    }
}
