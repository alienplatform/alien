use crate::error::{ErrorData, Result};
use crate::traits::{Binding, Worker, WorkerInvokeRequest, WorkerInvokeResponse};
use alien_error::{Context as _, IntoAlienError as _};
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::fmt::{Debug, Formatter};
use tonic::transport::Channel;

// Import generated protobuf types
pub mod proto {
    tonic::include_proto!("alien_bindings.worker");
}

use proto::{worker_service_client::WorkerServiceClient, GetWorkerUrlRequest, InvokeRequest};

/// gRPC-based Worker implementation that forwards calls to a remote Worker service
pub struct GrpcWorker {
    client: WorkerServiceClient<Channel>,
    binding_name: String,
}

impl Debug for GrpcWorker {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GrpcWorker")
            .field("binding_name", &self.binding_name)
            .finish()
    }
}

impl GrpcWorker {
    /// Create a new gRPC Worker client
    pub async fn new(binding_name: String, grpc_endpoint: String) -> Result<Self> {
        let channel = crate::providers::grpc_provider::create_grpc_channel(grpc_endpoint).await?;
        Self::new_from_channel(channel, binding_name).await
    }

    /// Create a new gRPC Worker client from an existing channel
    pub async fn new_from_channel(channel: Channel, binding_name: String) -> Result<Self> {
        let client = WorkerServiceClient::new(channel);

        Ok(Self {
            client,
            binding_name,
        })
    }
}

impl Binding for GrpcWorker {}

#[async_trait]
impl Worker for GrpcWorker {
    async fn invoke(&self, request: WorkerInvokeRequest) -> Result<WorkerInvokeResponse> {
        let mut client = self.client.clone();

        let grpc_request = tonic::Request::new(InvokeRequest {
            binding_name: self.binding_name.clone(),
            target_worker: request.target_worker,
            method: request.method,
            path: request.path,
            headers: request.headers.into_iter().collect(),
            body: request.body,
            timeout_seconds: request.timeout.map(|t| t.as_secs()),
        });

        let response = client
            .invoke(grpc_request)
            .await
            .into_alien_error()
            .context(ErrorData::GrpcRequestFailed {
                service: "WorkerService".to_string(),
                method: "invoke".to_string(),
                details: "Failed to invoke worker".to_string(),
            })?;

        let response_inner = response.into_inner();
        Ok(WorkerInvokeResponse {
            status: response_inner.status as u16,
            headers: response_inner
                .headers
                .into_iter()
                .collect::<BTreeMap<_, _>>(),
            body: response_inner.body,
        })
    }

    async fn get_worker_url(&self) -> Result<Option<String>> {
        let mut client = self.client.clone();

        let grpc_request = tonic::Request::new(GetWorkerUrlRequest {
            binding_name: self.binding_name.clone(),
        });

        let response = client
            .get_worker_url(grpc_request)
            .await
            .into_alien_error()
            .context(ErrorData::GrpcRequestFailed {
                service: "WorkerService".to_string(),
                method: "get_worker_url".to_string(),
                details: "Failed to get worker URL".to_string(),
            })?;

        Ok(response.into_inner().url)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
