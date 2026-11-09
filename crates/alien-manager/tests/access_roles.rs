//! Which roles may hold a deployment's tunnel, and which may pull charts and
//! the Operator image.
//!
//! The standalone manager only issues deployment-scoped tokens to Operators,
//! but embedders' validators issue other deployment-scoped roles (viewers,
//! telemetry writers, binding resolvers) and project-scoped capability roles.
//! This test plugs in such a validator so every role reaches the real routes.

use alien_bindings::{BindingsProvider, BindingsProviderApi};
use alien_core::{
    ManagementPermissions, PermissionProfile, Resource, ResourceStatus, Stack, StackResourceState,
    Vault, VaultBinding,
};
use alien_manager::traits::{CreateReleaseParams, UpdateImportedDeploymentParams};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use http::header;
use httpmock::{
    Method::{GET, POST},
    MockServer,
};
use serde_json::{json, Value};
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
    AuthValidator, CreateDeploymentGroupParams, CreateImportedDeploymentParams, CredentialResolver,
    DeploymentRecord, DeploymentStore, ReleaseStore, TokenStore,
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
async fn state() -> (AppState, String, tempfile::TempDir) {
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
        Role::VerifiedSetupImporter,
        Role::OperatorSync,
        Role::OperatorTunnel,
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

    let state = AppState {
        deployment_store,
        release_store,
        project_store: Arc::new(alien_manager::providers::SingleWorkspaceProjects),
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
        values: None,
        value_access: None,
        verify_limiter: std::sync::Arc::new(alien_manager::values::VerifyLimiter::default()),
    };
    (state, deployment.id, tmp)
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
    let (state, deployment_id, _directory) = state().await;
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
        Role::VerifiedSetupImporter,
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
    let (state, _, _directory) = state().await;

    for path in [
        "charts/files/manifests/0.1.0",
        "alien-operator/manifests/v1",
    ] {
        for role in [
            Role::ImageRepositoryProvisioner,
            Role::SandboxImagePusher,
            Role::VerifiedSetupImporter,
            Role::OperatorSync,
            Role::OperatorTunnel,
        ] {
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

/// Before it enrolls, an operator pulls its own image from the manager with
/// its one-hour code, the only credential its chart holds. The code pulls the
/// Operator image and nothing else, and stops working when it expires.
#[tokio::test]
async fn an_enrollment_code_pulls_only_the_operator_image() {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let (mut state, deployment_id, _directory) = state().await;
    let upstream = MockServer::start_async().await;
    let manifest = upstream
        .mock_async(|when, then| {
            when.method(GET).path("/v2/ops/alien-operator/manifests/v1");
            then.status(200)
                .header("content-type", "application/vnd.oci.image.manifest.v1+json")
                .body("{\"schemaVersion\":2}");
        })
        .await;
    let identity_directory = tempfile::tempdir().unwrap();
    let store = Arc::new(
        alien_manager::stores::sqlite::SqliteRuntimeIdentityStore::new(Arc::new(
            SqliteDatabase::new(&identity_directory.path().join("id.db").to_string_lossy())
                .await
                .unwrap(),
        )),
    );
    let now = Arc::new(std::sync::Mutex::new(chrono::Utc::now()));
    let clock = now.clone();
    let verifier = alien_manager::auth::standalone_identity::StandaloneRuntimeIdentity::with_clock(
        store.clone(),
        state.deployment_store.clone(),
        Arc::new(NoCredentials),
        Arc::new(move || *clock.lock().unwrap()),
    )
    .unwrap();
    let mut charts = alien_manager::routes::charts::ChartSettings::new(
        Some(format!("{}/ops/alien-operator:v1", upstream.address())),
        true,
    );
    charts.enrollment_codes = Some(Arc::new(verifier));
    state.charts = Some(Arc::new(charts));
    let (code, _) = store
        .issue_enrollment_code(&deployment_id, *now.lock().unwrap())
        .await
        .unwrap();

    let basic_pull = |password: String, path: &'static str| {
        let state = state.clone();
        async move {
            let request = Request::builder()
                .uri(format!("/v2/{path}"))
                .header(
                    header::AUTHORIZATION,
                    format!("Basic {}", STANDARD.encode(format!("prod:{password}"))),
                )
                .body(Body::empty())
                .unwrap();
            let response = alien_manager::routes::registry_proxy::router()
                .with_state(state)
                .oneshot(request)
                .await
                .unwrap();
            let status = response.status();
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            (status, String::from_utf8_lossy(&body).into_owned())
        }
    };

    let (status, body) = basic_pull(code.clone(), "alien-operator/manifests/v1").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, "{\"schemaVersion\":2}");
    manifest.assert_hits_async(1).await;

    // The code opens no other repository.
    let (status, _) = basic_pull(code.clone(), "artifacts/default/manifests/v1").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // An unknown code pulls nothing.
    let (status, _) = basic_pull(
        "ax_enroll_unknown".to_string(),
        "alien-operator/manifests/v1",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // After its hour the code pulls nothing.
    *now.lock().unwrap() += chrono::Duration::hours(1);
    let (status, _) = basic_pull(code, "alien-operator/manifests/v1").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    manifest.assert_hits_async(1).await;
}

struct LocalVaultCredentials(String);

#[async_trait]
impl CredentialResolver for LocalVaultCredentials {
    async fn resolve(&self, _deployment: &DeploymentRecord) -> Result<ClientConfig, AlienError> {
        Ok(ClientConfig::Local {
            state_directory: self.0.clone(),
        })
    }
}

#[tokio::test]
async fn vault_routes_refuse_deployer_values_and_preserve_shared_secrets() {
    let (mut state, deployment_id, _state_directory) = state().await;
    let directory = tempfile::tempdir().unwrap();
    let directory_name = directory.path().to_string_lossy().to_string();
    state.credential_resolver = Arc::new(LocalVaultCredentials(directory_name.clone()));
    let mut stack = Stack::new("vault-route-test".to_string())
        .management(ManagementPermissions::Extend(
            PermissionProfile::new()
                .resource("deployer-secrets", ["vault/data-read", "vault/data-write"]),
        ))
        .build();
    stack.inputs = vec![serde_json::from_value(serde_json::json!({
        "id": "apiKey", "kind": "secret", "providedBy": ["deployer"],
        "required": true, "label": "API key", "description": ""
    }))
    .unwrap()];
    let release = state
        .release_store
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
    let mut stack_state = StackState::new(Platform::Kubernetes);
    for name in ["secrets", "deployer-secrets"] {
        stack_state.resources.insert(
            name.to_string(),
            StackResourceState::builder()
                .resource_type(Vault::RESOURCE_TYPE.to_string())
                .status(ResourceStatus::Running)
                .config(Resource::new(Vault::new(name.to_string()).build()))
                .remote_binding_params(
                    serde_json::to_value(VaultBinding::local(name, &directory_name)).unwrap(),
                )
                .dependencies(Vec::new())
                .build(),
        );
    }
    let provider = BindingsProvider::from_stack_state(
        &stack_state,
        ClientConfig::Local {
            state_directory: directory_name,
        },
    )
    .unwrap();
    let deployer = provider.load_vault("deployer-secrets").await.unwrap();
    let shared = provider.load_vault("secrets").await.unwrap();
    deployer
        .set_secret("api-key", "fake-customer-value")
        .await
        .unwrap();
    state
        .deployment_store
        .update_imported_stack_state(
            &Subject::system(),
            &deployment_id,
            UpdateImportedDeploymentParams {
                stack_settings: StackSettings::default(),
                stack_state,
                environment_info: None,
                runtime_metadata: RuntimeMetadata::default(),
                setup_metadata: None,
                current_release_id: Some(release.id),
                setup_target: "test".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
                activation_status: None,
                schedule_reconciliation: false,
                input_values: Default::default(),
            },
        )
        .await
        .unwrap();
    let router = alien_manager::routes::vault::router().with_state(state);
    for path in [
        "deployer-secrets/secrets/api-key",
        "DEPLOYER_SECRETS/secrets/api-key",
        "deployer%21-secrets/secrets/api-key",
    ] {
        for method in ["GET", "PUT", "DELETE"] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(format!("/v1/deployments/{deployment_id}/vault/{path}"))
                        .header(
                            header::AUTHORIZATION,
                            format!("Bearer {}", token(Role::ProjectDeveloper)),
                        )
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(r#"{"value":"overwrite-attempt"}"#))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {path}");
            assert_eq!(
                deployer.get_secret("api-key").await.unwrap(),
                "fake-customer-value"
            );
        }
    }
    for method in ["PUT", "GET", "DELETE"] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(format!(
                        "/v1/deployments/{deployment_id}/vault/secrets/secrets/APP_SECRET"
                    ))
                    .header(
                        header::AUTHORIZATION,
                        format!("Bearer {}", token(Role::ProjectDeveloper)),
                    )
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"value":"fake-developer-value"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{method} shared developer secret"
        );
        if method == "GET" {
            let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["value"],
                "fake-developer-value"
            );
        }
    }
    assert!(shared.get_secret("APP_SECRET").await.is_err());
}

struct VaultCredentials(ClientConfig);

#[async_trait]
impl CredentialResolver for VaultCredentials {
    async fn resolve(&self, _deployment: &DeploymentRecord) -> Result<ClientConfig, AlienError> {
        Ok(self.0.clone())
    }
}

async fn vault_router(
    config: ClientConfig,
    binding: VaultBinding,
) -> (axum::Router, String, tempfile::TempDir) {
    let (mut state, id, directory) = state().await;
    state.credential_resolver = Arc::new(VaultCredentials(config));
    let mut stack_state = StackState::new(Platform::Kubernetes);
    stack_state.resources.insert(
        "secrets".to_string(),
        StackResourceState::builder()
            .resource_type(Vault::RESOURCE_TYPE.to_string())
            .status(ResourceStatus::Running)
            .config(Resource::new(Vault::new("secrets".to_string()).build()))
            .remote_binding_params(serde_json::to_value(binding).unwrap())
            .dependencies(Vec::new())
            .build(),
    );
    state
        .deployment_store
        .update_imported_stack_state(
            &Subject::system(),
            &id,
            UpdateImportedDeploymentParams {
                stack_settings: StackSettings::default(),
                stack_state,
                environment_info: None,
                runtime_metadata: RuntimeMetadata::default(),
                setup_metadata: None,
                current_release_id: None,
                setup_target: "test".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
                activation_status: None,
                schedule_reconciliation: false,
                input_values: Default::default(),
            },
        )
        .await
        .unwrap();
    (
        alien_manager::routes::vault::router().with_state(state),
        id,
        directory,
    )
}

async fn read_vault(router: &axum::Router, id: &str) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/v1/deployments/{id}/vault/secrets/secrets/TEST_KEY"
                ))
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", token(Role::ProjectDeveloper)),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn vault_get_local_missing_key_is_404_but_corrupt_storage_is_500() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().to_string_lossy().to_string();
    let (router, id, _state_directory) = vault_router(
        ClientConfig::Local {
            state_directory: path.clone(),
        },
        VaultBinding::local("secrets", &path),
    )
    .await;
    let (status, body) = read_vault(&router, &id).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["code"], "VAULT_SECRET_NOT_FOUND");
    assert_eq!(body["retryable"], false);
    // A malformed file is not evidence that the requested key is absent.
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(directory.path().join("secrets.json"), "not json").unwrap();
    let (status, _) = read_vault(&router, &id).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}

async fn cloud_vault_get(backend: &str, missing: bool) {
    let server = MockServer::start_async().await;
    let (config, binding, method, path, status, body) = match backend {
        "aws" => (
            json!({"platform":"aws", "accountId":"000000000000", "region":"us-east-1",
            "credentials":{"type":"accessKeys", "access_key_id":"test", "secret_access_key":"test"},
            "serviceOverrides":{"endpoints":{"ssm":server.base_url()}}}),
            VaultBinding::parameter_store("test-secrets"),
            POST,
            "/".to_string(),
            400,
            json!({"__type":if missing {"ParameterNotFound"} else {"AccessDeniedException"}, "message":"synthetic failure"}),
        ),
        "gcp" => (
            json!({"platform":"gcp", "projectId":"test-project", "region":"us-central1",
            "credentials":{"type":"accessToken", "token":"test"},
            "serviceOverrides":{"endpoints":{"secretmanager":server.base_url()}}}),
            VaultBinding::secret_manager("test-secrets"),
            GET,
            "/projects/test-project/secrets/test-secrets-TEST_KEY/versions/latest:access"
                .to_string(),
            if missing { 404 } else { 403 },
            json!({"error":{"code":if missing {404} else {403},
                "status":if missing {"NOT_FOUND"} else {"PERMISSION_DENIED"}, "message":"synthetic failure"}}),
        ),
        "azure" => (
            json!({"platform":"azure", "subscriptionId":"test", "tenantId":"test",
            "credentials":{"type":"accessToken", "token":"test"},
            "serviceOverrides":{"endpoints":{"keyvault":server.base_url()}}}),
            VaultBinding::key_vault("test-vault"),
            GET,
            "/secrets/TEST-KEY".to_string(),
            if missing { 404 } else { 403 },
            json!({"error":{"code":if missing {"SecretNotFound"} else {"Forbidden"},
                "message":"synthetic failure"}}),
        ),
        "kubernetes" => (
            json!({"platform":"kubernetes", "mode":"manual", "server_url":server.base_url(),
            "token":"test", "namespace":"default", "additional_headers":{}}),
            VaultBinding::kubernetes_secret("default", "test-secrets"),
            GET,
            "/api/v1/namespaces/default/secrets/test-secrets-test-key".to_string(),
            if missing { 404 } else { 403 },
            json!({"apiVersion":"v1", "kind":"Status", "status":"Failure",
                "reason":if missing {"NotFound"} else {"Forbidden"}, "message":"synthetic failure", "code":if missing {404} else {403}}),
        ),
        _ => panic!("unknown backend"),
    };
    let read = server
        .mock_async(|when, then| {
            when.method(method).path(path);
            then.status(status).json_body(body);
        })
        .await;
    let (router, id, _state_directory) =
        vault_router(serde_json::from_value(config).unwrap(), binding).await;
    let (status, body) = read_vault(&router, &id).await;
    assert!(
        read.hits_async().await >= 1,
        "the provider must receive the read"
    );
    if missing {
        assert_eq!(status, StatusCode::NOT_FOUND, "{backend}: {body}");
        assert_eq!(body["code"], "VAULT_SECRET_NOT_FOUND");
        assert_eq!(body["retryable"], false);
    } else {
        assert_eq!(
            status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "{backend}: {body}"
        );
    }
}

#[tokio::test]
async fn vault_get_aws_missing_key_is_404() {
    cloud_vault_get("aws", true).await;
}
#[tokio::test]
async fn vault_get_gcp_missing_key_is_404() {
    cloud_vault_get("gcp", true).await;
}
#[tokio::test]
async fn vault_get_azure_missing_key_is_404() {
    cloud_vault_get("azure", true).await;
}
#[tokio::test]
async fn vault_get_kubernetes_missing_key_is_404() {
    cloud_vault_get("kubernetes", true).await;
}
#[tokio::test]
async fn vault_get_provider_denials_stay_internal_errors() {
    for backend in ["aws", "gcp", "azure", "kubernetes"] {
        cloud_vault_get(backend, false).await;
    }
}

mod runtime_credentials {
    use super::*;
    use alien_manager::auth::identity_exchange::{RuntimeIdentityProof, VerifiedRuntimeIdentity};
    use alien_manager::auth::runtime_capability::{
        RuntimeAuthValidator, RuntimeCredentials, RuntimeGrant, RuntimePurpose,
    };
    use chrono::{TimeZone, Utc};
    use opentelemetry_proto::tonic::{
        collector::logs::v1::ExportLogsServiceRequest,
        common::v1::{any_value, AnyValue},
        logs::v1::{LogRecord, ResourceLogs, ScopeLogs},
    };
    use prost::Message;
    use std::sync::atomic::{AtomicI64, Ordering};

    fn credentials(audience: &str, clock: Arc<AtomicI64>) -> Arc<RuntimeCredentials> {
        Arc::new(
            RuntimeCredentials::with_clock(
                audience.to_owned(),
                [7; 32],
                Arc::new(move || Utc.timestamp_opt(clock.load(Ordering::SeqCst), 0).unwrap()),
            )
            .unwrap(),
        )
    }

    fn grant(deployment_id: &str) -> RuntimeGrant {
        RuntimeGrant {
            workspace_id: "default".into(),
            project_id: "default".into(),
            deployment_id: deployment_id.into(),
            component_id: "worker-a".into(),
            purpose: RuntimePurpose::Telemetry,
        }
    }

    async fn logs(state: &AppState, bearer: &str) -> StatusCode {
        let payload = ExportLogsServiceRequest {
            resource_logs: vec![ResourceLogs {
                scope_logs: vec![ScopeLogs {
                    log_records: vec![LogRecord {
                        body: Some(AnyValue {
                            value: Some(any_value::Value::StringValue("runtime log".into())),
                        }),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        }
        .encode_to_vec();
        axum::Router::new()
            .route(
                "/v1/logs",
                axum::routing::post(alien_manager::routes::telemetry::ingest_logs),
            )
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/logs")
                    .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
                    .header("x-deployment-id", "different-deployment")
                    .body(Body::from(payload))
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn tunnel_stream_closes_at_parent_deadline_and_fresh_authority_reconnects() {
        use futures::StreamExt;
        use http_body_util::BodyExt;

        let (mut state, deployment_id, _tmp) = state().await;
        let issuer = Arc::new(RuntimeCredentials::new("manager-a".into(), [7; 32]).unwrap());
        state.auth_validator = Arc::new(RuntimeAuthValidator::new(
            issuer.clone(),
            state.auth_validator,
        ));
        let registry = state.tunnels.clone().unwrap();
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream.local_addr().unwrap();
        let upstream_task = tokio::spawn(async move {
            axum::serve(
                upstream,
                axum::Router::new().route(
                    "/",
                    axum::routing::get(|| async {
                        Body::from_stream(
                            futures::stream::once(async { Ok::<_, std::io::Error>("open") })
                                .chain(futures::stream::pending()),
                        )
                    }),
                ),
            )
            .await
            .unwrap();
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = alien_manager::routes::tunnel::router().with_state(state);
        let manager_task =
            tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let targets = TunnelTargets::new();
        targets.replace(std::collections::HashMap::from([(
            "app".into(),
            format!("http://{upstream_addr}").parse().unwrap(),
        )]));
        let mut authority = grant(&deployment_id);
        authority.purpose = RuntimePurpose::OperatorTunnel;
        let deadline = Utc::now() + chrono::Duration::seconds(3);
        let issued = issuer.issue(authority.clone(), deadline).unwrap();
        let operator_task = tokio::spawn(operator::run(
            TunnelClientConfig {
                manager_url: format!("http://{address}").parse().unwrap(),
                token: issued.token,
                connections: 1,
            },
            targets.clone(),
        ));
        tokio::time::timeout(Duration::from_secs(2), async {
            while registry.connection_count(&deployment_id) == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let request = http::Request::builder()
            .uri("http://app/")
            .body(
                Body::empty()
                    .map_err(|e| Box::new(e) as alien_tunnel::manager::BoxError)
                    .boxed_unsync(),
            )
            .unwrap();
        let mut response = registry
            .send(&deployment_id, request)
            .await
            .unwrap()
            .into_body();
        let first = response
            .frame()
            .await
            .unwrap()
            .unwrap()
            .into_data()
            .unwrap();
        assert_eq!(first.as_ref(), b"open");
        let ended = tokio::time::timeout(Duration::from_secs(4), response.frame())
            .await
            .expect("expired tunnel stream remained open");
        assert!(ended.is_none() || ended.unwrap().is_err());
        assert_eq!(registry.connection_count(&deployment_id), 0);
        assert!(Utc::now() >= deadline);
        operator_task.abort();
        let fresh = issuer
            .issue(authority, Utc::now() + chrono::Duration::minutes(1))
            .unwrap();
        let renewed = tokio::spawn(operator::run(
            TunnelClientConfig {
                manager_url: format!("http://{address}").parse().unwrap(),
                token: fresh.token,
                connections: 1,
            },
            targets,
        ));
        tokio::time::timeout(Duration::from_secs(2), async {
            while registry.connection_count(&deployment_id) == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fresh authorization did not reconnect");
        renewed.abort();
        manager_task.abort();
        upstream_task.abort();
    }

    #[tokio::test]
    async fn bounded_telemetry_reaches_its_deployment_and_stops_exactly_at_expiry() {
        let (mut state, deployment_id, _tmp) = state().await;
        let clock = Arc::new(AtomicI64::new(1_800_000_000));
        let credentials = credentials("manager-a", clock.clone());
        state.auth_validator = Arc::new(RuntimeAuthValidator::new(
            credentials.clone(),
            state.auth_validator,
        ));
        state.telemetry_backend = Arc::new(
            alien_manager::providers::in_memory_telemetry::InMemoryTelemetryBackend::new(
                state.log_buffer.clone(),
            ),
        );
        let now = Utc.timestamp_opt(clock.load(Ordering::SeqCst), 0).unwrap();
        let bearer = credentials
            .issue(grant(&deployment_id), now + chrono::Duration::hours(4))
            .unwrap()
            .token;
        assert_eq!(logs(&state, &bearer).await, StatusCode::OK);
        let entries = state.log_buffer.get_entries(None, 100).await;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].deployment_id, deployment_id);
        assert_eq!(entries[0].body, "runtime log");
        clock.fetch_add(3599, Ordering::SeqCst);
        assert_eq!(logs(&state, &bearer).await, StatusCode::OK);
        clock.fetch_add(1, Ordering::SeqCst);
        assert_eq!(logs(&state, &bearer).await, StatusCode::UNAUTHORIZED);
        assert_eq!(state.log_buffer.get_entries(None, 100).await.len(), 2);
        let fresh = credentials
            .issue(grant(&deployment_id), now + chrono::Duration::hours(4))
            .unwrap()
            .token;
        assert_eq!(logs(&state, &fresh).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn parent_deadline_and_manager_audience_cannot_be_extended_or_replaced() {
        let (mut state, deployment_id, _tmp) = state().await;
        let clock = Arc::new(AtomicI64::new(1_800_000_000));
        let issuer = credentials("manager-a", clock.clone());
        let deadline = Utc.timestamp_opt(1_800_000_010, 0).unwrap();
        let bearer = issuer.issue(grant(&deployment_id), deadline).unwrap().token;
        state.auth_validator = Arc::new(RuntimeAuthValidator::new(
            credentials("manager-b", clock.clone()),
            state.auth_validator.clone(),
        ));
        assert_eq!(logs(&state, &bearer).await, StatusCode::UNAUTHORIZED);
        state.auth_validator = Arc::new(RuntimeAuthValidator::new(
            issuer.clone(),
            state.auth_validator.clone(),
        ));
        clock.fetch_add(9, Ordering::SeqCst);
        let replay = issuer.issue(grant(&deployment_id), deadline).unwrap().token;
        assert_eq!(logs(&state, &replay).await, StatusCode::OK);
        clock.fetch_add(1, Ordering::SeqCst);
        assert_eq!(logs(&state, &bearer).await, StatusCode::UNAUTHORIZED);
        assert_eq!(logs(&state, &replay).await, StatusCode::UNAUTHORIZED);
        assert!(issuer.issue(grant(&deployment_id), deadline).is_err());
    }

    struct NoLegacyFallback;

    #[async_trait]
    impl AuthValidator for NoLegacyFallback {
        async fn validate(
            &self,
            _headers: &http::HeaderMap,
        ) -> Result<Option<Subject>, AlienError> {
            panic!("Runtime credential reached legacy authentication");
        }
    }

    #[tokio::test]
    async fn modified_capability_never_falls_back_to_legacy_authentication() {
        let (mut state, deployment_id, _tmp) = state().await;
        let clock = Arc::new(AtomicI64::new(1_800_000_000));
        let credentials = credentials("manager-a", clock);
        let bearer = credentials
            .issue(
                grant(&deployment_id),
                Utc.timestamp_opt(1_800_003_600, 0).unwrap(),
            )
            .unwrap()
            .token;
        state.auth_validator = Arc::new(RuntimeAuthValidator::new(
            credentials,
            Arc::new(NoLegacyFallback),
        ));
        let (payload, signature) = bearer.split_once('.').unwrap();
        let forged = format!("{payload}A.{signature}");
        assert_eq!(logs(&state, &forged).await, StatusCode::UNAUTHORIZED);
        assert_eq!(
            logs(&state, "ax_cap_v2_unknown").await,
            StatusCode::UNAUTHORIZED
        );
        let basic = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            format!("runtime:{forged}"),
        );
        let response = alien_manager::routes::telemetry::ingest_logs(
            axum::extract::State(state),
            [(
                header::AUTHORIZATION,
                format!("Basic {basic}").parse().unwrap(),
            )]
            .into_iter()
            .collect(),
            bytes::Bytes::new(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn runtime_purposes_cannot_read_release_mint_tokens_or_delete_deployment() {
        let (mut state, deployment_id, _tmp) = state().await;
        let clock = Arc::new(AtomicI64::new(1_800_000_000));
        let credentials = credentials("manager-a", clock);
        let deadline = Utc.timestamp_opt(1_800_003_600, 0).unwrap();
        let release = state
            .release_store
            .create_release(
                &Subject::system(),
                CreateReleaseParams {
                    project_id: "default".into(),
                    stacks: HashMap::from([(
                        Platform::Kubernetes,
                        Stack::new("test".to_owned()).build(),
                    )]),
                    git_commit_sha: None,
                    git_commit_ref: None,
                    git_commit_message: None,
                },
            )
            .await
            .unwrap();
        state.auth_validator = Arc::new(RuntimeAuthValidator::new(
            credentials.clone(),
            state.auth_validator,
        ));
        let router = alien_manager::routes::releases::router()
            .merge(alien_manager::routes::tokens::router())
            .merge(alien_manager::routes::vault::router())
            .merge(alien_manager::routes::bindings::router())
            .merge(alien_manager::routes::deployments::router())
            .with_state(state.clone());
        for purpose in [
            RuntimePurpose::OperatorSync,
            RuntimePurpose::RegistryPull,
            RuntimePurpose::OperatorTunnel,
            RuntimePurpose::OperatorOperations,
            RuntimePurpose::OperationRelay {
                target: alien_core::CommandTarget::new(
                    "worker-a",
                    alien_core::CommandTargetType::Worker,
                ),
            },
            RuntimePurpose::Telemetry,
            RuntimePurpose::ReceiveCommands {
                target: alien_core::CommandTarget::new(
                    "worker-a",
                    alien_core::CommandTargetType::Worker,
                ),
            },
        ] {
            let mut grant = grant(&deployment_id);
            grant.purpose = purpose;
            let bearer = credentials.issue(grant, deadline).unwrap().token;
            let bindings_body =
                json!({"deploymentId": deployment_id, "resourceId": "storage"}).to_string();
            for (method, path, body) in [
                (
                    "POST",
                    "/v1/bindings/resolve".into(),
                    bindings_body.as_str(),
                ),
                ("GET", format!("/v1/releases/{}", release.id), ""),
                (
                    "GET",
                    format!("/v1/deployments/{deployment_id}/vault/secrets/secrets/TEST_KEY"),
                    "",
                ),
                ("POST", "/v1/tokens".into(), r#"{"type":"tunnel"}"#),
                (
                    "POST",
                    format!("/v1/deployments/{deployment_id}/delete"),
                    r#"{"action":"retain"}"#,
                ),
            ] {
                let response = router
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method(method)
                            .uri(&path)
                            .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
                            .header(header::CONTENT_TYPE, "application/json")
                            .body(Body::from(body.to_owned()))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {path}");
            }
        }
        let deployment = state
            .deployment_store
            .get_deployment(&Subject::system(), &deployment_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(deployment.status, "running");
        assert!(state.token_store.list_tokens().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn receiver_authority_expires_with_parent_and_fresh_receiver_can_refresh() {
        let (mut state, deployment_id, tmp) = state().await;
        let clock = Arc::new(AtomicI64::new(Utc::now().timestamp()));
        let server_clock = clock.clone();
        let registry = Arc::new(InMemoryCommandRegistry::new());
        registry
            .register_target("worker-a", alien_core::CommandTargetType::Worker)
            .await
            .unwrap();
        state.command_server = Arc::new(
            CommandServer::new(
                state.kv.clone(),
                Arc::new(
                    LocalStorage::new(tmp.path().join("commands").to_string_lossy().into_owned())
                        .unwrap(),
                ),
                Arc::new(NullCommandDispatcher),
                registry,
                "http://localhost:0/v1".to_owned(),
                b"test-signing-key".to_vec(),
            )
            .with_clock(Arc::new(move || {
                Utc.timestamp_opt(server_clock.load(Ordering::SeqCst), 0)
                    .unwrap()
            })),
        );
        let created = state
            .command_server
            .create_command(alien_core::CreateCommandRequest {
                deployment_id: deployment_id.clone(),
                command: "echo".to_owned(),
                params: alien_core::BodySpec::inline(b"hello"),
                deadline: None,
                idempotency_key: None,
                target_resource_id: Some("worker-a".to_owned()),
                operation_result_contract: None,
            })
            .await
            .unwrap();
        let issuer = credentials("manager-a", clock.clone());
        let mut authority = grant(&deployment_id);
        authority.purpose = RuntimePurpose::ReceiveCommands {
            target: alien_core::CommandTarget::new(
                "worker-a",
                alien_core::CommandTargetType::Worker,
            ),
        };
        let deadline = Utc
            .timestamp_opt(clock.load(Ordering::SeqCst) + 30, 0)
            .unwrap();
        let issued = issuer.issue(authority.clone(), deadline).unwrap();
        state.auth_validator = Arc::new(RuntimeAuthValidator::new(
            issuer.clone(),
            state.auth_validator,
        ));
        let router = alien_manager::routes::commands::router().with_state(state.clone());
        let response = router.clone().oneshot(Request::builder().method("POST").uri("/v1/commands/leases")
            .header(header::AUTHORIZATION, format!("Bearer {}", issued.token))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&json!({"deploymentId": deployment_id, "target": {"resourceId": "worker-a", "resourceType": "worker"}, "maxLeases": 1, "leaseSeconds": 60})).unwrap())).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let leases: alien_core::LeaseResponse =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
        assert_eq!(leases.leases.len(), 1);
        let callback = &leases.leases[0]
            .envelope
            .response_handling
            .submit_response_url;
        let base = url::Url::parse("http://localhost/v1/commands/leases").unwrap();
        let callback = base.join(callback).unwrap();
        assert!(
            leases.leases[0]
                .envelope
                .response_handling
                .storage_upload_request
                .expiration
                <= leases.leases[0].lease_expires_at
        );
        let callback_expiry = callback
            .query_pairs()
            .find(|(key, _)| key == "expires")
            .unwrap()
            .1
            .parse::<i64>()
            .unwrap();
        // The result credential follows the lease, not the receiver's
        // shorter credential.
        assert_eq!(
            callback_expiry,
            leases.leases[0].lease_expires_at.timestamp()
        );
        clock.store(deadline.timestamp(), Ordering::SeqCst);
        assert_eq!(
            state
                .command_server
                .get_command_status_record(&created.command_id)
                .await
                .unwrap()
                .unwrap()
                .state,
            alien_core::CommandState::Dispatched
        );
        let context_path = format!(
            "/v1/commands/{}/response-context?leaseId={}",
            created.command_id, leases.leases[0].lease_id
        );
        let old_context = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&context_path)
                    .header(header::AUTHORIZATION, format!("Bearer {}", issued.token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(old_context.status(), StatusCode::UNAUTHORIZED);
        let fresh_deadline = deadline + chrono::Duration::seconds(60);
        let mut wrong = authority.clone();
        wrong.purpose = RuntimePurpose::ReceiveCommands {
            target: alien_core::CommandTarget::new(
                "worker-b",
                alien_core::CommandTargetType::Worker,
            ),
        };
        let wrong = issuer.issue(wrong, fresh_deadline).unwrap();
        let denied = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&context_path)
                    .header(header::AUTHORIZATION, format!("Bearer {}", wrong.token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        let fresh = issuer.issue(authority, fresh_deadline).unwrap();
        let wrong_lease = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/v1/commands/{}/response-context?leaseId=lease_wrong",
                        created.command_id
                    ))
                    .header(header::AUTHORIZATION, format!("Bearer {}", fresh.token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(wrong_lease.status(), StatusCode::NOT_FOUND);
        let refreshed = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&context_path)
                    .header(header::AUTHORIZATION, format!("Bearer {}", fresh.token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(refreshed.status(), StatusCode::OK);
        let handling: alien_core::ResponseHandling =
            serde_json::from_slice(&to_bytes(refreshed.into_body(), 65536).await.unwrap()).unwrap();
        assert!(handling.storage_upload_request.expiration <= fresh_deadline);
        assert!(handling.storage_upload_request.expiration <= leases.leases[0].lease_expires_at);
        let callback = url::Url::parse(&handling.submit_response_url).unwrap();
        let path = format!("{}?{}", callback.path(), callback.query().unwrap());
        let completed = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(path)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&alien_core::SubmitResponseRequest {
                            response: alien_core::CommandResponse::success(b"fresh authority"),
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(completed.status(), StatusCode::OK);
        assert_eq!(
            state
                .command_server
                .get_command_status_record(&created.command_id)
                .await
                .unwrap()
                .unwrap()
                .state,
            alien_core::CommandState::Succeeded
        );
    }

    #[tokio::test]
    async fn long_command_still_completes_after_the_relay_capability_expires() {
        let (mut state, deployment_id, tmp) = state().await;
        let clock = Arc::new(AtomicI64::new(Utc::now().timestamp()));
        let server_clock = clock.clone();
        let registry = Arc::new(InMemoryCommandRegistry::new());
        registry
            .register_target("worker-a", alien_core::CommandTargetType::Worker)
            .await
            .unwrap();
        state.command_server = Arc::new(
            CommandServer::new(
                state.kv.clone(),
                Arc::new(
                    LocalStorage::new(tmp.path().join("commands").to_string_lossy().into_owned())
                        .unwrap(),
                ),
                Arc::new(NullCommandDispatcher),
                registry,
                "http://localhost:0/v1".to_owned(),
                b"test-signing-key".to_vec(),
            )
            .with_clock(Arc::new(move || {
                Utc.timestamp_opt(server_clock.load(Ordering::SeqCst), 0)
                    .unwrap()
            })),
        );
        let mut command_ids = Vec::new();
        for _ in 0..2 {
            command_ids.push(
                state
                    .command_server
                    .create_command(alien_core::CreateCommandRequest {
                        deployment_id: deployment_id.clone(),
                        command: "echo".to_owned(),
                        params: alien_core::BodySpec::inline(b"hello"),
                        deadline: None,
                        idempotency_key: None,
                        target_resource_id: Some("worker-a".to_owned()),
                        operation_result_contract: None,
                    })
                    .await
                    .unwrap()
                    .command_id,
            );
        }
        // The operator's relay capability lasts five minutes. The command it
        // leases may run for an hour.
        let issuer = credentials("manager-a", clock.clone());
        let mut authority = grant(&deployment_id);
        authority.purpose = RuntimePurpose::ReceiveCommands {
            target: alien_core::CommandTarget::new(
                "worker-a",
                alien_core::CommandTargetType::Worker,
            ),
        };
        let relay_deadline = Utc
            .timestamp_opt(clock.load(Ordering::SeqCst) + 300, 0)
            .unwrap();
        let issued = issuer.issue(authority, relay_deadline).unwrap();
        state.auth_validator = Arc::new(RuntimeAuthValidator::new(issuer, state.auth_validator));
        let router = alien_manager::routes::commands::router().with_state(state.clone());
        let response = router.clone().oneshot(Request::builder().method("POST").uri("/v1/commands/leases")
            .header(header::AUTHORIZATION, format!("Bearer {}", issued.token))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&json!({"deploymentId": deployment_id, "target": {"resourceId": "worker-a", "resourceType": "worker"}, "maxLeases": 2, "leaseSeconds": 3660})).unwrap())).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let leases: alien_core::LeaseResponse =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
        assert_eq!(leases.leases.len(), 2);
        let base = url::Url::parse("http://localhost/v1/commands/leases").unwrap();
        let submit_path = |lease: &alien_core::LeaseInfo| {
            let callback = base
                .join(&lease.envelope.response_handling.submit_response_url)
                .unwrap();
            format!("{}?{}", callback.path(), callback.query().unwrap())
        };
        let submit = |path: String, body: &'static [u8]| {
            let router = router.clone();
            async move {
                router
                    .oneshot(
                        Request::builder()
                            .method("PUT")
                            .uri(path)
                            .header(header::CONTENT_TYPE, "application/json")
                            .body(Body::from(
                                serde_json::to_vec(&alien_core::SubmitResponseRequest {
                                    response: alien_core::CommandResponse::success(body),
                                })
                                .unwrap(),
                            ))
                            .unwrap(),
                    )
                    .await
                    .unwrap()
                    .status()
            }
        };

        // Ten minutes in: the relay capability is long gone.
        clock.fetch_add(600, Ordering::SeqCst);
        assert_eq!(
            submit(submit_path(&leases.leases[0]), b"ten minutes").await,
            StatusCode::OK,
            "a command still inside its lease must be able to report its result"
        );
        assert_eq!(
            state
                .command_server
                .get_command_status_record(&leases.leases[0].command_id)
                .await
                .unwrap()
                .unwrap()
                .state,
            alien_core::CommandState::Succeeded
        );

        // The result credential still ends with the lease.
        clock.store(
            leases.leases[1].lease_expires_at.timestamp(),
            Ordering::SeqCst,
        );
        assert_eq!(
            submit(submit_path(&leases.leases[1]), b"too late").await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn command_receiver_is_bound_to_target_deployment_and_five_minutes() {
        async fn request(
            router: &axum::Router,
            bearer: &str,
            method: &str,
            path: &str,
            body: Value,
        ) -> axum::response::Response {
            router
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap()
        }

        let (mut state, deployment_id, tmp) = state().await;
        let registry = Arc::new(InMemoryCommandRegistry::new());
        registry
            .register_target("worker-a", alien_core::CommandTargetType::Worker)
            .await
            .unwrap();
        state.command_server = Arc::new(CommandServer::new(
            state.kv.clone(),
            Arc::new(
                LocalStorage::new(tmp.path().join("commands").to_string_lossy().into_owned())
                    .unwrap(),
            ),
            Arc::new(NullCommandDispatcher),
            registry,
            "http://localhost:0/v1".to_owned(),
            b"test-signing-key".to_vec(),
        ));
        let created = state
            .command_server
            .create_command(alien_core::CreateCommandRequest {
                deployment_id: deployment_id.clone(),
                command: "echo".to_owned(),
                params: alien_core::BodySpec::inline(b"hello"),
                deadline: None,
                idempotency_key: None,
                target_resource_id: Some("worker-a".to_owned()),
                operation_result_contract: None,
            })
            .await
            .unwrap();
        assert_eq!(created.state, alien_core::CommandState::Pending);
        let clock = Arc::new(AtomicI64::new(1_800_000_000));
        let credentials = credentials("manager-a", clock.clone());
        let receiver = |deployment: &str, resource: &str| {
            let mut grant = grant(deployment);
            grant.purpose = RuntimePurpose::ReceiveCommands {
                target: alien_core::CommandTarget::new(
                    resource,
                    alien_core::CommandTargetType::Worker,
                ),
            };
            credentials
                .issue(grant, Utc.timestamp_opt(1_800_003_600, 0).unwrap())
                .unwrap()
                .token
        };
        let bearer = receiver(&deployment_id, "worker-a");
        let wrong_target = receiver(&deployment_id, "worker-b");
        let wrong_deployment = receiver("other-deployment", "worker-a");
        let mut typed_grant = grant(&deployment_id);
        typed_grant.purpose = RuntimePurpose::ReceiveCommands {
            target: alien_core::CommandTarget::new(
                "worker-a",
                alien_core::CommandTargetType::Container,
            ),
        };
        let wrong_type = credentials
            .issue(typed_grant, Utc.timestamp_opt(1_800_003_600, 0).unwrap())
            .unwrap()
            .token;
        let mut reserved = grant(&deployment_id);
        reserved.purpose = RuntimePurpose::ReceiveCommands {
            target: alien_core::CommandTarget::new(
                alien_commands::server::command_registry::OPERATOR_COMMAND_TARGET_ID,
                alien_core::CommandTargetType::Daemon,
            ),
        };
        assert!(credentials
            .issue(reserved, Utc.timestamp_opt(1_800_003_600, 0).unwrap())
            .is_err());
        state.auth_validator = Arc::new(RuntimeAuthValidator::new(
            credentials.clone(),
            state.auth_validator.clone(),
        ));
        assert_eq!(logs(&state, &bearer).await, StatusCode::FORBIDDEN);
        let router = alien_manager::routes::commands::router().with_state(state.clone());
        let lease_body = json!({"deploymentId": deployment_id, "target": {
            "resourceId": "worker-a", "resourceType": "worker"
        }, "maxLeases": 1, "leaseSeconds": 60});
        for denied in [&wrong_target, &wrong_deployment, &wrong_type] {
            assert_eq!(
                request(
                    &router,
                    denied,
                    "POST",
                    "/v1/commands/leases",
                    lease_body.clone()
                )
                .await
                .status(),
                StatusCode::FORBIDDEN
            );
        }
        let response = request(
            &router,
            &bearer,
            "POST",
            "/v1/commands/leases",
            lease_body.clone(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let leases: alien_core::LeaseResponse =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
        assert_eq!(leases.leases.len(), 1);
        assert_eq!(leases.leases[0].command_id, created.command_id);
        assert_eq!(leases.leases[0].envelope.command, "echo");
        let release_path = format!("/v1/commands/leases/{}/release", leases.leases[0].lease_id);
        let response_path = format!("/v1/commands/{}/response", created.command_id);
        let result = serde_json::to_value(alien_core::SubmitResponseRequest {
            response: alien_core::CommandResponse::success(b"done"),
        })
        .unwrap();
        for denied in [&wrong_target, &wrong_deployment, &wrong_type] {
            assert_eq!(
                request(&router, denied, "POST", &release_path, json!({}))
                    .await
                    .status(),
                StatusCode::FORBIDDEN
            );
            assert_eq!(
                request(&router, denied, "PUT", &response_path, result.clone())
                    .await
                    .status(),
                StatusCode::FORBIDDEN
            );
        }
        assert_eq!(
            request(&router, &bearer, "POST", &release_path, json!({}))
                .await
                .status(),
            StatusCode::OK
        );
        clock.store(1_800_000_299, Ordering::SeqCst);
        let response = request(
            &router,
            &bearer,
            "POST",
            "/v1/commands/leases",
            lease_body.clone(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let leases: alien_core::LeaseResponse =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
        assert_eq!(leases.leases.len(), 1);
        assert_eq!(leases.leases[0].command_id, created.command_id);
        let release_path = format!("/v1/commands/leases/{}/release", leases.leases[0].lease_id);
        clock.store(1_800_000_300, Ordering::SeqCst);
        assert_eq!(
            request(&router, &bearer, "POST", "/v1/commands/leases", lease_body)
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request(&router, &bearer, "POST", &release_path, json!({}))
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request(&router, &bearer, "PUT", &response_path, result.clone())
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let fresh = receiver(&deployment_id, "worker-a");
        assert_eq!(
            request(&router, &fresh, "PUT", &response_path, result)
                .await
                .status(),
            StatusCode::OK
        );
        let completed = state
            .command_server
            .get_command_status_record(&created.command_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(completed.state, alien_core::CommandState::Succeeded);
    }

    #[tokio::test]
    async fn exchange_refuses_overgrants_and_redacts_provider_errors() {
        use alien_error::ContextError;
        use alien_manager::auth::identity_exchange::{
            IdentityExchangeRequest, RuntimeIdentityVerifier, VerifiedRuntimeIdentity,
        };
        const MARKER: &str = "synthetic-proof-do-not-expose";
        struct FaultyVerifier {
            deployment: String,
            scenario: u8,
        }
        #[async_trait]
        impl RuntimeIdentityVerifier for FaultyVerifier {
            async fn verify(
                &self,
                _: &IdentityExchangeRequest,
                _: &str,
            ) -> Result<VerifiedRuntimeIdentity, AlienError> {
                let mut grant = grant(&self.deployment);
                match self.scenario {
                    0 => grant.deployment_id = "other-deployment".to_owned(),
                    1 => {
                        grant.purpose = RuntimePurpose::ReceiveCommands {
                            target: alien_core::CommandTarget::new(
                                "worker",
                                alien_core::CommandTargetType::Worker,
                            ),
                        }
                    }
                    2 => {
                        return Err(
                            alien_manager::error::ErrorData::unauthorized(MARKER).into_generic()
                        );
                    }
                    3 => {
                        return Err(alien_manager::error::ErrorData::unauthorized(MARKER)
                            .context(alien_manager::error::ErrorData::AuthenticationUnavailable)
                            .into_generic());
                    }
                    _ => (),
                }
                Ok(VerifiedRuntimeIdentity {
                    grant,
                    authority_deadline: Utc.timestamp_opt(1_800_000_000, 0).unwrap(),
                })
            }
        }
        #[derive(Clone)]
        struct Capture(Arc<std::sync::Mutex<Vec<u8>>>);
        impl std::io::Write for Capture {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let captured = Capture(Arc::new(std::sync::Mutex::new(Vec::new())));
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let (state, deployment, _tmp) = state().await;
        for scenario in 0..5 {
            let credentials = credentials("manager-a", Arc::new(AtomicI64::new(1_800_000_000)));
            let router = alien_manager::auth::identity_exchange::router(
                credentials,
                Arc::new(FaultyVerifier {
                    deployment: deployment.clone(),
                    scenario,
                }),
            )
            .with_state(state.clone());
            let response = router.oneshot(Request::builder().method("POST").uri("/v1/identity/exchange")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&json!({"deploymentId": deployment, "purpose": {"type": "telemetry"}, "proof": {"type": "session", "token": MARKER}})).unwrap())).unwrap()).await.unwrap();
            assert_eq!(
                response.status(),
                if scenario == 3 {
                    StatusCode::SERVICE_UNAVAILABLE
                } else {
                    StatusCode::UNAUTHORIZED
                }
            );
            let body = to_bytes(response.into_body(), 16384).await.unwrap();
            assert!(!String::from_utf8_lossy(&body).contains(MARKER));
            let value: Value = serde_json::from_slice(&body).unwrap();
            assert!(value.get("token").is_none());
        }
        assert!(!String::from_utf8_lossy(&captured.0.lock().unwrap()).contains(MARKER));
    }
}
