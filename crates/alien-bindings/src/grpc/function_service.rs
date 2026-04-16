#![cfg(feature = "grpc")]

use crate::grpc::status_conversion::alien_error_to_status;
use crate::{
    traits::{Function as AlienFunction, FunctionInvokeRequest as AlienFunctionInvokeRequest},
    BindingsProviderApi,
};
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tonic::{Request, Response, Status};

// Module for the generated gRPC code.
pub mod alien_bindings {
    pub mod function {
        tonic::include_proto!("alien_bindings.function");
        pub const FILE_DESCRIPTOR_SET: &[u8] =
            tonic::include_file_descriptor_set!("alien_bindings.function_descriptor");
    }
}

use alien_bindings::function::{
    function_service_server::{FunctionService, FunctionServiceServer},
    GetFunctionUrlRequest, GetFunctionUrlResponse, InvokeRequest, InvokeResponse,
};

pub struct FunctionGrpcServer {
    provider: Arc<dyn BindingsProviderApi>,
}

impl FunctionGrpcServer {
    pub fn new(provider: Arc<dyn BindingsProviderApi>) -> Self {
        Self { provider }
    }

    pub fn into_service(self) -> FunctionServiceServer<Self> {
        FunctionServiceServer::new(self)
    }

    async fn get_function_binding(
        &self,
        binding_name: &str,
    ) -> Result<Arc<dyn AlienFunction>, Status> {
        self.provider
            .load_function(binding_name)
            .await
            .map_err(alien_error_to_status)
    }
}

#[async_trait]
impl FunctionService for FunctionGrpcServer {
    async fn invoke(
        &self,
        request: Request<InvokeRequest>,
    ) -> Result<Response<InvokeResponse>, Status> {
        let req_inner = request.into_inner();
        let function = self.get_function_binding(&req_inner.binding_name).await?;

        // Convert proto request to AlienFunctionInvokeRequest
        let invoke_request = AlienFunctionInvokeRequest {
            target_function: req_inner.target_function,
            method: req_inner.method,
            path: req_inner.path,
            headers: req_inner.headers.into_iter().collect::<BTreeMap<_, _>>(),
            body: req_inner.body,
            timeout: req_inner.timeout_seconds.map(Duration::from_secs),
        };

        let invoke_response = function
            .invoke(invoke_request)
            .await
            .map_err(alien_error_to_status)?;

        // Convert AlienFunctionInvokeResponse to proto response
        let response = InvokeResponse {
            status: invoke_response.status as u32,
            headers: invoke_response.headers.into_iter().collect(),
            body: invoke_response.body,
        };

        Ok(Response::new(response))
    }

    async fn get_function_url(
        &self,
        request: Request<GetFunctionUrlRequest>,
    ) -> Result<Response<GetFunctionUrlResponse>, Status> {
        let req_inner = request.into_inner();
        let function = self.get_function_binding(&req_inner.binding_name).await?;

        let url = function
            .get_function_url()
            .await
            .map_err(alien_error_to_status)?;

        Ok(Response::new(GetFunctionUrlResponse { url }))
    }
}
