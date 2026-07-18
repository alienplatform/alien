//! Integration test: external-app credential bootstrap through the *real*
//! manager mint route.
//!
//! `crates/alien-manager/tests/credentials_mint.rs` drives the mint handler
//! through `tower::ServiceExt::oneshot` — no real network involved. This file
//! instead spins the real manager `AppState`/router on a real TCP listener
//! (`axum::serve`) and points `alien_bindings::provider::BindingsProvider::
//! from_env_lazy` — the entry point an explicitly configured external/bootstrap
//! integration uses — at it over real HTTP, with a real deployment token minted by a real
//! `TokenStore`. That proves the client (`alien-bindings`'s minting resolver)
//! and server (`alien-manager`'s mint route) round-trip correctly end-to-end,
//! not just that each side unit-tests cleanly against a fake counterpart.
//!
//! Two scenarios, matching the two things that must never go wrong for a
//! bootstrapping external app:
//!
//!   * authorized — a real deployment token mints a working `ClientConfig`
//!     (the manager's local/resolver path, `ClientConfig::Local`) and a KV
//!     binding load through it actually works (a put/get round trip, not
//!     just "the load call returned Ok").
//!   * unauthorized — a token scoped to a *different* deployment surfaces a
//!     typed `REMOTE_ACCESS_FAILED` error through the resolver, bounded by a
//!     timeout so a regression that hangs instead of erroring fails loudly.

use std::collections::HashMap;
use std::io::Write;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sha2::{Digest, Sha256};
use tracing_subscriber::fmt::MakeWriter;

use alien_bindings::providers::kv::local::LocalKv;
use alien_bindings::providers::storage::local::LocalStorage;
use alien_bindings::traits::BindingsProviderApi;
use alien_bindings::BindingsProvider;
use alien_commands::dispatchers::NullCommandDispatcher;
use alien_commands::server::{CommandDispatcher, CommandRegistry, CommandServer};
use alien_commands::InMemoryCommandRegistry;
use alien_core::{
    Container, ContainerCode, PermissionProfile, Platform, ResourceLifecycle, ResourceSpec,
    RuntimeMetadata, ServiceAccount, Stack, StackSettings, StackState,
    CURRENT_DEPLOYMENT_PROTOCOL_VERSION, ENV_ALIEN_DEPLOYMENT_ID,
    ENV_ALIEN_DEPLOYMENT_SERVICE_ACCOUNT, ENV_ALIEN_DEPLOYMENT_TOKEN, ENV_ALIEN_DEPLOYMENT_TYPE,
    ENV_ALIEN_MANAGER_URL, ENV_ALIEN_RESOURCE_ID,
};
use alien_manager::auth::Subject;
use alien_manager::config::ManagerConfig;
use alien_manager::providers::local_credentials::LocalCredentialResolver;
use alien_manager::providers::token_db_validator::TokenDbValidator;
use alien_manager::providers::{NullTelemetryBackend, OssAuthz};
use alien_manager::routes::registry_proxy::{
    CredentialCache, PullValidationCache, RegistryRoutingTable,
};
use alien_manager::routes::AppState;
use alien_manager::stores::sqlite::{
    SqliteDatabase, SqliteDeploymentStore, SqliteReleaseStore, SqliteTokenStore,
};
use alien_manager::traits::{
    AuthValidator, CreateDeploymentGroupParams, CreateImportedDeploymentParams,
    CreateReleaseParams, CreateTokenParams, CredentialResolver, DeploymentStore, ReleaseStore,
    TelemetryBackend, TokenStore, TokenType,
};

const BINDING_NAME: &str = "execution-sa";

/// Bound every mint round trip in these tests: a regression that hangs
/// instead of erroring must fail the test quickly, not stall CI.
const ROUND_TRIP_TIMEOUT: Duration = Duration::from_secs(10);

// ---------------------------------------------------------------------------
// Fixture: a real manager AppState wired to the local/resolver credential
// path (`ClientConfig::Local`), so the happy-path binding load never needs
// real cloud credentials.
// ---------------------------------------------------------------------------

struct Fixture {
    state: AppState,
    /// Deployment the caller's real deployment token is scoped to.
    deployment_a: String,
    token_a: String,
    /// A deployment token scoped to a *different* deployment — used to prove
    /// the manager rejects a cross-deployment mint request.
    token_b: String,
}

async fn build() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("test.db");
    let local_state_dir = tmp.path().join("local-state");
    // Leak the tempdir so it survives until the test process exits (turso's
    // WAL mode needs a real file path for the duration of the test).
    std::mem::forget(tmp);

    let db = Arc::new(
        SqliteDatabase::new(&db_path.to_string_lossy())
            .await
            .unwrap(),
    );
    let deployment_store: Arc<dyn DeploymentStore> =
        Arc::new(SqliteDeploymentStore::new(db.clone()));
    let release_store: Arc<dyn ReleaseStore> = Arc::new(SqliteReleaseStore::new(db.clone()));
    let token_store: Arc<dyn TokenStore> = Arc::new(SqliteTokenStore::new(db.clone()));

    let dg = deployment_store
        .create_deployment_group(
            &Subject::system(),
            CreateDeploymentGroupParams {
                name: "bootstrap-group".to_string(),
                max_deployments: 100,
            },
        )
        .await
        .unwrap();

    let release = release_store
        .create_release(
            &Subject::system(),
            CreateReleaseParams {
                project_id: "default".to_string(),
                stacks: HashMap::from([(Platform::Local, mint_test_stack())]),
                git_commit_sha: None,
                git_commit_ref: None,
                git_commit_message: None,
            },
        )
        .await
        .unwrap();

    let (deployment_a, token_a) = create_deployment(
        &deployment_store,
        &token_store,
        &dg.id,
        "app-a",
        &release.id,
    )
    .await;
    let (_deployment_b, token_b) = create_deployment(
        &deployment_store,
        &token_store,
        &dg.id,
        "app-b",
        &release.id,
    )
    .await;

    let auth_validator: Arc<dyn AuthValidator> =
        Arc::new(TokenDbValidator::new(token_store.clone()));
    let authz: Arc<dyn alien_manager::auth::Authz> = Arc::new(OssAuthz);
    let telemetry_backend: Arc<dyn TelemetryBackend> = Arc::new(NullTelemetryBackend);
    // The real single-account/local-mode resolver: it's what a Local-platform
    // deployment resolves to in production, giving this test a real
    // production type on the credential-resolution side, not a test double.
    let credential_resolver: Arc<dyn CredentialResolver> =
        Arc::new(LocalCredentialResolver::new(local_state_dir));

    let kv_dir = db_path.parent().unwrap().join("kv");
    let storage_dir = db_path.parent().unwrap().join("storage");
    let kv: Arc<dyn alien_bindings::traits::Kv> =
        Arc::new(LocalKv::new(kv_dir.clone()).await.unwrap());
    let command_storage: Arc<dyn alien_bindings::traits::Storage> =
        Arc::new(LocalStorage::new(storage_dir.to_string_lossy().to_string()).unwrap());
    let command_dispatcher: Arc<dyn CommandDispatcher> = Arc::new(NullCommandDispatcher);
    let command_registry: Arc<dyn CommandRegistry> = Arc::new(InMemoryCommandRegistry::default());
    let command_server = Arc::new(CommandServer::new(
        kv.clone(),
        command_storage,
        command_dispatcher,
        command_registry,
        "http://localhost:0/v1".to_string(),
        b"test-signing-key".to_vec(),
    ));

    let state = AppState {
        deployment_store: deployment_store.clone(),
        release_store,
        token_store: token_store.clone(),
        auth_validator,
        authz,
        telemetry_backend,
        credential_resolver,
        command_server,
        config: Arc::new(ManagerConfig::default()),
        bindings_provider: None,
        target_bindings_providers: HashMap::new(),
        kv,
        http_client: reqwest::Client::new(),
        credential_cache: Arc::new(CredentialCache::new()),
        pull_validation_cache: Arc::new(PullValidationCache::new()),
        registry_routing_table: Arc::new(
            RegistryRoutingTable::new(vec![]).expect("empty routing table is unambiguous"),
        ),
        import_registry: Arc::new(alien_infra::ImporterRegistry::built_in()),
    };

    Fixture {
        state,
        deployment_a,
        token_a,
        token_b,
    }
}

fn mint_test_stack() -> Stack {
    let container = Container::new("api".to_string())
        .code(ContainerCode::Image {
            image: "example.invalid/api:latest".to_string(),
        })
        .cpu(ResourceSpec {
            min: "0.25".to_string(),
            desired: "0.5".to_string(),
        })
        .memory(ResourceSpec {
            min: "256Mi".to_string(),
            desired: "512Mi".to_string(),
        })
        .port(8080)
        .permissions("execution".to_string())
        .build();
    Stack::new("mint-bootstrap".to_string())
        .platforms(vec![Platform::Local])
        .permission("execution", PermissionProfile::new())
        .add(
            ServiceAccount::new(BINDING_NAME.to_string()).build(),
            ResourceLifecycle::Live,
        )
        .add(container, ResourceLifecycle::Live)
        .build()
}

async fn create_deployment(
    deployment_store: &Arc<dyn DeploymentStore>,
    token_store: &Arc<dyn TokenStore>,
    deployment_group_id: &str,
    name: &str,
    release_id: &str,
) -> (String, String) {
    let record = deployment_store
        .create_with_state(
            &Subject::system(),
            CreateImportedDeploymentParams {
                deployment_protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                name: name.to_string(),
                deployment_group_id: deployment_group_id.to_string(),
                platform: Platform::Local,
                base_platform: None,
                stack_settings: StackSettings::default(),
                stack_state: StackState::new(Platform::Local),
                environment_info: None,
                runtime_metadata: RuntimeMetadata::default(),
                status: "running".to_string(),
                current_release_id: Some(release_id.to_string()),
                desired_release_id: None,
                import_source: None,
                setup_metadata: None,
                setup_target: "test".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
                input_values: Default::default(),
                deployment_token: None,
                management_config: None,
            },
        )
        .await
        .unwrap();

    let token = mint_token(
        token_store,
        TokenType::Deployment,
        "ax_deploy_",
        Some(deployment_group_id.to_string()),
        Some(record.id.clone()),
    )
    .await;

    (record.id, token)
}

async fn mint_token(
    token_store: &Arc<dyn TokenStore>,
    token_type: TokenType,
    prefix: &str,
    deployment_group_id: Option<String>,
    deployment_id: Option<String>,
) -> String {
    let raw = format!("{}{}", prefix, uuid::Uuid::new_v4().simple());
    let key_prefix = raw[..12.min(raw.len())].to_string();
    let key_hash = {
        let mut h = Sha256::new();
        h.update(raw.as_bytes());
        format!("{:x}", h.finalize())
    };
    token_store
        .create_token(CreateTokenParams {
            token_type,
            key_prefix,
            key_hash,
            deployment_group_id,
            deployment_id,
        })
        .await
        .unwrap();
    raw
}

/// Serve the manager's real credentials router on a real TCP listener; returns
/// its base URL. The server task runs for the lifetime of the test process
/// (single-threaded `#[tokio::test]` runtime cooperatively schedules it
/// alongside the test body — see the tracing-capture comment below).
async fn spawn_manager(state: AppState) -> String {
    let router = alien_manager::routes::credentials::router().with_state(state);
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind real manager listener");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve manager");
    });
    format!("http://{addr}")
}

/// Env that forces `ClientConfig::from_env` to fail for a platform-agnostic
/// reason (no metadata service, no profile), so `LazyEnvBindingsProvider`
/// falls through to minting. Mirrors `mints_when_native_config_unavailable`
/// in `crate::provider`'s own selection tests.
fn base_env(
    manager_url: &str,
    deployment_token: &str,
    deployment_id: &str,
) -> HashMap<String, String> {
    HashMap::from([
        (
            ENV_ALIEN_DEPLOYMENT_TYPE.to_string(),
            Platform::Aws.as_str().to_string(),
        ),
        ("AWS_EC2_METADATA_DISABLED".to_string(), "true".to_string()),
        (
            "AWS_PROFILE".to_string(),
            "__alien_missing_test_profile__".to_string(),
        ),
        (ENV_ALIEN_MANAGER_URL.to_string(), manager_url.to_string()),
        (
            ENV_ALIEN_DEPLOYMENT_TOKEN.to_string(),
            deployment_token.to_string(),
        ),
        (
            ENV_ALIEN_DEPLOYMENT_ID.to_string(),
            deployment_id.to_string(),
        ),
        (
            ENV_ALIEN_DEPLOYMENT_SERVICE_ACCOUNT.to_string(),
            BINDING_NAME.to_string(),
        ),
        (ENV_ALIEN_RESOURCE_ID.to_string(), "api".to_string()),
    ])
}

fn local_kv_binding(data_dir: &Path) -> String {
    serde_json::json!({
        "service": "local-kv",
        "dataDir": data_dir.display().to_string(),
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// tracing capture (same pattern as `credentials_mint.rs`'s
// `audit_log_never_contains_credential_material`).
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct BufWriter(Arc<Mutex<Vec<u8>>>);

impl Write for BufWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for BufWriter {
    type Writer = BufWriter;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn authorized_bootstrap_mints_via_real_manager_and_loads_a_working_binding() {
    let fixture = build().await;
    let manager_url = spawn_manager(fixture.state.clone()).await;
    let data_dir = tempfile::tempdir().expect("tempdir");

    let mut env = base_env(&manager_url, &fixture.token_a, &fixture.deployment_a);
    env.insert(
        "ALIEN_CACHE_BINDING".to_string(),
        local_kv_binding(data_dir.path()),
    );

    // Capture tracing across the whole round trip. `#[tokio::test]` defaults
    // to a current-thread runtime, so the `axum::serve`-spawned server task
    // (and its per-request handler) is cooperatively scheduled on this same
    // OS thread — the thread-local default subscriber set here also captures
    // the manager's own audit event, not just anything logged by the client.
    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    let subscriber = tracing_subscriber::fmt()
        .with_writer(BufWriter(buf.clone()))
        .with_ansi(false)
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);

    let provider = BindingsProvider::from_env_lazy(env).expect("lazy construct");
    let kv = tokio::time::timeout(ROUND_TRIP_TIMEOUT, provider.load_kv("cache"))
        .await
        .expect("mint round trip must not hang")
        .expect("mint path should resolve a usable kv binding");

    // Prove the minted credentials actually work end-to-end, not just that
    // `load_kv` returned `Ok` — a put/get round trip through the resulting
    // binding.
    kv.put("hello", b"world".to_vec(), None)
        .await
        .expect("put through the minted binding");
    let value = kv
        .get("hello")
        .await
        .expect("get through the minted binding");

    drop(guard);

    assert_eq!(
        value,
        Some(b"world".to_vec()),
        "a binding loaded through minted credentials must actually read/write, not just construct"
    );

    let logs = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
    assert!(
        logs.contains("Minted deployment credentials"),
        "audit event must be emitted; captured logs: {logs}"
    );
    assert!(
        logs.contains(&fixture.deployment_a),
        "audit event must carry deploymentId; captured logs: {logs}"
    );
    assert!(
        logs.contains(BINDING_NAME),
        "audit event must carry bindingName; captured logs: {logs}"
    );
    assert!(
        logs.contains("resource_id=api"),
        "audit event must carry resourceId; captured logs: {logs}"
    );
    assert!(
        logs.contains("provider=local") && logs.contains("credential_source=resolver"),
        "audit event must carry the actual provider and credential source; captured logs: {logs}"
    );
    assert!(
        logs.contains("principal"),
        "audit event must carry the principal field; captured logs: {logs}"
    );
    assert!(
        logs.contains("expires_at"),
        "audit event must carry the expiry field; captured logs: {logs}"
    );
    assert!(
        !logs.contains(&fixture.token_a),
        "audit log must never contain the deployment token: {logs}"
    );
}

#[tokio::test]
async fn wrong_deployment_token_surfaces_a_typed_error_not_a_panic_or_hang() {
    let fixture = build().await;
    let manager_url = spawn_manager(fixture.state.clone()).await;
    let data_dir = tempfile::tempdir().expect("tempdir");

    // token_b is scoped to a different deployment than deployment_a: the real
    // manager must reject this with 403, and the resolver must turn that into
    // a typed error rather than panicking or hanging.
    let mut env = base_env(&manager_url, &fixture.token_b, &fixture.deployment_a);
    env.insert(
        "ALIEN_CACHE_BINDING".to_string(),
        local_kv_binding(data_dir.path()),
    );

    let provider = BindingsProvider::from_env_lazy(env).expect("lazy construct");
    let result = tokio::time::timeout(ROUND_TRIP_TIMEOUT, provider.load_kv("cache"))
        .await
        .expect("a rejected mint must fail fast, not hang");

    let error = result.expect_err("a token scoped to a different deployment must not mint");
    assert_eq!(
        error.code, "REMOTE_ACCESS_FAILED",
        "wrong-deployment rejection must surface as a typed error, got: {error}"
    );
}
