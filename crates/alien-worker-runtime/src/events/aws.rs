use crate::error::{Error, ErrorData};
use alien_core::{
    MessagePayload, QueueMessage, ScheduledEvent, StorageEvent, StorageEventType, StorageEvents,
};
use alien_error::AlienError;
use std::collections::HashMap;

use aws_lambda_events::s3::{S3Event, S3EventRecord};
use aws_lambda_events::sqs::{SqsEvent, SqsMessage};

/// Convert AWS SQS event to standardized queue messages
pub fn sqs_event_to_queue_messages(event: SqsEvent) -> Result<Vec<QueueMessage>, Error> {
    let messages = event
        .records
        .into_iter()
        .map(sqs_message_to_queue_message)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(messages)
}

/// Convert AWS SQS message to standardized queue message
pub fn sqs_message_to_queue_message(message: SqsMessage) -> Result<QueueMessage, Error> {
    let body = message.body.ok_or_else(|| {
        AlienError::new(ErrorData::EventProcessingFailed {
            event_type: "AWS SQS Message".to_string(),
            reason: "Missing message body".to_string(),
        })
    })?;

    // Try to parse body as JSON, fall back to text
    let payload = match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(json) => MessagePayload::Json(json),
        Err(_) => MessagePayload::Text(body),
    };

    let receipt_handle = message.receipt_handle.ok_or_else(|| {
        AlienError::new(ErrorData::EventProcessingFailed {
            event_type: "AWS SQS Message".to_string(),
            reason: "Missing receipt handle".to_string(),
        })
    })?;

    let message_id = message.message_id.ok_or_else(|| {
        AlienError::new(ErrorData::EventProcessingFailed {
            event_type: "AWS SQS Message".to_string(),
            reason: "Missing message ID".to_string(),
        })
    })?;

    // Parse timestamp from SentTimestamp attribute
    let timestamp = message
        .attributes
        .get("SentTimestamp")
        .and_then(|ts| ts.parse::<i64>().ok())
        .and_then(|ts| chrono::DateTime::from_timestamp(ts / 1000, (ts % 1000) as u32 * 1_000_000))
        .unwrap_or_else(chrono::Utc::now);

    // Extract queue name from event source ARN
    let source = message
        .event_source_arn
        .as_ref()
        .and_then(|arn| arn.split(':').last())
        .unwrap_or("unknown-queue")
        .to_string();

    // Flatten message attributes to simple string map
    let mut attributes = HashMap::new();
    for (key, attr) in message.message_attributes {
        if let Some(value) = attr.string_value {
            attributes.insert(key, value);
        }
    }

    // Parse attempt count from ApproximateReceiveCount
    let attempt_count = message
        .attributes
        .get("ApproximateReceiveCount")
        .and_then(|count| count.parse::<u32>().ok());

    Ok(QueueMessage {
        id: message_id,
        payload,
        receipt_handle,
        timestamp,
        source,
        attributes,
        attempt_count,
    })
}

/// Convert AWS CloudWatch event to scheduled event
pub fn cloudwatch_event_to_scheduled_event<T: serde::Serialize + serde::de::DeserializeOwned>(
    event: aws_lambda_events::cloudwatch_events::CloudWatchEvent<T>,
) -> Result<ScheduledEvent, Error> {
    if event.detail_type.as_deref() == Some("Scheduled Event") {
        Ok(ScheduledEvent {
            timestamp: event.time,
        })
    } else {
        Err(AlienError::new(ErrorData::EventProcessingFailed {
            event_type: "CloudWatch Scheduled Event".to_string(),
            reason: format!("Expected 'Scheduled Event', got {:?}", event.detail_type),
        }))
    }
}

/// Convert AWS S3 event record to storage event
pub fn s3_event_record_to_storage_event(record: S3EventRecord) -> Result<StorageEvent, Error> {
    let event_type = match record.event_name.as_deref() {
        Some(name) if name.starts_with("ObjectCreated:") => StorageEventType::Created,
        Some(name) if name.starts_with("ObjectRemoved:") => StorageEventType::Deleted,
        Some(name) if name.starts_with("ObjectRestore:") => StorageEventType::Restored,
        Some(name) if name.starts_with("ObjectReplication:") => StorageEventType::Copied, // Approximate mapping
        Some(name) if name.starts_with("s3:ObjectTagging:") => StorageEventType::MetadataUpdated, // Approximate mapping
        _ => {
            return Err(AlienError::new(ErrorData::EventProcessingFailed {
                event_type: "AWS S3 Event".to_string(),
                reason: format!("Unsupported S3 event name: {:?}", record.event_name),
            }))
        }
    };

    let object = record.s3.object;
    let bucket = record.s3.bucket;

    Ok(StorageEvent {
        event_type,
        bucket_name: bucket.name.unwrap_or_default(),
        object_key: object.key.unwrap_or_default(),
        timestamp: record.event_time,
        size: object.size.map(|s| s as u64), // Convert i64 to u64
        etag: object.e_tag,
        content_type: None, // S3EventRecord doesn't directly provide content_type
        metadata: HashMap::new(), // S3EventRecord doesn't directly provide user metadata
        copy_source: None,  // S3EventRecord needs more context for copy source
        previous_tier: None, // Specific tier change events needed
        current_tier: None, // Specific tier change events needed
        region: record.aws_region,
        version_id: object.version_id,
    })
}

/// Convert AWS S3 event to storage events
pub fn s3_event_to_storage_events(event: S3Event) -> Result<StorageEvents, Error> {
    let events = event
        .records
        .into_iter()
        .map(s3_event_record_to_storage_event)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(StorageEvents(events))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_lambda_events::cloudwatch_events::CloudWatchEvent;
    use aws_lambda_events::s3::{S3Event, S3EventRecord};
    use aws_lambda_events::sqs::{SqsEvent, SqsMessage, SqsMessageAttribute};
    use chrono::{DateTime, TimeZone, Utc};
    use serde_json::json;
    use std::collections::HashMap;

    fn parse_datetime(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn test_sqs_message_to_queue_message_json() {
        let mut attributes = HashMap::new();
        attributes.insert("SentTimestamp".to_string(), "1640995200000".to_string());
        attributes.insert("ApproximateReceiveCount".to_string(), "1".to_string());

        let mut message_attributes = HashMap::new();
        message_attributes.insert(
            "source".to_string(),
            SqsMessageAttribute {
                string_value: Some("order-service".to_string()),
                data_type: Some("String".to_string()),
                ..Default::default()
            },
        );

        let sqs_message = SqsMessage {
            message_id: Some("msg-12345".to_string()),
            receipt_handle: Some("receipt-handle-123".to_string()),
            body: Some(r#"{"hello": "world", "count": 42}"#.to_string()),
            attributes,
            message_attributes,
            event_source_arn: Some("arn:aws:sqs:us-east-1:123456789012:test-queue".to_string()),
            ..Default::default()
        };

        let queue_message: QueueMessage = sqs_message_to_queue_message(sqs_message).unwrap();

        assert_eq!(queue_message.id, "msg-12345");
        assert_eq!(queue_message.receipt_handle, "receipt-handle-123");
        assert_eq!(queue_message.source, "test-queue");
        assert_eq!(queue_message.attempt_count, Some(1));
        assert_eq!(
            queue_message.attributes.get("source"),
            Some(&"order-service".to_string())
        );

        match queue_message.payload {
            MessagePayload::Json(json) => {
                assert_eq!(json["hello"], "world");
                assert_eq!(json["count"], 42);
            }
            _ => panic!("Expected JSON payload"),
        }
    }

    #[test]
    fn test_sqs_message_to_queue_message_text() {
        let mut attributes = HashMap::new();
        attributes.insert("SentTimestamp".to_string(), "1640995200000".to_string());

        let sqs_message = SqsMessage {
            message_id: Some("msg-text-123".to_string()),
            receipt_handle: Some("receipt-handle-text".to_string()),
            body: Some("This is plain text".to_string()),
            attributes,
            event_source_arn: Some("arn:aws:sqs:us-west-2:123456789012:text-queue".to_string()),
            ..Default::default()
        };

        let queue_message: QueueMessage = sqs_message_to_queue_message(sqs_message).unwrap();

        assert_eq!(queue_message.id, "msg-text-123");
        assert_eq!(queue_message.source, "text-queue");

        match queue_message.payload {
            MessagePayload::Text(text) => {
                assert_eq!(text, "This is plain text");
            }
            _ => panic!("Expected Text payload"),
        }
    }

    #[test]
    fn test_sqs_event_to_queue_messages() {
        let mut attributes1 = HashMap::new();
        attributes1.insert("SentTimestamp".to_string(), "1640995200000".to_string());

        let mut attributes2 = HashMap::new();
        attributes2.insert("SentTimestamp".to_string(), "1640995260000".to_string());

        let sqs_event = SqsEvent {
            records: vec![
                SqsMessage {
                    message_id: Some("msg-1".to_string()),
                    receipt_handle: Some("handle-1".to_string()),
                    body: Some(r#"{"message": 1}"#.to_string()),
                    attributes: attributes1,
                    event_source_arn: Some(
                        "arn:aws:sqs:us-east-1:123456789012:batch-queue".to_string(),
                    ),
                    ..Default::default()
                },
                SqsMessage {
                    message_id: Some("msg-2".to_string()),
                    receipt_handle: Some("handle-2".to_string()),
                    body: Some("Plain text message".to_string()),
                    attributes: attributes2,
                    event_source_arn: Some(
                        "arn:aws:sqs:us-east-1:123456789012:batch-queue".to_string(),
                    ),
                    ..Default::default()
                },
            ],
        };

        let queue_messages: Vec<QueueMessage> = sqs_event_to_queue_messages(sqs_event).unwrap();

        assert_eq!(queue_messages.len(), 2);
        assert_eq!(queue_messages[0].id, "msg-1");
        assert_eq!(queue_messages[1].id, "msg-2");
        assert_eq!(queue_messages[0].source, "batch-queue");
        assert_eq!(queue_messages[1].source, "batch-queue");
    }

    #[test]
    fn test_try_from_s3_put_event_record() {
        let json_data = json!({
          "eventVersion": "2.1",
          "eventSource": "aws:s3",
          "awsRegion": "us-east-1",
          "eventTime": "2025-04-09T12:15:30.123Z",
          "eventName": "ObjectCreated:Put",
          "userIdentity": { "principalId": "EXAMPLE" },
          "requestParameters": { "sourceIPAddress": "127.0.0.1" },
          "responseElements": { "x-amz-request-id": "EXAMPLE12345", "x-amz-id-2": "EXAMPLE123/..." },
          "s3": {
            "s3SchemaVersion": "1.0",
            "configurationId": "my-trigger-config",
            "bucket": {
              "name": "my-bucket",
              "ownerIdentity": { "principalId": "EXAMPLE" },
              "arn": "arn:aws:s3:::my-bucket"
            },
            "object": {
              "key": "uploads/image.png",
              "size": 102400,
              "eTag": "123456789abcdef123456789abcdef",
              "sequencer": "005A1B2C3D4E5F6789"
            }
          }
        });
        let record: S3EventRecord = serde_json::from_value(json_data).unwrap();
        let storage_event: StorageEvent = s3_event_record_to_storage_event(record).unwrap();

        assert_eq!(storage_event.event_type, StorageEventType::Created);
        assert_eq!(storage_event.bucket_name, "my-bucket");
        assert_eq!(storage_event.object_key, "uploads/image.png");
        assert_eq!(
            storage_event.timestamp,
            parse_datetime("2025-04-09T12:15:30.123Z")
        );
        assert_eq!(storage_event.size, Some(102400));
        assert_eq!(
            storage_event.etag,
            Some("123456789abcdef123456789abcdef".to_string())
        );
        assert_eq!(storage_event.region, Some("us-east-1".to_string()));
        assert!(storage_event.version_id.is_none());
    }

    #[test]
    fn test_try_from_scheduled_cloudwatch_event() {
        let json_data = json!({
          "version": "0",
          "id": "c4ca4238-a0b9-3382-8dcc-509a6f75849b",
          "detail-type": "Scheduled Event",
          "source": "aws.events",
          "account": "123456789012",
          "time": "2025-04-09T14:00:00Z",
          "region": "us-east-1",
          "resources": [
            "arn:aws:events:us-east-1:123456789012:rule/daily-job"
          ],
          "detail": {}
        });
        // Use serde_json::Value for the detail type T as it's not relevant
        let cw_event: CloudWatchEvent<serde_json::Value> =
            serde_json::from_value(json_data).unwrap();
        let scheduled_event: ScheduledEvent =
            cloudwatch_event_to_scheduled_event(cw_event).unwrap();

        assert_eq!(
            scheduled_event.timestamp,
            Utc.with_ymd_and_hms(2025, 4, 9, 14, 0, 0).unwrap()
        );
    }

    #[test]
    fn test_sqs_event_real_example() {
        // Test with the exact SQS event example structure
        let json_data = json!({
            "Records": [
                {
                    "messageId": "059f36b4-87a3-44ab-83d2-661975830a7d",
                    "receiptHandle": "AQEBwJnKyrHigUMZj6rYigCgxlaS3SLy0a...",
                    "body": "Test message.",
                    "attributes": {
                        "ApproximateReceiveCount": "1",
                        "SentTimestamp": "1545082649183",
                        "SenderId": "AIDAIENQZJOLO23YVJ4VO",
                        "ApproximateFirstReceiveTimestamp": "1545082649185"
                    },
                    "messageAttributes": {
                        "myAttribute": {
                            "stringValue": "myValue",
                            "stringListValues": [],
                            "binaryListValues": [],
                            "dataType": "String"
                        }
                    },
                    "md5OfBody": "e4e68fb7bd0e697a0ae8f1bb342846b3",
                    "eventSource": "aws:sqs",
                    "eventSourceARN": "arn:aws:sqs:us-east-2:123456789012:my-queue",
                    "awsRegion": "us-east-2"
                },
                {
                    "messageId": "2e1424d4-f796-459a-8184-9c92662be6da",
                    "receiptHandle": "AQEBzWwaftRI0KuVm4tP+/7q1rGgNqicHq...",
                    "body": "Test message.",
                    "attributes": {
                        "ApproximateReceiveCount": "1",
                        "SentTimestamp": "1545082650636",
                        "SenderId": "AIDAIENQZJOLO23YVJ4VO",
                        "ApproximateFirstReceiveTimestamp": "1545082650649"
                    },
                    "messageAttributes": {},
                    "md5OfBody": "e4e68fb7bd0e697a0ae8f1bb342846b3",
                    "eventSource": "aws:sqs",
                    "eventSourceARN": "arn:aws:sqs:us-east-2:123456789012:my-queue",
                    "awsRegion": "us-east-2"
                }
            ]
        });

        let sqs_event: SqsEvent = serde_json::from_value(json_data).unwrap();
        let queue_messages: Vec<QueueMessage> = sqs_event_to_queue_messages(sqs_event).unwrap();

        assert_eq!(queue_messages.len(), 2);

        // Test first message
        let msg1 = &queue_messages[0];
        assert_eq!(msg1.id, "059f36b4-87a3-44ab-83d2-661975830a7d");
        assert_eq!(msg1.receipt_handle, "AQEBwJnKyrHigUMZj6rYigCgxlaS3SLy0a...");
        assert_eq!(msg1.source, "my-queue");
        assert_eq!(msg1.attempt_count, Some(1));

        // Verify timestamp conversion (1545082649183 ms -> 1545082649 seconds)
        let expected_timestamp = chrono::DateTime::from_timestamp(1545082649, 183_000_000).unwrap();
        assert_eq!(msg1.timestamp, expected_timestamp);

        // Verify payload (should be Text since "Test message." is not valid JSON)
        match &msg1.payload {
            MessagePayload::Text(text) => {
                assert_eq!(text, "Test message.");
            }
            _ => panic!("Expected Text payload, got {:?}", msg1.payload),
        }

        // Verify message attributes are correctly flattened
        assert_eq!(
            msg1.attributes.get("myAttribute"),
            Some(&"myValue".to_string())
        );

        // Test second message
        let msg2 = &queue_messages[1];
        assert_eq!(msg2.id, "2e1424d4-f796-459a-8184-9c92662be6da");
        assert_eq!(msg2.receipt_handle, "AQEBzWwaftRI0KuVm4tP+/7q1rGgNqicHq...");
        assert_eq!(msg2.source, "my-queue");
        assert_eq!(msg2.attempt_count, Some(1));

        // Verify second timestamp (1545082650636 ms -> 1545082650 seconds + 636ms)
        let expected_timestamp2 =
            chrono::DateTime::from_timestamp(1545082650, 636_000_000).unwrap();
        assert_eq!(msg2.timestamp, expected_timestamp2);

        // Second message has empty messageAttributes
        assert!(msg2.attributes.is_empty());
    }

    #[test]
    fn test_try_from_s3_event_multiple_records() {
        let json_data = json!({
          "Records": [
            {
              "eventVersion": "2.1",
              "eventSource": "aws:s3",
              "awsRegion": "us-east-1",
              "eventTime": "2025-04-09T12:15:30.123Z",
              "eventName": "ObjectCreated:Put",
               "userIdentity": { "principalId": "EXAMPLE" },
              "requestParameters": { "sourceIPAddress": "127.0.0.1" },
              "responseElements": { },
              "s3": {
                "s3SchemaVersion": "1.0",
                "configurationId": "config1",
                "bucket": { "name": "bucket1", "arn": "arn:aws:s3:::bucket1", "ownerIdentity": { "principalId": "EXAMPLE" }},
                "object": { "key": "key1", "size": 100, "eTag": "tag1" }
              }
            },
            {
              "eventVersion": "2.1",
              "eventSource": "aws:s3",
              "awsRegion": "us-west-2",
              "eventTime": "2025-04-09T13:00:00.456Z",
              "eventName": "ObjectRemoved:Delete",
               "userIdentity": { "principalId": "EXAMPLE" },
              "requestParameters": { "sourceIPAddress": "127.0.0.1" },
              "responseElements": { },
              "s3": {
                 "s3SchemaVersion": "1.0",
                "configurationId": "config2",
                "bucket": { "name": "bucket2", "arn": "arn:aws:s3:::bucket2", "ownerIdentity": { "principalId": "EXAMPLE" } },
                "object": { "key": "key2" }
              }
            }
          ]
        });
        let s3_event: S3Event = serde_json::from_value(json_data).unwrap();
        let storage_events: StorageEvents = s3_event_to_storage_events(s3_event).unwrap();

        assert_eq!(storage_events.0.len(), 2);

        let event1 = &storage_events.0[0];
        assert_eq!(event1.event_type, StorageEventType::Created);
        assert_eq!(event1.bucket_name, "bucket1");
        assert_eq!(event1.object_key, "key1");
        assert_eq!(event1.timestamp, parse_datetime("2025-04-09T12:15:30.123Z"));
        assert_eq!(event1.size, Some(100));
        assert_eq!(event1.etag, Some("tag1".to_string()));
        assert_eq!(event1.region, Some("us-east-1".to_string()));

        let event2 = &storage_events.0[1];
        assert_eq!(event2.event_type, StorageEventType::Deleted);
        assert_eq!(event2.bucket_name, "bucket2");
        assert_eq!(event2.object_key, "key2");
        assert_eq!(event2.timestamp, parse_datetime("2025-04-09T13:00:00.456Z"));
        assert!(event2.size.is_none());
        assert!(event2.etag.is_none());
        assert_eq!(event2.region, Some("us-west-2".to_string()));
    }
}
