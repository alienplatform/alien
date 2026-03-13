use crate::error::ErrorData;
use crate::grpc::storage_utils;
use crate::grpc::MAX_GRPC_MESSAGE_SIZE;
use crate::{
    grpc::storage_service::alien_bindings::storage::{
        get_response_part,
        storage_put_multipart_chunk_request::Part as StoragePutMultipartChunkRequestPart,
        storage_service_client::StorageServiceClient, StorageCopyRequest, StorageDeleteRequest,
        StorageGetBaseDirRequest, StorageGetUrlRequest, StorageHeadRequest, StorageHttpMethod,
        StorageListRequest, StorageListWithDelimiterRequest, StoragePutMultipartChunkRequest,
        StoragePutMultipartMetadata, StoragePutRequest, StoragePutResponse, StorageRenameRequest,
        StorageSignedUrlRequest,
    },
    presigned::{LocalOperation, PresignedOperation, PresignedRequest, PresignedRequestBackend},
    traits::Binding,
};

use alien_error::AlienError;
use alien_error::Context as _;
use alien_error::IntoAlienError as _;
use async_stream::try_stream;
use async_trait::async_trait;
use bytes::Bytes;
use chrono::{self, Utc};
use futures::{stream::BoxStream, StreamExt};
use object_store::{
    path::Path, Attributes as OsAttributes, Error as ObjectStoreError, GetOptions,
    GetRange as OsGetRange, GetResult, GetResultPayload, ListResult, MultipartUpload, ObjectMeta,
    PutMultipartOpts, PutOptions, PutPayload, PutResult,
};
use prost_types;
use std::fmt::{Debug, Formatter};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{transport::Channel, Request, Status};
use url::Url;

#[derive(Debug)]
pub struct GrpcStorage {
    client: StorageServiceClient<Channel>,
    binding_name: String,
    base_dir: Path,
    base_url: Url,
}

impl GrpcStorage {
    pub async fn new(binding_name: String, grpc_address: String) -> crate::error::Result<Self> {
        let channel = crate::providers::grpc_provider::create_grpc_channel(grpc_address).await?;
        Self::new_from_channel(channel, binding_name).await
    }

    pub async fn new_from_channel(
        channel: Channel,
        binding_name: String,
    ) -> crate::error::Result<Self> {
        tracing::debug!(
            "GrpcStorage::new_from_channel: Creating client for binding: {}",
            binding_name
        );
        let mut client = StorageServiceClient::new(channel.clone())
            .max_decoding_message_size(MAX_GRPC_MESSAGE_SIZE);

        tracing::debug!(
            "GrpcStorage::new_from_channel: Calling get_base_dir for binding: {}",
            binding_name
        );
        let base_dir_req = StorageGetBaseDirRequest {
            binding_name: binding_name.clone(),
        };
        let base_dir_resp = client
            .get_base_dir(Request::new(base_dir_req))
            .await
            .into_alien_error()
            .context(ErrorData::GrpcRequestFailed {
                service: "storage".to_string(),
                method: "get_base_dir".to_string(),
                details: format!("Failed to get base directory for binding {}", binding_name),
            })?
            .into_inner();
        let base_dir = Path::from(base_dir_resp.path);
        tracing::debug!("GrpcStorage::new_from_channel: Got base_dir: {}", base_dir);

        tracing::debug!(
            "GrpcStorage::new_from_channel: Calling get_url for binding: {}",
            binding_name
        );
        let get_url_req = StorageGetUrlRequest {
            binding_name: binding_name.clone(),
        };
        let get_url_resp = client
            .get_url(Request::new(get_url_req))
            .await
            .into_alien_error()
            .context(ErrorData::GrpcRequestFailed {
                service: "storage".to_string(),
                method: "get_url".to_string(),
                details: format!("Failed to get URL for binding {}", binding_name),
            })?
            .into_inner();
        tracing::debug!(
            "GrpcStorage::new_from_channel: Got url: {}",
            get_url_resp.url
        );
        let base_url = Url::parse(&get_url_resp.url).into_alien_error().context(
            ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: format!("Invalid base_url: {}", get_url_resp.url),
            },
        )?;

        Ok(Self {
            client: StorageServiceClient::new(channel) // Can re-use the original channel
                .max_decoding_message_size(MAX_GRPC_MESSAGE_SIZE)
                .max_encoding_message_size(MAX_GRPC_MESSAGE_SIZE),
            binding_name,
            base_dir,
            base_url,
        })
    }

    fn client(&self) -> StorageServiceClient<Channel> {
        self.client.clone()
    }
}

impl std::fmt::Display for GrpcStorage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "GrpcStorage(binding='{}', base_url='{}')",
            self.binding_name, self.base_url
        )
    }
}

impl Binding for GrpcStorage {}

#[async_trait]
impl crate::Storage for GrpcStorage {
    fn get_base_dir(&self) -> Path {
        self.base_dir.clone()
    }

    fn get_url(&self) -> Url {
        self.base_url.clone()
    }

    async fn presigned_put(
        &self,
        path: &Path,
        expires_in: Duration,
    ) -> crate::error::Result<PresignedRequest> {
        let mut client = self.client();
        let expiration = Utc::now()
            + chrono::Duration::from_std(expires_in)
                .into_alien_error()
                .context(ErrorData::Other {
                    message: "Invalid duration for presigned PUT request".to_string(),
                })?;

        let request = StorageSignedUrlRequest {
            binding_name: self.binding_name.clone(),
            path: path.to_string(),
            http_method: StorageHttpMethod::HttpMethodPut as i32,
            expiration_time: Some(prost_types::Timestamp {
                seconds: expiration.timestamp(),
                nanos: expiration.timestamp_subsec_nanos() as i32,
            }),
        };

        let response = client
            .signed_url(tonic::Request::new(request))
            .await
            .into_alien_error()
            .context(ErrorData::GrpcRequestFailed {
                service: "storage".to_string(),
                method: "signed_url".to_string(),
                details: "Failed to generate presigned PUT URL".to_string(),
            })?
            .into_inner();

        // Parse the returned URL to determine if it's HTTP or local
        let url = &response.url;
        let backend = if url.starts_with("http://") || url.starts_with("https://") {
            // HTTP-based presigned URL (AWS S3, GCP GCS, Azure Blob)
            let parsed_url = reqwest::Url::parse(url).into_alien_error().context(
                ErrorData::InvalidConfigurationUrl {
                    url: url.to_string(),
                    reason: "Invalid presigned PUT URL format".to_string(),
                },
            )?;

            // Extract headers from query parameters if any (some providers include them)
            let mut headers = std::collections::HashMap::new();
            for (key, value) in parsed_url.query_pairs() {
                if key.starts_with("X-") || key.starts_with("x-") {
                    headers.insert(key.to_string(), value.to_string());
                }
            }

            PresignedRequestBackend::Http {
                url: url.clone(),
                method: "PUT".to_string(),
                headers,
            }
        } else if url.starts_with("local://") {
            // Local filesystem URL
            let file_path = url.strip_prefix("local://").unwrap_or(url);
            PresignedRequestBackend::Local {
                file_path: file_path.to_string(),
                operation: LocalOperation::Put,
            }
        } else {
            return Err(AlienError::new(ErrorData::InvalidConfigurationUrl {
                url: url.to_string(),
                reason: "Unsupported presigned URL scheme".to_string(),
            }));
        };

        Ok(PresignedRequest {
            backend,
            expiration,
            operation: PresignedOperation::Put,
            path: path.to_string(),
        })
    }

    async fn presigned_get(
        &self,
        path: &Path,
        expires_in: Duration,
    ) -> crate::error::Result<PresignedRequest> {
        let mut client = self.client();
        let expiration = Utc::now()
            + chrono::Duration::from_std(expires_in)
                .into_alien_error()
                .context(ErrorData::Other {
                    message: "Invalid duration for presigned GET request".to_string(),
                })?;

        let request = StorageSignedUrlRequest {
            binding_name: self.binding_name.clone(),
            path: path.to_string(),
            http_method: StorageHttpMethod::HttpMethodGet as i32,
            expiration_time: Some(prost_types::Timestamp {
                seconds: expiration.timestamp(),
                nanos: expiration.timestamp_subsec_nanos() as i32,
            }),
        };

        let response = client
            .signed_url(tonic::Request::new(request))
            .await
            .into_alien_error()
            .context(ErrorData::GrpcRequestFailed {
                service: "storage".to_string(),
                method: "signed_url".to_string(),
                details: "Failed to generate presigned GET URL".to_string(),
            })?
            .into_inner();

        // Parse the returned URL to determine if it's HTTP or local
        let url = &response.url;
        let backend = if url.starts_with("http://") || url.starts_with("https://") {
            // HTTP-based presigned URL (AWS S3, GCP GCS, Azure Blob)
            let parsed_url = reqwest::Url::parse(url).into_alien_error().context(
                ErrorData::InvalidConfigurationUrl {
                    url: url.to_string(),
                    reason: "Invalid presigned GET URL format".to_string(),
                },
            )?;

            // Extract headers from query parameters if any (some providers include them)
            let mut headers = std::collections::HashMap::new();
            for (key, value) in parsed_url.query_pairs() {
                if key.starts_with("X-") || key.starts_with("x-") {
                    headers.insert(key.to_string(), value.to_string());
                }
            }

            PresignedRequestBackend::Http {
                url: url.clone(),
                method: "GET".to_string(),
                headers,
            }
        } else if url.starts_with("local://") {
            // Local filesystem URL
            let file_path = url.strip_prefix("local://").unwrap_or(url);
            PresignedRequestBackend::Local {
                file_path: file_path.to_string(),
                operation: LocalOperation::Get,
            }
        } else {
            return Err(AlienError::new(ErrorData::InvalidConfigurationUrl {
                url: url.to_string(),
                reason: "Unsupported presigned URL scheme".to_string(),
            }));
        };

        Ok(PresignedRequest {
            backend,
            expiration,
            operation: PresignedOperation::Get,
            path: path.to_string(),
        })
    }

    async fn presigned_delete(
        &self,
        path: &Path,
        expires_in: Duration,
    ) -> crate::error::Result<PresignedRequest> {
        let mut client = self.client();
        let expiration = Utc::now()
            + chrono::Duration::from_std(expires_in)
                .into_alien_error()
                .context(ErrorData::Other {
                    message: "Invalid duration for presigned DELETE request".to_string(),
                })?;

        let request = StorageSignedUrlRequest {
            binding_name: self.binding_name.clone(),
            path: path.to_string(),
            http_method: StorageHttpMethod::HttpMethodDelete as i32,
            expiration_time: Some(prost_types::Timestamp {
                seconds: expiration.timestamp(),
                nanos: expiration.timestamp_subsec_nanos() as i32,
            }),
        };

        let response = client
            .signed_url(tonic::Request::new(request))
            .await
            .into_alien_error()
            .context(ErrorData::GrpcRequestFailed {
                service: "storage".to_string(),
                method: "signed_url".to_string(),
                details: "Failed to generate presigned DELETE URL".to_string(),
            })?
            .into_inner();

        // Parse the returned URL to determine if it's HTTP or local
        let url = &response.url;
        let backend = if url.starts_with("http://") || url.starts_with("https://") {
            // HTTP-based presigned URL (AWS S3, GCP GCS, Azure Blob)
            let parsed_url = reqwest::Url::parse(url).into_alien_error().context(
                ErrorData::InvalidConfigurationUrl {
                    url: url.to_string(),
                    reason: "Invalid presigned DELETE URL format".to_string(),
                },
            )?;

            // Extract headers from query parameters if any (some providers include them)
            let mut headers = std::collections::HashMap::new();
            for (key, value) in parsed_url.query_pairs() {
                if key.starts_with("X-") || key.starts_with("x-") {
                    headers.insert(key.to_string(), value.to_string());
                }
            }

            PresignedRequestBackend::Http {
                url: url.clone(),
                method: "DELETE".to_string(),
                headers,
            }
        } else if url.starts_with("local://") {
            // Local filesystem URL
            let file_path = url.strip_prefix("local://").unwrap_or(url);
            PresignedRequestBackend::Local {
                file_path: file_path.to_string(),
                operation: LocalOperation::Delete,
            }
        } else {
            return Err(AlienError::new(ErrorData::InvalidConfigurationUrl {
                url: url.to_string(),
                reason: "Unsupported presigned URL scheme".to_string(),
            }));
        };

        Ok(PresignedRequest {
            backend,
            expiration,
            operation: PresignedOperation::Delete,
            path: path.to_string(),
        })
    }
}

#[async_trait]
impl object_store::ObjectStore for GrpcStorage {
    async fn put_opts(
        &self,
        location: &Path,
        payload: PutPayload,
        options: PutOptions,
    ) -> object_store::Result<PutResult> {
        let mut client = self.client();
        let path_str = location.to_string();
        // PutPayload from object_store 0.11.2 can be converted to Bytes directly (synchronously)
        let data_bytes: Bytes = payload.into();

        let proto_request = StoragePutRequest {
            binding_name: self.binding_name.clone(),
            path: path_str.clone(),
            data: data_bytes.into(), // prost Bytes from std Vec<u8> or bytes::Bytes
            options: storage_utils::map_os_put_options_to_proto(options),
        };

        let response = client
            .put(Request::new(proto_request))
            .await
            .map_err(|s| storage_utils::map_status_to_os_error(s, Some(path_str)))?
            .into_inner();

        Ok(PutResult {
            e_tag: response.e_tag,
            version: response.version,
        })
    }

    async fn put_multipart_opts(
        &self,
        location: &Path,
        opts: PutMultipartOpts,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        let mut client = self.client();
        let path_str = location.to_string();

        let metadata_proto = StoragePutMultipartMetadata {
            binding_name: self.binding_name.clone(),
            path: path_str.clone(),
            options: storage_utils::map_os_put_multipart_opts_to_proto(opts),
        };

        let initial_request_part = StoragePutMultipartChunkRequestPart::Metadata(metadata_proto);
        let initial_request = StoragePutMultipartChunkRequest {
            part: Some(initial_request_part),
        };

        let (tx, rx) = mpsc::channel::<object_store::Result<StoragePutMultipartChunkRequest>>(4);

        tx.send(Ok(initial_request))
            .await
            .map_err(|_e| ObjectStoreError::Generic {
                store: "GrpcClient::put_multipart_opts",
                source: "Failed to send initial metadata for multipart upload".into(),
            })?;

        // tonic::Request::new can take a stream directly.
        // Ensure the stream item type matches what client.put_multipart expects.
        // The stream should yield `StoragePutMultipartChunkRequest`.
        let request_stream = ReceiverStream::new(rx).map(|result_item| {
            match result_item {
                Ok(req) => req, // This is StoragePutMultipartChunkRequest
                Err(e) => {     // This is object_store::Error
                    // The gRPC stream expects StoragePutMultipartChunkRequest, not Result<..., object_store::Error>
                    // If an error occurs converting a part, we should probably abort the stream from the client side.
                    // For now, this error path in the stream mapping is problematic.
                    // The put_part method itself returns Result, if it fails, the stream from rx will just end.
                    // So, we should not be sending object_store::Error into this stream.
                    // Let's assume rx only sends Ok(StoragePutMultipartChunkRequest) and ends if put_part fails.
                    // This implies put_part failing should not try to send an error message via this stream.
                    // This mapping needs to be infallible or handle errors differently.
                    // For now, we assume `put_part` handles its errors and closes the stream if needed.
                    // A simpler map that assumes `put_part` only sends `Ok` or stops:
                    panic!("Error received in put_multipart request stream from mpsc: {:?}. This should not happen.", e);
                }
            }
        });

        let response_join_handle: JoinHandle<Result<tonic::Response<StoragePutResponse>, Status>> =
            tokio::spawn(async move { client.put_multipart(Request::new(request_stream)).await });

        Ok(Box::new(GrpcMultipartUpload {
            path: location.clone(),
            client_stream_sender: Some(tx),
            response_join_handle: Some(response_join_handle),
        }))
    }

    async fn get_opts(
        &self,
        location: &Path,
        options: GetOptions,
    ) -> object_store::Result<GetResult> {
        let mut client = self.client();
        let path_str = location.to_string();

        let request_options = options.clone(); // Clone for potential later use with range
        let proto_request = storage_utils::map_os_get_options_to_proto_request(
            options,
            self.binding_name.clone(),
            path_str.clone(),
        );

        let mut grpc_stream = client
            .get(Request::new(proto_request))
            .await
            .map_err(|s| storage_utils::map_status_to_os_error(s, Some(path_str.clone())))?
            .into_inner();

        let first_part_res = grpc_stream.next().await;
        let first_part = match first_part_res {
            Some(Ok(part)) => part,
            Some(Err(status)) => {
                return Err(storage_utils::map_status_to_os_error(
                    status,
                    Some(path_str.clone()),
                ))
            }
            None => {
                return Err(ObjectStoreError::Generic {
                    store: "gRPC",
                    source: "GetResponsePart stream was empty, expected metadata".into(),
                })
            }
        };

        let proto_meta = match first_part.part {
            Some(get_response_part::Part::Metadata(meta)) => meta,
            _ => {
                return Err(ObjectStoreError::Generic {
                    store: "gRPC",
                    source: "First message in GetResponsePart stream was not Metadata".into(),
                })
            }
        };
        let object_meta = storage_utils::map_proto_object_meta_to_os(proto_meta)?;

        let (tx, rx) = mpsc::channel::<object_store::Result<Bytes>>(4);
        let error_path_clone = path_str.clone();

        tokio::spawn(async move {
            while let Some(stream_item_result) = grpc_stream.next().await {
                match stream_item_result {
                    Ok(response_part) => match response_part.part {
                        Some(get_response_part::Part::ChunkData(data)) => {
                            if tx.send(Ok(data.into())).await.is_err() {
                                break;
                            }
                        }
                        Some(get_response_part::Part::Metadata(_)) => {
                            let _ = tx
                                .send(Err(ObjectStoreError::Generic {
                                    store: "gRPC",
                                    source: "Received metadata again in GetResponsePart stream"
                                        .into(),
                                }))
                                .await;
                            break;
                        }
                        None => {
                            let _ = tx
                                .send(Err(ObjectStoreError::Generic {
                                    store: "gRPC",
                                    source: "Empty part in GetResponsePart stream".into(),
                                }))
                                .await;
                            break;
                        }
                    },
                    Err(status) => {
                        let _ = tx
                            .send(Err(storage_utils::map_status_to_os_error(
                                status,
                                Some(error_path_clone.clone()),
                            )))
                            .await;
                        break;
                    }
                }
            }
        });

        let calculated_range = request_options
            .range
            .map_or(0..object_meta.size, |r| match r {
                OsGetRange::Bounded(br) => br.start..br.end,
                OsGetRange::Offset(o) => std::cmp::min(o, object_meta.size)..object_meta.size,
                OsGetRange::Suffix(s) => object_meta.size.saturating_sub(s)..object_meta.size,
            });

        Ok(GetResult {
            payload: GetResultPayload::Stream(ReceiverStream::new(rx).boxed()),
            meta: object_meta,
            range: calculated_range,
            attributes: OsAttributes::default(), // TODO: Map attributes from proto_meta if available
        })
    }

    async fn head(&self, location: &Path) -> object_store::Result<ObjectMeta> {
        let mut client = self.client();
        let path_str = location.to_string();

        let proto_request = StorageHeadRequest {
            binding_name: self.binding_name.clone(),
            path: path_str.clone(),
        };

        let response = client
            .head(Request::new(proto_request))
            .await
            .map_err(|s| storage_utils::map_status_to_os_error(s, Some(path_str)))?
            .into_inner();
        storage_utils::map_proto_object_meta_to_os(response)
    }

    async fn delete(&self, location: &Path) -> object_store::Result<()> {
        let mut client = self.client();
        let path_str = location.to_string();
        let proto_request = StorageDeleteRequest {
            binding_name: self.binding_name.clone(),
            path: path_str.clone(),
        };
        client
            .delete(Request::new(proto_request))
            .await
            .map_err(|s| storage_utils::map_status_to_os_error(s, Some(path_str)))?;
        Ok(())
    }

    fn list(&self, prefix: Option<&Path>) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        let mut client = self.client();
        let binding_name = self.binding_name.clone();
        let prefix_path_str = prefix.map(|p| p.to_string());

        try_stream! { // ensure async-stream is in Cargo.toml
            let proto_request = StorageListRequest {
                binding_name: binding_name.clone(),
                prefix: prefix_path_str.clone(),
                offset: None,
            };

            let mut stream = client.list(Request::new(proto_request)).await
                .map_err(|s| storage_utils::map_status_to_os_error(s, prefix_path_str.clone()))?
                .into_inner();

            while let Some(item_result) = stream.next().await {
                match item_result {
                    Ok(proto_meta) => {
                        yield storage_utils::map_proto_object_meta_to_os(proto_meta)?;
                    }
                    Err(status) => {
                        Err(storage_utils::map_status_to_os_error(status, prefix_path_str.clone()))?;
                    }
                }
            }
        }
        .boxed()
    }

    async fn list_with_delimiter(&self, prefix: Option<&Path>) -> object_store::Result<ListResult> {
        let mut client = self.client();
        let path_str = prefix.map(|p| p.to_string());
        let proto_request = StorageListWithDelimiterRequest {
            binding_name: self.binding_name.clone(),
            prefix: path_str.clone(),
        };
        let response = client
            .list_with_delimiter(Request::new(proto_request))
            .await
            .map_err(|s| storage_utils::map_status_to_os_error(s, path_str))?
            .into_inner();
        let common_prefixes = response
            .common_prefixes
            .into_iter()
            .map(Path::from)
            .collect();
        let objects = response
            .objects
            .into_iter()
            .map(storage_utils::map_proto_object_meta_to_os)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ListResult {
            common_prefixes,
            objects,
        })
    }

    async fn copy(&self, from: &Path, to: &Path) -> object_store::Result<()> {
        let mut client = self.client();
        let from_str = from.to_string();
        let to_str = to.to_string();
        let proto_request = StorageCopyRequest {
            binding_name: self.binding_name.clone(),
            from_path: from_str.clone(),
            to_path: to_str.clone(),
        };
        client
            .copy(Request::new(proto_request))
            .await
            .map_err(|s| {
                storage_utils::map_status_to_os_error(
                    s,
                    Some(format!("copy from {} to {}", from_str, to_str)),
                )
            })?;
        Ok(())
    }

    async fn rename(&self, from: &Path, to: &Path) -> object_store::Result<()> {
        let mut client = self.client();
        let from_str = from.to_string();
        let to_str = to.to_string();
        let proto_request = StorageRenameRequest {
            binding_name: self.binding_name.clone(),
            from_path: from_str.clone(),
            to_path: to_str.clone(),
        };
        client
            .rename(Request::new(proto_request))
            .await
            .map_err(|s| {
                storage_utils::map_status_to_os_error(
                    s,
                    Some(format!("rename from {} to {}", from_str, to_str)),
                )
            })?;
        Ok(())
    }

    async fn copy_if_not_exists(&self, from: &Path, to: &Path) -> object_store::Result<()> {
        let mut client = self.client();
        let from_str = from.to_string();
        let to_str = to.to_string();
        let proto_request = StorageCopyRequest {
            binding_name: self.binding_name.clone(),
            from_path: from_str.clone(),
            to_path: to_str.clone(),
        };
        client
            .copy_if_not_exists(Request::new(proto_request))
            .await
            .map_err(|s| {
                storage_utils::map_status_to_os_error(
                    s,
                    Some(format!(
                        "copy_if_not_exists from {} to {}",
                        from_str, to_str
                    )),
                )
            })?;
        Ok(())
    }

    async fn rename_if_not_exists(&self, from: &Path, to: &Path) -> object_store::Result<()> {
        let mut client = self.client();
        let from_str = from.to_string();
        let to_str = to.to_string();
        let proto_request = StorageRenameRequest {
            binding_name: self.binding_name.clone(),
            from_path: from_str.clone(),
            to_path: to_str.clone(),
        };
        client
            .rename_if_not_exists(Request::new(proto_request))
            .await
            .map_err(|s| {
                storage_utils::map_status_to_os_error(
                    s,
                    Some(format!(
                        "rename_if_not_exists from {} to {}",
                        from_str, to_str
                    )),
                )
            })?;
        Ok(())
    }
}

#[derive(Debug)]
struct GrpcMultipartUpload {
    path: Path, // For error reporting
    client_stream_sender:
        Option<mpsc::Sender<object_store::Result<StoragePutMultipartChunkRequest>>>,
    response_join_handle: Option<JoinHandle<Result<tonic::Response<StoragePutResponse>, Status>>>,
}

#[async_trait]
impl MultipartUpload for GrpcMultipartUpload {
    fn put_part(&mut self, data: PutPayload) -> object_store::UploadPart {
        let sender_clone = match self.client_stream_sender.as_ref() {
            Some(s) => s.clone(),
            None => {
                return Box::pin(async {
                    Err(ObjectStoreError::Generic {
                        store: "GrpcMultipartUpload::put_part",
                        source: "Sender unavailable; put_part called after complete/abort or on failed init.".into(),
                    })
                });
            }
        };

        Box::pin(async move {
            let bytes_data: Bytes = data.into(); // Synchronous conversion
            let chunk_data_part = StoragePutMultipartChunkRequestPart::ChunkData(bytes_data.into());
            let request = StoragePutMultipartChunkRequest {
                part: Some(chunk_data_part),
            };

            sender_clone
                .send(Ok(request))
                .await
                .map_err(|e| ObjectStoreError::Generic {
                    store: "GrpcMultipartUpload::put_part",
                    source: format!("Failed to send part, gRPC call might have failed: {}", e)
                        .into(),
                })
        })
    }

    async fn complete(&mut self) -> object_store::Result<PutResult> {
        if let Some(sender) = self.client_stream_sender.take() {
            drop(sender);
        }

        let handle = self
            .response_join_handle
            .take()
            .ok_or_else(|| ObjectStoreError::Generic {
                store: "GrpcMultipartUpload::complete",
                source: "complete called more than once or on an already aborted/failed upload"
                    .into(),
            })?;

        match handle.await {
            Ok(Ok(response)) => {
                let put_response = response.into_inner();
                Ok(PutResult {
                    e_tag: put_response.e_tag,
                    version: put_response.version,
                })
            }
            Ok(Err(status)) => Err(storage_utils::map_status_to_os_error(
                status,
                Some(self.path.to_string()),
            )),
            Err(join_err) => Err(ObjectStoreError::from(join_err)),
        }
    }

    async fn abort(&mut self) -> object_store::Result<()> {
        if let Some(sender) = self.client_stream_sender.take() {
            drop(sender);
        }

        if let Some(handle) = self.response_join_handle.take() {
            handle.abort();
            match handle.await {
                Ok(Ok(_resp)) => {
                    // Task completed successfully even after abort signal.
                    // Consider this as successful abortion from client's perspective.
                    Ok(())
                }
                Ok(Err(status)) => {
                    if status.code() == tonic::Code::Cancelled {
                        Ok(()) // Expected outcome for cancellation
                    } else {
                        Err(storage_utils::map_status_to_os_error(
                            status,
                            Some(self.path.to_string()),
                        ))
                    }
                }
                Err(_join_err) => {
                    // JoinError after abort is expected if the task panicked or was forcefully terminated.
                    Ok(())
                }
            }
        } else {
            // Abort called after already completed or aborted.
            Ok(())
        }
    }
}
