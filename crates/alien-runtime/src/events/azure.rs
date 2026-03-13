use crate::error::{Error, ErrorData};
use alien_core::{MessagePayload, QueueMessage, StorageEvent, StorageEventType, StorageEvents};
use alien_error::{AlienError, Context, IntoAlienError};
use base64::{engine::general_purpose, Engine};
use chrono::{DateTime, Utc};
use cloudevents::AttributesReader;
use cloudevents::{event::ExtensionValue, Data, Event};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Dapr CloudEvent structure for Azure Service Bus
/// Note: The actual message payload is in the CloudEvent's `data` field directly,
/// and metadata like topic/pubsubname are CloudEvent extension attributes
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DaprCloudEventExtensions {
    /// The topic/queue name
    pub topic: Option<String>,
    /// The Dapr component name  
    pub pubsubname: Option<String>,
}

/// Azure Blob Storage event data structure
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AzureBlobStorageData {
    /// The API operation that triggered the event
    pub api: String,
    /// Client request ID
    pub client_request_id: Option<String>,
    /// Request ID
    pub request_id: Option<String>,
    /// ETag of the blob
    pub e_tag: Option<String>,
    /// Content type of the blob
    pub content_type: Option<String>,
    /// Content length of the blob
    pub content_length: Option<u64>,
    /// Blob type (BlockBlob, PageBlob, etc.)
    pub blob_type: Option<String>,
    /// URL of the blob
    pub url: String,
    /// Sequencer for ordering events
    pub sequencer: Option<String>,
    /// Storage diagnostics
    pub storage_diagnostics: Option<StorageDiagnostics>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StorageDiagnostics {
    pub batch_id: Option<String>,
}

// Convert Dapr CloudEvents to QueueMessage
pub fn dapr_cloudevent_to_queue_messages(event: Event) -> Result<Vec<QueueMessage>, Error> {
    let event_type_str = event.ty();

    // Only handle Dapr Service Bus events
    if event_type_str != "com.dapr.event.sent" {
        return Err(AlienError::new(ErrorData::EventProcessingFailed {
            event_type: event_type_str.to_string(),
            reason: "Not a Dapr Service Bus event".to_string(),
        }));
    }

    let timestamp = event.time().cloned().ok_or_else(|| {
        AlienError::new(ErrorData::EventProcessingFailed {
            event_type: "Azure Service Bus Event".to_string(),
            reason: "CloudEvent missing timestamp".to_string(),
        })
    })?;

    // Extract topic and pubsubname from CloudEvent extension attributes
    let topic = event
        .extension("topic")
        .and_then(|v| match v {
            ExtensionValue::String(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "unknown-topic".to_string());

    let _pubsubname = event
        .extension("pubsubname")
        .and_then(|v| match v {
            ExtensionValue::String(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "unknown-pubsub".to_string());

    // Extract Azure Service Bus metadata from CloudEvent extension attributes
    let mut attributes = HashMap::new();

    // Helper function to extract string extensions
    let extract_string_extension = |name: &str| -> Option<String> {
        event.extension(name).and_then(|v| match v {
            ExtensionValue::String(s) => Some(s.clone()),
            _ => None,
        })
    };

    // Settable metadata fields
    if let Some(message_id) = extract_string_extension("messageid") {
        attributes.insert("MessageId".to_string(), message_id);
    }
    if let Some(correlation_id) = extract_string_extension("correlationid") {
        attributes.insert("CorrelationId".to_string(), correlation_id);
    }
    if let Some(session_id) = extract_string_extension("sessionid") {
        attributes.insert("SessionId".to_string(), session_id);
    }
    if let Some(label) = extract_string_extension("label") {
        attributes.insert("Label".to_string(), label);
    }
    if let Some(reply_to) = extract_string_extension("replyto") {
        attributes.insert("ReplyTo".to_string(), reply_to);
    }
    if let Some(partition_key) = extract_string_extension("partitionkey") {
        attributes.insert("PartitionKey".to_string(), partition_key);
    }
    if let Some(to) = extract_string_extension("to") {
        attributes.insert("To".to_string(), to);
    }
    if let Some(content_type) = extract_string_extension("contenttype") {
        attributes.insert("ContentType".to_string(), content_type);
    }

    // Read-only metadata fields
    if let Some(delivery_count) = extract_string_extension("deliverycount") {
        attributes.insert("DeliveryCount".to_string(), delivery_count);
    }
    if let Some(locked_until) = extract_string_extension("lockeduntilutc") {
        attributes.insert("LockedUntilUtc".to_string(), locked_until);
    }
    if let Some(lock_token) = extract_string_extension("locktoken") {
        attributes.insert("LockToken".to_string(), lock_token);
    }
    if let Some(enqueued_time) = extract_string_extension("enqueuedtimeutc") {
        attributes.insert("EnqueuedTimeUtc".to_string(), enqueued_time);
    }
    if let Some(sequence_number) = extract_string_extension("sequencenumber") {
        attributes.insert("SequenceNumber".to_string(), sequence_number);
    }

    // Extract delivery count for attempt_count (convert to number if available)
    let attempt_count =
        extract_string_extension("deliverycount").and_then(|s| s.parse::<u32>().ok());

    // The data field contains the actual message payload directly
    let data = event.data().ok_or_else(|| {
        AlienError::new(ErrorData::EventProcessingFailed {
            event_type: "Azure Service Bus Event".to_string(),
            reason: "CloudEvent missing data payload".to_string(),
        })
    })?;

    // Parse the message payload - it can be JSON or text
    let payload = match data {
        Data::Json(value) => MessagePayload::Json(value.clone()),
        Data::Binary(bytes) => {
            // Try to parse as JSON first, fall back to treating as text
            match serde_json::from_slice::<serde_json::Value>(bytes.as_slice()) {
                Ok(json_value) => MessagePayload::Json(json_value),
                Err(_) => {
                    // Try to parse as UTF-8 text, fall back to base64 encoding as text if it fails
                    match String::from_utf8(bytes.to_vec()) {
                        Ok(text) => MessagePayload::Text(text),
                        Err(_) => {
                            // Encode binary data as base64 text
                            let encoded = general_purpose::STANDARD.encode(bytes);
                            MessagePayload::Text(encoded)
                        }
                    }
                }
            }
        }
        Data::String(s) => {
            // Try to parse as JSON first, fall back to treating as text
            match serde_json::from_str::<serde_json::Value>(s.as_str()) {
                Ok(json_value) => MessagePayload::Json(json_value),
                Err(_) => MessagePayload::Text(s.clone()),
            }
        }
    };

    // Use Azure Service Bus MessageId if available, otherwise fall back to CloudEvent ID
    let message_id =
        extract_string_extension("messageid").unwrap_or_else(|| event.id().to_string());

    let queue_message = QueueMessage {
        id: message_id,
        payload,
        receipt_handle: event.id().to_string(), // Use CloudEvent ID as receipt handle
        timestamp,
        source: topic, // Use topic as source queue name
        attributes,    // Include Azure Service Bus metadata
        attempt_count, // Use delivery count from Azure Service Bus metadata
    };

    Ok(vec![queue_message])
}

// Convert Azure Blob Storage CloudEvents to StorageEvents
pub fn azure_storage_cloudevent_to_storage_events(event: Event) -> Result<StorageEvents, Error> {
    let event_type_str = event.ty();

    // Only handle Azure Blob Storage events
    if !event_type_str.starts_with("Microsoft.Storage.Blob") {
        return Err(AlienError::new(ErrorData::EventProcessingFailed {
            event_type: event_type_str.to_string(),
            reason: "Not an Azure Blob Storage event".to_string(),
        }));
    }

    let timestamp = event.time().cloned().ok_or_else(|| {
        AlienError::new(ErrorData::EventProcessingFailed {
            event_type: "Azure Blob Storage Event".to_string(),
            reason: "CloudEvent missing timestamp".to_string(),
        })
    })?;

    let data = event.data().ok_or_else(|| {
        AlienError::new(ErrorData::EventProcessingFailed {
            event_type: "Azure Blob Storage Event".to_string(),
            reason: "CloudEvent missing data payload".to_string(),
        })
    })?;

    let expected_content_type = event.datacontenttype();

    let storage_data: AzureBlobStorageData = match data {
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

    // Determine event type
    let alien_event_type = match event_type_str {
        "Microsoft.Storage.BlobCreated" => StorageEventType::Created,
        "Microsoft.Storage.BlobDeleted" => StorageEventType::Deleted,
        "Microsoft.Storage.BlobTierChanged" => StorageEventType::TierChanged,
        _ => {
            return Err(AlienError::new(ErrorData::EventProcessingFailed {
                event_type: event_type_str.to_string(),
                reason: "Unsupported Azure Blob Storage event type".to_string(),
            }));
        }
    };

    // Parse blob URL to extract bucket and object key
    let url_parts = storage_data.url.split('/').collect::<Vec<&str>>();
    if url_parts.len() < 4 {
        return Err(AlienError::new(ErrorData::EventProcessingFailed {
            event_type: event_type_str.to_string(),
            reason: format!("Invalid blob URL format: {}", storage_data.url),
        }));
    }

    // URL format: https://{account}.blob.core.windows.net/{container}/{blob}
    let bucket_name = url_parts[3].to_string(); // container name
    let object_key = url_parts[4..].join("/"); // blob path (may contain slashes)

    // Extract subject for additional context (like region) - currently unused
    let _subject = event.subject().unwrap_or("");

    let storage_event = StorageEvent {
        event_type: alien_event_type,
        bucket_name,
        object_key,
        timestamp,
        size: storage_data.content_length,
        etag: storage_data.e_tag,
        content_type: storage_data.content_type,
        metadata: HashMap::new(), // Azure events don't typically include blob metadata
        copy_source: None,
        previous_tier: None,
        current_tier: None, // Not available in basic Azure blob events
        region: None,       // Could be extracted from account URL if needed
        version_id: None,   // Azure blob versioning info not in basic events
    };

    Ok(StorageEvents(vec![storage_event]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloudevents::{EventBuilder as _, EventBuilderV10};
    use serde_json::json;

    fn parse_datetime(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn test_try_from_dapr_service_bus_cloudevent() {
        let event_time_str = "2023-01-01T12:00:00Z";
        let event_time = parse_datetime(event_time_str);

        // Actual message payload - simple JSON object
        let event_data = json!({
            "orderId": "A-123",
            "total": 99.5
        });

        let cloud_event = EventBuilderV10::new()
            .id("ce-id-12345")
            .ty("com.dapr.event.sent")
            .source("your-publisher")
            .time(event_time)
            .data("application/json", event_data)
            .extension("topic", "<your-queue-name>")
            .extension("pubsubname", "servicebus-pubsub")
            .build()
            .unwrap();

        let queue_messages: Vec<QueueMessage> =
            dapr_cloudevent_to_queue_messages(cloud_event).unwrap();

        assert_eq!(queue_messages.len(), 1);
        let msg = &queue_messages[0];

        assert_eq!(msg.id, "ce-id-12345"); // Uses CloudEvent ID
        assert_eq!(msg.receipt_handle, "ce-id-12345"); // Uses CloudEvent ID
        assert_eq!(msg.source, "<your-queue-name>"); // From topic extension
        assert_eq!(msg.timestamp, event_time); // Uses CloudEvent time
        assert_eq!(msg.attempt_count, None); // Not available in basic Dapr format

        // Verify payload
        match &msg.payload {
            MessagePayload::Json(json_value) => {
                assert_eq!(json_value["orderId"], "A-123");
                assert_eq!(json_value["total"], 99.5);
            }
            _ => panic!("Expected Json payload, got {:?}", msg.payload),
        }

        // Verify attributes are empty in basic Dapr format
        assert!(msg.attributes.is_empty());
    }

    #[test]
    fn test_try_from_dapr_text_payload() {
        let event_time_str = "2023-01-01T12:00:00Z";
        let event_time = parse_datetime(event_time_str);

        // Simple text message payload
        let text_message = "Hello from Service Bus!";

        let cloud_event = EventBuilderV10::new()
            .id("ce-text-456")
            .ty("com.dapr.event.sent")
            .source("azure-servicebus")
            .time(event_time)
            .data("text/plain", text_message)
            .extension("topic", "orders-queue")
            .extension("pubsubname", "servicebus-component")
            .build()
            .unwrap();

        let queue_messages: Vec<QueueMessage> =
            dapr_cloudevent_to_queue_messages(cloud_event).unwrap();
        let msg = &queue_messages[0];

        // Verify text payload
        match &msg.payload {
            MessagePayload::Text(text) => {
                assert_eq!(text, "Hello from Service Bus!");
            }
            _ => panic!("Expected Text payload, got {:?}", msg.payload),
        }

        assert_eq!(msg.source, "orders-queue");
        assert_eq!(msg.attempt_count, None);
    }

    #[test]
    fn test_try_from_unsupported_event_type() {
        let event_time = Utc::now();
        let cloud_event = EventBuilderV10::new()
            .id("test-unsupported")
            .ty("microsoft.storage.blob.created") // Not a Dapr event
            .source("azure-storage")
            .time(event_time)
            .data("application/json", json!({}))
            .build()
            .unwrap();

        let result = dapr_cloudevent_to_queue_messages(cloud_event);
        assert!(result.is_err());

        let error = result.err().unwrap();
        match &error.error {
            Some(ErrorData::EventProcessingFailed { event_type, reason }) => {
                assert_eq!(event_type, "microsoft.storage.blob.created");
                assert_eq!(reason, "Not a Dapr Service Bus event");
            }
            _ => panic!(
                "Expected EventProcessingFailed error, got {:?}",
                error.error
            ),
        }
    }

    #[test]
    fn test_missing_topic_extension_fallback() {
        let event_time_str = "2023-01-01T12:00:00Z";
        let event_time = parse_datetime(event_time_str);

        let event_data = json!({
            "orderId": "test-123",
            "message": "Test message"
        });

        // CloudEvent without topic extension
        let cloud_event = EventBuilderV10::new()
            .id("ce-fallback")
            .ty("com.dapr.event.sent")
            .source("azure-servicebus")
            .time(event_time)
            .data("application/json", event_data)
            .build()
            .unwrap();

        let queue_messages: Vec<QueueMessage> =
            dapr_cloudevent_to_queue_messages(cloud_event).unwrap();
        let msg = &queue_messages[0];

        // Should use fallback topic name when extension is missing
        assert_eq!(msg.source, "unknown-topic");
        assert_eq!(msg.timestamp, event_time);
    }

    #[test]
    fn test_azure_blob_storage_cloudevent() {
        let event_time_str = "2024-11-18T15:13:39.4589254Z";
        let event_time = parse_datetime(event_time_str);

        let event_data = json!({
            "api": "PutBlockList",
            "clientRequestId": "4c5dd7fb-2c48-4a27-bb30-5361b5de920a",
            "requestId": "9aeb0fdf-c01e-0131-0922-9eb549000000",
            "eTag": "0x8D76C39E4407333",
            "contentType": "image/png",
            "contentLength": 30699,
            "blobType": "BlockBlob",
            "url": "https://teststorage.blob.core.windows.net/test-container/new-file.png",
            "sequencer": "000000000000000000000000000099240000000000c41c18",
            "storageDiagnostics": {
                "batchId": "681fe319-3006-00a8-0022-9e7cde000000"
            }
        });

        let cloud_event = EventBuilderV10::new()
            .id("9aeb0fdf-c01e-0131-0922-9eb54906e209")
            .ty("Microsoft.Storage.BlobCreated")
            .source("/subscriptions/sub-id/resourceGroups/rg/providers/Microsoft.Storage/storageAccounts/teststorage")
            .subject("blobServices/default/containers/test-container/blobs/new-file.png")
            .time(event_time)
            .data("application/json", event_data)
            .build()
            .unwrap();

        let storage_events: StorageEvents =
            azure_storage_cloudevent_to_storage_events(cloud_event).unwrap();

        assert_eq!(storage_events.0.len(), 1);
        let event = &storage_events.0[0];

        assert_eq!(event.event_type, StorageEventType::Created);
        assert_eq!(event.bucket_name, "test-container");
        assert_eq!(event.object_key, "new-file.png");
        assert_eq!(event.timestamp, event_time);
        assert_eq!(event.size, Some(30699));
        assert_eq!(event.etag, Some("0x8D76C39E4407333".to_string()));
        assert_eq!(event.content_type, Some("image/png".to_string()));
    }

    #[test]
    fn test_azure_unsupported_storage_event() {
        let event_time = Utc::now();
        let cloud_event = EventBuilderV10::new()
            .id("test-unsupported")
            .ty("Microsoft.EventGrid.SubscriptionValidationEvent") // Not a blob storage event
            .source("azure-eventgrid")
            .time(event_time)
            .data("application/json", json!({}))
            .build()
            .unwrap();

        let result = azure_storage_cloudevent_to_storage_events(cloud_event);
        assert!(result.is_err());

        let error = result.err().unwrap();
        match &error.error {
            Some(ErrorData::EventProcessingFailed { event_type, reason }) => {
                assert_eq!(
                    event_type,
                    "Microsoft.EventGrid.SubscriptionValidationEvent"
                );
                assert_eq!(reason, "Not an Azure Blob Storage event");
            }
            _ => panic!(
                "Expected EventProcessingFailed error, got {:?}",
                error.error
            ),
        }
    }
}
