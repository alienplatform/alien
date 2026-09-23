use crate::error::{ErrorData, Result};
use crate::traits::{
    Binding, MessagePayload, Queue, QueueMessage, MAX_BATCH_SIZE, MAX_MESSAGE_BYTES,
};
use alien_aws_clients::sqs::{
    DeleteMessageRequest, Message, ReceiveMessageRequest, SendMessageRequest, SqsApi, SqsClient,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
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
            // The SQS Query-protocol client uses AttributeName.N. AWS keeps
            // this parameter supported for backward compatibility.
            .attribute_names(vec!["ApproximateReceiveCount".to_string()])
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
        resp.receive_message_result
            .messages
            .into_iter()
            .map(|m| {
                let attempt = receive_count(&m)?;
                let raw = m.body;
                let payload = serde_json::from_str::<serde_json::Value>(&raw)
                    .map(MessagePayload::Json)
                    .unwrap_or(MessagePayload::Text(raw));
                Ok(QueueMessage {
                    payload,
                    receipt_handle: m.receipt_handle,
                    attempt,
                })
            })
            .collect()
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

    async fn nack(&self, _queue: &str, _receipt_handle: &str) -> Result<()> {
        // SQS nack is ChangeMessageVisibility(VisibilityTimeout=0). The
        // alien-aws-clients SqsApi wrapper does not expose that call, so we
        // fail explicitly rather than silently waiting out the lease.
        Err(alien_error::AlienError::new(
            ErrorData::OperationNotSupported {
                operation: "queue.nack".to_string(),
                reason: "AWS SQS nack requires ChangeMessageVisibility, which the SQS client does not expose".to_string(),
            },
        ))
    }

    async fn purge(&self, _queue: &str) -> Result<()> {
        self.client
            .purge_queue(&self.queue_url)
            .await
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.sqs".to_string(),
                reason: "Failed to purge queue".to_string(),
            })
    }
}

fn receive_count(message: &Message) -> Result<u32> {
    let reason = || ErrorData::QueueProviderResponseInvalid {
        reason: format!(
            "SQS message '{}' has no positive ApproximateReceiveCount",
            message.message_id
        ),
    };
    let raw = message
        .attributes
        .as_ref()
        .and_then(|attributes| attributes.get("ApproximateReceiveCount"))
        .ok_or_else(|| AlienError::new(reason()))?;
    let count = raw.parse::<u32>().into_alien_error().context(reason())?;
    if count == 0 {
        return Err(AlienError::new(reason()));
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn message(count: Option<&str>) -> Message {
        Message {
            attributes: count.map(|count| {
                HashMap::from([("ApproximateReceiveCount".to_string(), count.to_string())])
            }),
            body: "payload".to_string(),
            md5_of_body: "unused".to_string(),
            md5_of_message_attributes: None,
            message_attributes: None,
            message_id: "message-1".to_string(),
            receipt_handle: "receipt-1".to_string(),
        }
    }

    #[test]
    fn reads_positive_sqs_delivery_attempts() {
        assert_eq!(receive_count(&message(Some("1"))).expect("first"), 1);
        assert_eq!(receive_count(&message(Some("2"))).expect("redelivery"), 2);
    }

    #[test]
    fn rejects_missing_or_invalid_sqs_delivery_attempts() {
        for count in [
            None,
            Some(""),
            Some("not-a-number"),
            Some("0"),
            Some("4294967296"),
        ] {
            let error = receive_count(&message(count)).expect_err("invalid attempt must fail");
            assert!(matches!(
                error.error,
                Some(ErrorData::QueueProviderResponseInvalid { .. })
            ));
        }
    }
}
