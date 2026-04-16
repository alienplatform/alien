use crate::error::{ErrorData, Result};
use crate::traits::{
    Binding, MessagePayload, Queue, QueueMessage, MAX_BATCH_SIZE, MAX_MESSAGE_BYTES,
};
use alien_aws_clients::sqs::{
    DeleteMessageRequest, ReceiveMessageRequest, SendMessageRequest, SqsApi, SqsClient,
};
use alien_error::{Context, ContextError, IntoAlienError};
use async_trait::async_trait;
use std::fmt::{Debug, Formatter};

pub struct AwsSqsQueue {
    queue_url: String,
    client: SqsClient,
}

impl Debug for AwsSqsQueue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AwsSqsQueue")
            .field("queue_url", &self.queue_url)
            .finish()
    }
}

impl AwsSqsQueue {
    pub fn new(queue_url: String, client: SqsClient) -> Self {
        Self { queue_url, client }
    }
}

impl Binding for AwsSqsQueue {}

#[async_trait]
impl Queue for AwsSqsQueue {
    async fn send(&self, _queue: &str, message: MessagePayload) -> Result<()> {
        let (body, _ct) = match message {
            MessagePayload::Json(v) => (
                serde_json::to_string(&v).into_alien_error().context(
                    ErrorData::BindingSetupFailed {
                        binding_type: "queue.sqs".to_string(),
                        reason: "Failed to serialize JSON payload".to_string(),
                    },
                )?,
                "application/json".to_string(),
            ),
            MessagePayload::Text(s) => (s, "text/plain; charset=utf-8".to_string()),
        };

        // Client-side validation: check message size
        if body.len() > MAX_MESSAGE_BYTES {
            return Err(alien_error::AlienError::new(
                ErrorData::BindingSetupFailed {
                    binding_type: "queue.sqs".to_string(),
                    reason: format!(
                        "Message size {} bytes exceeds limit of {} bytes",
                        body.len(),
                        MAX_MESSAGE_BYTES
                    ),
                },
            ));
        }

        let req = SendMessageRequest::builder().message_body(body).build();
        self.client
            .send_message(&self.queue_url, req)
            .await
            .map(|_| ())
            .map_err(|e| {
                e.context(ErrorData::BindingSetupFailed {
                    binding_type: "queue.sqs".to_string(),
                    reason: "Failed to send message".to_string(),
                })
            })
    }

    async fn receive(&self, _queue: &str, max_messages: usize) -> Result<Vec<QueueMessage>> {
        // Client-side validation: check batch size
        if max_messages == 0 || max_messages > MAX_BATCH_SIZE {
            return Err(alien_error::AlienError::new(
                ErrorData::BindingSetupFailed {
                    binding_type: "queue.sqs".to_string(),
                    reason: format!(
                        "Batch size {} is invalid. Must be between 1 and {}",
                        max_messages, MAX_BATCH_SIZE
                    ),
                },
            ));
        }

        let req = ReceiveMessageRequest::builder()
            .maybe_max_number_of_messages(Some(max_messages as i32))
            .maybe_wait_time_seconds(Some(20))
            .build();
        let resp = self
            .client
            .receive_message(&self.queue_url, req)
            .await
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.sqs".to_string(),
                reason: "Failed to receive".to_string(),
            })?;
        let msgs = resp
            .receive_message_result
            .messages
            .into_iter()
            .map(|m| {
                let raw = m.body;
                let payload = serde_json::from_str::<serde_json::Value>(&raw)
                    .map(MessagePayload::Json)
                    .unwrap_or(MessagePayload::Text(raw));
                QueueMessage {
                    payload,
                    receipt_handle: m.receipt_handle,
                }
            })
            .collect();
        Ok(msgs)
    }

    async fn ack(&self, _queue: &str, receipt_handle: &str) -> Result<()> {
        let req = DeleteMessageRequest::builder()
            .receipt_handle(receipt_handle.to_string())
            .build();
        self.client
            .delete_message(&self.queue_url, req)
            .await
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.sqs".to_string(),
                reason: "Failed to delete message".to_string(),
            })
    }
}
