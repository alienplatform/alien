//! Integration tests for remote binding resolution.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use http::header;
use sha2::{Digest, Sha256};
use tower::ServiceExt;

use alien_bindings::{providers::kv::local::LocalKv, providers::storage::local::LocalStorage};
use alien_commands::{
    dispatchers::NullCommandDispatcher,
    server::{CommandDispatcher, CommandRegistry, CommandServer},
    InMemoryCommandRegistry,
};
use alien_core::{
    AwsClientConfig, AwsCredentials, ClientConfig, Platform, Resource, ResourceLifecycle,
    ResourceStatus, RuntimeMetadata, Stack, StackResourceState, StackSettings, StackState, Storage,
    CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
};
use alien_error::AlienError;
use alien_manager::{
    auth::{Authz, Subject},
    config::ManagerConfig,
    providers::{NullTelemetryBackend, OssAuthz},
    routes::{
        registry_proxy::{CredentialCache, PullValidationCache, RegistryRoutingTable},
        AppState,
    },
    stores::sqlite::{SqliteDatabase, SqliteDeploymentStore, SqliteReleaseStore, SqliteTokenStore},
    traits::{
        AuthValidator, CreateDeploymentGroupParams, CreateImportedDeploymentParams,
        CreateReleaseParams, CreateTokenParams, CredentialResolver, DeploymentRecord,
        DeploymentStore, ReleaseStore, RemoteStorageCredentialSource, TelemetryBackend, TokenStore,
        TokenType, UpdateImportedDeploymentParams,
    },
};

const FAKE_SECRET: &str = "TOP_SECRET_ACCESS_KEY_material_must_never_be_logged";
const FAKE_SESSION_TOKEN: &str = "TOP_SECRET_SESSION_TOKEN_material_must_never_be_logged";

/// Managed short-lived credentials, modelling what STS AssumeRole returns.
fn managed_aws_config() -> ClientConfig {
    ClientConfig::Aws(Box::new(AwsClientConfig {
        account_id: "210987654321".to_string(),
        region: "us-east-1".to_string(),
        credentials: AwsCredentials::SessionCredentials {
            access_key_id: "ASIAEXAMPLE".to_string(),
            secret_access_key: FAKE_SECRET.to_string(),
            session_token: FAKE_SESSION_TOKEN.to_string(),
            expires_at: "2099-01-01T00:00:00Z".to_string(),
        },
        service_overrides: None,
    }))
}

/// Resolver spy used by remote-binding route tests. It proves the handler does
/// not touch credential resolution before authorization and stack-state checks.
struct CountingCredentialResolver {
    config: ClientConfig,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl CredentialResolver for CountingCredentialResolver {
    async fn resolve(&self, _deployment: &DeploymentRecord) -> Result<ClientConfig, AlienError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.config.clone())
    }
}

struct Fixture {
    state: AppState,
    deployment_a: String,
    token_a: String,
    token_b: String,
    admin_token: String,
    group_token: String,
}

async fn build(credential_resolver: Arc<dyn CredentialResolver>) -> Fixture {
    build_for_platform(credential_resolver, Platform::Aws).await
}

async fn build_for_platform(
    credential_resolver: Arc<dyn CredentialResolver>,
    platform: Platform,
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

    let admin_token = create_token()
        .token_store(&token_store)
        .token_type(TokenType::Admin)
        .prefix("ax_admin_")
        .call()
        .await;

    let dg = deployment_store
        .create_deployment_group(
            &Subject::system(),
            CreateDeploymentGroupParams {
                name: "bindings-group".to_string(),
                max_deployments: 100,
                setup: Default::default(),
            },
        )
        .await
        .unwrap();

    let release = release_store
        .create_release(
            &Subject::system(),
            CreateReleaseParams {
                project_id: "default".to_string(),
                stacks: HashMap::from([(platform, test_stack(platform))]),
                git_commit_sha: None,
                git_commit_ref: None,
                git_commit_message: None,
            },
        )
        .await
        .unwrap();

    let (deployment_a, token_a) = create_deployment()
        .deployment_store(&deployment_store)
        .token_store(&token_store)
        .deployment_group_id(&dg.id)
        .name("deploy-a")
        .platform(platform)
        .release_id(&release.id)
        .call()
        .await;
    let (_deployment_b, token_b) = create_deployment()
        .deployment_store(&deployment_store)
        .token_store(&token_store)
        .deployment_group_id(&dg.id)
        .name("deploy-b")
        .platform(platform)
        .release_id(&release.id)
        .call()
        .await;

    let group_token = create_token()
        .token_store(&token_store)
        .token_type(TokenType::DeploymentGroup)
        .prefix("ax_dg_")
        .deployment_group_id(dg.id.clone())
        .call()
        .await;

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
        project_store: Arc::new(alien_manager::providers::SingleWorkspaceProjects),
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
        tunnels: None,
        charts: None,
        release_channels: None,
        bundle_signing_key: None,
        bundle_sources: None,
        log_buffer: std::sync::Arc::new(alien_manager::LogBuffer::new()),
    };

    Fixture {
        state,
        deployment_a,
        token_a,
        token_b,
        admin_token,
        group_token,
    }
}

fn test_stack(platform: Platform) -> Stack {
    Stack::new("bindings-test".to_string())
        .platforms(vec![platform])
        .add_with_remote_access(
            Storage {
                id: "files".to_string(),
                public_read: false,
                versioning: false,
                lifecycle_rules: Vec::new(),
                cors_allowed_origins: Vec::new(),
                encryption_key: None,
            },
            ResourceLifecycle::Frozen,
        )
        .build()
}

/// Create a deployment pinned to the test release and a deployment token scoped to it. Returns
/// `(deployment_id, raw_token)`.
#[bon::builder]
async fn create_deployment(
    deployment_store: &Arc<dyn DeploymentStore>,
    token_store: &Arc<dyn TokenStore>,
    deployment_group_id: &str,
    name: &str,
    platform: Platform,
    release_id: &str,
) -> (String, String) {
    let record = deployment_store
        .create_with_state(
            &Subject::system(),
            CreateImportedDeploymentParams {
                deployment_protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                name: name.to_string(),
                deployment_group_id: deployment_group_id.to_string(),
                platform,
                base_platform: None,
                stack_settings: StackSettings::default(),
                stack_state: StackState::new(platform),
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

    let token = create_token()
        .token_store(token_store)
        .token_type(TokenType::Deployment)
        .prefix("ax_deploy_")
        .deployment_group_id(deployment_group_id.to_string())
        .deployment_id(record.id.clone())
        .call()
        .await;

    (record.id, token)
}

#[bon::builder]
async fn create_token(
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

#[path = "bindings_resolve/bindings_resolve.rs"]
mod bindings_resolve;
