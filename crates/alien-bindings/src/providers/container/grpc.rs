//! gRPC container binding implementation
//!
//! For accessing container bindings via gRPC from TypeScript/Python SDKs.

use crate::error::{ErrorData, Result};
use crate::traits::{Binding, Container};
use alien_error::{Context as _, IntoAlienError as _};
use async_trait::async_trait;
use std::fmt::{Debug, Formatter};
use tonic::transport::Channel;

// Import generated protobuf types
pub mod proto {
    tonic::include_proto!("alien_bindings.container");
}

use proto::{
    container_service_client::ContainerServiceClient, GetContainerNameRequest,
    GetInternalUrlRequest, GetPublicUrlRequest,
};

/// gRPC-based Container implementation that forwards calls to a remote Container service
pub struct GrpcContainer {
    client: ContainerServiceClient<Channel>,
    binding_name: String,
    // Cached values for synchronous getters
    internal_url: String,
    public_url: Option<String>,
    container_name: String,
}

impl Debug for GrpcContainer {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GrpcContainer")
            .field("binding_name", &self.binding_name)
            .finish()
    }
}

impl GrpcContainer {
    /// Create a new gRPC Container client
    pub async fn new(binding_name: String, grpc_endpoint: String) -> Result<Self> {
        let channel = crate::providers::grpc_provider::create_grpc_channel(grpc_endpoint).await?;
        Self::new_from_channel(channel, binding_name).await
    }

    /// Create a new gRPC Container client from an existing channel
    pub async fn new_from_channel(channel: Channel, binding_name: String) -> Result<Self> {
        let mut client = ContainerServiceClient::new(channel);

        // Fetch and cache all values since the Container trait has sync getters
        let internal_url = {
            let request = tonic::Request::new(GetInternalUrlRequest {
                binding_name: binding_name.clone(),
            });
            client
                .get_internal_url(request)
                .await
                .into_alien_error()
                .context(ErrorData::GrpcRequestFailed {
                    service: "ContainerService".to_string(),
                    method: "get_internal_url".to_string(),
                    details: "Failed to get internal URL".to_string(),
                })?
                .into_inner()
                .url
        };

        let public_url = {
            let request = tonic::Request::new(GetPublicUrlRequest {
                binding_name: binding_name.clone(),
            });
            client
                .get_public_url(request)
                .await
                .into_alien_error()
                .context(ErrorData::GrpcRequestFailed {
                    service: "ContainerService".to_string(),
                    method: "get_public_url".to_string(),
                    details: "Failed to get public URL".to_string(),
                })?
                .into_inner()
                .url
        };

        let container_name = {
            let request = tonic::Request::new(GetContainerNameRequest {
                binding_name: binding_name.clone(),
            });
            client
                .get_container_name(request)
                .await
                .into_alien_error()
                .context(ErrorData::GrpcRequestFailed {
                    service: "ContainerService".to_string(),
                    method: "get_container_name".to_string(),
                    details: "Failed to get container name".to_string(),
                })?
                .into_inner()
                .name
        };

        Ok(Self {
            client,
            binding_name,
            internal_url,
            public_url,
            container_name,
        })
    }
}

impl Binding for GrpcContainer {}

#[async_trait]
impl Container for GrpcContainer {
    fn get_internal_url(&self) -> &str {
        &self.internal_url
    }

    fn get_public_url(&self) -> Option<&str> {
        self.public_url.as_deref()
    }

    fn get_container_name(&self) -> &str {
        &self.container_name
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
