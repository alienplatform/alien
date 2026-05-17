#![cfg(feature = "grpc")]

use crate::grpc::status_conversion::alien_error_to_status;
use crate::{
    traits::{Worker as AlienWorker, WorkerInvokeRequest as AlienWorkerInvokeRequest},
    BindingsProviderApi,
};
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tonic::{Request, Response, Status};

// Module for the generated gRPC code.
pub mod alien_bindings {
    pub mod worker {
        tonic::include_proto!("alien_bindings.worker");
        pub const FILE_DESCRIPTOR_SET: &[u8] =
            tonic::include_file_descriptor_set!("alien_bindings.worker_descriptor");
    }
}

use alien_bindings::worker::{
    worker_service_server::{WorkerService, WorkerServiceServer},
    GetWorkerUrlRequest, GetWorkerUrlResponse, InvokeRequest, InvokeResponse,
};

pub struct WorkerGrpcServer {
    provider: Arc<dyn BindingsProviderApi>,
}

impl WorkerGrpcServer {
    pub fn new(provider: Arc<dyn BindingsProviderApi>) -> Self {
        Self { provider }
    }

    pub fn into_service(self) -> WorkerServiceServer<Self> {
        WorkerServiceServer::new(self)
    }

    async fn get_worker_binding(
        &self,
        binding_name: &str,
    ) -> Result<Arc<dyn AlienWorker>, Status> {
        self.provider
            .load_worker(binding_name)
            .await
            .map_err(alien_error_to_status)
    }
}

#[async_trait]
impl WorkerService for WorkerGrpcServer {
    async fn invoke(
        &self,
        request: Request<InvokeRequest>,
    ) -> Result<Response<InvokeResponse>, Status> {
        let req_inner = request.into_inner();
        let function = self.get_worker_binding(&req_inner.binding_name).await?;

        // Convert proto request to AlienWorkerInvokeRequest
        let invoke_request = AlienWorkerInvokeRequest {
            target_worker: req_inner.target_worker,
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

        // Convert AlienWorkerInvokeResponse to proto response
        let response = InvokeResponse {
            status: invoke_response.status as u32,
            headers: invoke_response.headers.into_iter().collect(),
            body: invoke_response.body,
        };

        Ok(Response::new(response))
    }

    async fn get_worker_url(
        &self,
        request: Request<GetWorkerUrlRequest>,
    ) -> Result<Response<GetWorkerUrlResponse>, Status> {
        let req_inner = request.into_inner();
        let function = self.get_worker_binding(&req_inner.binding_name).await?;

        let url = function
            .get_worker_url()
            .await
            .map_err(alien_error_to_status)?;

        Ok(Response::new(GetWorkerUrlResponse { url }))
    }
}
