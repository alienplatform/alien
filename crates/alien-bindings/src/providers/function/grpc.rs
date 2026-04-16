use crate::error::{ErrorData, Result};
use crate::traits::{Binding, Function, FunctionInvokeRequest, FunctionInvokeResponse};
use alien_error::{Context as _, IntoAlienError as _};
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::fmt::{Debug, Formatter};
use tonic::transport::Channel;

// Import generated protobuf types
pub mod proto {
    tonic::include_proto!("alien_bindings.function");
}

use proto::{function_service_client::FunctionServiceClient, GetFunctionUrlRequest, InvokeRequest};

/// gRPC-based Function implementation that forwards calls to a remote Function service
pub struct GrpcFunction {
    client: FunctionServiceClient<Channel>,
    binding_name: String,
}

impl Debug for GrpcFunction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GrpcFunction")
            .field("binding_name", &self.binding_name)
            .finish()
    }
}

impl GrpcFunction {
    /// Create a new gRPC Function client
    pub async fn new(binding_name: String, grpc_endpoint: String) -> Result<Self> {
        let channel = crate::providers::grpc_provider::create_grpc_channel(grpc_endpoint).await?;
        Self::new_from_channel(channel, binding_name).await
    }

    /// Create a new gRPC Function client from an existing channel
    pub async fn new_from_channel(channel: Channel, binding_name: String) -> Result<Self> {
        let client = FunctionServiceClient::new(channel);

        Ok(Self {
            client,
            binding_name,
        })
    }
}

impl Binding for GrpcFunction {}

#[async_trait]
impl Function for GrpcFunction {
    async fn invoke(&self, request: FunctionInvokeRequest) -> Result<FunctionInvokeResponse> {
        let mut client = self.client.clone();

        let grpc_request = tonic::Request::new(InvokeRequest {
            binding_name: self.binding_name.clone(),
            target_function: request.target_function,
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
                service: "FunctionService".to_string(),
                method: "invoke".to_string(),
                details: "Failed to invoke function".to_string(),
            })?;

        let response_inner = response.into_inner();
        Ok(FunctionInvokeResponse {
            status: response_inner.status as u16,
            headers: response_inner
                .headers
                .into_iter()
                .collect::<BTreeMap<_, _>>(),
            body: response_inner.body,
        })
    }

    async fn get_function_url(&self) -> Result<Option<String>> {
        let mut client = self.client.clone();

        let grpc_request = tonic::Request::new(GetFunctionUrlRequest {
            binding_name: self.binding_name.clone(),
        });

        let response = client
            .get_function_url(grpc_request)
            .await
            .into_alien_error()
            .context(ErrorData::GrpcRequestFailed {
                service: "FunctionService".to_string(),
                method: "get_function_url".to_string(),
                details: "Failed to get function URL".to_string(),
            })?;

        Ok(response.into_inner().url)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
