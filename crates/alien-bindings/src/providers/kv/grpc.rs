use crate::error::{ErrorData, Result};
use crate::traits::{Binding, Kv, PutOptions, ScanResult};
use alien_error::{Context as _, IntoAlienError as _};
use async_trait::async_trait;
use std::fmt::{Debug, Formatter};
use tonic::transport::Channel;

// Import generated protobuf types
pub mod proto {
    tonic::include_proto!("alien_bindings.kv");
}

use proto::{
    kv_service_client::KvServiceClient, DeleteRequest, ExistsRequest, GetRequest,
    PutOptions as ProtoPutOptions, PutRequest, ScanPrefixRequest,
};

/// gRPC-based KV implementation that forwards calls to a remote KV service
pub struct GrpcKv {
    client: KvServiceClient<Channel>,
    binding_name: String,
}

impl Debug for GrpcKv {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GrpcKv")
            .field("binding_name", &self.binding_name)
            .finish()
    }
}

impl GrpcKv {
    /// Create a new gRPC KV client
    pub async fn new(binding_name: String, grpc_endpoint: String) -> Result<Self> {
        let channel = crate::providers::grpc_provider::create_grpc_channel(grpc_endpoint).await?;
        Self::new_from_channel(channel, binding_name).await
    }

    /// Create a new gRPC KV client from an existing channel
    pub async fn new_from_channel(channel: Channel, binding_name: String) -> Result<Self> {
        let client = KvServiceClient::new(channel);

        Ok(Self {
            client,
            binding_name,
        })
    }

    /// Convert PutOptions to proto PutOptions
    fn put_options_to_proto(options: Option<PutOptions>) -> Option<ProtoPutOptions> {
        options.map(|opts| ProtoPutOptions {
            ttl_seconds: opts.ttl.map(|ttl| ttl.as_secs()),
            if_not_exists: opts.if_not_exists,
        })
    }
}

impl Binding for GrpcKv {}

#[async_trait]
impl Kv for GrpcKv {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let mut client = self.client.clone();

        let request = tonic::Request::new(GetRequest {
            binding_name: self.binding_name.clone(),
            key: key.to_string(),
        });

        let response =
            client
                .get(request)
                .await
                .into_alien_error()
                .context(ErrorData::GrpcRequestFailed {
                    service: "KvService".to_string(),
                    method: "get".to_string(),
                    details: format!("Failed to get key: {}", key),
                })?;

        Ok(response.into_inner().value)
    }

    async fn put(&self, key: &str, value: Vec<u8>, options: Option<PutOptions>) -> Result<bool> {
        let mut client = self.client.clone();

        let request = tonic::Request::new(PutRequest {
            binding_name: self.binding_name.clone(),
            key: key.to_string(),
            value,
            options: Self::put_options_to_proto(options),
        });

        let response =
            client
                .put(request)
                .await
                .into_alien_error()
                .context(ErrorData::GrpcRequestFailed {
                    service: "KvService".to_string(),
                    method: "put".to_string(),
                    details: format!("Failed to put key: {}", key),
                })?;

        Ok(response.into_inner().success)
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let mut client = self.client.clone();

        let request = tonic::Request::new(DeleteRequest {
            binding_name: self.binding_name.clone(),
            key: key.to_string(),
        });

        client
            .delete(request)
            .await
            .into_alien_error()
            .context(ErrorData::GrpcRequestFailed {
                service: "KvService".to_string(),
                method: "delete".to_string(),
                details: format!("Failed to delete key: {}", key),
            })?;

        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        let mut client = self.client.clone();

        let request = tonic::Request::new(ExistsRequest {
            binding_name: self.binding_name.clone(),
            key: key.to_string(),
        });

        let response = client.exists(request).await.into_alien_error().context(
            ErrorData::GrpcRequestFailed {
                service: "KvService".to_string(),
                method: "exists".to_string(),
                details: format!("Failed to check exists for key: {}", key),
            },
        )?;

        Ok(response.into_inner().exists)
    }

    async fn scan_prefix(
        &self,
        prefix: &str,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> Result<ScanResult> {
        let mut client = self.client.clone();

        let request = tonic::Request::new(ScanPrefixRequest {
            binding_name: self.binding_name.clone(),
            prefix: prefix.to_string(),
            limit: limit.map(|l| l as u32),
            cursor,
        });

        let response = client
            .scan_prefix(request)
            .await
            .into_alien_error()
            .context(ErrorData::GrpcRequestFailed {
                service: "KvService".to_string(),
                method: "scan_prefix".to_string(),
                details: format!("Failed to scan prefix: {}", prefix),
            })?;

        let response_inner = response.into_inner();
        let items = response_inner
            .items
            .into_iter()
            .map(|item| (item.key, item.value))
            .collect();

        Ok(ScanResult {
            items,
            next_cursor: response_inner.next_cursor,
        })
    }
}
