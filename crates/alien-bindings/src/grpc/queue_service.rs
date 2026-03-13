#![cfg(feature = "grpc")]

use crate::grpc::status_conversion::alien_error_to_status;
use crate::{
    error::ErrorData,
    traits::{
        MessagePayload as AlienMessagePayload, Queue as AlienQueue,
        QueueMessage as AlienQueueMessage,
    },
    BindingsProviderApi,
};
use alien_error::AlienError;
use async_trait::async_trait;
use std::sync::Arc;
use tonic::{Request, Response, Status};

// Module for the generated gRPC code.
pub mod alien_bindings {
    pub mod queue {
        tonic::include_proto!("alien_bindings.queue");
        pub const FILE_DESCRIPTOR_SET: &[u8] =
            tonic::include_file_descriptor_set!("alien_bindings.queue_descriptor");
    }
}

use alien_bindings::queue::{
    queue_service_server::{QueueService, QueueServiceServer},
    AckRequest, AckResponse, MessagePayload, QueueMessage, ReceiveRequest, ReceiveResponse,
    SendRequest, SendResponse,
};

pub struct QueueGrpcServer {
    provider: Arc<dyn BindingsProviderApi>,
}

impl QueueGrpcServer {
    pub fn new(provider: Arc<dyn BindingsProviderApi>) -> Self {
        Self { provider }
    }

    pub fn into_service(self) -> QueueServiceServer<Self> {
        QueueServiceServer::new(self)
    }

    async fn get_queue_binding(&self, binding_name: &str) -> Result<Arc<dyn AlienQueue>, Status> {
        self.provider
            .load_queue(binding_name)
            .await
            .map_err(alien_error_to_status)
    }

    fn convert_message_payload_to_alien(
        &self,
        payload: Option<MessagePayload>,
    ) -> Result<AlienMessagePayload, Status> {
        let payload =
            payload.ok_or_else(|| Status::invalid_argument("Message payload is required"))?;

        match payload.payload {
            Some(alien_bindings::queue::message_payload::Payload::Json(json_str)) => {
                // Parse JSON string to Value
                let json_value: serde_json::Value =
                    serde_json::from_str(&json_str).map_err(|e| {
                        Status::invalid_argument(format!("Invalid JSON payload: {}", e))
                    })?;
                Ok(AlienMessagePayload::Json(json_value))
            }
            Some(alien_bindings::queue::message_payload::Payload::Text(text)) => {
                Ok(AlienMessagePayload::Text(text))
            }
            None => Err(Status::invalid_argument(
                "Message payload type not specified",
            )),
        }
    }

    fn convert_alien_message_to_grpc(&self, message: AlienQueueMessage) -> QueueMessage {
        let payload = match message.payload {
            AlienMessagePayload::Json(value) => {
                // Convert JSON Value to string
                let json_str = serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string());
                Some(MessagePayload {
                    payload: Some(alien_bindings::queue::message_payload::Payload::Json(
                        json_str,
                    )),
                })
            }
            AlienMessagePayload::Text(text) => Some(MessagePayload {
                payload: Some(alien_bindings::queue::message_payload::Payload::Text(text)),
            }),
        };

        QueueMessage {
            payload,
            receipt_handle: message.receipt_handle,
        }
    }
}

#[async_trait]
impl QueueService for QueueGrpcServer {
    async fn send(&self, request: Request<SendRequest>) -> Result<Response<SendResponse>, Status> {
        let req_inner = request.into_inner();
        let queue = self.get_queue_binding(&req_inner.binding_name).await?;

        let message_payload = self.convert_message_payload_to_alien(req_inner.message)?;

        queue
            .send(&req_inner.queue, message_payload)
            .await
            .map_err(alien_error_to_status)?;

        Ok(Response::new(SendResponse {}))
    }

    async fn receive(
        &self,
        request: Request<ReceiveRequest>,
    ) -> Result<Response<ReceiveResponse>, Status> {
        let req_inner = request.into_inner();
        let queue = self.get_queue_binding(&req_inner.binding_name).await?;

        let messages = queue
            .receive(&req_inner.queue, req_inner.max_messages as usize)
            .await
            .map_err(alien_error_to_status)?;

        let grpc_messages = messages
            .into_iter()
            .map(|msg| self.convert_alien_message_to_grpc(msg))
            .collect();

        Ok(Response::new(ReceiveResponse {
            messages: grpc_messages,
        }))
    }

    async fn ack(&self, request: Request<AckRequest>) -> Result<Response<AckResponse>, Status> {
        let req_inner = request.into_inner();
        let queue = self.get_queue_binding(&req_inner.binding_name).await?;

        queue
            .ack(&req_inner.queue, &req_inner.receipt_handle)
            .await
            .map_err(alien_error_to_status)?;

        Ok(Response::new(AckResponse {}))
    }
}
