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
        operator_image_provider: None,
        release_channels: None,
        bundle_signing_key: None,
        bundle_sources: None,
        log_buffer: Arc::new(alien_manager::LogBuffer::new()),
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
