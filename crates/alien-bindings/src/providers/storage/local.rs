use crate::providers::utils::{prefixed_path, relativize_path};
use crate::{
    error::{Error, ErrorData},
    presigned::{LocalOperation, PresignedOperation, PresignedRequest, PresignedRequestBackend},
    traits::{Binding, Storage},
};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use futures::stream::BoxStream;
use futures::TryStreamExt as _;
use object_store::{
    local::LocalFileSystem, // Use LocalFileSystem directly
    parse_url,              // Also needed for the URL case
    path::Path,
    GetOptions,
    GetResult,
    ListResult,
    ObjectMeta,
    ObjectStore,
    PutMultipartOpts,
    PutOptions,
    PutPayload,
    PutResult,
    Result as ObjectStoreResult,
};
use std::path::PathBuf as StdPathBuf;
use std::time::Duration;
use url::Url;

/// Local storage implementation.
///
/// Can be backed by either a generic store parsed from a URL (typically `file://`)
/// or a `LocalFileSystem` instance pointing to a specific directory.
#[derive(Debug)]
pub struct LocalStorage {
    // Store the URL for reference, even if using LocalFileSystem directly
    url: Url,
    base_dir: Path,
    inner: Box<dyn ObjectStore>,
}

impl LocalStorage {
    /// Creates a new `LocalStorage` instance from a storage path.
    ///
    /// The path can be either:
    /// - A file:// URL (e.g., "file:///path/to/storage")
    /// - An absolute filesystem path (e.g., "/path/to/storage")
    pub fn new(storage_path: String) -> Result<Self, Error> {
        // Check if it's a file:// URL or an absolute path
        if storage_path.starts_with("file://") {
            Self::new_from_url(&storage_path)
        } else {
            // Treat as an absolute filesystem path
            Self::new_from_path(&storage_path)
        }
    }

    /// Creates a new `LocalStorage` instance from a URL string.
    /// Uses `parse_url` to handle the URL (likely `file://`).
    pub fn new_from_url(url_str: &str) -> Result<Self, Error> {
        let url =
            Url::parse(url_str)
                .into_alien_error()
                .context(ErrorData::InvalidConfigurationUrl {
                    url: url_str.to_string(),
                    reason: "Invalid storage URL for local storage".to_string(),
                })?;

        let (store, base_dir) =
            parse_url(&url)
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "local storage".to_string(),
                    reason: format!("Failed to initialize storage from URL '{}'", url_str),
                })?;

        Ok(Self {
            url,
            base_dir,
            inner: store,
        })
    }

    /// Creates a new `LocalStorage` instance from an absolute filesystem path.
    pub fn new_from_path(path: &str) -> Result<Self, Error> {
        let path_buf = StdPathBuf::from(path);

        // Create the directory if it doesn't exist
        std::fs::create_dir_all(&path_buf)
            .into_alien_error()
            .context(ErrorData::LocalFilesystemError {
                path: path_buf.to_string_lossy().to_string(),
                operation: "create_dir_all".to_string(),
            })?;

        // Build the LocalFileSystem store
        let store = LocalFileSystem::new_with_prefix(&path_buf)
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "local storage".to_string(),
                reason: format!("Failed to initialize LocalFileSystem at: {:?}", path_buf),
            })?;

        // Construct a file:// URL for reference
        let url_string = if cfg!(windows) {
            format!("file:///{}?path={}", path_buf.display(), path_buf.display()).replace('\\', "/")
        } else {
            format!("file://{}", path_buf.display())
        };

        let url = Url::parse(&url_string).into_alien_error().context(
            ErrorData::InvalidConfigurationUrl {
                url: url_string.clone(),
                reason: format!("Failed to construct file URL for path: {:?}", path_buf),
            },
        )?;

        Ok(Self {
            url,
            base_dir: Path::default(),
            inner: Box::new(store),
        })
    }
}

impl Binding for LocalStorage {}

#[async_trait]
impl Storage for LocalStorage {
    fn get_base_dir(&self) -> Path {
        // Note: When using LocalFileSystem::new_with_prefix, the prefix
        // is handled internally by the store. So the base_dir passed
        // to the ObjectStore methods should be relative to that prefix.
        // If created via parse_url, base_dir will have the path part.
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

        // For local storage, we need to convert the logical path to a filesystem path
        // This is a bit tricky since we need to map from the object store path to the actual file path
        let file_path = if let Some(url_path) = self.url.to_file_path().ok() {
            // URL-based local storage (file://)
            url_path.join(dst.to_string()).to_string_lossy().to_string()
        } else {
            // Direct LocalFileSystem usage - we need to infer the base directory
            // This is a simplification - in practice you might want to store the actual base path
            format!("{}/{}", self.url.path().trim_start_matches('/'), dst)
        };

        Ok(PresignedRequest {
            backend: PresignedRequestBackend::Local {
                file_path,
                operation: LocalOperation::Put,
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

        let file_path = if let Some(url_path) = self.url.to_file_path().ok() {
            url_path.join(dst.to_string()).to_string_lossy().to_string()
        } else {
            format!("{}/{}", self.url.path().trim_start_matches('/'), dst)
        };

        Ok(PresignedRequest {
            backend: PresignedRequestBackend::Local {
                file_path,
                operation: LocalOperation::Get,
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

        let file_path = if let Some(url_path) = self.url.to_file_path().ok() {
            url_path.join(dst.to_string()).to_string_lossy().to_string()
        } else {
            format!("{}/{}", self.url.path().trim_start_matches('/'), dst)
        };

        Ok(PresignedRequest {
            backend: PresignedRequestBackend::Local {
                file_path,
                operation: LocalOperation::Delete,
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

// Delegate ObjectStore trait implementation to the inner store.
// When using LocalFileSystem::new_with_prefix, paths passed here are
// relative to the prefix directory.
// When using parse_url("file:///prefix/..."), the base_dir adjustment handles the prefix.
#[async_trait]
impl ObjectStore for LocalStorage {
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
        // LocalFileSystem doesn't support attributes or tags, strip them to avoid UNIMPLEMENTED
        let opts = PutOptions {
            mode: opts.mode,
            tags: Default::default(),
            attributes: Default::default(),
            extensions: opts.extensions,
        };
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
        meta.location = relativize_path(&self.base_dir, meta.location, "LocalStorage")?;
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
                            relativize_path(&captured_base_dir, meta.location, "LocalStorage")?;
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

        for meta in &mut result.objects {
            let original_location = std::mem::take(&mut meta.location);
            meta.location = relativize_path(&self.base_dir, original_location, "LocalStorage")?;
        }

        let mut new_common_prefixes = Vec::with_capacity(result.common_prefixes.len());
        for cp in result.common_prefixes {
            new_common_prefixes.push(relativize_path(&self.base_dir, cp, "LocalStorage")?);
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

impl std::fmt::Display for LocalStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LocalStorage(url={})", self.url)
    }
}
