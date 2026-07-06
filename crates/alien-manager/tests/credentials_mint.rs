//! Integration tests for `POST /v1/credentials/mint`.
//!
//! Drives the manager's credential-minting handler through a manually-assembled
//! [`alien_manager::routes::AppState`] and `tower::ServiceExt::oneshot`, mirroring
//! `tests/stack_import.rs`. The focus is the auth matrix (deployment token vs.
//! admin vs. wrong-deployment vs. missing) plus mint behaviour: duration
//! clamping, session-name wiring, short-lived-only managed credentials, response
//! shape, and the guarantee that the audit log never carries credential material.
//!
//! Two credential paths are exercised explicitly rather than faked into one:
//!   * **managed / impersonation** — a target bindings provider is configured
//!     for the platform, so the handler impersonates a service-account binding
//!     and returns session-token credentials.
//!   * **local / resolver** — no target provider, so the handler falls back to
//!     the deployment-level credential resolver, which returns static keys.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use http::header;
use sha2::{Digest, Sha256};
use tower::ServiceExt;

use alien_bindings::error::{ErrorData as BindingErrorData, Result as BindingResult};
use alien_bindings::traits::{
    AwsServiceAccountInfo, Binding, ImpersonationRequest, ServiceAccount, ServiceAccountInfo,
};
use alien_bindings::{providers::kv::local::LocalKv, providers::storage::local::LocalStorage};
use alien_bindings::BindingsProviderApi;
use alien_commands::dispatchers::NullCommandDispatcher;
use alien_commands::server::{CommandDispatcher, CommandRegistry, CommandServer};
use alien_commands::InMemoryCommandRegistry;
use alien_core::{
    AwsClientConfig, AwsCredentials, ClientConfig, Platform, StackSettings,
    CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
};
use alien_error::AlienError;
use alien_manager::auth::{Authz, Subject};
use alien_manager::config::ManagerConfig;
use alien_manager::providers::{NullTelemetryBackend, OssAuthz};
use alien_manager::routes::registry_proxy::{
    CredentialCache, PullValidationCache, RegistryRoutingTable,
};
use alien_manager::routes::AppState;
use alien_manager::stores::sqlite::{
    SqliteDatabase, SqliteDeploymentStore, SqliteReleaseStore, SqliteTokenStore,
};
use alien_manager::traits::{
    AuthValidator, CreateDeploymentGroupParams, CreateDeploymentParams, CreateTokenParams,
    CredentialResolver, DeploymentRecord, DeploymentStore, ReleaseStore, TelemetryBackend,
    TokenStore, TokenType,
};

// ---------------------------------------------------------------------------
// Test doubles
// ---------------------------------------------------------------------------

const FAKE_SECRET: &str = "TOP_SECRET_ACCESS_KEY_material_must_never_be_logged";
const FAKE_SESSION_TOKEN: &str = "TOP_SECRET_SESSION_TOKEN_material_must_never_be_logged";

fn missing_binding(binding_name: &str) -> AlienError<BindingErrorData> {
    AlienError::new(BindingErrorData::BindingConfigInvalid {
        binding_name: binding_name.to_string(),
        env_var: alien_core::bindings::binding_env_var_name(binding_name),
        reason: "not found".to_string(),
    })
}

/// Managed short-lived credentials, modelling what STS AssumeRole returns:
/// access keys *with* a session token.
fn managed_aws_config() -> ClientConfig {
    ClientConfig::Aws(Box::new(AwsClientConfig {
        account_id: "210987654321".to_string(),
        region: "us-east-1".to_string(),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: "ASIAEXAMPLE".to_string(),
            secret_access_key: FAKE_SECRET.to_string(),
            session_token: Some(FAKE_SESSION_TOKEN.to_string()),
        },
        service_overrides: None,
    }))
}

/// Local static credentials: access keys with *no* session token.
fn static_aws_config() -> ClientConfig {
    ClientConfig::Aws(Box::new(AwsClientConfig {
        account_id: "123456789012".to_string(),
        region: "us-east-1".to_string(),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: "AKIAEXAMPLE".to_string(),
            secret_access_key: FAKE_SECRET.to_string(),
            session_token: None,
        },
        service_overrides: None,
    }))
}

/// Fake service account that records the impersonation request it received and
/// returns preset short-lived credentials.
#[derive(Debug)]
struct FakeServiceAccount {
    info: ServiceAccountInfo,
    minted: ClientConfig,
    captured: Arc<Mutex<Option<ImpersonationRequest>>>,
}

impl Binding for FakeServiceAccount {}

#[async_trait]
impl ServiceAccount for FakeServiceAccount {
    async fn get_info(&self) -> BindingResult<ServiceAccountInfo> {
        Ok(self.info.clone())
    }

    async fn impersonate(&self, request: ImpersonationRequest) -> BindingResult<ClientConfig> {
        *self.captured.lock().unwrap() = Some(request);
        Ok(self.minted.clone())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Bindings provider that only serves a single service-account binding.
#[derive(Debug)]
struct FakeServiceAccountProvider {
    binding_name: String,
    service_account: Arc<FakeServiceAccount>,
}

#[async_trait]
impl BindingsProviderApi for FakeServiceAccountProvider {
    async fn load_service_account(
        &self,
        binding_name: &str,
    ) -> BindingResult<Arc<dyn ServiceAccount>> {
        if binding_name == self.binding_name {
            Ok(self.service_account.clone())
        } else {
            Err(missing_binding(binding_name))
        }
    }

    async fn load_storage(
        &self,
        binding_name: &str,
    ) -> BindingResult<Arc<dyn alien_bindings::traits::Storage>> {
        Err(missing_binding(binding_name))
    }

    async fn load_build(
        &self,
        binding_name: &str,
    ) -> BindingResult<Arc<dyn alien_bindings::traits::Build>> {
        Err(missing_binding(binding_name))
    }

    async fn load_artifact_registry(
        &self,
        binding_name: &str,
    ) -> BindingResult<Arc<dyn alien_bindings::traits::ArtifactRegistry>> {
        Err(missing_binding(binding_name))
    }

    async fn load_vault(
        &self,
        binding_name: &str,
    ) -> BindingResult<Arc<dyn alien_bindings::traits::Vault>> {
        Err(missing_binding(binding_name))
    }

    async fn load_kv(
        &self,
        binding_name: &str,
    ) -> BindingResult<Arc<dyn alien_bindings::traits::Kv>> {
        Err(missing_binding(binding_name))
    }

    async fn load_postgres(
        &self,
        binding_name: &str,
    ) -> BindingResult<Arc<dyn alien_bindings::traits::Postgres>> {
        Err(missing_binding(binding_name))
    }

    async fn load_queue(
        &self,
        binding_name: &str,
    ) -> BindingResult<Arc<dyn alien_bindings::traits::Queue>> {
        Err(missing_binding(binding_name))
    }

    async fn load_worker(
        &self,
        binding_name: &str,
    ) -> BindingResult<Arc<dyn alien_bindings::traits::Worker>> {
        Err(missing_binding(binding_name))
    }

    async fn load_container(
        &self,
        binding_name: &str,
    ) -> BindingResult<Arc<dyn alien_bindings::traits::Container>> {
        Err(missing_binding(binding_name))
    }
}

/// Credential resolver that always returns a preset config (the local path).
struct StaticCredentialResolver {
    config: ClientConfig,
}

#[async_trait]
impl CredentialResolver for StaticCredentialResolver {
    async fn resolve(&self, _deployment: &DeploymentRecord) -> Result<ClientConfig, AlienError> {
        Ok(self.config.clone())
    }
}

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

struct Fixture {
    state: AppState,
    /// Deployment the caller's token is scoped to.
    deployment_a: String,
    token_a: String,
    /// A deployment token scoped to a *different* deployment, used to prove
    /// cross-deployment access is denied.
    token_b: String,
    admin_token: String,
    /// The last impersonation request the fake service account received
    /// (only populated on the managed path).
    captured: Arc<Mutex<Option<ImpersonationRequest>>>,
}

const BINDING_NAME: &str = "management";

/// Build a fixture wired for the managed / impersonation path: a target bindings
/// provider for AWS that impersonates `BINDING_NAME`.
async fn impersonation_fixture() -> Fixture {
    let captured = Arc::new(Mutex::new(None));
    let service_account = Arc::new(FakeServiceAccount {
        info: ServiceAccountInfo::Aws(AwsServiceAccountInfo {
            role_name: "AlienManaged".to_string(),
            role_arn: "arn:aws:iam::210987654321:role/AlienManaged".to_string(),
        }),
        minted: managed_aws_config(),
        captured: captured.clone(),
    });
    let provider: Arc<dyn BindingsProviderApi> = Arc::new(FakeServiceAccountProvider {
        binding_name: BINDING_NAME.to_string(),
        service_account,
    });
    let target_providers = HashMap::from([(Platform::Aws, provider)]);

    // Resolver present but should never be hit on the impersonation path; give
    // it a config distinct from the managed one so a wrong branch is visible.
    let resolver: Arc<dyn CredentialResolver> = Arc::new(StaticCredentialResolver {
        config: static_aws_config(),
    });

    build(target_providers, resolver, captured).await
}

/// Build a fixture wired for the local / resolver path: no target providers, so
/// the handler falls back to the credential resolver.
async fn resolver_fixture() -> Fixture {
    let resolver: Arc<dyn CredentialResolver> = Arc::new(StaticCredentialResolver {
        config: static_aws_config(),
    });
    build(HashMap::new(), resolver, Arc::new(Mutex::new(None))).await
}

async fn build(
    target_bindings_providers: HashMap<Platform, Arc<dyn BindingsProviderApi>>,
    credential_resolver: Arc<dyn CredentialResolver>,
    captured: Arc<Mutex<Option<ImpersonationRequest>>>,
) -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("test.db");
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

    let admin_token = mint_token(&token_store, TokenType::Admin, "ax_admin_", None, None).await;

    let dg = deployment_store
        .create_deployment_group(
            &Subject::system(),
            CreateDeploymentGroupParams {
                name: "mint-group".to_string(),
                max_deployments: 100,
            },
        )
        .await
        .unwrap();

    let (deployment_a, token_a) =
        create_deployment(&deployment_store, &token_store, &dg.id, "deploy-a").await;
    let (_deployment_b, token_b) =
        create_deployment(&deployment_store, &token_store, &dg.id, "deploy-b").await;

    let auth_validator: Arc<dyn AuthValidator> = Arc::new(
        alien_manager::providers::token_db_validator::TokenDbValidator::new(token_store.clone()),
    );
    let authz: Arc<dyn Authz> = Arc::new(OssAuthz);
    let telemetry_backend: Arc<dyn TelemetryBackend> = Arc::new(NullTelemetryBackend);

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
        target_bindings_providers,
        kv,
        http_client: reqwest::Client::new(),
        credential_cache: Arc::new(CredentialCache::new()),
        pull_validation_cache: Arc::new(PullValidationCache::new()),
        registry_routing_table: Arc::new(RegistryRoutingTable::new(vec![])),
        import_registry: Arc::new(alien_infra::ImporterRegistry::built_in()),
    };

    Fixture {
        state,
        deployment_a,
        token_a,
        token_b,
        admin_token,
        captured,
    }
}

/// Create an AWS deployment and a deployment token scoped to it. Returns
/// `(deployment_id, raw_token)`.
async fn create_deployment(
    deployment_store: &Arc<dyn DeploymentStore>,
    token_store: &Arc<dyn TokenStore>,
    deployment_group_id: &str,
    name: &str,
) -> (String, String) {
    let record = deployment_store
        .create_deployment(
            &Subject::system(),
            CreateDeploymentParams {
                deployment_protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                name: name.to_string(),
                deployment_group_id: deployment_group_id.to_string(),
                platform: Platform::Aws,
                base_platform: None,
                stack_settings: StackSettings::default(),
                stack_state: None,
                environment_variables: None,
                input_values: Default::default(),
                deployment_token: None,
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

// ---------------------------------------------------------------------------
// Request helper
// ---------------------------------------------------------------------------

async fn post_mint(
    fixture: &Fixture,
    bearer: Option<&str>,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let router =
        alien_manager::routes::credentials::router().with_state(fixture.state.clone());

    let mut req = Request::builder()
        .method("POST")
        .uri("/v1/credentials/mint")
        .header(header::CONTENT_TYPE, "application/json");

    if let Some(token) = bearer {
        req = req.header(header::AUTHORIZATION, format!("Bearer {}", token));
    }

    let request = req.body(Body::from(serde_json::to_vec(&body).unwrap())).unwrap();
    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let json = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };
    (status, json)
}

fn mint_body(deployment_id: &str) -> serde_json::Value {
    serde_json::json!({
        "deploymentId": deployment_id,
        "bindingName": BINDING_NAME,
    })
}

// ---------------------------------------------------------------------------
// Auth matrix
// ---------------------------------------------------------------------------

#[tokio::test]
async fn deployment_token_for_its_deployment_mints_200() {
    let fixture = impersonation_fixture().await;
    let (status, json) = post_mint(
        &fixture,
        Some(&fixture.token_a),
        mint_body(&fixture.deployment_a),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "body = {json:#}");
    // Response shape.
    assert!(json["clientConfig"].is_object(), "clientConfig present");
    assert_eq!(
        json["principal"], "arn:aws:iam::210987654321:role/AlienManaged",
        "principal is the impersonated role"
    );
    let expires_at = json["expiresAt"].as_str().expect("expiresAt is a string");
    chrono::DateTime::parse_from_rfc3339(expires_at)
        .expect("expiresAt must be RFC3339");
}

#[tokio::test]
async fn deployment_token_for_other_deployment_is_forbidden() {
    let fixture = impersonation_fixture().await;
    // token_b is scoped to deployment_b; ask for deployment_a.
    let (status, json) = post_mint(
        &fixture,
        Some(&fixture.token_b),
        mint_body(&fixture.deployment_a),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN, "body = {json:#}");
}

#[tokio::test]
async fn missing_bearer_is_unauthorized() {
    let fixture = impersonation_fixture().await;
    let (status, _) = post_mint(&fixture, None, mint_body(&fixture.deployment_a)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn garbage_bearer_is_unauthorized() {
    let fixture = impersonation_fixture().await;
    let (status, _) = post_mint(
        &fixture,
        Some("ax_deploy_not-a-real-token"),
        mint_body(&fixture.deployment_a),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admin_token_mints_200() {
    let fixture = impersonation_fixture().await;
    let (status, json) = post_mint(
        &fixture,
        Some(&fixture.admin_token),
        mint_body(&fixture.deployment_a),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body = {json:#}");
    assert!(json["clientConfig"].is_object());
}

// ---------------------------------------------------------------------------
// Behaviour
// ---------------------------------------------------------------------------

#[tokio::test]
async fn duration_is_clamped_to_the_allowed_window() {
    // Below the floor -> 900.
    let fixture = impersonation_fixture().await;
    let mut body = mint_body(&fixture.deployment_a);
    body["durationSeconds"] = serde_json::json!(10);
    let (status, _) = post_mint(&fixture, Some(&fixture.token_a), body).await;
    assert_eq!(status, StatusCode::OK);
    let captured = fixture.captured.lock().unwrap().clone().expect("impersonated");
    assert_eq!(captured.duration_seconds, Some(900), "clamped up to the floor");

    // Above the ceiling -> 3600.
    let fixture = impersonation_fixture().await;
    let mut body = mint_body(&fixture.deployment_a);
    body["durationSeconds"] = serde_json::json!(999_999);
    let (status, _) = post_mint(&fixture, Some(&fixture.token_a), body).await;
    assert_eq!(status, StatusCode::OK);
    let captured = fixture.captured.lock().unwrap().clone().expect("impersonated");
    assert_eq!(
        captured.duration_seconds,
        Some(3600),
        "clamped down to the ceiling"
    );

    // Default when omitted -> 3600.
    let fixture = impersonation_fixture().await;
    let (status, _) = post_mint(
        &fixture,
        Some(&fixture.token_a),
        mint_body(&fixture.deployment_a),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let captured = fixture.captured.lock().unwrap().clone().expect("impersonated");
    assert_eq!(captured.duration_seconds, Some(3600), "default duration");
}

#[tokio::test]
async fn session_name_is_scoped_to_deployment_and_resource() {
    let fixture = impersonation_fixture().await;
    let mut body = mint_body(&fixture.deployment_a);
    body["resourceId"] = serde_json::json!("api");
    let (status, _) = post_mint(&fixture, Some(&fixture.token_a), body).await;
    assert_eq!(status, StatusCode::OK);

    let captured = fixture.captured.lock().unwrap().clone().expect("impersonated");
    assert_eq!(
        captured.session_name,
        Some(format!("alien-mint-{}-api", fixture.deployment_a)),
        "session name embeds deployment and resource"
    );
}

#[tokio::test]
async fn managed_path_returns_session_token_credentials() {
    let fixture = impersonation_fixture().await;
    let (status, json) = post_mint(
        &fixture,
        Some(&fixture.token_a),
        mint_body(&fixture.deployment_a),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body = {json:#}");

    let credentials = &json["clientConfig"]["credentials"];
    assert_eq!(credentials["type"], "accessKeys");
    assert!(
        credentials["session_token"].is_string(),
        "managed impersonation must return short-lived session-token creds: {credentials:#}"
    );
}

#[tokio::test]
async fn local_path_returns_static_credentials_without_session_token() {
    let fixture = resolver_fixture().await;
    let (status, json) = post_mint(
        &fixture,
        Some(&fixture.token_a),
        mint_body(&fixture.deployment_a),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body = {json:#}");

    // No target provider -> resolver fallback -> principal derived from config.
    assert_eq!(json["principal"], "aws:123456789012");
    let credentials = &json["clientConfig"]["credentials"];
    assert_eq!(credentials["type"], "accessKeys");
    assert!(
        credentials["session_token"].is_null(),
        "local static credentials carry no session token: {credentials:#}"
    );
}

#[tokio::test]
async fn unknown_binding_returns_400() {
    let fixture = impersonation_fixture().await;
    let body = serde_json::json!({
        "deploymentId": fixture.deployment_a,
        "bindingName": "does-not-exist",
    });
    let (status, json) = post_mint(&fixture, Some(&fixture.token_a), body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body = {json:#}");
}

#[tokio::test]
async fn unknown_field_is_rejected() {
    let fixture = impersonation_fixture().await;
    let body = serde_json::json!({
        "deploymentId": fixture.deployment_a,
        "bindingName": BINDING_NAME,
        "platform": "aws",
    });
    let (status, _) = post_mint(&fixture, Some(&fixture.token_a), body).await;
    assert!(
        status.is_client_error(),
        "deny_unknown_fields must reject smuggled resolver internals, got {status}"
    );
}

#[tokio::test]
async fn audit_log_never_contains_credential_material() {
    use std::io::Write;
    use tracing_subscriber::fmt::MakeWriter;

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

    let fixture = impersonation_fixture().await;
    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    let subscriber = tracing_subscriber::fmt()
        .with_writer(BufWriter(buf.clone()))
        .with_ansi(false)
        .finish();

    // `#[tokio::test]` uses a current-thread runtime, so the awaited handler
    // work stays on this thread and the thread-local default subscriber
    // captures its audit event.
    let guard = tracing::subscriber::set_default(subscriber);
    let (status, _) = post_mint(
        &fixture,
        Some(&fixture.token_a),
        mint_body(&fixture.deployment_a),
    )
    .await;
    drop(guard);
    assert_eq!(status, StatusCode::OK);

    let logs = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
    assert!(
        logs.contains("Minted deployment credentials"),
        "audit event must be emitted; captured logs: {logs}"
    );
    assert!(
        logs.contains(&fixture.deployment_a) && logs.contains("AlienManaged"),
        "audit event must carry the deployment id and principal; captured logs: {logs}"
    );
    assert!(
        !logs.contains(FAKE_SECRET),
        "audit log leaked the secret access key: {logs}"
    );
    assert!(
        !logs.contains(FAKE_SESSION_TOKEN),
        "audit log leaked the session token: {logs}"
    );
}
