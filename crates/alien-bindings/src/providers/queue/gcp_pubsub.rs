use super::{
    batch_end, encode_batch,
    storage_notifications::{gcp_queue_payload, GcpQueueDelivery},
};
use crate::error::{ErrorData, Result};
use crate::traits::{
    Binding, MessagePayload, Queue, QueueAckResult, QueueMessage, QueueSendResult, MAX_BATCH_SIZE,
    MAX_MESSAGE_BYTES,
};
use alien_error::{Context, IntoAlienError};
use alien_gcp_clients::pubsub::{
    AcknowledgeRequest, ModifyAckDeadlineRequest, PubSubApi, PubSubClient, PublishRequest,
    PubsubMessage, PullRequest,
};
use async_trait::async_trait;
use base64::prelude::*;
use std::fmt::{Debug, Formatter};

pub struct GcpPubSubQueue {
    topic: String,
    subscription: String,
    client: PubSubClient,
}

impl Debug for GcpPubSubQueue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GcpPubSubQueue")
            .field("topic", &self.topic)
            .field("subscription", &self.subscription)
            .finish()
    }
}

impl GcpPubSubQueue {
    pub async fn new(
        topic: String,
        subscription: String,
        gcp_config: alien_gcp_clients::GcpClientConfig,
    ) -> Result<Self> {
        Ok(Self {
            topic,
            subscription,
            client: PubSubClient::new(crate::http_client::create_http_client(), gcp_config),
        })
    }
}

impl Binding for GcpPubSubQueue {}

#[async_trait]
impl Queue for GcpPubSubQueue {
    async fn send(&self, _queue: &str, message: MessagePayload) -> Result<()> {
        let data = match message {
            MessagePayload::Json(v) => serde_json::to_vec(&v).into_alien_error().context(
                ErrorData::BindingSetupFailed {
                    binding_type: "queue.pubsub".to_string(),
                    reason: "Failed to serialize JSON payload".to_string(),
                },
            )?,
            MessagePayload::Text(s) => s.into_bytes(),
        };

        // Client-side validation: check message size
        if data.len() > MAX_MESSAGE_BYTES {
            return Err(alien_error::AlienError::new(
                ErrorData::BindingSetupFailed {
                    binding_type: "queue.pubsub".to_string(),
                    reason: format!(
                        "Message size {} bytes exceeds limit of {} bytes",
                        data.len(),
                        MAX_MESSAGE_BYTES
                    ),
                },
            ));
        }
        let msg = PubsubMessage {
            data: Some(base64::prelude::BASE64_STANDARD.encode(data)),
            attributes: None,
            message_id: None,
            publish_time: None,
            ordering_key: None,
        };
        let req = PublishRequest {
            messages: vec![msg],
        };
        self.client
            .publish(self.topic.clone(), req)
            .await
            .map(|_| ())
            .map_err(|e| {
                let reason = format!("Failed to publish to topic '{}': {}", self.topic, e);
                tracing::error!(%reason, "PubSub publish failed");
                alien_error::AlienError::new(ErrorData::BindingSetupFailed {
                    binding_type: "queue.pubsub".to_string(),
                    reason,
                })
            })
    }

    async fn send_batch(
        &self,
        _queue: &str,
        messages: Vec<MessagePayload>,
    ) -> Result<Vec<QueueSendResult>> {
        let (entries, mut results) = encode_batch(messages)?;
        let mut start = 0;
        while start < entries.len() {
            // Far below Pub/Sub's 10MB request limit, including base64 overhead.
            let end = batch_end(&entries, start, 1000, 256 * 1024);
            let chunk = &entries[start..end];
            let request = PublishRequest {
                messages: chunk
                    .iter()
                    .map(|(_, body)| PubsubMessage {
                        data: Some(BASE64_STANDARD.encode(body)),
                        attributes: None,
                        message_id: None,
                        publish_time: None,
                        ordering_key: None,
                    })
                    .collect(),
            };
            let result = self.client.publish_once(self.topic.clone(), request).await;
            for (index, _) in chunk {
                results[*index] = match &result {
                    Ok(response) if response.message_ids.len() == chunk.len() => {
                        QueueSendResult::Sent
                    }
                    Ok(_) => QueueSendResult::Unknown {
                        code: "QUEUE_BATCH_RESPONSE_INVALID".to_string(),
                        message: "Pub/Sub returned an unexpected number of message IDs".to_string(),
                    },
                    Err(error) => QueueSendResult::Unknown {
                        code: error.code.clone(),
                        message: error.to_string(),
                    },
                };
            }
            start = end;
        }
        Ok(results)
    }

    async fn receive(&self, _queue: &str, max_messages: usize) -> Result<Vec<QueueMessage>> {
        // Client-side validation: check batch size
        if max_messages == 0 || max_messages > MAX_BATCH_SIZE {
            return Err(alien_error::AlienError::new(
                ErrorData::BindingSetupFailed {
                    binding_type: "queue.pubsub".to_string(),
                    reason: format!(
                        "Batch size {} is invalid. Must be between 1 and {}",
                        max_messages, MAX_BATCH_SIZE
                    ),
                },
            ));
        }

        let req = PullRequest {
            max_messages: Some(std::cmp::min(max_messages, MAX_BATCH_SIZE) as i32),
            return_immediately: None,
            allow_excess_messages: None,
        };

        let response = self
            .client
            .pull(self.subscription.clone(), req)
            .await
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.pubsub".to_string(),
                reason: "Failed to pull messages".to_string(),
            })?;

        // Set ack deadline to 30s for all received messages
        if !response.received_messages.is_empty() {
            let ack_ids: Vec<String> = response
                .received_messages
                .iter()
                .map(|msg| msg.ack_id.clone())
                .collect();

            let modify_req = ModifyAckDeadlineRequest {
                ack_ids,
                ack_deadline_seconds: 30,
            };

            self.client
                .modify_ack_deadline(self.subscription.clone(), modify_req)
                .await
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "queue.pubsub".to_string(),
                    reason: "Failed to establish received message leases".to_string(),
                })?;
        }

        let mut messages = Vec::new();
        let mut filtered_receipts = Vec::new();
        for received in response.received_messages {
            let message = received.message;
            let data = BASE64_STANDARD
                .decode(message.data.unwrap_or_default())
                .into_alien_error()
                .context(ErrorData::QueueProviderResponseInvalid {
                    reason: "Pub/Sub message data is not valid base64".to_string(),
                })?;
            let attributes = message.attributes.unwrap_or_default();
            match gcp_queue_payload(&data, &attributes) {
                GcpQueueDelivery::Deliver(payload) => messages.push(QueueMessage {
                    payload: match payload {
                        alien_core::MessagePayload::Json(value) => MessagePayload::Json(value),
                        alien_core::MessagePayload::Text(text) => MessagePayload::Text(text),
                    },
                    receipt_handle: received.ack_id,
                    // Pub/Sub only populates delivery attempts with a dead-letter policy.
                    attempt: 1,
                }),
                GcpQueueDelivery::Filtered => filtered_receipts.push(received.ack_id),
            }
        }
        if !filtered_receipts.is_empty() {
            self.client
                .acknowledge(
                    self.subscription.clone(),
                    AcknowledgeRequest {
                        ack_ids: filtered_receipts,
                    },
                )
                .await
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "queue.pubsub".to_string(),
                    reason: "Failed to acknowledge suffix-filtered storage notifications"
                        .to_string(),
                })?;
        }

        Ok(messages)
    }

    async fn ack(&self, _queue: &str, receipt_handle: &str) -> Result<()> {
        let req = AcknowledgeRequest {
            ack_ids: vec![receipt_handle.to_string()],
        };

        self.client
            .acknowledge(self.subscription.clone(), req)
            .await
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.pubsub".to_string(),
                reason: "Failed to acknowledge message".to_string(),
            })
    }

    async fn ack_batch(&self, _queue: &str, receipts: &[String]) -> Result<Vec<QueueAckResult>> {
        let mut results = Vec::with_capacity(receipts.len());
        for chunk in receipts.chunks(100) {
            let outcome = match self
                .client
                .acknowledge(
                    self.subscription.clone(),
                    AcknowledgeRequest {
                        ack_ids: chunk.to_vec(),
                    },
                )
                .await
            {
                Ok(_) => QueueAckResult::Acknowledged,
                Err(error) => QueueAckResult::Failed {
                    code: error.code.clone(),
                    message: error.to_string(),
                },
            };
            results.extend(std::iter::repeat_n(outcome, chunk.len()));
        }
        Ok(results)
    }

    async fn renew_lease(
        &self,
        _queue: &str,
        receipt_handle: &str,
        duration_seconds: u32,
    ) -> Result<()> {
        crate::traits::validate_lease_duration(duration_seconds)?;
        self.client
            .modify_ack_deadline(
                self.subscription.clone(),
                ModifyAckDeadlineRequest {
                    ack_ids: vec![receipt_handle.to_string()],
                    ack_deadline_seconds: duration_seconds as i32,
                },
            )
            .await
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.pubsub".to_string(),
                reason: "Failed to renew delivery lease".to_string(),
            })
    }

    async fn nack(&self, _queue: &str, receipt_handle: &str) -> Result<()> {
        // Pub/Sub nack = ModifyAckDeadline(deadline=0): drop the lease so the
        // message is immediately eligible for redelivery.
        let req = ModifyAckDeadlineRequest {
            ack_ids: vec![receipt_handle.to_string()],
            ack_deadline_seconds: 0,
        };

        self.client
            .modify_ack_deadline(self.subscription.clone(), req)
            .await
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.pubsub".to_string(),
                reason: "Failed to nack message".to_string(),
            })
    }

    async fn purge(&self, _queue: &str) -> Result<()> {
        // Pub/Sub purge is a subscription Seek to the current time. The
        // alien-gcp-clients PubSubApi wrapper does not expose Seek, so this
        // fails explicitly.
        Err(alien_error::AlienError::new(
            ErrorData::OperationNotSupported {
                operation: "queue.purge".to_string(),
                reason: "GCP Pub/Sub purge requires subscription Seek, which the Pub/Sub client does not expose".to_string(),
            },
        ))
    }
}

#[cfg(test)]
mod notification_tests {
    use super::*;
    use alien_core::{GcpCredentials, GcpServiceOverrides, StorageEvent};
    use httpmock::prelude::*;
    use serde_json::json;
    use std::collections::HashMap;

    async fn queue(server: &MockServer) -> GcpPubSubQueue {
        GcpPubSubQueue::new(
            "jobs".into(),
            "jobs-sub".into(),
            alien_gcp_clients::GcpClientConfig {
                project_id: "test".into(),
                region: "us-central1".into(),
                credentials: GcpCredentials::AccessToken {
                    token: "test-token".into(),
                },
                service_overrides: Some(GcpServiceOverrides {
                    endpoints: HashMap::from([("pubsub".into(), server.base_url())]),
                }),
                project_number: None,
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn receive_normalizes_matches_and_acknowledges_only_excluded_notifications() {
        for ack_status in [200, 403] {
            let server = MockServer::start_async().await;
            let notifications = ["in/matching.json", "in/excluded.txt"].into_iter().enumerate().map(|(i, key)| json!({
                "ackId":format!("receipt-{i}"), "message": {
                    "messageId": format!("id-{i}"),
                    "data":BASE64_STANDARD.encode(json!({"bucket":"source","name":key,"size":"7"}).to_string()),
                    "attributes":{"notificationConfig":"config-1","storageNotificationRule":"source/0","eventType":"OBJECT_FINALIZE","payloadFormat":"JSON_API_V1","eventTime":"2026-10-30T08:00:00Z","storageEventSuffix":".json"}
                }
            })).collect::<Vec<_>>();
            let pull = server
                .mock_async(|when, then| {
                    when.method(POST)
                        .path("/projects/test/subscriptions/jobs-sub:pull")
                        .json_body(json!({"maxMessages":10}));
                    then.status(200)
                        .json_body(json!({"receivedMessages":notifications}));
                })
                .await;
            let lease = server
                .mock_async(|when, then| {
                    when.method(POST)
                        .path("/projects/test/subscriptions/jobs-sub:modifyAckDeadline")
                        .json_body(
                            json!({"ackIds":["receipt-0","receipt-1"],"ackDeadlineSeconds":30}),
                        );
                    then.status(200).json_body(json!({}));
                })
                .await;
            let ack = server.mock_async(|when, then| {
                when.method(POST).path("/projects/test/subscriptions/jobs-sub:acknowledge")
                    .json_body(json!({"ackIds":["receipt-1"]}));
                then.status(ack_status).json_body(if ack_status == 200 { json!({}) } else {json!({"error":{"code":403,"message":"denied","status":"PERMISSION_DENIED"}})});
            }).await;
            let result = queue(&server).await.receive("jobs", 10).await;
            if ack_status == 200 {
                let messages = result.unwrap();
                assert_eq!(messages.len(), 1);
                assert_eq!(messages[0].receipt_handle, "receipt-0");
                assert_eq!(messages[0].attempt, 1);
                let MessagePayload::Json(value) = &messages[0].payload else {
                    panic!("expected JSON")
                };
                let event: StorageEvent = serde_json::from_value(value.clone()).unwrap();
                assert_eq!(event.object_key, "in/matching.json");
                assert_eq!(event.bucket_name, "source");
                assert_eq!(event.size, Some(7));
            } else {
                assert_eq!(result.unwrap_err().code, "BINDING_SETUP_FAILED");
            }
            pull.assert_hits_async(1).await;
            lease.assert_hits_async(1).await;
            if ack_status == 200 {
                ack.assert_hits_async(1).await;
            } else {
                // The existing client retry policy can attempt a failed request
                // more than once; every matched attempt targets only the exclusion.
                assert!(ack.hits_async().await > 0);
            }
        }
    }

    #[tokio::test]
    async fn receive_rejects_malformed_base64_without_acknowledging() {
        let server = MockServer::start_async().await;
        let pull = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/projects/test/subscriptions/jobs-sub:pull");
                then.status(200).json_body(
                    json!({"receivedMessages":[{"ackId":"receipt", "message":{"data":"%%%"}}]}),
                );
            })
            .await;
        let lease = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/projects/test/subscriptions/jobs-sub:modifyAckDeadline");
                then.status(200).json_body(json!({}));
            })
            .await;
        let ack = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/projects/test/subscriptions/jobs-sub:acknowledge");
                then.status(200).json_body(json!({}));
            })
            .await;
        assert_eq!(
            queue(&server)
                .await
                .receive("jobs", 1)
                .await
                .unwrap_err()
                .code,
            "QUEUE_PROVIDER_RESPONSE_INVALID"
        );
        pull.assert_hits_async(1).await;
        lease.assert_hits_async(1).await;
        ack.assert_hits_async(0).await;
    }
}
