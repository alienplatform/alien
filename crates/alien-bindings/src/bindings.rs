//! App-facing convenience API for accessing bindings.
//!
//! [`Bindings`] wraps a [`crate::traits::BindingsProviderApi`], giving application code a
//! small, stable surface — `storage`, `kv`, `queue`, `vault` — instead of the full provider
//! API used internally by the manager and controllers. Environment-backed clients support
//! all configured kinds; remote v0 clients support Storage only.

use crate::error::Result;
use crate::provider::BindingsProvider;
use crate::refreshing::{RefreshingKv, RefreshingQueue, RefreshingStorage, RefreshingVault};
#[cfg(feature = "platform-sdk")]
use crate::remote::RemoteBindingsProvider;
use crate::traits::{BindingsProviderApi, Kv, Queue, Storage, Vault};
use std::collections::HashMap;
use std::sync::Arc;

/// App-facing entry point for environment-backed or resource-scoped remote
/// bindings.
///
/// Construction is synchronous and only validates each configured binding's JSON
/// shape (see [`BindingsProvider::from_env_deferred`]); the deployment platform,
/// cloud client configuration, and each binding's backing client are resolved
/// lazily, on first use. A first operation against a binding that is not
/// configured reports `BINDING_NOT_CONFIGURED` before any platform resolution, so
/// a zero-environment process still constructs and fails cleanly.
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
    provider: Arc<dyn BindingsProviderApi>,
}

impl Bindings {
    #[cfg(test)]
    pub(crate) fn from_provider(provider: Arc<dyn BindingsProviderApi>) -> Self {
        Self { provider }
    }

    /// Sync-constructs `Bindings` from the current process environment.
    pub fn from_env() -> Result<Self> {
        Self::from_env_map(std::env::vars().collect())
    }

    /// Sync-constructs `Bindings` from an explicit environment map instead of the process
    /// environment.
    ///
    /// This is public for embedders that resolve bindings from a caller-supplied map rather
    /// than `std::env` — notably the napi addon, which merges `std::env::vars()` with
    /// per-call overrides before constructing `Bindings`. It is also what `from_env`
    /// delegates to and what this module's tests use to inject `ALIEN_*_BINDING` variables
    /// (avoiding process-global state that's unsafe to share across parallel tests).
    pub fn from_env_map(env: HashMap<String, String>) -> Result<Self> {
        Ok(Self {
            provider: Arc::new(BindingsProvider::from_env_deferred(env)?),
        })
    }

    /// Discovers a deployment's assigned manager and creates a resource-scoped
    /// remote bindings client.
    ///
    /// The Platform API supplies only manager discovery. Each Storage binding
    /// is validated and resolved independently by the assigned manager, and its
    /// short-lived credentials refresh lazily without reconstructing this value
    /// or previously returned Storage handles.
    #[cfg(feature = "platform-sdk")]
    pub async fn for_remote_deployment(
        deployment_id: &str,
        token: &str,
        api_base_url: Option<&str>,
    ) -> Result<Self> {
        Ok(Self {
            provider: Arc::new(
                RemoteBindingsProvider::for_remote_deployment(deployment_id, token, api_base_url)
                    .await?,
            ),
        })
    }

    /// Loads the object storage binding named `binding_name`.
    ///
    /// The returned handle checks credential freshness before each operation.
    /// Native credentials and fresh short-lived credentials remain cached; a
    /// provider inside its refresh window is refreshed once under its shared
    /// resolver's single-flight guard.
    pub async fn storage(&self, binding_name: &str) -> Result<Arc<dyn Storage>> {
        let initial = self.provider.load_storage(binding_name).await?;
        Ok(Arc::new(RefreshingStorage::new(
            self.provider.clone(),
            binding_name.to_string(),
            initial,
        )))
    }

    /// Loads an environment-backed key-value binding that refreshes minted
    /// credentials before use. Remote v0 clients return
    /// `OPERATION_NOT_SUPPORTED`.
    pub async fn kv(&self, binding_name: &str) -> Result<Arc<dyn Kv>> {
        self.provider.load_kv(binding_name).await?;
        Ok(Arc::new(RefreshingKv::new(
            self.provider.clone(),
            binding_name.to_string(),
        )))
    }

    /// Loads an environment-backed queue binding that refreshes minted
    /// credentials before use. Remote v0 clients return
    /// `OPERATION_NOT_SUPPORTED`.
    pub async fn queue(&self, binding_name: &str) -> Result<Arc<dyn Queue>> {
        self.provider.load_queue(binding_name).await?;
        Ok(Arc::new(RefreshingQueue::new(
            self.provider.clone(),
            binding_name.to_string(),
        )))
    }

    /// Loads an environment-backed vault binding that refreshes minted
    /// credentials before use. Remote v0 clients return
    /// `OPERATION_NOT_SUPPORTED`.
    pub async fn vault(&self, binding_name: &str) -> Result<Arc<dyn Vault>> {
        self.provider.load_vault(binding_name).await?;
        Ok(Arc::new(RefreshingVault::new(
            self.provider.clone(),
            binding_name.to_string(),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::error::binding_env_var;
    use crate::traits::MessagePayload;
    use alien_core::{
        Platform, ENV_ALIEN_DEPLOYMENT_ID, ENV_ALIEN_DEPLOYMENT_SERVICE_ACCOUNT,
        ENV_ALIEN_DEPLOYMENT_TOKEN, ENV_ALIEN_DEPLOYMENT_TYPE, ENV_ALIEN_MANAGER_URL,
        ENV_ALIEN_RESOURCE_ID,
    };
    use axum::{extract::State, routing::post, Json, Router};
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

    #[derive(Clone)]
    struct MintServerState {
        calls: Arc<AtomicUsize>,
        state_directory: String,
    }

    async fn mint_handler(State(state): State<MintServerState>) -> Json<serde_json::Value> {
        let call = state.calls.fetch_add(1, Ordering::SeqCst) + 1;
        let lifetime_seconds = if call == 1 { 120 } else { 3600 };
        let expires_at =
            (chrono::Utc::now() + chrono::Duration::seconds(lifetime_seconds)).to_rfc3339();
        Json(serde_json::json!({
            "clientConfig": {
                "platform": "local",
                "state_directory": state.state_directory,
            },
            "expiresAt": expires_at,
            "principal": "local:refreshing-binding-test",
        }))
    }

    async fn spawn_mint_server(state_directory: &str) -> (String, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route("/v1/credentials/mint", post(mint_handler))
            .with_state(MintServerState {
                calls: calls.clone(),
                state_directory: state_directory.to_string(),
            });
        let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .expect("bind fake mint server");
        let address = listener.local_addr().expect("read fake server address");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve fake mint endpoint");
        });
        (format!("http://{address}"), calls)
    }

    fn mint_env(manager_url: &str) -> HashMap<String, String> {
        HashMap::from([
            (
                ENV_ALIEN_DEPLOYMENT_TYPE.to_string(),
                Platform::Aws.as_str().to_string(),
            ),
            ("AWS_EC2_METADATA_DISABLED".to_string(), "true".to_string()),
            (
                "AWS_PROFILE".to_string(),
                "__alien_missing_refresh_test_profile__".to_string(),
            ),
            (ENV_ALIEN_MANAGER_URL.to_string(), manager_url.to_string()),
            (
                ENV_ALIEN_DEPLOYMENT_TOKEN.to_string(),
                "refresh-test-token".to_string(),
            ),
            (
                ENV_ALIEN_DEPLOYMENT_ID.to_string(),
                "refresh-test-deployment".to_string(),
            ),
            (
                ENV_ALIEN_DEPLOYMENT_SERVICE_ACCOUNT.to_string(),
                "refresh-test-service-account".to_string(),
            ),
            (
                ENV_ALIEN_RESOURCE_ID.to_string(),
                "refresh-test-resource".to_string(),
            ),
        ])
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
    async fn long_lived_kv_handle_refreshes_minted_provider_before_expiry() {
        let temp_dir = TempDir::new().expect("tempdir");
        let (manager_url, calls) = spawn_mint_server(
            temp_dir
                .path()
                .to_str()
                .expect("tempdir path must be valid UTF-8"),
        )
        .await;
        let json = format!(
            r#"{{"service":"local-kv","dataDir":"{}"}}"#,
            temp_dir.path().display()
        );
        let env = with_binding(mint_env(&manager_url), "cache", &json);
        let bindings = Bindings::from_env_map(env).expect("minting env should construct Bindings");

        let kv = bindings
            .kv("cache")
            .await
            .expect("first binding resolution should mint credentials");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        kv.put("greeting", b"hi".to_vec(), None)
            .await
            .expect("the long-lived handle should refresh and write");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "the first mint is still unexpired but inside the refresh window"
        );

        let value = kv
            .get("greeting")
            .await
            .expect("the same long-lived handle should read")
            .expect("value should exist");
        assert_eq!(value, b"hi");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "the refreshed provider should stay cached while fresh"
        );
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
    async fn queue_nack_and_purge_reachable_through_trait_object() {
        // The point of promoting nack/purge onto the Queue trait: they must be
        // callable on the `Arc<dyn Queue>` the app-facing API hands back.
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

        // nack: an in-flight message under the default lease is hidden, but a
        // nack makes it immediately redeliverable.
        queue
            .send("jobs", MessagePayload::Text("retry".to_string()))
            .await
            .expect("send should succeed");
        let first = queue
            .receive("jobs", 1)
            .await
            .expect("receive should succeed");
        assert_eq!(first.len(), 1);
        assert!(
            queue
                .receive("jobs", 1)
                .await
                .expect("receive should succeed")
                .is_empty(),
            "in-flight message must be hidden before nack"
        );
        queue
            .nack("jobs", &first[0].receipt_handle)
            .await
            .expect("nack should succeed");
        let redelivered = queue
            .receive("jobs", 1)
            .await
            .expect("receive should succeed");
        assert_eq!(redelivered.len(), 1, "nacked message must be redelivered");

        // purge: clears everything, in flight or visible.
        queue.purge("jobs").await.expect("purge should succeed");
        assert!(
            queue
                .receive("jobs", 1)
                .await
                .expect("receive should succeed")
                .is_empty(),
            "purge must empty the queue"
        );
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

        // list_secrets must be reachable through the `Arc<dyn Vault>` surface
        // and return the stored names.
        vault
            .set_secret("db-url", "postgres://…")
            .await
            .expect("set_secret should succeed");
        let mut names = vault
            .list_secrets()
            .await
            .expect("list_secrets should succeed");
        names.sort();
        assert_eq!(names, vec!["api-key".to_string(), "db-url".to_string()]);
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

    #[tokio::test]
    async fn zero_env_construct_then_missing_binding_is_binding_not_configured() {
        // The app-facing contract: with NO deployment type and NO credentials,
        // construction must succeed and the FIRST op on a missing binding must
        // report BINDING_NOT_CONFIGURED (naming ALIEN_<NAME>_BINDING) BEFORE any
        // platform / client-config resolution. There is deliberately no
        // ALIEN_DEPLOYMENT_TYPE in this environment. Table test over all four
        // app-facing kinds so a future kind added to `Bindings` without wiring
        // `ensure_binding_present` into its `load_*` method fails this test
        // instead of silently regressing to ENVIRONMENT_VARIABLE_MISSING.
        for kind in ["storage", "kv", "queue", "vault"] {
            let bindings = Bindings::from_env_map(HashMap::new())
                .expect("zero-env construction must succeed (platform resolution deferred)");

            let error = match kind {
                "storage" => bindings.storage("x").await.unwrap_err(),
                "kv" => bindings.kv("x").await.unwrap_err(),
                "queue" => bindings.queue("x").await.unwrap_err(),
                "vault" => bindings.vault("x").await.unwrap_err(),
                other => unreachable!("unhandled kind in table test: {other}"),
            };

            assert_eq!(
                error.code, "BINDING_NOT_CONFIGURED",
                "{kind}: expected the missing-binding error, not a platform/deployment error: {error}"
            );
            assert!(
                error.to_string().contains("ALIEN_X_BINDING"),
                "{kind}: message should name the env var, got: {error}"
            );
        }
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
