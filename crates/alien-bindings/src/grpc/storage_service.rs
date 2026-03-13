#![cfg(feature = "grpc")]

use crate::grpc::status_conversion::alien_error_to_status;
use crate::{BindingsProviderApi, Storage as AlienStorage};
use async_trait::async_trait;
use futures::{stream::BoxStream, Stream, StreamExt};
use object_store::{
    path::Path as ObjectStorePath, Error as OsError, GetOptions as OsGetOptions,
    GetResult as OsGetResult, ObjectMeta as OsObjectMeta, PutMultipartOpts as OsPutMultipartOpts,
    PutOptions as OsPutOptions, PutPayload, PutResult as OsPutResult,
};
use std::{pin::Pin, sync::Arc};
use tokio_stream::once;
use tonic::{Request, Response, Status, Streaming};

// Module for the generated gRPC code.
// The package `alien_bindings.storage` will create nested Rust modules.
pub mod alien_bindings {
    pub mod storage {
        // This module corresponds to the `.storage` part of the package
        tonic::include_proto!("alien_bindings.storage"); // Full package name
        pub const FILE_DESCRIPTOR_SET: &[u8] =
            tonic::include_file_descriptor_set!("alien_bindings.storage_descriptor");
    }
}

use alien_bindings::storage::{
    get_response_part,
    storage_put_multipart_chunk_request::Part as StoragePutMultipartChunkRequestPart,
    storage_service_server::{StorageService, StorageServiceServer},
    GetResponsePart, StorageCopyRequest, StorageDeleteRequest, StorageGetBaseDirRequest,
    StorageGetBaseDirResponse, StorageGetRequest, StorageGetUrlRequest, StorageGetUrlResponse,
    StorageHeadRequest, StorageHttpMethod, StorageListRequest, StorageListResultProto,
    StorageListWithDelimiterRequest, StorageObjectMeta, StorageOperationResponse,
    StoragePutMultipartChunkRequest, StoragePutMultipartMetadata, StoragePutRequest,
    StoragePutResponse, StorageRenameRequest, StorageSignedUrlRequest, StorageSignedUrlResponse,
};

pub struct StorageGrpcServer {
    provider: Arc<dyn BindingsProviderApi>,
}

impl StorageGrpcServer {
    pub fn new(provider: Arc<dyn BindingsProviderApi>) -> Self {
        Self { provider }
    }

    pub fn into_service(self) -> StorageServiceServer<Self> {
        // Self refers to StorageGrpcServer
        StorageServiceServer::new(self)
    }

    async fn get_storage_binding(
        &self,
        binding_name: &str,
    ) -> Result<Arc<dyn AlienStorage>, Status> {
        tracing::debug!("Loading storage binding: {}", binding_name);
        let result = self
            .provider
            .load_storage(binding_name)
            .await
            .map_err(alien_error_to_status);

        match &result {
            Ok(_) => tracing::debug!("Successfully loaded storage binding: {}", binding_name),
            Err(e) => tracing::error!("Failed to load storage binding {}: {:?}", binding_name, e),
        }

        result
    }
}

#[async_trait]
impl StorageService for StorageGrpcServer {
    type GetStream = Pin<Box<dyn Stream<Item = Result<GetResponsePart, Status>> + Send + 'static>>;

    async fn get(
        &self,
        request: Request<StorageGetRequest>,
    ) -> Result<Response<Self::GetStream>, Status> {
        let req_inner = request.into_inner();
        let storage = self.get_storage_binding(&req_inner.binding_name).await?;
        let path = ObjectStorePath::from(req_inner.path);
        let options = req_inner.options.map_or_else(
            OsGetOptions::default,
            super::storage_utils::map_proto_get_options_to_os,
        );

        let get_result: OsGetResult = storage
            .get_opts(&path, options)
            .await
            .map_err(map_os_err_to_status)?;

        let proto_meta = super::storage_utils::map_os_object_meta_to_proto(get_result.meta.clone());

        let metadata_part = GetResponsePart {
            part: Some(get_response_part::Part::Metadata(proto_meta)),
        };
        let metadata_stream = once(Ok(metadata_part));

        let data_stream_from_os = get_result.into_stream();

        let data_stream_mapped = data_stream_from_os.map(|chunk_result| match chunk_result {
            Ok(bytes) => {
                let chunk_part = GetResponsePart {
                    part: Some(get_response_part::Part::ChunkData(bytes.into())),
                };
                Ok(chunk_part)
            }
            Err(e) => Err(map_os_err_to_status(e)),
        });

        let combined_stream = metadata_stream.chain(data_stream_mapped);

        Ok(Response::new(Box::pin(combined_stream) as Self::GetStream))
    }

    async fn put_multipart(
        &self,
        request: Request<Streaming<StoragePutMultipartChunkRequest>>,
    ) -> Result<Response<StoragePutResponse>, Status> {
        let mut stream = request.into_inner();
        let metadata_msg = stream
            .message()
            .await?
            .ok_or_else(|| Status::invalid_argument("Missing metadata in put multipart stream"))?;
        let proto_metadata: StoragePutMultipartMetadata = match metadata_msg.part {
            Some(StoragePutMultipartChunkRequestPart::Metadata(meta)) => meta, // Use imported enum variant
            _ => {
                return Err(Status::invalid_argument(
                    "First part of put multipart stream must be metadata",
                ))
            }
        };

        let storage = self
            .get_storage_binding(&proto_metadata.binding_name)
            .await?;
        let path = ObjectStorePath::from(proto_metadata.path);
        let options = proto_metadata.options.map_or_else(
            OsPutMultipartOpts::default,
            super::storage_utils::map_proto_put_multipart_options_to_os,
        );

        let mut multipart_upload = storage
            .put_multipart_opts(&path, options)
            .await
            .map_err(map_os_err_to_status)?;

        while let Some(chunk_msg_result) = stream.next().await {
            let chunk_msg = chunk_msg_result?;
            match chunk_msg.part {
                Some(StoragePutMultipartChunkRequestPart::ChunkData(data)) => {
                    // Use imported enum variant
                    if data.is_empty() {
                        continue;
                    }
                    multipart_upload
                        .put_part(data.into())
                        .await
                        .map_err(map_os_err_to_status)?;
                }
                Some(StoragePutMultipartChunkRequestPart::Metadata(_)) => {
                    // Use imported enum variant
                    return Err(Status::invalid_argument(
                        "Metadata can only be sent as the first part in put multipart stream",
                    ));
                }
                None => {
                    return Err(Status::invalid_argument(
                        "Empty chunk message part in put multipart stream",
                    ))
                }
            }
        }
        let put_result: OsPutResult = multipart_upload
            .complete()
            .await
            .map_err(map_os_err_to_status)?;
        Ok(Response::new(StoragePutResponse {
            e_tag: put_result.e_tag,
            version: put_result.version,
        }))
    }

    async fn put(
        &self,
        request: Request<StoragePutRequest>,
    ) -> Result<Response<StoragePutResponse>, Status> {
        let req_inner = request.into_inner();
        let storage = self.get_storage_binding(&req_inner.binding_name).await?;
        let path = ObjectStorePath::from(req_inner.path);
        let payload = PutPayload::from(req_inner.data);
        let options = req_inner.options.map_or_else(
            OsPutOptions::default,
            super::storage_utils::map_proto_put_options_to_os,
        );

        let put_result = storage
            .put_opts(&path, payload, options)
            .await
            .map_err(map_os_err_to_status)?;
        Ok(Response::new(StoragePutResponse {
            e_tag: put_result.e_tag,
            version: put_result.version,
        }))
    }

    async fn delete(
        &self,
        request: Request<StorageDeleteRequest>,
    ) -> Result<Response<StorageOperationResponse>, Status> {
        let req = request.into_inner();
        let storage = self.get_storage_binding(&req.binding_name).await?;
        let path = ObjectStorePath::from(req.path);
        storage.delete(&path).await.map_err(map_os_err_to_status)?;
        Ok(Response::new(StorageOperationResponse {}))
    }

    type ListStream =
        Pin<Box<dyn Stream<Item = Result<StorageObjectMeta, Status>> + Send + 'static>>;

    async fn list(
        &self,
        request: Request<StorageListRequest>,
    ) -> Result<Response<Self::ListStream>, Status> {
        let req = request.into_inner();
        let storage = self.get_storage_binding(&req.binding_name).await?;
        let prefix = req.prefix.map(ObjectStorePath::from);

        let list_from_store: BoxStream<'static, Result<OsObjectMeta, OsError>> =
            if let Some(offset_string_val) = req.offset {
                let offset_path = ObjectStorePath::from(offset_string_val);
                storage.list_with_offset(prefix.as_ref(), &offset_path)
            } else {
                storage.list(prefix.as_ref())
            };

        let response_stream = list_from_store.map(|meta_result| match meta_result {
            Ok(object_meta) => Ok(super::storage_utils::map_os_object_meta_to_proto(
                object_meta,
            )),
            Err(e) => Err(map_os_err_to_status(e)),
        });

        Ok(Response::new(Box::pin(response_stream) as Self::ListStream))
    }

    async fn list_with_delimiter(
        &self,
        request: Request<StorageListWithDelimiterRequest>,
    ) -> Result<Response<StorageListResultProto>, Status> {
        let req_inner = request.into_inner();
        let storage = self.get_storage_binding(&req_inner.binding_name).await?;
        let prefix = req_inner.prefix.map(ObjectStorePath::from);
        let list_result = storage
            .list_with_delimiter(prefix.as_ref())
            .await
            .map_err(map_os_err_to_status)?;
        Ok(Response::new(StorageListResultProto {
            common_prefixes: list_result
                .common_prefixes
                .into_iter()
                .map(|p| p.to_string())
                .collect(),
            objects: list_result
                .objects
                .into_iter()
                .map(super::storage_utils::map_os_object_meta_to_proto)
                .collect(),
        }))
    }

    async fn head(
        &self,
        request: Request<StorageHeadRequest>,
    ) -> Result<Response<StorageObjectMeta>, Status> {
        let req = request.into_inner();
        let storage = self.get_storage_binding(&req.binding_name).await?;
        let path = ObjectStorePath::from(req.path);
        let object_meta: OsObjectMeta = storage.head(&path).await.map_err(map_os_err_to_status)?;
        Ok(Response::new(
            super::storage_utils::map_os_object_meta_to_proto(object_meta),
        ))
    }

    async fn get_base_dir(
        &self,
        request: Request<StorageGetBaseDirRequest>,
    ) -> Result<Response<StorageGetBaseDirResponse>, Status> {
        let req = request.into_inner();
        tracing::debug!("get_base_dir called for binding: {}", req.binding_name);
        let storage = self.get_storage_binding(&req.binding_name).await?;
        tracing::debug!(
            "get_base_dir: storage binding loaded successfully for: {}",
            req.binding_name
        );
        let base_dir = storage.get_base_dir();
        tracing::debug!("get_base_dir: returning base_dir: {}", base_dir);
        Ok(Response::new(StorageGetBaseDirResponse {
            path: base_dir.to_string(),
        }))
    }

    async fn get_url(
        &self,
        request: Request<StorageGetUrlRequest>,
    ) -> Result<Response<StorageGetUrlResponse>, Status> {
        let req = request.into_inner();
        tracing::debug!("get_url called for binding: {}", req.binding_name);
        let storage = self.get_storage_binding(&req.binding_name).await?;
        tracing::debug!(
            "get_url: storage binding loaded successfully for: {}",
            req.binding_name
        );
        let url_ = storage.get_url();
        tracing::debug!("get_url: returning url: {}", url_);
        Ok(Response::new(StorageGetUrlResponse {
            url: url_.to_string(),
        }))
    }

    async fn copy(
        &self,
        request: Request<StorageCopyRequest>,
    ) -> Result<Response<StorageOperationResponse>, Status> {
        let req_inner = request.into_inner();
        let storage = self.get_storage_binding(&req_inner.binding_name).await?;
        let from = ObjectStorePath::from(req_inner.from_path);
        let to = ObjectStorePath::from(req_inner.to_path);
        storage
            .copy(&from, &to)
            .await
            .map_err(map_os_err_to_status)?;
        Ok(Response::new(StorageOperationResponse {}))
    }

    async fn rename(
        &self,
        request: Request<StorageRenameRequest>,
    ) -> Result<Response<StorageOperationResponse>, Status> {
        let req_inner = request.into_inner();
        let storage = self.get_storage_binding(&req_inner.binding_name).await?;
        let from = ObjectStorePath::from(req_inner.from_path);
        let to = ObjectStorePath::from(req_inner.to_path);
        storage
            .rename(&from, &to)
            .await
            .map_err(map_os_err_to_status)?;
        Ok(Response::new(StorageOperationResponse {}))
    }

    async fn copy_if_not_exists(
        &self,
        request: Request<StorageCopyRequest>,
    ) -> Result<Response<StorageOperationResponse>, Status> {
        let req_inner = request.into_inner();
        let storage = self.get_storage_binding(&req_inner.binding_name).await?;
        let from = ObjectStorePath::from(req_inner.from_path);
        let to = ObjectStorePath::from(req_inner.to_path);
        storage
            .copy_if_not_exists(&from, &to)
            .await
            .map_err(map_os_err_to_status)?;
        Ok(Response::new(StorageOperationResponse {}))
    }

    async fn rename_if_not_exists(
        &self,
        request: Request<StorageRenameRequest>,
    ) -> Result<Response<StorageOperationResponse>, Status> {
        let req_inner = request.into_inner();
        let storage = self.get_storage_binding(&req_inner.binding_name).await?;
        let from = ObjectStorePath::from(req_inner.from_path);
        let to = ObjectStorePath::from(req_inner.to_path);
        storage
            .rename_if_not_exists(&from, &to)
            .await
            .map_err(map_os_err_to_status)?;
        Ok(Response::new(StorageOperationResponse {}))
    }

    async fn signed_url(
        &self,
        request: Request<StorageSignedUrlRequest>,
    ) -> Result<Response<StorageSignedUrlResponse>, Status> {
        let req_inner = request.into_inner();
        let storage = self.get_storage_binding(&req_inner.binding_name).await?;
        let path = ObjectStorePath::from(req_inner.path);
        let method = super::storage_utils::map_proto_method_to_reqwest(
            StorageHttpMethod::from_i32(req_inner.http_method)
                .ok_or_else(|| Status::invalid_argument("Invalid HTTP method"))?,
        );

        // Calculate duration from current time to expiration time
        let expiration_time = req_inner
            .expiration_time
            .ok_or_else(|| Status::invalid_argument("Expiration time is required"))?;
        let expiration_datetime =
            chrono::DateTime::from_timestamp(expiration_time.seconds, expiration_time.nanos as u32)
                .ok_or_else(|| Status::invalid_argument("Invalid expiration timestamp"))?;

        let now = chrono::Utc::now();
        let duration = expiration_datetime.signed_duration_since(now);

        if duration.num_seconds() <= 0 {
            return Err(Status::invalid_argument(
                "Expiration time must be in the future",
            ));
        }

        let duration_std = std::time::Duration::from_secs(duration.num_seconds() as u64);

        let presigned_request = match method {
            reqwest::Method::PUT => storage.presigned_put(&path, duration_std).await,
            reqwest::Method::GET => storage.presigned_get(&path, duration_std).await,
            reqwest::Method::DELETE => storage.presigned_delete(&path, duration_std).await,
            _ => {
                return Err(Status::invalid_argument(
                    "Unsupported HTTP method for signed URL",
                ))
            }
        }
        .map_err(alien_error_to_status)?;

        let signed_url = presigned_request.url();

        Ok(Response::new(StorageSignedUrlResponse {
            url: signed_url.to_string(),
        }))
    }
}

// Helper functions for type conversions and error mapping

// Helper function to format the error chain for better debugging
fn format_error_chain(err: &(dyn std::error::Error + Send + Sync + 'static)) -> String {
    let mut result = format!("{:?}", err); // Main error's debug representation
    let mut current_err: &(dyn std::error::Error + 'static) = err; // Explicitly type current_err
    while let Some(source) = current_err.source() {
        result.push_str(&format!("\\n  Caused by: {:?}", source));
        current_err = source; // Now this assignment is valid
    }
    result
}

fn map_os_err_to_status(err: OsError) -> Status {
    match err {
        OsError::NotFound { path, source } => Status::not_found(format!(
            "Object not found at {}: {}",
            path,
            format_error_chain(source.as_ref())
        )),
        OsError::AlreadyExists { path, source } => Status::already_exists(format!(
            "Object already exists at {}: {}",
            path,
            format_error_chain(source.as_ref())
        )),
        OsError::InvalidPath { source } => {
            Status::invalid_argument(format!("Invalid path: {:?}", source))
        }
        OsError::Generic { store, source } => {
            let detailed_source_message = format_error_chain(source.as_ref());
            Status::internal(format!(
                "Generic object store error for {}. Detailed error: {}",
                store, detailed_source_message
            ))
        }
        OsError::NotImplemented => {
            Status::unimplemented("Operation not implemented by object store")
        }
        OsError::NotSupported { source } => Status::unimplemented(format!(
            "Operation not supported by object store: {}",
            format_error_chain(source.as_ref())
        )),
        OsError::Precondition { path, source } => Status::failed_precondition(format!(
            "Precondition failed for {}: {}",
            path,
            format_error_chain(source.as_ref())
        )),
        OsError::NotModified { path, source } => Status::aborted(format!(
            "Not modified (typically HTTP 304) for {}: {}",
            path,
            format_error_chain(source.as_ref())
        )),
        OsError::PermissionDenied { path, source } => Status::permission_denied(format!(
            "Permission denied for {}: {}",
            path,
            format_error_chain(source.as_ref())
        )),
        OsError::Unauthenticated { path, source } => Status::unauthenticated(format!(
            "Unauthenticated for {}: {}",
            path,
            format_error_chain(source.as_ref())
        )),
        OsError::UnknownConfigurationKey { store, key } => Status::invalid_argument(format!(
            "Unknown config key '{}' for store '{}'",
            key, store
        )),
        _ => Status::internal(format!("Unknown object store error: {}", err)),
    }
}
