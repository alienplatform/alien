//! Who may run the air-gapped exchange for a deployment.
//!
//! The site's own deployment token reports state and telemetry. Any other
//! token, including the deployment group's (which a customer's admin holds
//! from onboarding), must not be able to report state or push logs for it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use http::header;
use sha2::{Digest, Sha256};
use tower::ServiceExt;

use alien_bindings::{providers::kv::local::LocalKv, providers::storage::local::LocalStorage};
use alien_commands::dispatchers::NullCommandDispatcher;
use alien_commands::server::{CommandDispatcher, CommandRegistry, CommandServer};
use alien_commands::InMemoryCommandRegistry;
use alien_core::{
    ClientConfig, Platform, ResourceLifecycle, RuntimeMetadata, Stack, StackSettings, StackState,
    Storage, CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
};
use alien_error::AlienError;
use alien_manager::auth::Subject;
use alien_manager::config::ManagerConfig;
use alien_manager::providers::OssAuthz;
use alien_manager::routes::registry_proxy::{
    CredentialCache, PullValidationCache, RegistryRoutingTable,
};
use alien_manager::routes::AppState;
use alien_manager::stores::sqlite::{
    SqliteDatabase, SqliteDeploymentStore, SqliteReleaseStore, SqliteTokenStore,
};
use alien_manager::traits::{
    CreateDeploymentGroupParams, CreateImportedDeploymentParams, CreateReleaseParams,
    CreateTokenParams, CredentialResolver, DeploymentRecord, DeploymentStore, ReleaseStore,
    TelemetryBackend, TelemetryCaller, TelemetrySignal, TokenStore, TokenType,
};

/// Records every batch the manager passes on.
#[derive(Default)]
struct CapturedTelemetry(Mutex<Vec<(TelemetrySignal, Option<String>)>>);

#[async_trait]
impl TelemetryBackend for CapturedTelemetry {
    async fn ingest(
        &self,
        signal: TelemetrySignal,
        caller: &TelemetryCaller,
        _data: bytes::Bytes,
    ) -> Result<(), AlienError> {
        self.0
            .lock()
            .unwrap()
            .push((signal, caller.deployment_id.clone()));
        Ok(())
    }
}

struct NoCredentials;

#[async_trait]
impl CredentialResolver for NoCredentials {
    async fn resolve(&self, _deployment: &DeploymentRecord) -> Result<ClientConfig, AlienError> {
        unreachable!("the air-gapped exchange resolves no credentials")
    }
}

struct Fixture {
    state: AppState,
    telemetry: Arc<CapturedTelemetry>,
    deployment: String,
    /// The site's own token.
    site_token: String,
    /// A token for another deployment in the same group.
    other_token: String,
    /// The deployment group's token, as onboarding hands it out.
    group_token: String,
    /// The manager's admin token: it may update the deployment, but it is
    /// not the site.
    admin_token: String,
}

async fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("manager.db");
    let db = Arc::new(
        SqliteDatabase::new(&db_path.to_string_lossy())
            .await
            .unwrap(),
    );
    let deployment_store: Arc<dyn DeploymentStore> =
        Arc::new(SqliteDeploymentStore::new(db.clone()));
    let release_store: Arc<dyn ReleaseStore> = Arc::new(SqliteReleaseStore::new(db.clone()));
    let token_store: Arc<dyn TokenStore> = Arc::new(SqliteTokenStore::new(db.clone()));

    let group = deployment_store
        .create_deployment_group(
            &Subject::system(),
            CreateDeploymentGroupParams {
                name: "sites".to_string(),
                max_deployments: 10,
                setup: Default::default(),
            },
        )
        .await
        .unwrap();
    let stack = Stack::new("files".to_string())
        .platforms(vec![Platform::Kubernetes])
        .add(
            Storage::new("bucket".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let release = release_store
        .create_release(
            &Subject::system(),
            CreateReleaseParams {
                project_id: "default".to_string(),
                stacks: HashMap::from([(Platform::Kubernetes, stack)]),
                git_commit_sha: None,
                git_commit_ref: None,
                git_commit_message: None,
            },
        )
        .await
        .unwrap();

    let mut deployments = Vec::new();
    for name in ["site", "other"] {
        let record = deployment_store
            .create_with_state(
                &Subject::system(),
                CreateImportedDeploymentParams {
                    deployment_protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                    name: name.to_string(),
                    deployment_group_id: group.id.clone(),
                    platform: Platform::Kubernetes,
                    base_platform: None,
                    stack_settings: StackSettings::default(),
                    stack_state: StackState::new(Platform::Kubernetes),
                    environment_info: None,
                    runtime_metadata: RuntimeMetadata::default(),
                    status: "running".to_string(),
                    current_release_id: Some(release.id.clone()),
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
            &token_store,
            TokenType::Deployment,
            "ax_deploy_",
            Some(group.id.clone()),
            Some(record.id.clone()),
        )
        .await;
        deployments.push((record.id, token));
    }
    let group_token = mint_token(
        &token_store,
        TokenType::DeploymentGroup,
        "ax_dg_",
        Some(group.id.clone()),
        None,
    )
    .await;
    let admin_token = mint_token(&token_store, TokenType::Admin, "ax_admin_", None, None).await;

    let kv: Arc<dyn alien_bindings::traits::Kv> =
        Arc::new(LocalKv::new(tmp.path().join("kv")).await.unwrap());
    let command_storage: Arc<dyn alien_bindings::traits::Storage> = Arc::new(
        LocalStorage::new(tmp.path().join("storage").to_string_lossy().to_string()).unwrap(),
    );
    let command_dispatcher: Arc<dyn CommandDispatcher> = Arc::new(NullCommandDispatcher);
    let command_registry: Arc<dyn CommandRegistry> = Arc::new(InMemoryCommandRegistry::default());
    let telemetry = Arc::new(CapturedTelemetry::default());
    std::mem::forget(tmp);

    let state = AppState {
        deployment_store,
        release_store,
        token_store: token_store.clone(),
        auth_validator: Arc::new(
            alien_manager::providers::token_db_validator::TokenDbValidator::new(token_store),
        ),
        authz: Arc::new(OssAuthz),
        telemetry_backend: telemetry.clone(),
        credential_resolver: Arc::new(NoCredentials),
        command_server: Arc::new(CommandServer::new(
            kv.clone(),
            command_storage,
            command_dispatcher,
            command_registry,
            "http://localhost:0/v1".to_string(),
            b"test-signing-key".to_vec(),
        )),
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
        log_buffer: Arc::new(alien_manager::LogBuffer::new()),
    };

    let (deployment, site_token) = deployments.remove(0);
    let (_, other_token) = deployments.remove(0);
    Fixture {
        state,
        telemetry,
        deployment,
        site_token,
        other_token,
        group_token,
        admin_token,
    }
}

async fn mint_token(
    token_store: &Arc<dyn TokenStore>,
    token_type: TokenType,
    prefix: &str,
    deployment_group_id: Option<String>,
    deployment_id: Option<String>,
) -> String {
    let raw = format!("{prefix}{}", uuid::Uuid::new_v4().simple());
    token_store
        .create_token(CreateTokenParams {
            token_type,
            key_prefix: raw[..12].to_string(),
            key_hash: format!("{:x}", Sha256::digest(raw.as_bytes())),
            deployment_group_id,
            deployment_id,
        })
        .await
        .unwrap();
    raw
}

/// A report as `alien-deploy sync` sends it: the state, with one log batch
/// unless `state_only`.
async fn report(
    fixture: &Fixture,
    token: &str,
    state_only: bool,
) -> (StatusCode, serde_json::Value) {
    let telemetry = if state_only {
        serde_json::json!([])
    } else {
        serde_json::json!([{
            "signal": "logs",
            "data": base64::engine::general_purpose::STANDARD.encode(b"otlp"),
        }])
    };
    let body = serde_json::json!({
        "state": {
            "status": "running",
            "platform": "kubernetes",
            "protocolVersion": CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        },
        "telemetry": telemetry,
    });
    let request = Request::builder()
        .method("POST")
        .uri(format!(
            "/v1/deployments/{}/status-report",
            fixture.deployment
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = alien_manager::routes::airgap::router()
        .with_state(fixture.state.clone())
        .oneshot(request)
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

#[tokio::test]
async fn the_sites_own_token_reports_state_and_telemetry() {
    let fixture = fixture().await;

    let (status, body) = report(&fixture, &fixture.site_token, false).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["telemetryAccepted"], 1);
    assert_eq!(
        *fixture.telemetry.0.lock().unwrap(),
        vec![(TelemetrySignal::Logs, Some(fixture.deployment.clone()))],
        "the batch is attributed to the site's deployment"
    );
}

#[tokio::test]
async fn other_tokens_cannot_report_for_the_site() {
    let fixture = fixture().await;

    for (who, token) in [
        ("the deployment group's token", &fixture.group_token),
        ("another deployment's token", &fixture.other_token),
        ("the admin token", &fixture.admin_token),
    ] {
        for state_only in [false, true] {
            let (status, body) = report(&fixture, token, state_only).await;
            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "{who} (state only: {state_only}): {body}"
            );
        }
    }
    assert!(
        fixture.telemetry.0.lock().unwrap().is_empty(),
        "nothing reaches the telemetry backend"
    );
}
