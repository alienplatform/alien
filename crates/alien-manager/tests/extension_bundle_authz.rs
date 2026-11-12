//! Who the OSS manager's bundle route refuses.
#![allow(dead_code, unused_imports)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use alien_bindings::{providers::kv::local::LocalKv, providers::storage::local::LocalStorage};
use alien_commands::dispatchers::NullCommandDispatcher;
use alien_commands::server::CommandServer;
use alien_commands::InMemoryCommandRegistry;
use alien_core::{BinaryTarget, ClientConfig, DeploymentState};
use alien_error::AlienError;
use alien_infra::extension_bundle::testing::TestBundle;
use alien_manager::auth::{Role, Scope, Subject, SubjectKind};
use alien_manager::config::ManagerConfig;
use alien_manager::extension_bundles::ExtensionBundles;
use alien_manager::providers::{OssAuthz, StateDirExtensionBundles};
use alien_manager::routes::registry_proxy::{
    CredentialCache, PullValidationCache, RegistryRoutingTable,
};
use alien_manager::routes::AppState;
use alien_manager::stores::sqlite::{
    SqliteDatabase, SqliteDeploymentStore, SqliteReleaseStore, SqliteTokenStore,
};
use alien_manager::traits::{
    AuthValidator, BundleObject, CredentialResolver, DeploymentRecord, ExtensionBundleStore,
};
use async_trait::async_trait;
use axum::body::{to_bytes, Body};
use axum::http::{Method, Request, StatusCode};
use http::header;
use sha2::{Digest, Sha256};
use tower::ServiceExt;

struct RoleValidator(HashMap<String, Subject>);

#[async_trait]
impl AuthValidator for RoleValidator {
    async fn validate(&self, headers: &http::HeaderMap) -> Result<Option<Subject>, AlienError> {
        let token = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "));
        Ok(token.and_then(|token| self.0.get(token).cloned()))
    }
}

struct NoCredentials;

#[async_trait]
impl CredentialResolver for NoCredentials {
    async fn management_external_id(
        &self,
        _: &DeploymentRecord,
    ) -> Result<Option<alien_core::ManagementExternalId>, AlienError> {
        Ok(None)
    }

    async fn resolve(&self, _deployment: &DeploymentRecord) -> Result<ClientConfig, AlienError> {
        unreachable!("these routes resolve no credentials")
    }
}

fn subject(scope: Scope, role: Role) -> Subject {
    Subject {
        kind: SubjectKind::ServiceAccount {
            id: "test".to_string(),
        },
        workspace_id: "default".to_string(),
        scope,
        role,
        bearer_token: String::new(),
    }
}

const DEVELOPER: &str = "developer";
const OPERATOR: &str = "operator";

async fn state(store: Arc<dyn ExtensionBundleStore>) -> (AppState, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let db = Arc::new(
        SqliteDatabase::new(&tmp.path().join("manager.db").to_string_lossy())
            .await
            .unwrap(),
    );
    let kv: Arc<dyn alien_bindings::traits::Kv> =
        Arc::new(LocalKv::new(tmp.path().join("kv")).await.unwrap());
    let command_storage: Arc<dyn alien_bindings::traits::Storage> = Arc::new(
        LocalStorage::new(tmp.path().join("storage").to_string_lossy().to_string()).unwrap(),
    );
    let subjects = HashMap::from([
        (
            DEVELOPER.to_string(),
            subject(
                Scope::Project {
                    project_id: "default".to_string(),
                },
                Role::ProjectDeveloper,
            ),
        ),
        (
            OPERATOR.to_string(),
            subject(
                Scope::Deployment {
                    project_id: "default".to_string(),
                    deployment_id: "dep_1".to_string(),
                },
                Role::DeploymentManager,
            ),
        ),
        (
            "commands".to_string(),
            subject(
                Scope::Commands {
                    project_id: "default".to_string(),
                    deployment_id: "dep_1".to_string(),
                    capability: alien_manager::auth::CommandCapability::Send,
                },
                Role::CommandCapability,
            ),
        ),
        (
            "command-role".to_string(),
            subject(
                Scope::Deployment {
                    project_id: "default".to_string(),
                    deployment_id: "dep_1".to_string(),
                },
                Role::CommandCapability,
            ),
        ),
        (
            "tunnel".to_string(),
            subject(
                Scope::Project {
                    project_id: "default".to_string(),
                },
                Role::TunnelCaller,
            ),
        ),
        (
            "provisioner".to_string(),
            subject(
                Scope::Project {
                    project_id: "default".to_string(),
                },
                Role::ImageRepositoryProvisioner,
            ),
        ),
        (
            "pusher".to_string(),
            subject(
                Scope::Project {
                    project_id: "default".to_string(),
                },
                Role::SandboxImagePusher,
            ),
        ),
        (
            "telemetry-role".to_string(),
            subject(
                Scope::Project {
                    project_id: "default".to_string(),
                },
                Role::TelemetryCapability,
            ),
        ),
    ]);
    let state = AppState {
        deployment_store: Arc::new(SqliteDeploymentStore::new(db.clone())),
        release_store: Arc::new(SqliteReleaseStore::new(db.clone())),
        project_store: Arc::new(alien_manager::providers::SingleWorkspaceProjects),
        token_store: Arc::new(SqliteTokenStore::new(db)),
        auth_validator: Arc::new(RoleValidator(subjects)),
        authz: Arc::new(OssAuthz),
        telemetry_backend: Arc::new(alien_manager::providers::NullTelemetryBackend),
        credential_resolver: Arc::new(NoCredentials),
        command_server: Arc::new(CommandServer::new(
            kv.clone(),
            command_storage,
            Arc::new(NullCommandDispatcher),
            Arc::new(InMemoryCommandRegistry::default()),
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
        registry_routing_table: Arc::new(RegistryRoutingTable::new(vec![]).unwrap()),
        host_registrations: alien_infra::HostRegistrations::NONE,
        tunnels: None,
        charts: None,
        release_channels: None,
        bundle_signing_key: None,
        bundle_sources: None,
        extension_bundles: Some(Arc::new(ExtensionBundles::new(store))),
        values: None,
        value_access: None,
        verify_limiter: Arc::new(alien_manager::values::VerifyLimiter::default()),
        log_buffer: Arc::new(alien_manager::LogBuffer::new()),
    };
    (state, tmp)
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

async fn call(
    state: &AppState,
    method: Method,
    uri: String,
    token: &str,
    body: Vec<u8>,
) -> (StatusCode, Vec<u8>) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(body))
        .unwrap();
    let response = alien_manager::routes::extension_bundles::router()
        .with_state(state.clone())
        .oneshot(request)
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, body.to_vec())
}

async fn upload(state: &AppState, object: &str, bytes: &[u8]) -> (StatusCode, String) {
    let (status, body) = call(
        state,
        Method::POST,
        format!(
            "/v1/extensions/bundles/{}?extension=acme&object={object}",
            digest(bytes)
        ),
        DEVELOPER,
        bytes.to_vec(),
    )
    .await;
    (status, String::from_utf8_lossy(&body).to_string())
}

fn state_dir_store(dir: &std::path::Path) -> Arc<dyn ExtensionBundleStore> {
    Arc::new(StateDirExtensionBundles::new(dir))
}

/// Every capability credential is refused on GET and HEAD of a stored object, and gets no bytes.
#[tokio::test]
async fn capability_tokens_and_anonymous_callers_get_no_bundle_bytes() {
    let bundle = TestBundle::new("acme", "1.0.0", &[]);
    let dir = tempfile::tempdir().unwrap();
    let (state, _tmp) = state(state_dir_store(dir.path())).await;
    assert_eq!(
        upload(&state, "core", &bundle.zip).await.0,
        StatusCode::CREATED
    );
    let uri = format!("/v1/extensions/bundles/{}", bundle.manifest.digest);

    let (status, body) = call(&state, Method::GET, uri.clone(), OPERATOR, vec![]).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the deployment's own token reads it"
    );
    assert_eq!(body, bundle.zip);

    for token in [
        "commands",
        "command-role",
        "tunnel",
        "provisioner",
        "pusher",
        "telemetry-role",
        "no-such-token",
    ] {
        for method in [Method::GET, Method::HEAD] {
            let (status, body) = call(&state, method.clone(), uri.clone(), token, vec![]).await;
            assert!(
                status == StatusCode::FORBIDDEN || status == StatusCode::UNAUTHORIZED,
                "{token} {method}: {status}"
            );
            assert!(
                !body.windows(4).any(|w| w == b"PK\x03\x04"),
                "{token} {method} got zip bytes"
            );
        }
    }
}
