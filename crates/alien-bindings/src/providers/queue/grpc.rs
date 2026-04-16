use crate::error::{ErrorData, Result};
use crate::traits::{Binding, MessagePayload, Queue, QueueMessage};
use alien_error::{Context as _, IntoAlienError as _};
use async_trait::async_trait;
use std::fmt::{Debug, Formatter};
use tonic::transport::Channel;

// Import generated protobuf types
pub mod proto {
    tonic::include_proto!("alien_bindings.queue");
}

use proto::{
    queue_service_client::QueueServiceClient, AckRequest, MessagePayload as ProtoMessagePayload,
    ReceiveRequest, SendRequest,
};

/// gRPC-based Queue implementation that forwards calls to a remote Queue service
pub struct GrpcQueue {
    client: QueueServiceClient<Channel>,
    binding_name: String,
}

impl Debug for GrpcQueue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GrpcQueue")
            .field("binding_name", &self.binding_name)
            .finish()
    }
}

impl GrpcQueue {
    /// Create a new gRPC Queue client
    pub async fn new(binding_name: String, grpc_endpoint: String) -> Result<Self> {
        let channel = crate::providers::grpc_provider::create_grpc_channel(grpc_endpoint).await?;
        Self::new_from_channel(channel, binding_name).await
    }

    /// Create a new gRPC Queue client from an existing channel
    pub async fn new_from_channel(channel: Channel, binding_name: String) -> Result<Self> {
        let client = QueueServiceClient::new(channel);

        Ok(Self {
            client,
            binding_name,
        })
    }

    /// Convert MessagePayload to proto MessagePayload
    fn message_payload_to_proto(payload: MessagePayload) -> ProtoMessagePayload {
        let proto_payload = match payload {
            MessagePayload::Json(value) => {
                // Convert JSON Value to string
                let json_str = serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string());
                proto::message_payload::Payload::Json(json_str)
            }
            MessagePayload::Text(text) => proto::message_payload::Payload::Text(text),
        };

        ProtoMessagePayload {
            payload: Some(proto_payload),
        }
    }

    /// Convert proto QueueMessage to QueueMessage
    fn proto_to_queue_message(proto_msg: proto::QueueMessage) -> Result<QueueMessage> {
        let payload = proto_msg.payload.ok_or_else(|| {
            alien_error::AlienError::new(ErrorData::InvalidInput {
                operation_context: "Queue message deserialization".to_string(),
                details: "Queue message payload is missing".to_string(),
                field_name: Some("payload".to_string()),
            })
        })?;

        let message_payload = match payload.payload {
            Some(proto::message_payload::Payload::Json(json_str)) => {
                let json_value: serde_json::Value = serde_json::from_str(&json_str)
                    .into_alien_error()
                    .context(ErrorData::InvalidInput {
                        operation_context: "Queue message payload parsing".to_string(),
                        details: format!("Invalid JSON payload in queue message: {}", json_str),
                        field_name: Some("payload.json".to_string()),
                    })?;
                MessagePayload::Json(json_value)
            }
            Some(proto::message_payload::Payload::Text(text)) => MessagePayload::Text(text),
            None => {
                return Err(alien_error::AlienError::new(ErrorData::InvalidInput {
                    operation_context: "Queue message payload parsing".to_string(),
                    details: "Queue message payload type not specified".to_string(),
                    field_name: Some("payload.payload".to_string()),
                }));
            }
        };

        Ok(QueueMessage {
            payload: message_payload,
            receipt_handle: proto_msg.receipt_handle,
        })
    }
}

impl Binding for GrpcQueue {}

#[async_trait]
impl Queue for GrpcQueue {
    async fn send(&self, queue: &str, message: MessagePayload) -> Result<()> {
        let mut client = self.client.clone();

        let request = tonic::Request::new(SendRequest {
            binding_name: self.binding_name.clone(),
            queue: queue.to_string(),
            message: Some(Self::message_payload_to_proto(message)),
        });

        client
            .send(request)
            .await
            .into_alien_error()
            .context(ErrorData::GrpcRequestFailed {
                service: "QueueService".to_string(),
                method: "send".to_string(),
                details: format!("Failed to send message to queue: {}", queue),
            })?;

        Ok(())
    }

    async fn receive(&self, queue: &str, max_messages: usize) -> Result<Vec<QueueMessage>> {
        let mut client = self.client.clone();

        let request = tonic::Request::new(ReceiveRequest {
            binding_name: self.binding_name.clone(),
            queue: queue.to_string(),
            max_messages: max_messages as u32,
        });

        let response = client.receive(request).await.into_alien_error().context(
            ErrorData::GrpcRequestFailed {
                service: "QueueService".to_string(),
                method: "receive".to_string(),
                details: format!("Failed to receive messages from queue: {}", queue),
            },
        )?;

        let proto_messages = response.into_inner().messages;
        let messages: Result<Vec<QueueMessage>> = proto_messages
            .into_iter()
            .map(Self::proto_to_queue_message)
            .collect();

        messages
    }

    async fn ack(&self, queue: &str, receipt_handle: &str) -> Result<()> {
        let mut client = self.client.clone();

        let request = tonic::Request::new(AckRequest {
            binding_name: self.binding_name.clone(),
            queue: queue.to_string(),
            receipt_handle: receipt_handle.to_string(),
        });

        client
            .ack(request)
            .await
            .into_alien_error()
            .context(ErrorData::GrpcRequestFailed {
                service: "QueueService".to_string(),
                method: "ack".to_string(),
                details: format!("Failed to acknowledge message in queue: {}", queue),
            })?;

        Ok(())
    }
}
