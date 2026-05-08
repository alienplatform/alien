//! Integration tests for `POST /v1/stack/import`.
//!
//! Drives the manager's stack-import handler through a manually-assembled
//! [`alien_manager::routes::AppState`] and `tower::ServiceExt::oneshot` so the
//! tests can exercise auth, idempotency, and importer dispatch without
//! spinning up an HTTP server.
//!
//! Each test rebuilds a fresh in-memory-style sqlite database, registers a
//! deployment group, mints a deployment-group token, and seeds a release whose
//! stack matches the resources we then import.

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use http::header;
use sha2::{Digest, Sha256};
use tower::ServiceExt;

use alien_bindings::providers::{kv::local::LocalKv, storage::local::LocalStorage};
use alien_commands::dispatchers::NullCommandDispatcher;
use alien_commands::server::{CommandDispatcher, CommandRegistry, CommandServer};
use alien_commands::InMemoryCommandRegistry;
use alien_core::import::{
    ImportSourceKind, ImportedResource, StackImportRequest, StackImportResponse,
};
use alien_core::{
    AwsManagementConfig, AwsStorageImportData, ManagementConfig, Platform, ResourceLifecycle,
    ResourceStatus, Stack, StackSettings, Storage,
};
use alien_manager::auth::Authz;
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
    AuthValidator, CreateDeploymentGroupParams, CreateDeploymentParams, CreateReleaseParams,
    CreateTokenParams, CredentialResolver, DeploymentStore, ReleaseStore, TelemetryBackend,
    TokenStore, TokenType,
};

// ---------------------------------------------------------------------------
// AppState assembly
// ---------------------------------------------------------------------------

/// Holds everything a test fixture pre-creates so the assertion phase can
/// inspect the database without going through HTTP again.
struct Fixture {
    state: AppState,
    deployment_store: Arc<dyn DeploymentStore>,
    deployment_group_id: String,
    /// Raw bearer token to send in the `Authorization` header.
    dg_token: String,
    /// Workspace-admin token (for negative auth tests).
    admin_token: String,
    release_id: Option<String>,
}

/// Build a fresh fixture: temp DB, seeded release matching the import payload,
/// and a deployment-group token for the caller.
async fn make_fixture(seeded_stack: Option<Stack>) -> Fixture {
    // Use a temp directory because turso's WAL mode needs a real file path.
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("test.db");
    // Leak the tempdir so it survives until the test process exits.
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

    // --- Tokens ---------------------------------------------------------
    let admin_token = mint_token(&token_store, TokenType::Admin, "ax_admin_", None, None).await;

    // Deployment group + DG token.
    let dg = deployment_store
        .create_deployment_group(
            &alien_manager::auth::Subject::system(),
            CreateDeploymentGroupParams {
                name: "imported-group".to_string(),
                max_deployments: 100,
            },
        )
        .await
        .unwrap();

    let dg_token = mint_token(
        &token_store,
        TokenType::DeploymentGroup,
        "ax_dg_",
        Some(dg.id.clone()),
        None,
    )
    .await;

    // --- Release --------------------------------------------------------
    let release_id = if let Some(stack) = seeded_stack {
        Some(
            release_store
                .create_release(
                    &alien_manager::auth::Subject::system(),
                    CreateReleaseParams {
                        project_id: "default".to_string(),
                        stacks: HashMap::from([(Platform::Aws, stack)]),
                        git_commit_sha: None,
                        git_commit_ref: None,
                        git_commit_message: None,
                    },
                )
                .await
                .unwrap()
                .id,
        )
    } else {
        None
    };

    // --- AppState plumbing ---------------------------------------------
    let auth_validator: Arc<dyn AuthValidator> = Arc::new(
        alien_manager::providers::token_db_validator::TokenDbValidator::new(token_store.clone()),
    );
    let authz: Arc<dyn Authz> = Arc::new(OssAuthz);
    let telemetry_backend: Arc<dyn TelemetryBackend> = Arc::new(NullTelemetryBackend);
    let credential_resolver: Arc<dyn CredentialResolver> = Arc::new(
        alien_manager::providers::local_credentials::LocalCredentialResolver::new(
            db_path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| db_path.clone()),
        ),
    );

    // Command server stubs (not touched by the import path, but AppState
    // requires them).
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
        release_store: release_store.clone(),
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
        registry_routing_table: Arc::new(RegistryRoutingTable::new(vec![])),
        import_registry: Arc::new(alien_infra::ImporterRegistry::built_in()),
    };

    Fixture {
        state,
        deployment_store,
        deployment_group_id: dg.id,
        dg_token,
        admin_token,
        release_id,
    }
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
// Request helpers
// ---------------------------------------------------------------------------

/// Build a `StackImportRequest` body for an AWS S3 import. The stack must
/// register a Storage resource at the same `id` (`assets` by default).
fn aws_s3_import_request(
    deployment_name: &str,
    region: &str,
    resource_id: &str,
    bucket: &str,
) -> StackImportRequest {
    StackImportRequest {
        deployment_group_token: "ignored".to_string(),
        deployment_name: deployment_name.to_string(),
        stack_prefix: deployment_name.to_string(),
        source_kind: Some(ImportSourceKind::CloudFormation),
        release_id: None,
        platform: Platform::Aws,
        region: region.to_string(),
        stack_settings: StackSettings::default(),
        management_config: ManagementConfig::Aws(AwsManagementConfig {
            managing_role_arn: "arn:aws:iam::123456789012:role/AlienManager".to_string(),
        }),
        resources: vec![ImportedResource {
            id: resource_id.to_string(),
            resource_type: alien_core::Storage::RESOURCE_TYPE.into(),
            import_data: serde_json::to_value(AwsStorageImportData {
                bucket_name: bucket.to_string(),
                bucket_arn: format!("arn:aws:s3:::{}", bucket),
            })
            .unwrap(),
        }],
    }
}

/// Build a Stack with one Storage resource matching `aws_s3_import_request`.
fn stack_with_storage(resource_id: &str) -> Stack {
    Stack::new("imported".to_string())
        .add(
            Storage::new(resource_id.to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build()
}

async fn post_import(
    fixture: &Fixture,
    bearer: Option<&str>,
    body: &StackImportRequest,
) -> (StatusCode, serde_json::Value) {
    let router = alien_manager::routes::stack::router().with_state(fixture.state.clone());

    let mut req = Request::builder()
        .method("POST")
        .uri("/v1/stack/import")
        .header(header::CONTENT_TYPE, "application/json");

    if let Some(token) = bearer {
        req = req.header(header::AUTHORIZATION, format!("Bearer {}", token));
    }

    let request = req
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap();

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn happy_path_creates_imported_deployment() {
    let fixture = make_fixture(Some(stack_with_storage("assets"))).await;
    let body = aws_s3_import_request("acme-prod", "us-east-1", "assets", "acme-imports");

    let (status, json) = post_import(&fixture, Some(&fixture.dg_token), &body).await;
    assert_eq!(status, StatusCode::CREATED, "body = {:#}", json);

    let parsed: StackImportResponse = serde_json::from_value(json).unwrap();
    assert!(parsed.deployment_id.starts_with("dep_"));
    let resources = &parsed.stack_state.resources;
    assert_eq!(resources.len(), 1);
    let imported = resources.get("assets").expect("resource id round-trips");
    assert_eq!(
        imported.status,
        ResourceStatus::Running,
        "storage imports are already at their controller terminal state"
    );

    // Persistence-side check: the SQLite store has the row with
    // status=provisioning, the caller-supplied name, and stack_state
    // populated.
    let persisted = fixture
        .deployment_store
        .get_deployment(
            &alien_manager::auth::Subject::system(),
            &parsed.deployment_id,
        )
        .await
        .unwrap()
        .expect("deployment must persist");
    assert_eq!(persisted.status, "provisioning");
    assert_eq!(
        persisted.import_source,
        Some(ImportSourceKind::CloudFormation)
    );
    assert_eq!(
        persisted.name, "acme-prod",
        "deployment name must round-trip from the request body"
    );
    assert!(
        persisted.stack_state.is_some(),
        "stack_state must round-trip through SQLite"
    );
    let runtime_metadata = persisted
        .runtime_metadata
        .as_ref()
        .expect("runtime_metadata must be persisted");
    let prepared_stack = runtime_metadata
        .prepared_stack
        .as_ref()
        .expect("prepared_stack must be persisted for imported provisioning");
    assert_eq!(prepared_stack.id, "imported");
    assert!(
        prepared_stack.resources.contains_key("assets"),
        "prepared_stack must come from the release stack used for import"
    );
    assert_eq!(persisted.deployment_group_id, fixture.deployment_group_id);
    assert!(
        persisted.current_release_id.is_some(),
        "imported deployment must pin the release that produced it"
    );
}

#[tokio::test]
async fn re_import_replaces_stack_state() {
    let fixture = make_fixture(Some(stack_with_storage("assets"))).await;
    let body = aws_s3_import_request("acme-prod", "us-east-1", "assets", "acme-imports");

    let (s1, j1) = post_import(&fixture, Some(&fixture.dg_token), &body).await;
    assert_eq!(s1, StatusCode::CREATED);
    let first: StackImportResponse = serde_json::from_value(j1).unwrap();

    let mut body = body;
    body.resources[0].import_data = serde_json::to_value(AwsStorageImportData {
        bucket_name: "acme-imports-v2".to_string(),
        bucket_arn: "arn:aws:s3:::acme-imports-v2".to_string(),
    })
    .unwrap();
    let (s2, json) = post_import(&fixture, Some(&fixture.dg_token), &body).await;
    assert_eq!(s2, StatusCode::OK, "body = {:#}", json);
    let second: StackImportResponse = serde_json::from_value(json).unwrap();
    assert_eq!(second.deployment_id, first.deployment_id);

    let persisted = fixture
        .deployment_store
        .get_deployment(
            &alien_manager::auth::Subject::system(),
            &first.deployment_id,
        )
        .await
        .unwrap()
        .expect("deployment must persist");
    let state = persisted.stack_state.expect("stack state is updated");
    let outputs = state
        .resources
        .get("assets")
        .and_then(|r| r.outputs.as_ref())
        .expect("updated resource has outputs");
    let outputs_json = serde_json::to_value(outputs).unwrap();
    assert!(
        outputs_json.to_string().contains("acme-imports-v2"),
        "updated import data should replace the persisted stack state: {outputs_json:#}"
    );
}

#[tokio::test]
async fn explicit_release_id_is_persisted() {
    let fixture = make_fixture(Some(stack_with_storage("assets"))).await;
    let release_id = fixture.release_id.clone().expect("fixture has a release");
    let mut body = aws_s3_import_request("acme-prod", "us-east-1", "assets", "acme-imports");
    body.release_id = Some(release_id.clone());

    let (status, json) = post_import(&fixture, Some(&fixture.dg_token), &body).await;
    assert_eq!(status, StatusCode::CREATED, "body = {:#}", json);

    let parsed: StackImportResponse = serde_json::from_value(json).unwrap();
    let persisted = fixture
        .deployment_store
        .get_deployment(
            &alien_manager::auth::Subject::system(),
            &parsed.deployment_id,
        )
        .await
        .unwrap()
        .expect("deployment must persist");
    assert_eq!(persisted.current_release_id, Some(release_id));
}

#[tokio::test]
async fn native_deployment_blocks_imported_name() {
    let fixture = make_fixture(Some(stack_with_storage("assets"))).await;
    fixture
        .deployment_store
        .create_deployment(
            &alien_manager::auth::Subject::system(),
            CreateDeploymentParams {
                name: "acme-prod".to_string(),
                deployment_group_id: fixture.deployment_group_id.clone(),
                platform: Platform::Aws,
                stack_settings: StackSettings::default(),
                environment_variables: None,
                deployment_token: None,
            },
        )
        .await
        .unwrap();

    let body = aws_s3_import_request("acme-prod", "us-east-1", "assets", "acme-imports");
    let (status, json) = post_import(&fixture, Some(&fixture.dg_token), &body).await;
    assert_eq!(status, StatusCode::CONFLICT, "body = {:#}", json);
}

#[tokio::test]
async fn different_names_create_independent_deployments() {
    let fixture = make_fixture(Some(stack_with_storage("assets"))).await;
    let body_a = aws_s3_import_request("acme-prod", "us-east-1", "assets", "acme-imports");
    let body_b = aws_s3_import_request("acme-staging", "us-east-1", "assets", "acme-imports");

    let (sa, ja) = post_import(&fixture, Some(&fixture.dg_token), &body_a).await;
    assert_eq!(sa, StatusCode::CREATED);
    let ra: StackImportResponse = serde_json::from_value(ja).unwrap();

    let (sb, jb) = post_import(&fixture, Some(&fixture.dg_token), &body_b).await;
    assert_eq!(sb, StatusCode::CREATED);
    let rb: StackImportResponse = serde_json::from_value(jb).unwrap();

    assert_ne!(
        ra.deployment_id, rb.deployment_id,
        "different deployment names must produce different deployments"
    );
}

#[tokio::test]
async fn missing_bearer_returns_401() {
    let fixture = make_fixture(Some(stack_with_storage("assets"))).await;
    let body = aws_s3_import_request("acme-prod", "us-east-1", "assets", "acme-imports");

    let (status, _) = post_import(&fixture, None, &body).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admin_token_is_rejected_with_403() {
    let fixture = make_fixture(Some(stack_with_storage("assets"))).await;
    let body = aws_s3_import_request("acme-prod", "us-east-1", "assets", "acme-imports");

    let (status, json) = post_import(&fixture, Some(&fixture.admin_token), &body).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "workspace-admin tokens lack the deployment-group-scoped semantic that \
         the import endpoint requires; got body = {:#}",
        json
    );
}

#[tokio::test]
async fn missing_release_returns_400() {
    // No stack seeded -> get_latest_release returns None.
    let fixture = make_fixture(None).await;
    let body = aws_s3_import_request("acme-prod", "us-east-1", "assets", "acme-imports");

    let (status, json) = post_import(&fixture, Some(&fixture.dg_token), &body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body = {:#}", json);
}

#[tokio::test]
async fn unknown_resource_id_returns_400() {
    // Stack has `assets`, but the request claims `not-in-stack`.
    let fixture = make_fixture(Some(stack_with_storage("assets"))).await;
    let body = aws_s3_import_request("acme-prod", "us-east-1", "not-in-stack", "acme-imports");

    let (status, json) = post_import(&fixture, Some(&fixture.dg_token), &body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body = {:#}", json);
}

#[tokio::test]
async fn empty_resources_returns_400() {
    let fixture = make_fixture(Some(stack_with_storage("assets"))).await;
    let mut body = aws_s3_import_request("acme-prod", "us-east-1", "assets", "acme-imports");
    body.resources.clear();

    let (status, _) = post_import(&fixture, Some(&fixture.dg_token), &body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn missing_deployment_name_returns_400() {
    let fixture = make_fixture(Some(stack_with_storage("assets"))).await;
    let mut body = aws_s3_import_request("", "us-east-1", "assets", "acme-imports");
    body.deployment_name.clear();

    let (status, json) = post_import(&fixture, Some(&fixture.dg_token), &body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body = {:#}", json);
}
