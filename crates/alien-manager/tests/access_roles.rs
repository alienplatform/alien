//! Which roles may hold a deployment's tunnel, and which may pull charts and
//! the Operator image.
//!
//! The standalone manager only issues deployment-scoped tokens to Operators,
//! but embedders' validators issue other deployment-scoped roles (viewers,
//! telemetry writers, binding resolvers) and project-scoped capability roles.
//! This test plugs in such a validator so every role reaches the real routes.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

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
use alien_manager::auth::{Authz, DeploymentCreateCtx, Role, Scope, Subject, SubjectKind};
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
    CreateReleaseParams, CredentialResolver, DeploymentGroupRecord, DeploymentRecord,
    DeploymentStore, ProjectRecord, ReleaseRecord, ReleaseStore, TelemetrySignal, TokenStore,
};
use alien_tunnel::manager::TunnelRegistry;
use alien_tunnel::operator::{self, TunnelClientConfig, TunnelTargets};

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
/// pull roles. Returns the state and the deployment's ID.
async fn state() -> (AppState, String) {
    state_with(Arc::new(OssAuthz), false).await
}

/// The deployment's own token in [`state_with`] fixtures awaiting setup teardown.
const DEPLOYMENT_SECRET: &str = "deployment-own-secret";
/// A secret input value in [`state_with`] fixtures awaiting setup teardown.
const INPUT_SECRET: &str = "deployer-input-secret";

/// [`state`] with another policy. With `awaiting_teardown`, the deployment is
/// `teardown-required` and holds its own token, as one handed to setup teardown does.
async fn state_with(authz: Arc<dyn Authz>, awaiting_teardown: bool) -> (AppState, String) {
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
                status: if awaiting_teardown {
                    "teardown-required"
                } else {
                    "running"
                }
                .to_string(),
                current_release_id: None,
                desired_release_id: None,
                import_source: None,
                setup_metadata: None,
                setup_target: "test".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
                input_values: if awaiting_teardown {
                    HashMap::from([("apiKey".to_string(), serde_json::json!(INPUT_SECRET))])
                } else {
                    Default::default()
                },
                deployment_token: awaiting_teardown.then(|| DEPLOYMENT_SECRET.to_string()),
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

    let teardown_role: Role = serde_json::from_str("\"deployment-teardown\"")
        .expect("an embedder can authenticate the restricted teardown role");
    subjects.insert(
        token(teardown_role),
        subject(deployment_scope, teardown_role),
    );

    subjects.insert(
        "group-teardown".to_string(),
        subject(
            Scope::DeploymentGroup {
                project_id: deployment.project_id.clone(),
                deployment_group_id: group.id.clone(),
            },
            Role::DeploymentTeardown,
        ),
    );
    subjects.insert(
        "foreign-teardown".to_string(),
        subject(
            Scope::Deployment {
                project_id: deployment.project_id.clone(),
                deployment_id: "another-deployment".to_string(),
            },
            Role::DeploymentTeardown,
        ),
    );
    subjects.insert(
        "workspace-teardown".to_string(),
        subject(Scope::Workspace, Role::DeploymentTeardown),
    );
    subjects.insert(
        "workspace-admin".to_string(),
        subject(Scope::Workspace, Role::WorkspaceAdmin),
    );

    let kv: Arc<dyn alien_bindings::traits::Kv> =
        Arc::new(LocalKv::new(tmp.path().join("kv")).await.unwrap());
    let command_storage: Arc<dyn alien_bindings::traits::Storage> = Arc::new(
        LocalStorage::new(tmp.path().join("storage").to_string_lossy().to_string()).unwrap(),
    );
    let command_dispatcher: Arc<dyn CommandDispatcher> = Arc::new(NullCommandDispatcher);
    let command_registry: Arc<dyn CommandRegistry> = Arc::new(InMemoryCommandRegistry::default());
    std::mem::forget(tmp);

    let state = AppState {
        deployment_store,
        release_store,
        project_store: Arc::new(alien_manager::providers::SingleWorkspaceProjects),
        token_store,
        auth_validator: Arc::new(RoleValidator(subjects)),
        authz,
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
    };
    (state, deployment.id)
}

/// Ask to open a tunnel connection with a role's token, and return the status.
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
    let (state, deployment_id) = state().await;
    let registry = state.tunnels.clone().expect("tunnels are on");

    // The Operator's real tunnel client, dialing the manager's real route
    // over TCP, gets a working connection for its deployment.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let manager_addr = listener.local_addr().unwrap();
    let router = alien_manager::routes::tunnel::router().with_state(state.clone());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let operator = tokio::spawn(operator::run(
        TunnelClientConfig {
            manager_url: format!("http://{manager_addr}").parse().unwrap(),
            token: token(Role::DeploymentManager),
            connections: 1,
        },
        TunnelTargets::new(),
    ));
    let mut waited = Duration::ZERO;
    while registry.connection_count(&deployment_id) == 0 {
        assert!(
            waited < Duration::from_secs(10),
            "the Operator's tunnel never connected"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
        waited += Duration::from_millis(50);
    }
    operator.abort();

    // Any other role for the same deployment is refused before the upgrade.
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
    let (state, _) = state().await;

    for path in [
        "charts/files/manifests/0.1.0",
        "alien-operator/manifests/v1",
    ] {
        for role in [Role::ImageRepositoryProvisioner, Role::SandboxImagePusher] {
            let (status, body) = pull(&state, role, path).await;
            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "{role:?} pulling {path}: {body}"
            );
            assert!(body.contains("DENIED"), "{role:?} pulling {path}: {body}");
        }
        // This manager serves no charts or Operator image, so a role that may
        // pull gets past the role check and finds nothing there.
        let (status, body) = pull(&state, Role::ProjectDeveloper, path).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "developer pulling {path}: {body}"
        );
    }
}

/// Recognizing a cleanup credential must not turn it into an Operator or a
/// workload credential under the default policy. An embedder must authorize
/// its delete protocol explicitly.
#[tokio::test]
async fn teardown_credentials_have_no_default_workload_access() {
    let (state, deployment_id) = state().await;
    let role: Role = serde_json::from_str("\"deployment-teardown\"")
        .expect("restricted teardown role is recognized");
    let deployment = state
        .deployment_store
        .get_deployment(&Subject::system(), &deployment_id)
        .await
        .expect("load deployment")
        .expect("deployment exists");
    let caller = subject(
        Scope::Deployment {
            project_id: deployment.project_id.clone(),
            deployment_id: deployment_id.clone(),
        },
        role,
    );
    for scope in [
        caller.scope.clone(),
        Scope::Workspace,
        Scope::Project {
            project_id: deployment.project_id.clone(),
        },
        Scope::DeploymentGroup {
            project_id: deployment.project_id.clone(),
            deployment_group_id: deployment.deployment_group_id.clone(),
        },
    ] {
        let scoped = subject(scope, role);
        assert!(!state.authz.can_read_deployment(&scoped, &deployment));
        assert!(!state.authz.can_update_deployment(&scoped, &deployment));
        assert!(!state.authz.can_delete_deployment(&scoped, &deployment));
        assert!(!state.authz.can_sync_deployment(&scoped, &deployment));
        assert!(!state
            .authz
            .can_acquire_deployments(&scoped, &[deployment.clone()]));
        assert!(!state.authz.can_acquire_deployments(&scoped, &[]));
        assert!(!state.authz.can_dispatch_command(&scoped, &deployment));
        assert!(!state.authz.can_execute_command(&scoped, &deployment));
        assert!(!state
            .authz
            .can_ingest_telemetry(&scoped, TelemetrySignal::Logs));
        assert!(!state
            .authz
            .can_ingest_telemetry(&scoped, TelemetrySignal::Metrics));
    }

    let release = state
        .release_store
        .create_release(
            &Subject::system(),
            CreateReleaseParams {
                project_id: caller
                    .scope
                    .project_id()
                    .expect("project scope")
                    .to_string(),
                stacks: HashMap::new(),
                git_commit_sha: None,
                git_commit_ref: None,
                git_commit_message: None,
            },
        )
        .await
        .expect("persist release");
    let router = alien_manager::routes::releases::router()
        .merge(alien_manager::routes::deployments::router())
        .with_state(state.clone());
    for path in [
        format!("/v1/releases/{}", release.id),
        format!("/v1/deployments/{deployment_id}"),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header(header::AUTHORIZATION, format!("Bearer {}", token(role)))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("route response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    assert_eq!(connect(&state, role).await, StatusCode::FORBIDDEN);
    for path in [
        "charts/files/manifests/0.1.0",
        "alien-operator/manifests/v1",
    ] {
        let (status, _) = pull(&state, role, path).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "teardown token pulling {path}"
        );
    }
}

#[tokio::test]
async fn initialize_checks_deployment_read_authority() {
    let (state, deployment_id) = state().await;
    let router = alien_manager::routes::sync::initialize_router().with_state(state);
    for (role, expected) in [
        (Role::DeploymentTeardown, StatusCode::FORBIDDEN),
        (Role::DeploymentManager, StatusCode::OK),
        (Role::DeploymentViewer, StatusCode::OK),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/initialize")
                    .header(header::AUTHORIZATION, format!("Bearer {}", token(role)))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"initialDesiredRelease":"none"}"#))
                    .expect("request"),
            )
            .await
            .expect("initialize response");
        let status = response.status();
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("response body");
        assert_eq!(
            status,
            expected,
            "{role:?}: {}",
            String::from_utf8_lossy(&body)
        );
        if expected == StatusCode::OK {
            let value: serde_json::Value = serde_json::from_slice(&body).expect("initialize JSON");
            assert_eq!(value["deploymentId"], deployment_id);
        }
    }
}

/// A group-scoped initialize registers a new deployment and returns its token; a cleanup
/// credential must not, whatever scope an embedder gives it.
#[tokio::test]
async fn teardown_credentials_cannot_register_deployments() {
    let (state, _) = state().await;
    let count = || async {
        state
            .deployment_store
            .list_deployments(&Subject::system(), &Default::default())
            .await
            .unwrap()
            .len()
    };
    let before = count().await;
    let router = alien_manager::routes::sync::initialize_router().with_state(state.clone());
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/initialize")
                .header(header::AUTHORIZATION, "Bearer group-teardown")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"name":"registered-by-cleanup","platform":"kubernetes","initialDesiredRelease":"none"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "{}",
        String::from_utf8_lossy(&body)
    );
    assert_eq!(count().await, before);
}

#[tokio::test]
async fn workspace_acquire_checks_role_without_deployment_ids() {
    let (state, deployment_id) = state().await;
    let before = state
        .deployment_store
        .get_deployment(&Subject::system(), &deployment_id)
        .await
        .unwrap()
        .unwrap();
    let before = serde_json::to_value(before).unwrap();
    let router = alien_manager::routes::sync::router().with_state(state.clone());
    for mode in [
        None,
        Some("runtime"),
        Some("setup-run"),
        Some("setup-teardown"),
    ] {
        for (bearer, expected) in [
            ("workspace-teardown", StatusCode::FORBIDDEN),
            ("workspace-admin", StatusCode::OK),
        ] {
            let mut body = serde_json::json!({
                "session": "workspace-acquisition", "deploymentModel": "push",
                "statuses": if bearer == "workspace-teardown" { vec!["running"] } else { vec!["not-a-status"] }
            });
            if let Some(mode) = mode {
                body["acquireMode"] = mode.into();
            }
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/v1/sync/acquire")
                        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(body.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
            assert_eq!(
                status,
                expected,
                "{bearer} {mode:?}: {}",
                String::from_utf8_lossy(&bytes)
            );
            if expected == StatusCode::OK {
                let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(value["deployments"], serde_json::json!([]));
            }
            let after = state
                .deployment_store
                .get_deployment(&Subject::system(), &deployment_id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(serde_json::to_value(after).unwrap(), before);
        }
    }
}

#[tokio::test]
async fn workspace_admin_can_acquire_matching_deployment_without_ids() {
    let (state, deployment_id) = state().await;
    let response = alien_manager::routes::sync::router()
        .with_state(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sync/acquire")
                .header(header::AUTHORIZATION, "Bearer workspace-admin")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"session":"admin-acquisition","deploymentModel":"push","statuses":["running"]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["deployments"].as_array().unwrap().len(), 1);
    assert_eq!(value["deployments"][0]["deployment"]["id"], deployment_id);
    let saved = state
        .deployment_store
        .get_deployment(&Subject::system(), &deployment_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(saved).unwrap()["lockedBy"],
        "admin-acquisition"
    );
}

/// An embedder policy that admits setup teardown for a teardown credential's own
/// deployment and otherwise defers to the default policy.
struct SetupTeardownAuthz;

impl Authz for SetupTeardownAuthz {
    fn can_setup_teardown(&self, subject: &Subject, deployment: &DeploymentRecord) -> bool {
        subject.role == Role::DeploymentTeardown
            && matches!(
                &subject.scope,
                Scope::Deployment { deployment_id, .. } if deployment_id == &deployment.id
            )
    }

    fn can_create_release(&self, s: &Subject, project_id: &str) -> bool {
        OssAuthz.can_create_release(s, project_id)
    }
    fn can_read_release(&self, s: &Subject, release: &ReleaseRecord) -> bool {
        OssAuthz.can_read_release(s, release)
    }
    fn can_export_release(&self, s: &Subject, release: &ReleaseRecord) -> bool {
        OssAuthz.can_export_release(s, release)
    }
    fn can_create_deployment(&self, s: &Subject, ctx: DeploymentCreateCtx<'_>) -> bool {
        OssAuthz.can_create_deployment(s, ctx)
    }
    fn can_read_deployment(&self, s: &Subject, d: &DeploymentRecord) -> bool {
        OssAuthz.can_read_deployment(s, d)
    }
    fn can_update_deployment(&self, s: &Subject, d: &DeploymentRecord) -> bool {
        OssAuthz.can_update_deployment(s, d)
    }
    fn can_delete_deployment(&self, s: &Subject, d: &DeploymentRecord) -> bool {
        OssAuthz.can_delete_deployment(s, d)
    }
    fn can_create_deployment_group(&self, s: &Subject, project_id: &str) -> bool {
        OssAuthz.can_create_deployment_group(s, project_id)
    }
    fn can_read_deployment_group(&self, s: &Subject, dg: &DeploymentGroupRecord) -> bool {
        OssAuthz.can_read_deployment_group(s, dg)
    }
    fn can_update_deployment_group(&self, s: &Subject, dg: &DeploymentGroupRecord) -> bool {
        OssAuthz.can_update_deployment_group(s, dg)
    }
    fn can_delete_deployment_group(&self, s: &Subject, dg: &DeploymentGroupRecord) -> bool {
        OssAuthz.can_delete_deployment_group(s, dg)
    }
    fn can_dispatch_command(&self, s: &Subject, d: &DeploymentRecord) -> bool {
        OssAuthz.can_dispatch_command(s, d)
    }
    fn can_read_command(&self, s: &Subject, d: &DeploymentRecord) -> bool {
        OssAuthz.can_read_command(s, d)
    }
    fn can_read_command_context(
        &self,
        s: &Subject,
        command: &alien_commands::server::CommandAccessContext,
    ) -> bool {
        OssAuthz.can_read_command_context(s, command)
    }
    fn can_sync_deployment(&self, s: &Subject, d: &DeploymentRecord) -> bool {
        OssAuthz.can_sync_deployment(s, d)
    }
    fn can_acquire_deployments(&self, s: &Subject, deployments: &[DeploymentRecord]) -> bool {
        OssAuthz.can_acquire_deployments(s, deployments)
    }
    fn can_ingest_telemetry(&self, s: &Subject, signal: TelemetrySignal) -> bool {
        OssAuthz.can_ingest_telemetry(s, signal)
    }
    fn can_push_image(&self, s: &Subject, project: &ProjectRecord, repo_name: &str) -> bool {
        OssAuthz.can_push_image(s, project, repo_name)
    }
    fn can_act_on_deployment(&self, s: &Subject, d: &DeploymentRecord) -> bool {
        OssAuthz.can_act_on_deployment(s, d)
    }
}

async fn post_sync(
    router: &axum::Router,
    bearer: &str,
    path: &str,
    body: serde_json::Value,
) -> (StatusCode, String) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

/// With an embedder's opt-in, a teardown credential may hold a setup-teardown lease on its own
/// deployment and nothing else, and the acquire response gives it no deployment secrets. The
/// deployment manager's response to the same request shows what is withheld.
#[tokio::test]
async fn opted_in_teardown_credential_holds_only_a_setup_teardown_lease() {
    let (state, deployment_id) = state_with(Arc::new(SetupTeardownAuthz), true).await;
    let router = alien_manager::routes::sync::router().with_state(state.clone());
    let teardown = token(Role::DeploymentTeardown);
    let acquire = |mode: Option<&str>, ids: Option<Vec<&str>>, session: &str| {
        let mut body = serde_json::json!({
            "session": session,
            "deploymentModel": "push",
            "statuses": ["teardown-required"],
        });
        if let Some(mode) = mode {
            body["acquireMode"] = mode.into();
        }
        if let Some(ids) = ids {
            body["deploymentIds"] = serde_json::json!(ids);
        }
        body
    };
    let locked_by = || async {
        state
            .deployment_store
            .get_deployment(&Subject::system(), &deployment_id)
            .await
            .unwrap()
            .unwrap()
            .locked_by
    };

    // Anything but explicit setup teardown of its own deployment is refused, and takes no lease.
    for (bearer, mode, ids) in [
        (teardown.as_str(), None, Some(vec![deployment_id.as_str()])),
        (
            teardown.as_str(),
            Some("runtime"),
            Some(vec![deployment_id.as_str()]),
        ),
        (
            teardown.as_str(),
            Some("setup-run"),
            Some(vec![deployment_id.as_str()]),
        ),
        (teardown.as_str(), Some("setup-teardown"), None),
        (teardown.as_str(), Some("setup-teardown"), Some(vec![])),
        (
            "foreign-teardown",
            Some("setup-teardown"),
            Some(vec![deployment_id.as_str()]),
        ),
    ] {
        let (status, body) = post_sync(
            &router,
            bearer,
            "/v1/sync/acquire",
            acquire(mode, ids.clone(), "refused"),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{bearer} {mode:?} {ids:?}: {body}"
        );
        assert_eq!(locked_by().await, None);
    }

    let (status, body) = post_sync(
        &router,
        &teardown,
        "/v1/sync/acquire",
        acquire(
            Some("setup-teardown"),
            Some(vec![deployment_id.as_str()]),
            "cleanup",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    let acquired = &value["deployments"][0]["deployment"];
    assert_eq!(acquired["id"], deployment_id.as_str());
    assert!(acquired.get("deploymentConfig").is_none(), "{body}");
    assert!(!body.contains(DEPLOYMENT_SECRET), "{body}");
    assert!(!body.contains(INPUT_SECRET), "{body}");
    assert_eq!(locked_by().await.as_deref(), Some("cleanup"));

    // It may report deletion progress under its lease; another deployment's credential may not.
    let report = serde_json::json!({
        "deploymentId": deployment_id,
        "session": "cleanup",
        "state": alien_core::DeploymentState {
            status: alien_core::DeploymentStatus::TeardownRequired,
            platform: Platform::Kubernetes,
            current_release: None,
            target_release: None,
            stack_state: Some(StackState::new(Platform::Kubernetes)),
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        },
    });
    let (status, body) = post_sync(
        &router,
        "foreign-teardown",
        "/v1/sync/reconcile",
        report.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, body) = post_sync(&router, &teardown, "/v1/sync/reconcile", report).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(value["nativeImageHost"].is_null(), "{body}");
    assert!(value["current"]["targetRelease"].is_null(), "{body}");
    assert!(
        !body.contains(DEPLOYMENT_SECRET) && !body.contains(INPUT_SECRET),
        "{body}"
    );

    // Its own lease can be renewed and given back; another deployment's credential cannot.
    let lease = serde_json::json!({ "deploymentId": deployment_id, "session": "cleanup" });
    let (status, body) =
        post_sync(&router, "foreign-teardown", "/v1/sync/renew", lease.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, body) = post_sync(
        &router,
        "foreign-teardown",
        "/v1/sync/release",
        lease.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(locked_by().await.as_deref(), Some("cleanup"));
    let (status, body) = post_sync(&router, &teardown, "/v1/sync/renew", lease.clone()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = post_sync(&router, &teardown, "/v1/sync/release", lease).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(locked_by().await, None);

    // A deployment manager acquiring the same deployment receives its config and token.
    let (status, body) = post_sync(
        &router,
        &token(Role::DeploymentManager),
        "/v1/sync/acquire",
        acquire(
            Some("setup-teardown"),
            Some(vec![deployment_id.as_str()]),
            "manager",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        value["deployments"][0]["deployment"]["deploymentConfig"]["deploymentToken"],
        DEPLOYMENT_SECRET
    );
}
