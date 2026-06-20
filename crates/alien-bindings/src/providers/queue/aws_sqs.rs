use crate::error::{ErrorData, Result};
use crate::traits::{
    Binding, MessagePayload, Queue, QueueMessage, MAX_BATCH_SIZE, MAX_MESSAGE_BYTES,
};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;

/// Message returned by the SQS adapter.
#[derive(Debug, Clone)]
pub struct SqsQueueMessage {
    /// Message body.
    pub body: String,
    /// Receipt handle used for acknowledging the message.
    pub receipt_handle: String,
}

/// Minimal SQS operations required by the queue binding.
#[async_trait]
pub trait SqsQueueClient: Debug + Send + Sync {
    /// Send a message to a queue.
    async fn send_message(&self, queue_url: &str, body: String) -> Result<()>;

    /// Receive messages from a queue.
    async fn receive_messages(
        &self,
        queue_url: &str,
        max_messages: i32,
        wait_time_seconds: i32,
    ) -> Result<Vec<SqsQueueMessage>>;

    /// Delete a message from a queue by receipt handle.
    async fn delete_message(&self, queue_url: &str, receipt_handle: &str) -> Result<()>;
}

#[async_trait]
impl SqsQueueClient for aws_sdk_sqs::Client {
    async fn send_message(&self, queue_url: &str, body: String) -> Result<()> {
        self.send_message()
            .queue_url(queue_url)
            .message_body(body)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.sqs".to_string(),
                reason: "Failed to send message".to_string(),
            })?;

        Ok(())
    }

    async fn receive_messages(
        &self,
        queue_url: &str,
        max_messages: i32,
        wait_time_seconds: i32,
    ) -> Result<Vec<SqsQueueMessage>> {
        let response = self
            .receive_message()
            .queue_url(queue_url)
            .max_number_of_messages(max_messages)
            .wait_time_seconds(wait_time_seconds)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.sqs".to_string(),
                reason: "Failed to receive".to_string(),
            })?;

        Ok(response
            .messages()
            .iter()
            .filter_map(|message| {
                Some(SqsQueueMessage {
                    body: message.body()?.to_string(),
                    receipt_handle: message.receipt_handle()?.to_string(),
                })
            })
            .collect())
    }

    async fn delete_message(&self, queue_url: &str, receipt_handle: &str) -> Result<()> {
        self.delete_message()
            .queue_url(queue_url)
            .receipt_handle(receipt_handle)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.sqs".to_string(),
                reason: "Failed to delete message".to_string(),
            })?;

        Ok(())
    }
}

pub struct AwsSqsQueue {
    queue_url: String,
    client: Arc<dyn SqsQueueClient>,
}

impl Debug for AwsSqsQueue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AwsSqsQueue")
            .field("queue_url", &self.queue_url)
            .finish()
    }
}

impl AwsSqsQueue {
    pub fn new(queue_url: String, client: Arc<dyn SqsQueueClient>) -> Self {
        Self { queue_url, client }
    }
}

impl Binding for AwsSqsQueue {}

#[async_trait]
impl Queue for AwsSqsQueue {
    async fn send(&self, _queue: &str, message: MessagePayload) -> Result<()> {
        let (body, _content_type) = match message {
            MessagePayload::Json(value) => (
                serde_json::to_string(&value).into_alien_error().context(
                    ErrorData::BindingSetupFailed {
                        binding_type: "queue.sqs".to_string(),
                        reason: "Failed to serialize JSON payload".to_string(),
                    },
                )?,
                "application/json".to_string(),
            ),
            MessagePayload::Text(text) => (text, "text/plain; charset=utf-8".to_string()),
        };

        if body.len() > MAX_MESSAGE_BYTES {
            return Err(AlienError::new(ErrorData::BindingSetupFailed {
                binding_type: "queue.sqs".to_string(),
                reason: format!(
                    "Message size {} bytes exceeds limit of {} bytes",
                    body.len(),
                    MAX_MESSAGE_BYTES
                ),
            }));
        }

        self.client.send_message(&self.queue_url, body).await
    }

    async fn receive(&self, _queue: &str, max_messages: usize) -> Result<Vec<QueueMessage>> {
        if max_messages == 0 || max_messages > MAX_BATCH_SIZE {
            return Err(AlienError::new(ErrorData::BindingSetupFailed {
                binding_type: "queue.sqs".to_string(),
                reason: format!(
                    "Batch size {} is invalid. Must be between 1 and {}",
                    max_messages, MAX_BATCH_SIZE
                ),
            }));
        }

        let messages = self
            .client
            .receive_messages(&self.queue_url, max_messages as i32, 20)
            .await?;

        Ok(messages
            .into_iter()
            .map(|message| {
                let payload = serde_json::from_str::<serde_json::Value>(&message.body)
                    .map(MessagePayload::Json)
                    .unwrap_or(MessagePayload::Text(message.body));
                QueueMessage {
                    payload,
                    receipt_handle: message.receipt_handle,
                }
            })
            .collect())
    }

    async fn ack(&self, _queue: &str, receipt_handle: &str) -> Result<()> {
        self.client
            .delete_message(&self.queue_url, receipt_handle)
            .await
    }
}
