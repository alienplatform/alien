//! Vault (secrets) binding handle. Thin argument/error translation over the
//! `Vault` trait.

use crate::error::map_alien_error;
use alien_bindings::Vault;
use napi_derive::napi;
use std::sync::Arc;

/// Handle to a resolved vault binding.
#[napi]
pub struct VaultHandle {
    inner: Arc<dyn Vault>,
}

impl VaultHandle {
    pub(crate) fn new(inner: Arc<dyn Vault>) -> Self {
        Self { inner }
    }
}

#[napi]
impl VaultHandle {
    /// Get the value of the secret named `name`.
    #[napi]
    pub async fn get_secret(&self, name: String) -> napi::Result<String> {
        let vault = self.inner.clone();
        vault.get_secret(&name).await.map_err(map_alien_error)
    }

    /// Create or update the secret named `name`.
    #[napi]
    pub async fn set_secret(&self, name: String, value: String) -> napi::Result<()> {
        let vault = self.inner.clone();
        vault
            .set_secret(&name, &value)
            .await
            .map_err(map_alien_error)
    }

    /// Delete the secret named `name`.
    #[napi]
    pub async fn delete_secret(&self, name: String) -> napi::Result<()> {
        let vault = self.inner.clone();
        vault.delete_secret(&name).await.map_err(map_alien_error)
    }

    /// List the names of all secrets in this vault.
    #[napi]
    pub async fn list_secrets(&self) -> napi::Result<Vec<String>> {
        let vault = self.inner.clone();
        vault.list_secrets().await.map_err(map_alien_error)
    }
}
