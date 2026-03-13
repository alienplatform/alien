#![cfg(feature = "grpc")]

use crate::grpc::status_conversion::alien_error_to_status;
use crate::{
    traits::{Kv as AlienKv, PutOptions as AlienPutOptions},
    BindingsProviderApi,
};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use tonic::{Request, Response, Status};

// Module for the generated gRPC code.
pub mod alien_bindings {
    pub mod kv {
        tonic::include_proto!("alien_bindings.kv");
        pub const FILE_DESCRIPTOR_SET: &[u8] =
            tonic::include_file_descriptor_set!("alien_bindings.kv_descriptor");
    }
}

use alien_bindings::kv::{
    kv_service_server::{KvService, KvServiceServer},
    DeleteRequest, DeleteResponse, ExistsRequest, ExistsResponse, GetRequest, GetResponse, KvItem,
    PutOptions, PutRequest, PutResponse, ScanPrefixRequest, ScanPrefixResponse,
};

pub struct KvGrpcServer {
    provider: Arc<dyn BindingsProviderApi>,
}

impl KvGrpcServer {
    pub fn new(provider: Arc<dyn BindingsProviderApi>) -> Self {
        Self { provider }
    }

    pub fn into_service(self) -> KvServiceServer<Self> {
        KvServiceServer::new(self)
    }

    async fn get_kv_binding(&self, binding_name: &str) -> Result<Arc<dyn AlienKv>, Status> {
        self.provider
            .load_kv(binding_name)
            .await
            .map_err(alien_error_to_status)
    }

    fn convert_put_options(&self, options: Option<PutOptions>) -> Option<AlienPutOptions> {
        options.map(|opts| AlienPutOptions {
            ttl: opts.ttl_seconds.map(|secs| Duration::from_secs(secs)),
            if_not_exists: opts.if_not_exists,
        })
    }
}

#[async_trait]
impl KvService for KvGrpcServer {
    async fn get(&self, request: Request<GetRequest>) -> Result<Response<GetResponse>, Status> {
        let req_inner = request.into_inner();
        let kv = self.get_kv_binding(&req_inner.binding_name).await?;

        let value = kv
            .get(&req_inner.key)
            .await
            .map_err(alien_error_to_status)?;

        Ok(Response::new(GetResponse { value }))
    }

    async fn put(&self, request: Request<PutRequest>) -> Result<Response<PutResponse>, Status> {
        let req_inner = request.into_inner();
        let kv = self.get_kv_binding(&req_inner.binding_name).await?;
        let options = self.convert_put_options(req_inner.options);

        let success = kv
            .put(&req_inner.key, req_inner.value, options)
            .await
            .map_err(alien_error_to_status)?;

        Ok(Response::new(PutResponse { success }))
    }

    async fn delete(
        &self,
        request: Request<DeleteRequest>,
    ) -> Result<Response<DeleteResponse>, Status> {
        let req_inner = request.into_inner();
        let kv = self.get_kv_binding(&req_inner.binding_name).await?;

        kv.delete(&req_inner.key)
            .await
            .map_err(alien_error_to_status)?;

        Ok(Response::new(DeleteResponse {}))
    }

    async fn exists(
        &self,
        request: Request<ExistsRequest>,
    ) -> Result<Response<ExistsResponse>, Status> {
        let req_inner = request.into_inner();
        let kv = self.get_kv_binding(&req_inner.binding_name).await?;

        let exists = kv
            .exists(&req_inner.key)
            .await
            .map_err(alien_error_to_status)?;

        Ok(Response::new(ExistsResponse { exists }))
    }

    async fn scan_prefix(
        &self,
        request: Request<ScanPrefixRequest>,
    ) -> Result<Response<ScanPrefixResponse>, Status> {
        let req_inner = request.into_inner();
        let kv = self.get_kv_binding(&req_inner.binding_name).await?;

        let limit = req_inner.limit.map(|l| l as usize);
        let cursor = req_inner.cursor;

        let scan_result = kv
            .scan_prefix(&req_inner.prefix, limit, cursor)
            .await
            .map_err(alien_error_to_status)?;

        let items = scan_result
            .items
            .into_iter()
            .map(|(key, value)| KvItem { key, value })
            .collect();

        Ok(Response::new(ScanPrefixResponse {
            items,
            next_cursor: scan_result.next_cursor,
        }))
    }
}
