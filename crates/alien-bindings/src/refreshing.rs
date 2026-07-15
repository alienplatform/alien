//! App-facing binding handles that keep minted credentials fresh.
//!
//! Each operation re-enters [`LazyEnvBindingsProvider`]. Static/native
//! providers and fresh minted providers remain cheap cache hits; a minted
//! provider inside its refresh window is rebuilt once by `MintingResolver`'s
//! existing single-flight path. The resolved provider is held for the full
//! operation so credential rotation cannot swap it midway through a request.
//!
//! Methods that return an owned stream or multipart-upload session refresh
//! before creating it. An already-returned opaque stream/session remains bound
//! to that provider; the `object_store` API offers no way to replace its
//! credentials midway through the operation.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use alien_error::AlienError;
use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::{self, BoxStream};
use futures::{StreamExt, TryStreamExt};
use object_store::path::Path;
use object_store::{
    GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
    PutMultipartOptions, PutOptions as ObjectStorePutOptions, PutPayload, PutResult,
};
use url::Url;

use crate::error::{ErrorData, Result};
use crate::presigned::PresignedRequest;
use crate::provider::LazyEnvBindingsProvider;
use crate::traits::{
    Binding, BindingsProviderApi, Kv, MessagePayload, PutOptions as KvPutOptions, Queue,
    QueueMessage, ScanResult, Storage, Vault,
};

const OBJECT_STORE_NAME: &str = "Alien binding";

#[derive(Debug, Clone)]
struct Resolver {
    provider: Arc<LazyEnvBindingsProvider>,
    binding_name: String,
}

impl Resolver {
    fn new(provider: Arc<LazyEnvBindingsProvider>, binding_name: String) -> Self {
        Self {
            provider,
            binding_name,
        }
    }

    async fn storage(&self) -> Result<Arc<dyn Storage>> {
        self.provider.load_storage(&self.binding_name).await
    }

    async fn kv(&self) -> Result<Arc<dyn Kv>> {
        self.provider.load_kv(&self.binding_name).await
    }

    async fn queue(&self) -> Result<Arc<dyn Queue>> {
        self.provider.load_queue(&self.binding_name).await
    }

    async fn vault(&self) -> Result<Arc<dyn Vault>> {
        self.provider.load_vault(&self.binding_name).await
    }
}

fn object_store_error(source: AlienError<ErrorData>) -> object_store::Error {
    object_store::Error::Generic {
        store: OBJECT_STORE_NAME,
        source: Box::new(source),
    }
}

/// Storage handle that resolves a fresh-enough provider for every operation.
#[derive(Debug, Clone)]
pub(super) struct RefreshingStorage {
    resolver: Resolver,
    /// Storage topology does not change when credentials rotate. Capture it
    /// from the initially validated handle for the trait's synchronous calls,
    /// without retaining that handle's eventually stale credential client.
    base_dir: Path,
    url: Url,
}

impl RefreshingStorage {
    pub(super) fn new(
        provider: Arc<LazyEnvBindingsProvider>,
        binding_name: String,
        initial: Arc<dyn Storage>,
    ) -> Self {
        let base_dir = initial.get_base_dir();
        let url = initial.get_url();
        Self {
            resolver: Resolver::new(provider, binding_name),
            base_dir,
            url,
        }
    }

    async fn current(&self) -> object_store::Result<Arc<dyn Storage>> {
        self.resolver.storage().await.map_err(object_store_error)
    }
}

impl fmt::Display for RefreshingStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Alien storage binding '{}'", self.resolver.binding_name)
    }
}

impl Binding for RefreshingStorage {}

#[async_trait]
impl Storage for RefreshingStorage {
    fn get_base_dir(&self) -> Path {
        self.base_dir.clone()
    }

    fn get_url(&self) -> Url {
        self.url.clone()
    }

    async fn presigned_put(&self, path: &Path, expires_in: Duration) -> Result<PresignedRequest> {
        self.resolver
            .storage()
            .await?
            .presigned_put(path, expires_in)
            .await
    }

    async fn presigned_get(&self, path: &Path, expires_in: Duration) -> Result<PresignedRequest> {
        self.resolver
            .storage()
            .await?
            .presigned_get(path, expires_in)
            .await
    }

    async fn presigned_delete(
        &self,
        path: &Path,
        expires_in: Duration,
    ) -> Result<PresignedRequest> {
        self.resolver
            .storage()
            .await?
            .presigned_delete(path, expires_in)
            .await
    }
}

#[async_trait]
impl ObjectStore for RefreshingStorage {
    async fn put(&self, location: &Path, payload: PutPayload) -> object_store::Result<PutResult> {
        self.current().await?.put(location, payload).await
    }

    async fn put_opts(
        &self,
        location: &Path,
        payload: PutPayload,
        options: ObjectStorePutOptions,
    ) -> object_store::Result<PutResult> {
        self.current()
            .await?
            .put_opts(location, payload, options)
            .await
    }

    async fn put_multipart(
        &self,
        location: &Path,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        self.current().await?.put_multipart(location).await
    }

    async fn put_multipart_opts(
        &self,
        location: &Path,
        options: PutMultipartOptions,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        self.current()
            .await?
            .put_multipart_opts(location, options)
            .await
    }

    async fn get(&self, location: &Path) -> object_store::Result<GetResult> {
        self.current().await?.get(location).await
    }

    async fn get_opts(
        &self,
        location: &Path,
        options: GetOptions,
    ) -> object_store::Result<GetResult> {
        self.current().await?.get_opts(location, options).await
    }

    async fn get_range(&self, location: &Path, range: Range<u64>) -> object_store::Result<Bytes> {
        self.current().await?.get_range(location, range).await
    }

    async fn get_ranges(
        &self,
        location: &Path,
        ranges: &[Range<u64>],
    ) -> object_store::Result<Vec<Bytes>> {
        self.current().await?.get_ranges(location, ranges).await
    }

    async fn head(&self, location: &Path) -> object_store::Result<ObjectMeta> {
        self.current().await?.head(location).await
    }

    async fn delete(&self, location: &Path) -> object_store::Result<()> {
        self.current().await?.delete(location).await
    }

    fn list(&self, prefix: Option<&Path>) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        let resolver = self.resolver.clone();
        let prefix = prefix.cloned();
        stream::once(async move {
            let storage = resolver.storage().await.map_err(object_store_error)?;
            Ok::<BoxStream<'static, object_store::Result<ObjectMeta>>, object_store::Error>(
                storage.list(prefix.as_ref()),
            )
        })
        .try_flatten()
        .boxed()
    }

    fn list_with_offset(
        &self,
        prefix: Option<&Path>,
        offset: &Path,
    ) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        let resolver = self.resolver.clone();
        let prefix = prefix.cloned();
        let offset = offset.clone();
        stream::once(async move {
            let storage = resolver.storage().await.map_err(object_store_error)?;
            Ok::<BoxStream<'static, object_store::Result<ObjectMeta>>, object_store::Error>(
                storage.list_with_offset(prefix.as_ref(), &offset),
            )
        })
        .try_flatten()
        .boxed()
    }

    async fn list_with_delimiter(&self, prefix: Option<&Path>) -> object_store::Result<ListResult> {
        self.current().await?.list_with_delimiter(prefix).await
    }

    async fn copy(&self, from: &Path, to: &Path) -> object_store::Result<()> {
        self.current().await?.copy(from, to).await
    }

    async fn rename(&self, from: &Path, to: &Path) -> object_store::Result<()> {
        self.current().await?.rename(from, to).await
    }

    async fn copy_if_not_exists(&self, from: &Path, to: &Path) -> object_store::Result<()> {
        self.current().await?.copy_if_not_exists(from, to).await
    }

    async fn rename_if_not_exists(&self, from: &Path, to: &Path) -> object_store::Result<()> {
        self.current().await?.rename_if_not_exists(from, to).await
    }
}

/// Key-value handle that resolves a fresh-enough provider for every operation.
#[derive(Debug)]
pub(super) struct RefreshingKv {
    resolver: Resolver,
}

impl RefreshingKv {
    pub(super) fn new(provider: Arc<LazyEnvBindingsProvider>, binding_name: String) -> Self {
        Self {
            resolver: Resolver::new(provider, binding_name),
        }
    }
}

impl Binding for RefreshingKv {}

#[async_trait]
impl Kv for RefreshingKv {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.resolver.kv().await?.get(key).await
    }

    async fn put(&self, key: &str, value: Vec<u8>, options: Option<KvPutOptions>) -> Result<bool> {
        self.resolver.kv().await?.put(key, value, options).await
    }

    async fn delete(&self, key: &str) -> Result<()> {
        self.resolver.kv().await?.delete(key).await
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        self.resolver.kv().await?.exists(key).await
    }

    async fn scan_prefix(
        &self,
        prefix: &str,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> Result<ScanResult> {
        self.resolver
            .kv()
            .await?
            .scan_prefix(prefix, limit, cursor)
            .await
    }
}

/// Queue handle that resolves a fresh-enough provider for every operation.
#[derive(Debug)]
pub(super) struct RefreshingQueue {
    resolver: Resolver,
}

impl RefreshingQueue {
    pub(super) fn new(provider: Arc<LazyEnvBindingsProvider>, binding_name: String) -> Self {
        Self {
            resolver: Resolver::new(provider, binding_name),
        }
    }
}

impl Binding for RefreshingQueue {}

#[async_trait]
impl Queue for RefreshingQueue {
    async fn send(&self, queue: &str, message: MessagePayload) -> Result<()> {
        self.resolver.queue().await?.send(queue, message).await
    }

    async fn receive(&self, queue: &str, max_messages: usize) -> Result<Vec<QueueMessage>> {
        self.resolver
            .queue()
            .await?
            .receive(queue, max_messages)
            .await
    }

    async fn ack(&self, queue: &str, receipt_handle: &str) -> Result<()> {
        self.resolver
            .queue()
            .await?
            .ack(queue, receipt_handle)
            .await
    }

    async fn nack(&self, queue: &str, receipt_handle: &str) -> Result<()> {
        self.resolver
            .queue()
            .await?
            .nack(queue, receipt_handle)
            .await
    }

    async fn purge(&self, queue: &str) -> Result<()> {
        self.resolver.queue().await?.purge(queue).await
    }
}

/// Vault handle that resolves a fresh-enough provider for every operation.
#[derive(Debug)]
pub(super) struct RefreshingVault {
    resolver: Resolver,
}

impl RefreshingVault {
    pub(super) fn new(provider: Arc<LazyEnvBindingsProvider>, binding_name: String) -> Self {
        Self {
            resolver: Resolver::new(provider, binding_name),
        }
    }
}

impl Binding for RefreshingVault {}

#[async_trait]
impl Vault for RefreshingVault {
    async fn get_secret(&self, secret_name: &str) -> Result<String> {
        self.resolver.vault().await?.get_secret(secret_name).await
    }

    async fn set_secret(&self, secret_name: &str, value: &str) -> Result<()> {
        self.resolver
            .vault()
            .await?
            .set_secret(secret_name, value)
            .await
    }

    async fn delete_secret(&self, secret_name: &str) -> Result<()> {
        self.resolver
            .vault()
            .await?
            .delete_secret(secret_name)
            .await
    }

    async fn list_secrets(&self) -> Result<Vec<String>> {
        self.resolver.vault().await?.list_secrets().await
    }
}
