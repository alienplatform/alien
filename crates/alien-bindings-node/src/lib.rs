//! `alien-bindings-node` — a napi-rs addon exposing `alien-bindings`
//! storage / kv / queue / vault / container to JavaScript.
//!
//! This crate is a pure argument/error translation layer. It contains no
//! provider logic: every method marshals arguments across the JS boundary,
//! delegates to `alien-bindings`, and maps errors through `error.rs`. Anything
//! beyond translation belongs in `alien-bindings`.

#![deny(clippy::all)]

mod container;
mod error;
mod kv;
mod queue;
#[cfg(feature = "platform-sdk")]
mod remote_storage;
mod storage;
mod vault;

use crate::error::map_alien_error;
use alien_bindings::Bindings;
#[cfg(feature = "platform-sdk")]
use alien_bindings::RemoteBindings;
use napi_derive::napi;
use std::sync::Arc;

pub use container::ContainerHandle;
pub use kv::KvHandle;
pub use queue::QueueHandle;
#[cfg(feature = "platform-sdk")]
pub use remote_storage::RemoteStorageHandle;
pub use storage::StorageHandle;
pub use vault::VaultHandle;

/// Returns the addon crate version. A synchronous surface used to smoke-test
/// that the native module loads and calls under a given runtime.
#[napi]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// The JS-facing entry point. Constructing this resolves and validates the
/// binding environment eagerly (config parse errors surface here); individual
/// bindings are resolved lazily by the accessor methods.
#[napi]
pub struct BindingsHandle {
    inner: Arc<Bindings>,
}

#[napi]
impl BindingsHandle {
    /// Construct from the process environment.
    #[napi(constructor)]
    pub fn new() -> napi::Result<Self> {
        let bindings = Bindings::from_env().map_err(map_alien_error)?;
        Ok(Self {
            inner: Arc::new(bindings),
        })
    }

    /// Resolve the storage binding named `name`.
    #[napi]
    pub async fn storage(&self, name: String) -> napi::Result<StorageHandle> {
        let inner = self.inner.clone();
        let storage = inner.storage(&name).await.map_err(map_alien_error)?;
        Ok(StorageHandle::new(storage, name))
    }

    /// Resolve the key-value binding named `name`.
    #[napi]
    pub async fn kv(&self, name: String) -> napi::Result<KvHandle> {
        let inner = self.inner.clone();
        let kv = inner.kv(&name).await.map_err(map_alien_error)?;
        Ok(KvHandle::new(kv))
    }

    /// Resolve the queue binding named `name`.
    #[napi]
    pub async fn queue(&self, name: String) -> napi::Result<QueueHandle> {
        let inner = self.inner.clone();
        let queue = inner.queue(&name).await.map_err(map_alien_error)?;
        Ok(QueueHandle::new(queue))
    }

    /// Resolve the vault binding named `name`.
    #[napi]
    pub async fn vault(&self, name: String) -> napi::Result<VaultHandle> {
        let inner = self.inner.clone();
        let vault = inner.vault(&name).await.map_err(map_alien_error)?;
        Ok(VaultHandle::new(vault))
    }

    /// Resolve the linked-container binding named `name`.
    #[napi]
    pub async fn container(&self, name: String) -> napi::Result<ContainerHandle> {
        let inner = self.inner.clone();
        let container = inner.container(&name).await.map_err(map_alien_error)?;
        Ok(ContainerHandle::new(container))
    }
}

/// The JS-facing remote entry point. Its narrow surface makes unsupported
/// binding kinds impossible to request through the native addon.
#[cfg(feature = "platform-sdk")]
#[napi]
pub struct RemoteBindingsHandle {
    inner: Arc<RemoteBindings>,
}

#[cfg(feature = "platform-sdk")]
#[napi]
impl RemoteBindingsHandle {
    /// Discover a deployment's assigned manager and create remote bindings.
    #[napi(factory)]
    pub async fn for_deployment(
        deployment_id: String,
        token: String,
        api_base_url: Option<String>,
    ) -> napi::Result<Self> {
        let bindings =
            RemoteBindings::for_deployment(&deployment_id, &token, api_base_url.as_deref())
                .await
                .map_err(map_alien_error)?;
        Ok(Self {
            inner: Arc::new(bindings),
        })
    }

    /// Resolve the storage binding named `name`.
    #[napi]
    pub async fn storage(&self, name: String) -> napi::Result<RemoteStorageHandle> {
        let storage = self.inner.storage(&name).await.map_err(map_alien_error)?;
        Ok(RemoteStorageHandle::new(storage, name))
    }
}
