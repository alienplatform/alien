//! Provider notification decoding shared by pull bindings and Worker queue routes.

use crate::error::{ErrorData, Result};
use alien_core::{
    storage_notifications::GCS_SUFFIX_ATTRIBUTE, MessagePayload, StorageEvent, StorageEventType,
};
use alien_error::{AlienError, Context, IntoAlienError};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::HashMap;

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

/// Normalize GCS notifications while preserving ordinary queue bodies.
/// `None` means a valid notification failed its suffix filter and must be acknowledged.
/// Malformed notifications return an error and must remain available for redelivery.
pub fn gcp_queue_payload(
    data: &[u8],
    attributes: &HashMap<String, String>,
) -> Result<Option<MessagePayload>> {
    if !attributes.contains_key("notificationConfig") {
        let payload = match serde_json::from_slice(data) {
            Ok(value) => MessagePayload::Json(value),
            Err(_) => MessagePayload::Text(
                String::from_utf8(data.to_vec())
                    .into_alien_error()
                    .context(invalid("Pub/Sub message body is not UTF-8"))?,
            ),
        };
        return Ok(Some(payload));
    }
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
        .map(|value| Some(MessagePayload::Json(value)))
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

/// Normalize the singleton CloudEvent delivered by Event Grid to Service Bus.
/// Ordinary queue messages retain their original payload and settlement behavior.
pub fn azure_queue_payload(payload: MessagePayload) -> Result<MessagePayload> {
    let MessagePayload::Json(value) = &payload else {
        return Ok(payload);
    };
    if value.get("type").and_then(serde_json::Value::as_str)
        != Some("Microsoft.Storage.BlobCreated")
    {
        return Ok(payload);
    }
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
        .map(MessagePayload::Json)
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
            (GCS_SUFFIX_ATTRIBUTE, ".json"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn gcs_notification_preserves_metadata_and_literal_keys() {
        let body = json!({"bucket":"source", "name":"in/quote\"é*.json", "size":"7", "etag":"etag", "generation":"123", "contentType":"application/json", "storageClass":"STANDARD", "metadata":{"label":"value"}});
        let result = gcp_queue_payload(&serde_json::to_vec(&body).unwrap(), &attributes())
            .unwrap()
            .unwrap();
        let MessagePayload::Json(value) = result else {
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
            assert_eq!(gcp_queue_payload(&body, &attrs).unwrap().is_some(), matches);
        }
        attrs.remove(GCS_SUFFIX_ATTRIBUTE);
        assert!(
            gcp_queue_payload(br#"{"bucket":"source","name":"any"}"#, &attrs)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn malformed_notifications_fail_instead_of_being_filtered_or_passed_through() {
        let attrs = attributes();
        for body in [
            b"not json".as_slice(),
            br#"{"bucket":"source"}"#,
            br#"{"bucket":"source","name":"excluded.txt","size":"bad"}"#,
        ] {
            assert_eq!(
                gcp_queue_payload(body, &attrs).unwrap_err().code,
                "QUEUE_PROVIDER_RESPONSE_INVALID"
            );
        }
        for field in ["eventType", "payloadFormat", "eventTime"] {
            let mut broken = attrs.clone();
            broken.remove(field);
            assert!(
                gcp_queue_payload(br#"{"bucket":"source","name":"file.json"}"#, &broken).is_err()
            );
        }
    }

    #[test]
    fn ordinary_queue_messages_keep_their_payload() {
        let attrs = HashMap::new();
        assert_eq!(
            gcp_queue_payload(b"text", &attrs).unwrap(),
            Some(MessagePayload::Text("text".into()))
        );
        assert_eq!(
            gcp_queue_payload(br#"{"name":"regular"}"#, &attrs).unwrap(),
            Some(MessagePayload::Json(json!({"name":"regular"})))
        );
        assert!(gcp_queue_payload(&[0xff], &attrs).is_err());
    }
    #[test]
    fn azure_notification_preserves_literal_subject_and_metadata() {
        let payload = json!({
            "specversion":"1.0", "type":"Microsoft.Storage.BlobCreated",
            "subject":"/blobServices/default/containers/source/blobs/in/quote\"é*%2F.json",
            "time":"2026-10-30T08:00:00.1234567Z",
            "data":{"contentLength":7,"eTag":"etag","contentType":"application/json"}
        });
        let MessagePayload::Json(result) =
            azure_queue_payload(MessagePayload::Json(payload)).unwrap()
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
    fn azure_ordinary_messages_pass_through_but_malformed_notifications_fail() {
        for payload in [
            MessagePayload::Text("ordinary".into()),
            MessagePayload::Json(json!({"data":"ordinary"})),
        ] {
            assert_eq!(azure_queue_payload(payload.clone()).unwrap(), payload);
        }
        assert!(azure_queue_payload(MessagePayload::Json(
            json!({"type":"Microsoft.Storage.BlobCreated"})
        ))
        .is_err());
        for subject in [
            "",
            "/blobServices/default/containers/source/blobs/",
            "/blobServices/default/containers//blobs/key",
        ] {
            assert!(azure_queue_payload(MessagePayload::Json(json!({"type":"Microsoft.Storage.BlobCreated", "specversion":"1.0", "time":"2026-10-30T08:00:00Z", "subject":subject, "data":{}}))).is_err());
        }
    }
}
