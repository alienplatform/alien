#![cfg(feature = "grpc")]

use crate::grpc::status_conversion::alien_error_to_status;
use crate::traits::Container as AlienContainer;
use crate::BindingsProviderApi;
use async_trait::async_trait;
use std::sync::Arc;
use tonic::{Request, Response, Status};

// Module for the generated gRPC code.
pub mod alien_bindings {
    pub mod container {
        tonic::include_proto!("alien_bindings.container");
        pub const FILE_DESCRIPTOR_SET: &[u8] =
            tonic::include_file_descriptor_set!("alien_bindings.container_descriptor");
    }
}

use alien_bindings::container::{
    container_service_server::{ContainerService, ContainerServiceServer},
    GetContainerNameRequest, GetContainerNameResponse, GetInternalUrlRequest,
    GetInternalUrlResponse, GetPublicUrlRequest, GetPublicUrlResponse,
};

pub struct ContainerGrpcServer {
    provider: Arc<dyn BindingsProviderApi>,
}

impl ContainerGrpcServer {
    pub fn new(provider: Arc<dyn BindingsProviderApi>) -> Self {
        Self { provider }
    }

    pub fn into_service(self) -> ContainerServiceServer<Self> {
        ContainerServiceServer::new(self)
    }

    async fn get_container_binding(
        &self,
        binding_name: &str,
    ) -> Result<Arc<dyn AlienContainer>, Status> {
        self.provider
            .load_container(binding_name)
            .await
            .map_err(alien_error_to_status)
    }
}

#[async_trait]
impl ContainerService for ContainerGrpcServer {
    async fn get_internal_url(
        &self,
        request: Request<GetInternalUrlRequest>,
    ) -> Result<Response<GetInternalUrlResponse>, Status> {
        let req_inner = request.into_inner();
        let container = self.get_container_binding(&req_inner.binding_name).await?;

        let url = container.get_internal_url().to_string();

        Ok(Response::new(GetInternalUrlResponse { url }))
    }

    async fn get_public_url(
        &self,
        request: Request<GetPublicUrlRequest>,
    ) -> Result<Response<GetPublicUrlResponse>, Status> {
        let req_inner = request.into_inner();
        let container = self.get_container_binding(&req_inner.binding_name).await?;

        let url = container.get_public_url().map(|s| s.to_string());

        Ok(Response::new(GetPublicUrlResponse { url }))
    }

    async fn get_container_name(
        &self,
        request: Request<GetContainerNameRequest>,
    ) -> Result<Response<GetContainerNameResponse>, Status> {
        let req_inner = request.into_inner();
        let container = self.get_container_binding(&req_inner.binding_name).await?;

        let name = container.get_container_name().to_string();

        Ok(Response::new(GetContainerNameResponse { name }))
    }
}
