use crate::providers::storage::credential_bridge::AzureCredentialBridge;
use crate::providers::utils::{prefixed_path, relativize_path};
use crate::{
    error::{Error, ErrorData},
    presigned::{PresignedOperation, PresignedRequest, PresignedRequestBackend},
    traits::{Binding, Storage},
};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use futures::stream::BoxStream;
use futures::TryStreamExt as _;
use object_store::signer::Signer;
use object_store::{
    azure::MicrosoftAzure, path::Path, GetOptions, GetResult, ListResult, ObjectMeta, ObjectStore,
    PutMultipartOpts, PutOptions, PutPayload, PutResult, Result as ObjectStoreResult,
};
use reqwest::Method;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

/// Azure Blob Storage implementation.
#[derive(Debug)]
pub struct BlobStorage {
    url: Url,
    base_dir: Path,
    inner: MicrosoftAzure,
}

impl BlobStorage {
    /// Creates a new `BlobStorage` instance from container configuration.
    ///
    /// Uses Azure config for credentials.
    pub fn new(
        container_name: String,
        account_name: String,
        azure_config: &alien_core::AzureClientConfig,
    ) -> Result<Self, Error> {
        let azure_url = format!("azure://{}", container_name);
        let url: Url = Url::parse(&azure_url).into_alien_error().context(
            ErrorData::InvalidConfigurationUrl {
                url: azure_url.clone(),
                reason: "Invalid Azure Blob URL format".to_string(),
            },
        )?;

        // Build the store with credentials bridged from AzureClientConfig
        let credentials = AzureCredentialBridge::new(azure_config.clone());
        let store = object_store::azure::MicrosoftAzureBuilder::new()
            .with_container_name(&container_name)
            .with_account(&account_name)
            .with_credentials(Arc::new(credentials))
            .build()
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "Azure Blob storage".to_string(),
                reason: format!(
                    "Failed to build Azure Blob client for container: {}",
                    container_name
                ),
            })?;

        // Extract the base path from the URL path segments, handling the None case.
        let base_dir = match url.path_segments() {
            Some(segments) => Path::from_iter(segments.filter(|s| !s.is_empty())),
            None => Path::default(), // Use an empty path if there are no segments
        };

        Ok(Self {
            url,
            base_dir,
            inner: store,
        })
    }
}

impl Binding for BlobStorage {}

#[async_trait]
impl Storage for BlobStorage {
    fn get_base_dir(&self) -> Path {
        self.base_dir.clone()
    }

    fn get_url(&self) -> Url {
        self.url.clone()
    }

    async fn presigned_put(
        &self,
        path: &Path,
        expires_in: Duration,
    ) -> crate::error::Result<PresignedRequest> {
        let dst = prefixed_path(&self.base_dir, path);
        let signed_url = self
            .inner
            .signed_url(Method::PUT, &dst, expires_in)
            .await
            .into_alien_error()
            .context(ErrorData::StorageOperationFailed {
                binding_name: "azure-blob".to_string(),
                operation: format!("generate presigned PUT URL for {}", path),
            })?;

        let mut headers = HashMap::new();
        // Azure Blob Storage requires the blob type header for PUT operations
        headers.insert("x-ms-blob-type".to_string(), "BlockBlob".to_string());

        Ok(PresignedRequest {
            backend: PresignedRequestBackend::Http {
                url: signed_url.to_string(),
                method: "PUT".to_string(),
                headers,
            },
            expiration: Utc::now()
                + chrono::Duration::from_std(expires_in).map_err(|e| {
                    AlienError::new(ErrorData::Other {
                        message: format!("Invalid duration: {}", e),
                    })
                })?,
            operation: PresignedOperation::Put,
            path: path.to_string(),
        })
    }

    async fn presigned_get(
        &self,
        path: &Path,
        expires_in: Duration,
    ) -> crate::error::Result<PresignedRequest> {
        let dst = prefixed_path(&self.base_dir, path);
        let signed_url = self
            .inner
            .signed_url(Method::GET, &dst, expires_in)
            .await
            .into_alien_error()
            .context(ErrorData::StorageOperationFailed {
                binding_name: "azure-blob".to_string(),
                operation: format!("generate presigned GET URL for {}", path),
            })?;

        let headers = HashMap::new();

        Ok(PresignedRequest {
            backend: PresignedRequestBackend::Http {
                url: signed_url.to_string(),
                method: "GET".to_string(),
                headers,
            },
            expiration: Utc::now()
                + chrono::Duration::from_std(expires_in).map_err(|e| {
                    AlienError::new(ErrorData::Other {
                        message: format!("Invalid duration: {}", e),
                    })
                })?,
            operation: PresignedOperation::Get,
            path: path.to_string(),
        })
    }

    async fn presigned_delete(
        &self,
        path: &Path,
        expires_in: Duration,
    ) -> crate::error::Result<PresignedRequest> {
        let dst = prefixed_path(&self.base_dir, path);
        let signed_url = self
            .inner
            .signed_url(Method::DELETE, &dst, expires_in)
            .await
            .into_alien_error()
            .context(ErrorData::StorageOperationFailed {
                binding_name: "azure-blob".to_string(),
                operation: format!("generate presigned DELETE URL for {}", path),
            })?;

        let headers = HashMap::new();

        Ok(PresignedRequest {
            backend: PresignedRequestBackend::Http {
                url: signed_url.to_string(),
                method: "DELETE".to_string(),
                headers,
            },
            expiration: Utc::now()
                + chrono::Duration::from_std(expires_in).map_err(|e| {
                    AlienError::new(ErrorData::Other {
                        message: format!("Invalid duration: {}", e),
                    })
                })?,
            operation: PresignedOperation::Delete,
            path: path.to_string(),
        })
    }
}

// Delegate ObjectStore trait implementation to the inner store,
// prefixing paths with the base_dir.
#[async_trait]
impl ObjectStore for BlobStorage {
    async fn put(&self, location: &Path, payload: PutPayload) -> ObjectStoreResult<PutResult> {
        let dst = prefixed_path(&self.base_dir, location);
        self.inner.put(&dst, payload).await
    }

    async fn put_opts(
        &self,
        location: &Path,
        payload: PutPayload,
        opts: PutOptions,
    ) -> ObjectStoreResult<PutResult> {
        let dst = prefixed_path(&self.base_dir, location);
        self.inner.put_opts(&dst, payload, opts).await
    }

    async fn put_multipart(
        &self,
        location: &Path,
    ) -> ObjectStoreResult<Box<dyn object_store::MultipartUpload>> {
        let dst = prefixed_path(&self.base_dir, location);
        self.inner.put_multipart(&dst).await
    }

    async fn put_multipart_opts(
        &self,
        location: &Path,
        opts: PutMultipartOpts,
    ) -> ObjectStoreResult<Box<dyn object_store::MultipartUpload>> {
        let dst = prefixed_path(&self.base_dir, location);
        self.inner.put_multipart_opts(&dst, opts).await
    }

    async fn get(&self, location: &Path) -> ObjectStoreResult<GetResult> {
        let src = prefixed_path(&self.base_dir, location);
        self.inner.get(&src).await
    }

    async fn get_opts(&self, location: &Path, options: GetOptions) -> ObjectStoreResult<GetResult> {
        let src = prefixed_path(&self.base_dir, location);
        self.inner.get_opts(&src, options).await
    }

    async fn get_range(
        &self,
        location: &Path,
        range: std::ops::Range<u64>,
    ) -> ObjectStoreResult<Bytes> {
        let src = prefixed_path(&self.base_dir, location);
        self.inner.get_range(&src, range).await
    }

    async fn head(&self, location: &Path) -> ObjectStoreResult<ObjectMeta> {
        let src = prefixed_path(&self.base_dir, location);
        let mut meta = self.inner.head(&src).await?;
        meta.location = relativize_path(&self.base_dir, meta.location, "AzureStorage")?;
        Ok(meta)
    }

    async fn delete(&self, location: &Path) -> ObjectStoreResult<()> {
        let src = prefixed_path(&self.base_dir, location);
        self.inner.delete(&src).await
    }

    fn list(&self, prefix: Option<&Path>) -> BoxStream<'static, ObjectStoreResult<ObjectMeta>> {
        let list_prefix_for_inner = prefix
            .map(|p| prefixed_path(&self.base_dir, p))
            .unwrap_or_else(|| self.base_dir.clone());

        let base_dir_for_stream = self.base_dir.clone();

        Box::pin(
            self.inner
                .list(Some(&list_prefix_for_inner))
                .and_then(move |mut meta| {
                    let captured_base_dir = base_dir_for_stream.clone();
                    async move {
                        meta.location =
                            relativize_path(&captured_base_dir, meta.location, "AzureStorage")?;
                        Ok(meta)
                    }
                }),
        )
    }

    async fn list_with_delimiter(&self, prefix: Option<&Path>) -> ObjectStoreResult<ListResult> {
        let list_prefix_for_inner = prefix
            .map(|p| prefixed_path(&self.base_dir, p))
            .unwrap_or_else(|| self.base_dir.clone());
        let mut result = self
            .inner
            .list_with_delimiter(Some(&list_prefix_for_inner))
            .await?;

        for meta_obj in &mut result.objects {
            let original_location = std::mem::take(&mut meta_obj.location);
            meta_obj.location = relativize_path(&self.base_dir, original_location, "AzureStorage")?;
        }

        let mut new_common_prefixes = Vec::with_capacity(result.common_prefixes.len());
        for cp in result.common_prefixes {
            new_common_prefixes.push(relativize_path(&self.base_dir, cp, "AzureStorage")?);
        }
        result.common_prefixes = new_common_prefixes;

        Ok(result)
    }

    async fn copy(&self, from: &Path, to: &Path) -> ObjectStoreResult<()> {
        let src = prefixed_path(&self.base_dir, from);
        let dst = prefixed_path(&self.base_dir, to);
        self.inner.copy(&src, &dst).await
    }

    async fn copy_if_not_exists(&self, from: &Path, to: &Path) -> ObjectStoreResult<()> {
        let src = prefixed_path(&self.base_dir, from);
        let dst = prefixed_path(&self.base_dir, to);
        self.inner.copy_if_not_exists(&src, &dst).await
    }
}

impl std::fmt::Display for BlobStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AzureStorage(url={})", self.url)
    }
}
