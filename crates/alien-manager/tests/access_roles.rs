//! Which roles may hold a deployment's tunnel, and which may pull charts and
//! the Operator image.
//!
//! The standalone manager only issues deployment-scoped tokens to Operators,
//! but embedders' validators issue other deployment-scoped roles (viewers,
//! telemetry writers, binding resolvers) and project-scoped capability roles.
//! This test plugs in such a validator so every role reaches the real routes.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use http::header;
use tower::ServiceExt;

use alien_bindings::{providers::kv::local::LocalKv, providers::storage::local::LocalStorage};
use alien_commands::dispatchers::NullCommandDispatcher;
use alien_commands::server::{CommandDispatcher, CommandRegistry, CommandServer};
use alien_commands::InMemoryCommandRegistry;
use alien_core::{
    ClientConfig, Platform, RuntimeMetadata, StackSettings, StackState,
    CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
};
use alien_error::AlienError;
use alien_manager::auth::{Role, Scope, Subject, SubjectKind};
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
    AuthValidator, CreateDeploymentGroupParams, CreateImportedDeploymentParams,
    CredentialResolver, DeploymentRecord, DeploymentStore, ReleaseStore, TokenStore,
};
use alien_tunnel::manager::TunnelRegistry;

/// Hands out the subject registered for each bearer token, the way an
/// embedder's validator does.
struct RoleValidator(HashMap<String, Subject>);

#[async_trait]
impl AuthValidator for RoleValidator {
    async fn validate(&self, headers: &http::HeaderMap) -> Result<Option<Subject>, AlienError> {
        let token = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "));
        Ok(token.map(|token| {
            self.0
                .get(token)
                .cloned()
                .unwrap_or_else(|| panic!("no subject registered for token '{token}'"))
        }))
    }
}

struct NoCredentials;

#[async_trait]
impl CredentialResolver for NoCredentials {
    async fn resolve(&self, _deployment: &DeploymentRecord) -> Result<ClientConfig, AlienError> {
        unreachable!("these routes resolve no credentials")
    }
}

fn subject(scope: Scope, role: Role) -> Subject {
    Subject {
        kind: SubjectKind::ServiceAccount {
            id: "embedder".to_string(),
        },
        workspace_id: "default".to_string(),
        scope,
        role,
        bearer_token: String::new(),
    }
}

/// The token name for a role, so each role gets its own bearer token.
fn token(role: Role) -> String {
    format!("{role:?}")
}

/// A manager with tunnels on, one deployment, and a token for every role
/// below: deployment-scoped for the tunnel roles, project-scoped for the
/// pull roles.
async fn state() -> AppState {
    let tmp = tempfile::tempdir().unwrap();
    let db = Arc::new(
        SqliteDatabase::new(&tmp.path().join("manager.db").to_string_lossy())
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
                name: "customers".to_string(),
                max_deployments: 10,
                setup: Default::default(),
            },
        )
        .await
        .unwrap();
    let deployment = deployment_store
        .create_with_state(
            &Subject::system(),
            CreateImportedDeploymentParams {
                deployment_protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                name: "customer-1".to_string(),
                deployment_group_id: group.id.clone(),
                platform: Platform::Kubernetes,
                base_platform: None,
                stack_settings: StackSettings::default(),
                stack_state: StackState::new(Platform::Kubernetes),
                environment_info: None,
                runtime_metadata: RuntimeMetadata::default(),
                status: "running".to_string(),
                current_release_id: None,
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

    let deployment_scope = Scope::Deployment {
        project_id: deployment.project_id.clone(),
        deployment_id: deployment.id.clone(),
    };
    let project_scope = Scope::Project {
        project_id: deployment.project_id.clone(),
    };
    let mut subjects = HashMap::new();
    for role in [
        Role::DeploymentManager,
        Role::DeploymentViewer,
        Role::DeploymentTelemetryWriter,
        Role::RemoteBindingResolver,
    ] {
        subjects.insert(token(role), subject(deployment_scope.clone(), role));
    }
    for role in [
        Role::ProjectDeveloper,
        Role::ImageRepositoryProvisioner,
        Role::SandboxImagePusher,
    ] {
        subjects.insert(token(role), subject(project_scope.clone(), role));
    }

    let kv: Arc<dyn alien_bindings::traits::Kv> =
        Arc::new(LocalKv::new(tmp.path().join("kv")).await.unwrap());
    let command_storage: Arc<dyn alien_bindings::traits::Storage> = Arc::new(
        LocalStorage::new(tmp.path().join("storage").to_string_lossy().to_string()).unwrap(),
    );
    let command_dispatcher: Arc<dyn CommandDispatcher> = Arc::new(NullCommandDispatcher);
    let command_registry: Arc<dyn CommandRegistry> = Arc::new(InMemoryCommandRegistry::default());
    std::mem::forget(tmp);

    AppState {
        deployment_store,
        release_store,
        token_store,
        auth_validator: Arc::new(RoleValidator(subjects)),
        authz: Arc::new(OssAuthz),
        telemetry_backend: Arc::new(alien_manager::providers::NullTelemetryBackend),
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
        tunnels: Some(TunnelRegistry::new()),
        charts: None,
        release_channels: None,
        bundle_signing_key: None,
        bundle_sources: None,
        log_buffer: Arc::new(alien_manager::LogBuffer::new()),
    }
}

/// Open a tunnel connection as the Operator does, and return the status.
async fn connect(state: &AppState, role: Role) -> StatusCode {
    let request = Request::builder()
        .uri(alien_tunnel::CONNECT_PATH)
        .header(header::AUTHORIZATION, format!("Bearer {}", token(role)))
        .header(header::UPGRADE, "websocket")
        .header(header::CONNECTION, "Upgrade")
        .header(header::SEC_WEBSOCKET_VERSION, "13")
        .header(header::SEC_WEBSOCKET_KEY, "dGhlIHNhbXBsZSBub25jZQ==")
        .header(header::SEC_WEBSOCKET_PROTOCOL, alien_tunnel::SUBPROTOCOL)
        .body(Body::empty())
        .unwrap();
    alien_manager::routes::tunnel::router()
        .with_state(state.clone())
        .oneshot(request)
        .await
        .unwrap()
        .status()
}

/// Pull a registry path, and return the status and body.
async fn pull(state: &AppState, role: Role, path: &str) -> (StatusCode, String) {
    let request = Request::builder()
        .uri(format!("/v2/{path}"))
        .header(header::AUTHORIZATION, format!("Bearer {}", token(role)))
        .body(Body::empty())
        .unwrap();
    let response = alien_manager::routes::registry_proxy::router()
        .with_state(state.clone())
        .oneshot(request)
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8_lossy(&body).into_owned())
}

#[tokio::test]
async fn only_the_operator_can_hold_a_deployments_tunnel() {
    let state = state().await;

    assert_eq!(
        connect(&state, Role::DeploymentManager).await,
        StatusCode::SWITCHING_PROTOCOLS,
        "the deployment's Operator opens its tunnel"
    );
    for role in [
        Role::DeploymentViewer,
        Role::DeploymentTelemetryWriter,
        Role::RemoteBindingResolver,
    ] {
        assert_eq!(
            connect(&state, role).await,
            StatusCode::FORBIDDEN,
            "a {role:?} token for the deployment must not receive its tunnel traffic"
        );
    }
}

#[tokio::test]
async fn capability_credentials_cannot_pull_charts_or_the_operator_image() {
    let state = state().await;

    for path in ["charts/files/manifests/0.1.0", "alien-operator/manifests/v1"] {
        for role in [Role::ImageRepositoryProvisioner, Role::SandboxImagePusher] {
            let (status, body) = pull(&state, role, path).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{role:?} pulling {path}: {body}");
            assert!(body.contains("DENIED"), "{role:?} pulling {path}: {body}");
        }
        // This manager serves no charts or Operator image, so a role that may
        // pull gets past the role check and finds nothing there.
        let (status, body) = pull(&state, Role::ProjectDeveloper, path).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "developer pulling {path}: {body}");
    }
}
