use crate::error::{ErrorData, Result};
use crate::traits::{
    Binding, MessagePayload, Queue, QueueMessage, MAX_BATCH_SIZE, MAX_MESSAGE_BYTES,
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
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.pubsub".to_string(),
                reason: "Failed to publish".to_string(),
            })
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

            let _ = self
                .client
                .modify_ack_deadline(self.subscription.clone(), modify_req)
                .await;
        }

        let messages = response
            .received_messages
            .into_iter()
            .filter_map(|received_msg| {
                let message = received_msg.message;
                let raw_data = message.data.unwrap_or_default();
                let data = base64::prelude::BASE64_STANDARD.decode(&raw_data).ok()?;
                let raw = String::from_utf8_lossy(&data).into_owned();
                let payload = serde_json::from_str::<serde_json::Value>(&raw)
                    .map(MessagePayload::Json)
                    .unwrap_or_else(|_| MessagePayload::Text(raw));
                Some(QueueMessage {
                    payload,
                    receipt_handle: received_msg.ack_id,
                })
            })
            .collect();

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
}
