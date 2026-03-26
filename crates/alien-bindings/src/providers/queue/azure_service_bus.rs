use crate::error::{ErrorData, Result};
use crate::traits::{
    Binding, MessagePayload, Queue, QueueMessage, MAX_BATCH_SIZE, MAX_MESSAGE_BYTES,
};
use alien_azure_clients::service_bus::{
    AzureServiceBusDataPlaneClient, SendMessageParameters, ServiceBusDataPlaneApi,
};
use alien_error::{Context, ContextError};
use async_trait::async_trait;
use std::fmt::{Debug, Formatter};

pub struct AzureServiceBusQueue {
    namespace: String,
    queue_name: String,
    client: AzureServiceBusDataPlaneClient,
}

impl Debug for AzureServiceBusQueue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AzureServiceBusQueue")
            .field("namespace", &self.namespace)
            .field("queue_name", &self.queue_name)
            .finish()
    }
}

impl AzureServiceBusQueue {
    pub async fn new(
        namespace: String,
        queue_name: String,
        azure_config: alien_azure_clients::AzureClientConfig,
    ) -> Result<Self> {
        Ok(Self {
            namespace,
            queue_name,
            client: AzureServiceBusDataPlaneClient::new(
                crate::http_client::create_http_client(),
                alien_azure_clients::AzureTokenCache::new(azure_config),
            ),
        })
    }
}

impl Binding for AzureServiceBusQueue {}

#[async_trait]
impl Queue for AzureServiceBusQueue {
    async fn send(&self, _queue: &str, message: MessagePayload) -> Result<()> {
        let (body, _ct) = match message {
            MessagePayload::Json(v) => (
                serde_json::to_string(&v).unwrap(),
                Some("application/json".to_string()),
            ),
            MessagePayload::Text(s) => (s, Some("text/plain; charset=utf-8".to_string())),
        };

        // Client-side validation: check message size
        if body.len() > MAX_MESSAGE_BYTES {
            return Err(alien_error::AlienError::new(
                ErrorData::BindingSetupFailed {
                    binding_type: "queue.servicebus".to_string(),
                    reason: format!(
                        "Message size {} bytes exceeds limit of {} bytes",
                        body.len(),
                        MAX_MESSAGE_BYTES
                    ),
                },
            ));
        }
        let params = SendMessageParameters {
            body,
            broker_properties: None,
            custom_properties: std::collections::HashMap::new(),
        };
        self.client
            .send_message(self.namespace.clone(), self.queue_name.clone(), params)
            .await
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.servicebus".to_string(),
                reason: "Failed to send".to_string(),
            })
    }

    async fn receive(&self, _queue: &str, max_messages: usize) -> Result<Vec<QueueMessage>> {
        // Client-side validation: check batch size
        if max_messages == 0 || max_messages > MAX_BATCH_SIZE {
            return Err(alien_error::AlienError::new(
                ErrorData::BindingSetupFailed {
                    binding_type: "queue.servicebus".to_string(),
                    reason: format!(
                        "Batch size {} is invalid. Must be between 1 and {}",
                        max_messages, MAX_BATCH_SIZE
                    ),
                },
            ));
        }

        let mut messages = Vec::new();

        // Azure Service Bus typically receives one message at a time with peek-lock
        // We'll loop up to max_messages times to get multiple messages
        for _ in 0..std::cmp::min(max_messages, MAX_BATCH_SIZE) {
            match self
                .client
                .peek_lock(
                    self.namespace.clone(),
                    self.queue_name.clone(),
                    Some(30), // 30 second timeout
                )
                .await
            {
                Ok(Some(received_msg)) => {
                    let body = received_msg.body.clone();
                    let payload = match serde_json::from_str::<serde_json::Value>(&body) {
                        Ok(json_value) => MessagePayload::Json(json_value),
                        Err(_) => MessagePayload::Text(body),
                    };

                    // Use lock token as receipt handle for acknowledgment
                    let receipt_handle = received_msg
                        .broker_properties
                        .as_ref()
                        .and_then(|bp| bp.lock_token.clone())
                        .ok_or_else(|| {
                            alien_error::AlienError::new(ErrorData::BindingSetupFailed {
                                binding_type: "queue.servicebus".to_string(),
                                reason: "Received message without lock token".to_string(),
                            })
                        })?;

                    messages.push(QueueMessage {
                        payload,
                        receipt_handle,
                    });
                }
                Ok(None) => {
                    // No more messages available
                    break;
                }
                Err(e) => {
                    return Err(e.context(ErrorData::BindingSetupFailed {
                        binding_type: "queue.servicebus".to_string(),
                        reason: "Failed to receive message".to_string(),
                    }));
                }
            }
        }

        Ok(messages)
    }

    async fn ack(&self, _queue: &str, receipt_handle: &str) -> Result<()> {
        // For Azure Service Bus, receipt_handle is the lock token
        // We use the lock token as both message ID and lock token for the complete_message call
        self.client
            .complete_message(
                self.namespace.clone(),
                self.queue_name.clone(),
                receipt_handle.to_string(), // message_id
                receipt_handle.to_string(), // lock_token
            )
            .await
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.servicebus".to_string(),
                reason: "Failed to complete message".to_string(),
            })
    }
}
