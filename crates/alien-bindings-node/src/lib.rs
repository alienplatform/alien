//! `alien-bindings-node` — a napi-rs addon exposing `alien-bindings`
//! storage / kv / queue / vault to JavaScript.
//!
//! This crate is a pure argument/error translation layer. It contains no
//! provider logic: every method marshals arguments across the JS boundary,
//! delegates to `alien-bindings`, and maps errors through `error.rs`. Anything
//! beyond translation belongs in `alien-bindings`.

#![deny(clippy::all)]

mod error;
mod kv;
mod queue;
mod storage;
mod vault;

use crate::error::map_alien_error;
use alien_bindings::Bindings;
use napi_derive::napi;
use std::collections::HashMap;
use std::sync::Arc;

pub use kv::KvHandle;
pub use queue::QueueHandle;
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
    /// Construct from the process environment, optionally overlaid with
    /// `env_override`.
    ///
    /// - `None` → resolve from `std::env::vars()`.
    /// - `Some(overrides)` → merge `std::env::vars()` with `overrides` (override
    ///   wins) and resolve from the merged map.
    #[napi(constructor)]
    pub fn new(env_override: Option<HashMap<String, String>>) -> napi::Result<Self> {
        let bindings = match env_override {
            None => Bindings::from_env(),
            Some(overrides) => {
                let mut env: HashMap<String, String> = std::env::vars().collect();
                env.extend(overrides);
                Bindings::from_env_map(env)
            }
        }
        .map_err(map_alien_error)?;
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
}
