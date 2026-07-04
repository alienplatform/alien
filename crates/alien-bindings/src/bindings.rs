//! App-facing convenience API for accessing bindings.
//!
//! [`Bindings`] wraps a [`crate::provider::LazyEnvBindingsProvider`], giving application
//! code a small, stable surface — `storage`, `kv`, `queue`, `vault` — instead of the full
//! [`crate::traits::BindingsProviderApi`] used internally by the manager and controllers.

use crate::error::Result;
use crate::provider::{BindingsProvider, LazyEnvBindingsProvider};
use crate::traits::{BindingsProviderApi, Kv, Queue, Storage, Vault};
use std::collections::HashMap;
use std::sync::Arc;

/// App-facing entry point for accessing bindings configured via `ALIEN_*_BINDING`
/// environment variables.
///
/// Construction is synchronous and only validates the platform and each configured
/// binding's JSON shape (see [`BindingsProvider::from_env_lazy`]); cloud client
/// configuration and each binding's backing client are resolved lazily, on first use.
///
/// # Examples
///
/// This is the canonical usage for a Container/Daemon-shaped app (a long-running
/// resident process that only needs bindings, with no Worker event handlers):
///
/// ```no_run
/// use alien_bindings::Bindings;
/// use object_store::{path::Path, PutPayload};
///
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// let bindings = Bindings::from_env()?;
///
/// let storage = bindings.storage("files").await?;
/// storage
///     .put(&Path::from("greeting.txt"), PutPayload::from_static(b"hello"))
///     .await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct Bindings {
    provider: LazyEnvBindingsProvider,
}

impl Bindings {
    /// Sync-constructs `Bindings` from the current process environment.
    pub fn from_env() -> Result<Self> {
        Self::from_env_map(std::env::vars().collect())
    }

    /// Sync-constructs `Bindings` from an explicit environment map. Shared by `from_env`
    /// and by this module's tests, which inject `ALIEN_*_BINDING` variables this way
    /// instead of mutating the real process environment (process-global state that's
    /// unsafe to share across parallel tests).
    fn from_env_map(env: HashMap<String, String>) -> Result<Self> {
        Ok(Self {
            provider: BindingsProvider::from_env_lazy(env)?,
        })
    }

    /// Loads the object storage binding named `binding_name`.
    pub async fn storage(&self, binding_name: &str) -> Result<Arc<dyn Storage>> {
        self.provider.load_storage(binding_name).await
    }

    /// Loads the key-value store binding named `binding_name`.
    pub async fn kv(&self, binding_name: &str) -> Result<Arc<dyn Kv>> {
        self.provider.load_kv(binding_name).await
    }

    /// Loads the queue binding named `binding_name`.
    pub async fn queue(&self, binding_name: &str) -> Result<Arc<dyn Queue>> {
        self.provider.load_queue(binding_name).await
    }

    /// Loads the vault (secrets) binding named `binding_name`.
    pub async fn vault(&self, binding_name: &str) -> Result<Arc<dyn Vault>> {
        self.provider.load_vault(binding_name).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::binding_env_var;
    use crate::traits::MessagePayload;
    use alien_core::{Platform, ENV_ALIEN_DEPLOYMENT_TYPE};
    use object_store::{path::Path as ObjectPath, PutPayload};
    use std::collections::HashMap;
    use tempfile::TempDir;

    /// Minimal valid environment (no bindings configured yet).
    fn base_env() -> HashMap<String, String> {
        HashMap::from([(
            ENV_ALIEN_DEPLOYMENT_TYPE.to_string(),
            Platform::Local.as_str().to_string(),
        )])
    }

    fn with_binding(
        mut env: HashMap<String, String>,
        binding_name: &str,
        json: &str,
    ) -> HashMap<String, String> {
        env.insert(binding_env_var(binding_name), json.to_string());
        env
    }

    #[test]
    fn from_env_map_constructs_synchronously_from_injected_env() {
        // No `.await` here at all: proves construction is a plain sync function,
        // not something that merely returns a Future.
        let bindings =
            Bindings::from_env_map(base_env()).expect("valid env should construct Bindings");
        drop(bindings);
    }

    #[tokio::test]
    async fn storage_delegates_to_local_provider_and_performs_real_io() {
        let temp_dir = TempDir::new().expect("tempdir");
        let json = format!(
            r#"{{"service":"local-storage","storagePath":"{}"}}"#,
            temp_dir.path().display()
        );
        let env = with_binding(base_env(), "files", &json);
        let bindings = Bindings::from_env_map(env).expect("valid env should construct Bindings");

        let storage = bindings
            .storage("files")
            .await
            .expect("storage binding should load");

        let path = ObjectPath::from("greeting.txt");
        storage
            .put(&path, PutPayload::from(bytes::Bytes::from_static(b"hello")))
            .await
            .expect("put should succeed");
        let fetched = storage
            .get(&path)
            .await
            .expect("get should succeed")
            .bytes()
            .await
            .expect("reading bytes should succeed");
        assert_eq!(fetched.as_ref(), b"hello");
    }

    #[tokio::test]
    async fn kv_delegates_to_local_provider_and_performs_real_io() {
        let temp_dir = TempDir::new().expect("tempdir");
        let json = format!(
            r#"{{"service":"local-kv","dataDir":"{}"}}"#,
            temp_dir.path().display()
        );
        let env = with_binding(base_env(), "cache", &json);
        let bindings = Bindings::from_env_map(env).expect("valid env should construct Bindings");

        let kv = bindings.kv("cache").await.expect("kv binding should load");

        kv.put("greeting", b"hi".to_vec(), None)
            .await
            .expect("put should succeed");
        let value = kv
            .get("greeting")
            .await
            .expect("get should succeed")
            .expect("value should exist");
        assert_eq!(value, b"hi");
    }

    #[tokio::test]
    async fn queue_delegates_to_local_provider_and_performs_real_io() {
        let temp_dir = TempDir::new().expect("tempdir");
        let json = format!(
            r#"{{"service":"local-queue","queuePath":"{}"}}"#,
            temp_dir.path().join("queue.db").display()
        );
        let env = with_binding(base_env(), "jobs", &json);
        let bindings = Bindings::from_env_map(env).expect("valid env should construct Bindings");

        let queue = bindings
            .queue("jobs")
            .await
            .expect("queue binding should load");

        queue
            .send("jobs", MessagePayload::Text("hello".to_string()))
            .await
            .expect("send should succeed");
        let messages = queue
            .receive("jobs", 1)
            .await
            .expect("receive should succeed");
        assert_eq!(messages.len(), 1);
    }

    #[tokio::test]
    async fn vault_delegates_to_local_provider_and_performs_real_io() {
        let temp_dir = TempDir::new().expect("tempdir");
        let json = format!(
            r#"{{"service":"local-vault","vaultName":"secrets","dataDir":"{}"}}"#,
            temp_dir.path().display()
        );
        let env = with_binding(base_env(), "secrets", &json);
        let bindings = Bindings::from_env_map(env).expect("valid env should construct Bindings");

        let vault = bindings
            .vault("secrets")
            .await
            .expect("vault binding should load");

        vault
            .set_secret("api-key", "sekrit")
            .await
            .expect("set_secret should succeed");
        let value = vault
            .get_secret("api-key")
            .await
            .expect("get_secret should succeed");
        assert_eq!(value, "sekrit");
    }

    #[tokio::test]
    async fn missing_storage_binding_returns_binding_not_configured() {
        let bindings = Bindings::from_env_map(base_env())
            .expect("construction should succeed with no bindings configured");

        let error = bindings
            .storage("files")
            .await
            .expect_err("missing binding should error");

        assert_eq!(error.code, "BINDING_NOT_CONFIGURED");
        assert!(
            error.to_string().contains("ALIEN_FILES_BINDING"),
            "message should name the env var, got: {error}"
        );
    }

    #[test]
    fn malformed_binding_json_returns_binding_config_invalid_naming_env_var() {
        let env = with_binding(base_env(), "files", "not-json");

        let error =
            Bindings::from_env_map(env).expect_err("malformed binding JSON should fail to load");

        assert_eq!(error.code, "BINDING_CONFIG_INVALID");
        assert!(
            error.to_string().contains("ALIEN_FILES_BINDING"),
            "message should name the env var, got: {error}"
        );
    }

    #[tokio::test]
    async fn redis_kv_binding_returns_unsupported_binding_provider() {
        let json = r#"{"service":"redis","connectionUrl":"redis://localhost:6379"}"#;
        let env = with_binding(base_env(), "cache", json);
        let bindings = Bindings::from_env_map(env).expect("valid JSON should construct");

        let error = bindings
            .kv("cache")
            .await
            .expect_err("redis is not a supported kv provider in this build");

        assert_eq!(error.code, "UNSUPPORTED_BINDING_PROVIDER");
        assert!(
            error.to_string().contains("ALIEN_CACHE_BINDING"),
            "message should name the env var, got: {error}"
        );
    }
}
