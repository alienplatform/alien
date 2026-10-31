//! Provider notification decoding shared by pull bindings and Worker queue routes.

use crate::error::{ErrorData, Result};
use alien_core::{
    storage_notifications::{gcs_suffixes, GCS_RULE_ATTRIBUTE},
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
    /// A storage notification whose key matches none of its config's suffixes: acknowledge
    /// it unseen.
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
    let suffixes = gcs_suffixes(attributes)
        .context(invalid("GCS notification suffix attributes are invalid"))?;
    if suffixes.is_some_and(|suffixes| {
        !suffixes
            .iter()
            .any(|suffix| object.name.ends_with(suffix.as_str()))
    }) {
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

/// How a consumer handles one SQS message body read from a Queue.
#[derive(Debug, PartialEq)]
pub enum AwsQueueDelivery {
    /// Hand each payload to the consumer, in order. An ordinary message yields exactly one
    /// payload. An S3 event notification yields one `StorageEvent` per object-created
    /// record; S3 normally sends one record per message, but when it sends several, every
    /// payload comes from the same SQS message, so they share its receipt handle and
    /// message ID: completing one completes them all, and a retry redelivers them all.
    Deliver(Vec<MessagePayload>),
    /// The `s3:TestEvent` S3 sends whenever a bucket's notification configuration
    /// changes: there is nothing to deliver, so acknowledge it unseen.
    Discard,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct S3EventRecord {
    event_name: String,
    event_time: DateTime<Utc>,
    s3: S3EventEntity,
}

#[derive(Deserialize)]
struct S3EventEntity {
    bucket: S3EventBucket,
    object: S3EventObject,
}

#[derive(Deserialize)]
struct S3EventBucket {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct S3EventObject {
    key: String,
    size: Option<u64>,
    e_tag: Option<String>,
}

/// Normalize the S3 event notifications S3 delivers straight to SQS for a Storage's
/// rules; every other message keeps its body. As on GCP and Azure, each message is
/// decided on its own: a notification that doesn't parse is delivered unchanged instead
/// of failing the batch it arrived in.
///
/// A body is converted only when it is an object whose `Records` array is non-empty and
/// every record has `eventSource == "aws:s3"` and an `ObjectCreated:*` `eventName`.
/// Storage rules only subscribe to object creation, so any other S3 notification, such as
/// `ObjectRemoved:*`, comes from a configuration Alien doesn't own, and its consumer gets
/// it unchanged.
pub fn aws_queue_payload(body: &str) -> AwsQueueDelivery {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return AwsQueueDelivery::Deliver(vec![MessagePayload::Text(body.to_string())]);
    };
    if is_s3_test_event(&value) {
        return AwsQueueDelivery::Discard;
    }
    let Some(records) = s3_notification_records(&value) else {
        return AwsQueueDelivery::Deliver(vec![MessagePayload::Json(value)]);
    };
    let created = records.iter().all(|record| {
        record
            .get("eventName")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|name| name.starts_with("ObjectCreated:"))
    });
    if !created {
        return AwsQueueDelivery::Deliver(vec![MessagePayload::Json(value)]);
    }
    match s3_storage_events(records) {
        Ok(events) => {
            AwsQueueDelivery::Deliver(events.into_iter().map(MessagePayload::Json).collect())
        }
        Err(error) => {
            warn!(%error, "Delivering an unparseable storage notification unchanged");
            AwsQueueDelivery::Deliver(vec![MessagePayload::Json(value)])
        }
    }
}

fn is_s3_test_event(value: &serde_json::Value) -> bool {
    value.get("Service").and_then(serde_json::Value::as_str) == Some("Amazon S3")
        && value.get("Event").and_then(serde_json::Value::as_str) == Some("s3:TestEvent")
}

fn s3_notification_records(value: &serde_json::Value) -> Option<&Vec<serde_json::Value>> {
    let records = value.get("Records")?.as_array()?;
    let from_s3 = !records.is_empty()
        && records.iter().all(|record| {
            record
                .get("eventSource")
                .and_then(serde_json::Value::as_str)
                == Some("aws:s3")
        });
    from_s3.then_some(records)
}

fn s3_storage_events(records: &[serde_json::Value]) -> Result<Vec<serde_json::Value>> {
    let mut events = Vec::with_capacity(records.len());
    for record in records {
        let record: S3EventRecord = serde_json::from_value(record.clone())
            .into_alien_error()
            .context(invalid("S3 notification record is invalid"))?;
        let object_key = decode_s3_key(&record.s3.object.key)?;
        if record.s3.bucket.name.is_empty() || object_key.is_empty() {
            return Err(AlienError::new(invalid(
                "S3 notification bucket/key is empty",
            )));
        }
        let event = StorageEvent {
            event_type: StorageEventType::Created,
            bucket_name: record.s3.bucket.name,
            object_key,
            timestamp: record.event_time,
            size: record.s3.object.size,
            etag: record.s3.object.e_tag,
            content_type: None,
            metadata: HashMap::new(),
            version_id: None,
            current_tier: None,
            copy_source: None,
            previous_tier: None,
            region: None,
        };
        events.push(serde_json::to_value(event).into_alien_error().context(
            ErrorData::SerializationFailed {
                message: "Storage notification queue payload".to_string(),
            },
        )?);
    }
    Ok(events)
}

/// S3 notifications carry object keys in HTML-form URL encoding: `+` is a space and
/// `%XX` is a UTF-8 byte, so a literal `+` arrives as `%2B` and `%` as `%25`.
fn decode_s3_key(encoded: &str) -> Result<String> {
    let malformed = || AlienError::new(invalid("S3 notification object key encoding is invalid"));
    let hex = |byte: Option<u8>| {
        byte.and_then(|byte| char::from(byte).to_digit(16))
            .map(|digit| digit as u8)
    };
    let mut decoded = Vec::with_capacity(encoded.len());
    let mut bytes = encoded.bytes();
    while let Some(byte) = bytes.next() {
        decoded.push(match byte {
            b'+' => b' ',
            b'%' => {
                let high = hex(bytes.next()).ok_or_else(malformed)?;
                let low = hex(bytes.next()).ok_or_else(malformed)?;
                high << 4 | low
            }
            other => other,
        });
    }
    String::from_utf8(decoded).map_err(|_| malformed())
}

/// Notifications captured from a real bucket's S3 event notifications delivered to SQS.
#[cfg(test)]
pub(crate) mod aws_fixtures {
    use serde_json::{json, Value};

    /// The SQS body S3 sent for one object-created event, with the key as S3 encoded it.
    pub(crate) fn s3_record(event_name: &str, key: &str, size: u64, etag: &str) -> Value {
        json!({
            "eventVersion": "2.6",
            "eventSource": "aws:s3",
            "awsRegion": "us-east-2",
            "eventTime": "2026-10-30T18:28:41.358Z",
            "eventName": event_name,
            "userIdentity": {"principalId": "AWS:AIDAEXAMPLEPRINCIPAL"},
            "requestParameters": {"sourceIPAddress": "192.0.2.10"},
            "responseElements": {
                "x-amz-request-id": "EXAMPLEREQUEST01",
                "x-amz-id-2": "EXAMPLEid2"
            },
            "s3": {
                "s3SchemaVersion": "1.0",
                "configurationId": "rule-0",
                "bucket": {
                    "name": "example-bucket",
                    "ownerIdentity": {"principalId": "EXAMPLEOWNER"},
                    "arn": "arn:aws:s3:::example-bucket"
                },
                "object": {"key": key, "size": size, "eTag": etag, "sequencer": "0055AED6DCD90281E5"}
            }
        })
    }

    pub(crate) fn s3_put(key: &str) -> Value {
        s3_record(
            "ObjectCreated:Put",
            key,
            7,
            "321c3cf486ed509164edec1e1981fec8",
        )
    }

    pub(crate) fn notification(records: Vec<Value>) -> String {
        json!({ "Records": records }).to_string()
    }

    /// The message S3 sends to a destination whenever a notification configuration changes.
    pub(crate) fn test_event() -> String {
        json!({
            "Service": "Amazon S3",
            "Event": "s3:TestEvent",
            "Time": "2026-10-30T18:28:30.022Z",
            "Bucket": "example-bucket",
            "RequestId": "EXAMPLEREQUEST02",
            "HostId": "EXAMPLEhostid"
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::storage_notifications::GCS_SUFFIXES_ATTRIBUTE;
    use serde_json::json;

    fn suffixes(attrs: &mut HashMap<String, String>, suffixes: &[&str]) {
        attrs.insert(
            format!("{GCS_SUFFIXES_ATTRIBUTE}0"),
            serde_json::to_string(suffixes).unwrap(),
        );
    }

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
            ("storageEventSuffixes0", r#"[".json"]"#),
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
    fn gcs_suffixes_are_literal_case_sensitive_any_of_and_optional() {
        let mut attrs = attributes();
        suffixes(&mut attrs, &["*.json", ".csv"]);
        for (key, matches) in [
            ("in/a*.json", true),
            ("in/a.csv", true),
            ("in/a.json", false),
            ("in/a*.JSON", false),
            ("in/a.csv.gz", false),
        ] {
            let body = serde_json::to_vec(&json!({"bucket":"source", "name":key})).unwrap();
            assert_eq!(
                gcp_queue_payload(&body, &attrs) != GcpQueueDelivery::Filtered,
                matches,
                "{key}"
            );
        }
        attrs.remove(&format!("{GCS_SUFFIXES_ATTRIBUTE}0"));
        assert!(matches!(
            gcp_queue_payload(br#"{"bucket":"source","name":"any"}"#, &attrs),
            GcpQueueDelivery::Deliver(MessagePayload::Json(_))
        ));
    }

    #[test]
    fn gcs_suffixes_split_across_attributes_are_joined_in_order() {
        let mut attrs = attributes();
        attrs.insert(format!("{GCS_SUFFIXES_ATTRIBUTE}0"), r#"[".js"#.into());
        attrs.insert(format!("{GCS_SUFFIXES_ATTRIBUTE}1"), r#"on"]"#.into());
        let body = |key: &str| serde_json::to_vec(&json!({"bucket":"source", "name":key})).unwrap();
        assert!(matches!(
            gcp_queue_payload(&body("in/a.json"), &attrs),
            GcpQueueDelivery::Deliver(MessagePayload::Json(_))
        ));
        assert_eq!(
            gcp_queue_payload(&body("in/a.js"), &attrs),
            GcpQueueDelivery::Filtered
        );
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
        let mut broken = attrs.clone();
        broken.insert(format!("{GCS_SUFFIXES_ATTRIBUTE}0"), "not json".into());
        assert_eq!(
            gcp_queue_payload(body, &broken),
            GcpQueueDelivery::Deliver(MessagePayload::Json(
                json!({"bucket":"source","name":"file.txt"})
            )),
            "undecodable suffixes must not filter the notification"
        );
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

    fn aws_events(body: &str) -> Vec<StorageEvent> {
        let AwsQueueDelivery::Deliver(payloads) = aws_queue_payload(body) else {
            panic!("expected S3 notification to be delivered")
        };
        payloads
            .into_iter()
            .map(|payload| {
                let MessagePayload::Json(value) = payload else {
                    panic!("expected JSON StorageEvent")
                };
                serde_json::from_value(value).expect("StorageEvent")
            })
            .collect()
    }

    #[test]
    fn s3_notification_keys_decode_with_form_url_semantics() {
        for (encoded, key) in [
            ("in/plain.json", "in/plain.json"),
            ("in/with+space.json", "in/with space.json"),
            ("in/plus%2Bsign.json", "in/plus+sign.json"),
            ("in/unicode-%C3%A9%E6%BC%A2.json", "in/unicode-é漢.json"),
            ("in/percent%2525.json", "in/percent%25.json"),
            ("in/quote%22star*.json", "in/quote\"star*.json"),
            ("in/back%5Cslash.json", "in/back\\slash.json"),
        ] {
            let body = aws_fixtures::notification(vec![aws_fixtures::s3_put(encoded)]);
            let events = aws_events(&body);
            assert_eq!(events.len(), 1, "{encoded}");
            let event = &events[0];
            assert_eq!(event.object_key, key, "{encoded}");
            assert_eq!(event.event_type, StorageEventType::Created);
            assert_eq!(event.bucket_name, "example-bucket");
            assert_eq!(
                event.timestamp.to_rfc3339(),
                "2026-10-30T18:28:41.358+00:00"
            );
            assert_eq!(event.size, Some(7));
            assert_eq!(
                event.etag.as_deref(),
                Some("321c3cf486ed509164edec1e1981fec8")
            );
            assert!(event.content_type.is_none() && event.metadata.is_empty());
            assert!(
                event.version_id.is_none()
                    && event.current_tier.is_none()
                    && event.region.is_none()
                    && event.copy_source.is_none()
                    && event.previous_tier.is_none()
            );
        }
    }

    #[test]
    fn s3_multipart_and_multi_record_notifications_yield_one_event_per_record() {
        let multipart = aws_fixtures::s3_record(
            "ObjectCreated:CompleteMultipartUpload",
            "in/multi+part.json",
            6291456,
            "844e79d332c99c9540b048cb480ebce5-1",
        );
        let events = aws_events(&aws_fixtures::notification(vec![multipart.clone()]));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].object_key, "in/multi part.json");
        assert_eq!(events[0].size, Some(6291456));
        assert_eq!(
            events[0].etag.as_deref(),
            Some("844e79d332c99c9540b048cb480ebce5-1")
        );

        // Size and eTag are absent on some S3 event types; the event is still delivered.
        let mut sparse = aws_fixtures::s3_put("other/any.bin");
        let object = sparse["s3"]["object"].as_object_mut().expect("object");
        object.remove("size");
        object.remove("eTag");
        let events = aws_events(&aws_fixtures::notification(vec![
            aws_fixtures::s3_put("in/plain.json"),
            multipart,
            sparse,
        ]));
        let keys: Vec<_> = events
            .iter()
            .map(|event| event.object_key.as_str())
            .collect();
        assert_eq!(
            keys,
            ["in/plain.json", "in/multi part.json", "other/any.bin"]
        );
        assert_eq!(
            (events[2].size.as_ref(), events[2].etag.as_ref()),
            (None, None)
        );
    }

    #[test]
    fn s3_test_events_are_discarded_and_other_s3_notifications_pass_through() {
        assert_eq!(
            aws_queue_payload(&aws_fixtures::test_event()),
            AwsQueueDelivery::Discard
        );
        // Rules only subscribe to object creation: any other S3 notification, alone or next
        // to a creation, comes from someone else's configuration and reaches its consumer
        // unchanged. Shapes follow S3's: a removal has no size or eTag, a restore carries
        // its glacier event data.
        let mut removed = aws_fixtures::s3_record("ObjectRemoved:Delete", "in/gone.json", 0, "x");
        let object = removed["s3"]["object"].as_object_mut().expect("object");
        object.remove("size");
        object.remove("eTag");
        let mut restored =
            aws_fixtures::s3_record("ObjectRestore:Completed", "archive/a.bin", 7, "etag");
        restored["glacierEventData"] = json!({"restoreEventData": {
            "lifecycleRestorationExpiryTime": "2026-11-06T00:00:00.000Z",
            "lifecycleRestoreStorageClass": "GLACIER"
        }});
        let expired = aws_fixtures::s3_record("LifecycleExpiration:Delete", "logs/old.txt", 0, "x");
        for records in [
            vec![removed.clone()],
            vec![restored],
            vec![expired],
            vec![removed.clone(), removed.clone()],
            vec![aws_fixtures::s3_put("in/plain.json"), removed],
        ] {
            let body = aws_fixtures::notification(records);
            assert_eq!(
                aws_queue_payload(&body),
                AwsQueueDelivery::Deliver(vec![MessagePayload::Json(
                    serde_json::from_str(&body).unwrap()
                )])
            );
        }
    }

    #[test]
    fn aws_ordinary_and_malformed_messages_pass_through_unchanged() {
        let mut no_key = aws_fixtures::s3_put("in/plain.json");
        no_key["s3"]["object"]
            .as_object_mut()
            .expect("object")
            .remove("key");
        let mut bad_time = aws_fixtures::s3_put("in/plain.json");
        bad_time["eventTime"] = json!("yesterday");
        let mut empty_bucket = aws_fixtures::s3_put("in/plain.json");
        empty_bucket["s3"]["bucket"]["name"] = json!("");
        let mut foreign = aws_fixtures::s3_put("in/plain.json");
        foreign["eventSource"] = json!("aws:sqs");
        let mut cases = vec![
            json!({"hello": "world"}),
            json!({"Records": []}),
            json!({"Records": "not-an-array"}),
            // A test event needs both markers; anything else is the user's own message.
            json!({"Event": "s3:TestEvent"}),
            // Only bodies whose every record comes from S3 are notifications.
            json!({"Records": [aws_fixtures::s3_put("in/plain.json"), foreign]}),
        ];
        // One malformed record keeps the whole notification unchanged, valid ones included.
        for broken in [
            no_key,
            bad_time,
            empty_bucket,
            aws_fixtures::s3_put("in/bad%2"),
            aws_fixtures::s3_put("in/bad%zz.json"),
            aws_fixtures::s3_put("in/not-utf8-%FF.json"),
        ] {
            cases.push(json!({"Records": [aws_fixtures::s3_put("in/plain.json"), broken]}));
        }
        for value in cases {
            assert_eq!(
                aws_queue_payload(&value.to_string()),
                AwsQueueDelivery::Deliver(vec![MessagePayload::Json(value)])
            );
        }
        for text in ["plain text", "{\"Records\":", ""] {
            assert_eq!(
                aws_queue_payload(text),
                AwsQueueDelivery::Deliver(vec![MessagePayload::Text(text.into())])
            );
        }
    }
}
