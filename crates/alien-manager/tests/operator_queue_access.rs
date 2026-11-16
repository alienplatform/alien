//! Who may lease, answer and release commands on an Operator's own queue.
//!
//! The Operator's fixed queue is reserved: ordinary command receivers never
//! reach it. An Operator that holds an `operatorOperations` credential for its
//! own deployment may. Every caller below goes through the real commands
//! router, the real `OssAuthz`, real SQLite stores and the real runtime
//! credential validator; nothing about the decision is mocked.

use std::collections::HashMap;
use std::sync::Arc;

use alien_bindings::{providers::kv::local::LocalKv, providers::storage::local::LocalStorage};
use alien_commands::dispatchers::NullCommandDispatcher;
use alien_commands::server::command_registry::{
    OPERATIONS_WORKER_COMMAND_TARGET_ID, OPERATOR_COMMAND_TARGET_ID,
};
use alien_commands::server::{CommandRegistry, CommandServer};
use alien_commands::InMemoryCommandRegistry;
use alien_core::{
    CommandTarget, CommandTargetType, Platform, RuntimeMetadata, StackSettings, StackState,
    CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
};
use alien_error::AlienError;
use alien_manager::auth::identity_exchange::{
    IdentityEnrollmentRequest, IdentityRotationRequest, RuntimeIdentityVerifier,
};
use alien_manager::auth::runtime_capability::{
    RuntimeAuthValidator, RuntimeCredentials, RuntimeGrant, RuntimePurpose,
};
use alien_manager::auth::standalone_identity::StandaloneRuntimeIdentity;
use alien_manager::auth::{CommandCapability, Role, Scope, Subject, SubjectKind};
use alien_manager::config::ManagerConfig;
use alien_manager::providers::OssAuthz;
use alien_manager::routes::registry_proxy::{
    CredentialCache, PullValidationCache, RegistryRoutingTable,
};
use alien_manager::routes::AppState;
use alien_manager::stores::sqlite::{
    SqliteCommandRegistry, SqliteDatabase, SqliteDeploymentStore, SqliteReleaseStore,
    SqliteRuntimeIdentityStore, SqliteTokenStore,
};
use alien_manager::traits::{
    AuthValidator, CreateDeploymentGroupParams, CreateImportedDeploymentParams, CredentialResolver,
    DeploymentRecord, DeploymentStore, ReleaseStore, TokenStore, TokenType,
};
use async_trait::async_trait;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use http::header;
use serde_json::{json, Value};
use tower::ServiceExt;

const WORKSPACE: &str = "default";
const AUDIENCE: &str = "https://manager.example.test";

struct TokenValidator(HashMap<String, Subject>);

#[async_trait]
impl AuthValidator for TokenValidator {
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
    async fn management_external_id(
        &self,
        _: &DeploymentRecord,
    ) -> Result<Option<alien_core::ManagementExternalId>, AlienError> {
        Ok(None)
    }

    async fn resolve(
        &self,
        _deployment: &DeploymentRecord,
    ) -> Result<alien_core::ClientConfig, AlienError> {
        unreachable!("these routes resolve no credentials")
    }
}

/// The subject the real token validator builds for a raw token.
async fn validated(
    validator: &alien_manager::providers::token_db_validator::TokenDbValidator,
    raw: &str,
) -> Subject {
    let mut headers = http::HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        format!("Bearer {raw}").parse().unwrap(),
    );
    validator.validate(&headers).await.unwrap().unwrap()
}

fn legacy(workspace: &str, scope: Scope, role: Role, user: bool) -> Subject {
    Subject {
        kind: if user {
            SubjectKind::User {
                id: "user-1".to_owned(),
                email: "user@example.test".to_owned(),
                workspace_name: None,
            }
        } else {
            SubjectKind::ServiceAccount {
                id: "key-1".to_owned(),
            }
        },
        workspace_id: workspace.to_owned(),
        scope,
        role,
        bearer_token: String::new(),
    }
}

fn confused_targets() -> Vec<(&'static str, CommandTargetType)> {
    vec![
        (OPERATOR_COMMAND_TARGET_ID, CommandTargetType::Container),
        (OPERATOR_COMMAND_TARGET_ID, CommandTargetType::Worker),
        (
            OPERATIONS_WORKER_COMMAND_TARGET_ID,
            CommandTargetType::Worker,
        ),
        (
            OPERATIONS_WORKER_COMMAND_TARGET_ID,
            CommandTargetType::Daemon,
        ),
    ]
}

fn confused_name(id: &str, resource_type: CommandTargetType) -> String {
    format!("confused-{id}-{resource_type:?}")
}

fn operator_queue() -> CommandTarget {
    CommandTarget::new(OPERATOR_COMMAND_TARGET_ID, CommandTargetType::Daemon)
}

struct Fixture {
    state: AppState,
    dep_a: String,
    dep_b: String,
    /// Another deployment in deployment A's group.
    dep_a2: String,
    project: String,
    credentials: Arc<RuntimeCredentials>,
    identity: Arc<StandaloneRuntimeIdentity>,
    _tmp: tempfile::TempDir,
}

/// A manager whose commands live in the real SQLite stores, so the owner
/// lookup behind answer and release is the production one.
async fn fixture() -> Fixture {
    fixture_with(true).await
}

/// The same manager with an in-memory registry that knows a stack target.
async fn memory_fixture() -> Fixture {
    fixture_with(false).await
}

async fn fixture_with(sqlite_registry: bool) -> Fixture {
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

    let mut ids = Vec::new();
    for name in ["group-a", "group-b"] {
        let group = deployment_store
            .create_deployment_group(
                &Subject::system(),
                CreateDeploymentGroupParams {
                    name: name.to_owned(),
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
                    name: format!("deployment-{name}"),
                    deployment_group_id: group.id.clone(),
                    platform: Platform::Kubernetes,
                    base_platform: None,
                    stack_settings: StackSettings::default(),
                    stack_state: StackState::new(Platform::Kubernetes),
                    environment_info: None,
                    runtime_metadata: RuntimeMetadata::default(),
                    status: "running".to_owned(),
                    current_release_id: None,
                    desired_release_id: None,
                    import_source: None,
                    setup_metadata: None,
                    setup_target: "test".to_owned(),
                    setup_fingerprint: "test".to_owned(),
                    setup_fingerprint_version: 1,
                    input_values: Default::default(),
                    deployment_token: None,
                    management_config: None,
                },
            )
            .await
            .unwrap();
        ids.push((group.id, deployment));
    }
    // A second deployment in the first group.
    let group_a_id = ids[0].0.clone();
    let second_in_group = deployment_store
        .create_with_state(
            &Subject::system(),
            CreateImportedDeploymentParams {
                deployment_protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                name: "deployment-group-a-second".to_owned(),
                deployment_group_id: group_a_id,
                platform: Platform::Kubernetes,
                base_platform: None,
                stack_settings: StackSettings::default(),
                stack_state: StackState::new(Platform::Kubernetes),
                environment_info: None,
                runtime_metadata: RuntimeMetadata::default(),
                status: "running".to_owned(),
                current_release_id: None,
                desired_release_id: None,
                import_source: None,
                setup_metadata: None,
                setup_target: "test".to_owned(),
                setup_fingerprint: "test".to_owned(),
                setup_fingerprint_version: 1,
                input_values: Default::default(),
                deployment_token: None,
                management_config: None,
            },
        )
        .await
        .unwrap();
    let (group_a, dep_a) = ids.remove(0);
    let (group_b, dep_b) = ids.remove(0);
    let project = dep_a.project_id.clone();

    let mut legacy_tokens = HashMap::new();
    let mut add = |name: &str, subject: Subject| {
        legacy_tokens.insert(name.to_owned(), subject);
    };
    let deployment_scope = |id: &str| Scope::Deployment {
        project_id: project.clone(),
        deployment_id: id.to_owned(),
    };
    add(
        "workspace-key",
        legacy(WORKSPACE, Scope::Workspace, Role::WorkspaceAdmin, false),
    );
    add(
        "workspace-user",
        legacy(WORKSPACE, Scope::Workspace, Role::WorkspaceMember, true),
    );
    add(
        "project-developer",
        legacy(
            WORKSPACE,
            Scope::Project {
                project_id: project.clone(),
            },
            Role::ProjectDeveloper,
            true,
        ),
    );
    add(
        "project-viewer",
        legacy(
            WORKSPACE,
            Scope::Project {
                project_id: project.clone(),
            },
            Role::ProjectViewer,
            true,
        ),
    );
    add(
        "group-a-token",
        legacy(
            WORKSPACE,
            Scope::DeploymentGroup {
                project_id: project.clone(),
                deployment_group_id: group_a.clone(),
            },
            Role::DeploymentGroupDeployer,
            false,
        ),
    );
    add(
        "group-b-token",
        legacy(
            WORKSPACE,
            Scope::DeploymentGroup {
                project_id: project.clone(),
                deployment_group_id: group_b.clone(),
            },
            Role::DeploymentGroupDeployer,
            false,
        ),
    );
    add(
        "deployment-a-token",
        legacy(
            WORKSPACE,
            deployment_scope(&dep_a.id),
            Role::DeploymentManager,
            false,
        ),
    );
    add(
        "deployment-a2-token",
        legacy(
            WORKSPACE,
            deployment_scope(&second_in_group.id),
            Role::DeploymentManager,
            false,
        ),
    );
    add(
        "deployment-b-token",
        legacy(
            WORKSPACE,
            deployment_scope(&dep_b.id),
            Role::DeploymentManager,
            false,
        ),
    );
    add(
        "workspace-viewer",
        legacy(WORKSPACE, Scope::Workspace, Role::WorkspaceViewer, true),
    );
    // Setup, setup-import and deployment tokens go through the real token
    // validator, so the subject is exactly what the manager builds from a
    // stored token.
    let real_validator =
        alien_manager::providers::token_db_validator::TokenDbValidator::new(token_store.clone());
    for (name, token_type, raw) in [
        ("setup-token", TokenType::Setup, "ax_setup_0123456789abcdef"),
        (
            "setup-import-token",
            TokenType::SetupImport,
            "ax_setup_fedcba9876543210",
        ),
    ] {
        token_store
            .create_setup_token(
                token_type,
                &dep_a.id,
                "ax_setup_",
                &key_hash(raw),
                chrono::Utc::now() + chrono::Duration::hours(1),
            )
            .await
            .unwrap();
        add(name, validated(&real_validator, raw).await);
    }
    token_store
        .create_token(alien_manager::traits::CreateTokenParams {
            token_type: TokenType::Deployment,
            key_prefix: "ax_deploy_".to_owned(),
            key_hash: key_hash("ax_deploy_0123456789abcdef"),
            deployment_group_id: None,
            deployment_id: Some(dep_a.id.clone()),
        })
        .await
        .unwrap();
    add(
        "deployment-a-real-token",
        validated(&real_validator, "ax_deploy_0123456789abcdef").await,
    );
    add(
        "deployment-a-viewer",
        legacy(
            WORKSPACE,
            deployment_scope(&dep_a.id),
            Role::DeploymentViewer,
            false,
        ),
    );
    add(
        "commands-key",
        legacy(
            WORKSPACE,
            deployment_scope(&dep_a.id),
            Role::CommandsReceiver,
            false,
        ),
    );
    add(
        "other-workspace-key",
        legacy("ws_b", Scope::Workspace, Role::WorkspaceAdmin, false),
    );
    add(
        "sender-operations-capability",
        legacy(
            WORKSPACE,
            Scope::Commands {
                project_id: project.clone(),
                deployment_id: dep_a.id.clone(),
                capability: CommandCapability::Operations {
                    command: "echo".to_owned(),
                },
            },
            Role::CommandCapability,
            false,
        ),
    );

    // Capabilities that agree with the request on a reserved id but not on
    // the Operator queue's type. The issuer refuses to sign these; the
    // policy must refuse them on its own.
    for (id, resource_type) in confused_targets() {
        add(
            &confused_name(id, resource_type),
            legacy(
                WORKSPACE,
                Scope::Commands {
                    project_id: project.clone(),
                    deployment_id: dep_a.id.clone(),
                    capability: CommandCapability::Receive {
                        target: CommandTarget::new(id, resource_type),
                    },
                },
                Role::CommandCapability,
                false,
            ),
        );
    }

    let credentials = Arc::new(RuntimeCredentials::new(AUDIENCE.to_owned(), [7u8; 32]).unwrap());

    let kv: Arc<dyn alien_bindings::traits::Kv> =
        Arc::new(LocalKv::new(tmp.path().join("kv")).await.unwrap());
    let storage: Arc<dyn alien_bindings::traits::Storage> = Arc::new(
        LocalStorage::new(tmp.path().join("storage").to_string_lossy().to_string()).unwrap(),
    );
    let registry: Arc<dyn CommandRegistry> = if sqlite_registry {
        Arc::new(SqliteCommandRegistry::new(
            db.clone(),
            deployment_store.clone(),
            release_store.clone(),
        ))
    } else {
        let registry = Arc::new(InMemoryCommandRegistry::new());
        registry
            .register_target(OPERATOR_COMMAND_TARGET_ID, CommandTargetType::Daemon)
            .await
            .unwrap();
        registry
            .register_target("daemon-a", CommandTargetType::Daemon)
            .await
            .unwrap();
        registry
            .register_target(
                OPERATIONS_WORKER_COMMAND_TARGET_ID,
                CommandTargetType::Worker,
            )
            .await
            .unwrap();
        registry
    };

    let identity = Arc::new(
        StandaloneRuntimeIdentity::new(
            Arc::new(SqliteRuntimeIdentityStore::new(db.clone())),
            deployment_store.clone(),
            Arc::new(NoCredentials),
        )
        .unwrap(),
    );

    let state = AppState {
        deployment_store,
        release_store,
        project_store: Arc::new(alien_manager::providers::SingleWorkspaceProjects),
        token_store,
        auth_validator: Arc::new(RuntimeAuthValidator::new(
            credentials.clone(),
            Arc::new(TokenValidator(legacy_tokens.clone())),
        )),
        authz: Arc::new(OssAuthz),
        telemetry_backend: Arc::new(alien_manager::providers::NullTelemetryBackend),
        credential_resolver: Arc::new(NoCredentials),
        command_server: Arc::new(CommandServer::new(
            kv.clone(),
            storage,
            Arc::new(NullCommandDispatcher),
            registry,
            "http://localhost:0/v1".to_owned(),
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
        extension_bundles: None,
        charts: None,
        runtime_identity: None,
        release_channels: None,
        signing: None,
        bundle_signing_key: None,
        bundle_sources: None,
        log_buffer: Arc::new(alien_manager::LogBuffer::new()),
        values: None,
        value_access: None,
        verify_limiter: Arc::new(alien_manager::values::VerifyLimiter::default()),
    };
    Fixture {
        state,
        dep_a: dep_a.id,
        dep_b: dep_b.id,
        dep_a2: second_in_group.id,
        project,
        credentials,
        identity,
        _tmp: tmp,
    }
}

impl Fixture {
    fn runtime(&self, workspace: &str, deployment: &str, purpose: RuntimePurpose) -> String {
        self.runtime_in_project(workspace, &self.project, deployment, purpose)
    }

    fn runtime_in_project(
        &self,
        workspace: &str,
        project: &str,
        deployment: &str,
        purpose: RuntimePurpose,
    ) -> String {
        self.credentials
            .issue(
                RuntimeGrant {
                    workspace_id: workspace.to_owned(),
                    project_id: project.to_owned(),
                    deployment_id: deployment.to_owned(),
                    component_id: "component-1".to_owned(),
                    purpose,
                },
                chrono::Utc::now() + chrono::Duration::hours(1),
            )
            .unwrap()
            .token
    }

    /// Every caller type, by name, with whether it may use deployment A's
    /// Operator queue.
    fn callers(&self) -> Vec<(&'static str, String, bool)> {
        let legacy = |name: &str| name.to_owned();
        let stack_target = CommandTarget::new("daemon-a", CommandTargetType::Daemon);
        let worker_target = CommandTarget::new("worker-a", CommandTargetType::Worker);
        vec![
            ("workspace key", legacy("workspace-key"), false),
            ("workspace viewer", legacy("workspace-viewer"), false),
            ("setup token, own deployment", legacy("setup-token"), false),
            (
                "setup-import token, own deployment",
                legacy("setup-import-token"),
                false,
            ),
            (
                "deployment token (real validator), own deployment",
                legacy("deployment-a-real-token"),
                true,
            ),
            ("workspace user", legacy("workspace-user"), false),
            ("project developer", legacy("project-developer"), false),
            ("project viewer", legacy("project-viewer"), false),
            ("group token, own group", legacy("group-a-token"), false),
            ("group token, other group", legacy("group-b-token"), false),
            (
                "deployment token, own deployment",
                legacy("deployment-a-token"),
                true,
            ),
            (
                "deployment token, other deployment",
                legacy("deployment-b-token"),
                false,
            ),
            (
                "deployment viewer, own deployment",
                legacy("deployment-a-viewer"),
                false,
            ),
            ("commands-only key", legacy("commands-key"), false),
            ("other workspace key", legacy("other-workspace-key"), false),
            (
                "sender operations capability",
                legacy("sender-operations-capability"),
                false,
            ),
            (
                "telemetry credential",
                self.runtime(WORKSPACE, &self.dep_a, RuntimePurpose::Telemetry),
                false,
            ),
            (
                "operatorSync credential",
                self.runtime(WORKSPACE, &self.dep_a, RuntimePurpose::OperatorSync),
                false,
            ),
            (
                "registryPull credential",
                self.runtime(WORKSPACE, &self.dep_a, RuntimePurpose::RegistryPull),
                false,
            ),
            (
                "receiveCommands, stack target",
                self.runtime(
                    WORKSPACE,
                    &self.dep_a,
                    RuntimePurpose::ReceiveCommands {
                        target: stack_target,
                    },
                ),
                false,
            ),
            (
                "operationRelay, stack worker",
                self.runtime(
                    WORKSPACE,
                    &self.dep_a,
                    RuntimePurpose::OperationRelay {
                        target: worker_target,
                    },
                ),
                false,
            ),
            (
                "operatorOperations, own deployment",
                self.runtime(WORKSPACE, &self.dep_a, RuntimePurpose::OperatorOperations),
                true,
            ),
            (
                "operatorOperations, other deployment",
                self.runtime(WORKSPACE, &self.dep_b, RuntimePurpose::OperatorOperations),
                false,
            ),
            (
                "operatorOperations, other project",
                self.runtime_in_project(
                    WORKSPACE,
                    "prj_other",
                    &self.dep_a,
                    RuntimePurpose::OperatorOperations,
                ),
                false,
            ),
            (
                "operatorOperations, other workspace",
                self.runtime("ws_b", &self.dep_a, RuntimePurpose::OperatorOperations),
                false,
            ),
        ]
    }

    fn router(&self) -> axum::Router {
        alien_manager::routes::commands::router().with_state(self.state.clone())
    }

    async fn queue_command(&self, deployment: &str) -> String {
        self.queue_command_for(deployment, OPERATOR_COMMAND_TARGET_ID)
            .await
    }

    async fn queue_command_for(&self, deployment: &str, target_id: &str) -> String {
        self.state
            .command_server
            .create_command(alien_core::CreateCommandRequest {
                deployment_id: deployment.to_owned(),
                command: "echo".to_owned(),
                params: alien_core::BodySpec::inline(b"hello"),
                deadline: None,
                idempotency_key: None,
                target_resource_id: Some(target_id.to_owned()),
                operation_result_contract: None,
            })
            .await
            .unwrap()
            .command_id
    }

    async fn lease_directly(&self, deployment: &str) -> alien_core::LeaseInfo {
        self.lease_target(deployment, &operator_queue()).await
    }

    async fn lease_target(
        &self,
        deployment: &str,
        target: &CommandTarget,
    ) -> alien_core::LeaseInfo {
        let mut leased = self
            .state
            .command_server
            .acquire_lease(
                deployment,
                &serde_json::from_value(lease_body(deployment, target)).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(leased.leases.len(), 1);
        leased.leases.remove(0)
    }

    async fn state_of(&self, command_id: &str) -> alien_core::CommandState {
        self.state
            .command_server
            .get_command_status_record(command_id)
            .await
            .unwrap()
            .unwrap()
            .state
    }
}

fn lease_body(deployment: &str, target: &CommandTarget) -> Value {
    json!({
        "deploymentId": deployment,
        "target": {"resourceId": target.resource_id, "resourceType": target.resource_type},
        "maxLeases": 1,
        "leaseSeconds": 60
    })
}

async fn send(
    router: &axum::Router,
    bearer: &str,
    method: &str,
    path: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = router
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
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// Each case gets a fresh manager so no earlier caller's lease is in the way.
async fn case(index: usize) -> (Fixture, &'static str, String, bool) {
    let fixture = fixture().await;
    let (name, bearer, allowed) = fixture.callers().remove(index);
    (fixture, name, bearer, allowed)
}

async fn case_count() -> usize {
    fixture().await.callers().len()
}

#[tokio::test]
async fn leasing_the_operator_queue_is_limited_to_its_own_credential_and_the_existing_roles() {
    for index in 0..case_count().await {
        let (fixture, name, bearer, allowed) = case(index).await;
        // A lease request names a deployment and a target, not a workspace, and
        // is decided from the credential before the deployment is loaded, for
        // stack receivers as well. Credentials are minted from the deployment's
        // recorded owner, so a workspace that does not match cannot be issued;
        // answering and releasing, which carry the command, compare it.
        if name == "operatorOperations, other workspace"
            || name == "operatorOperations, other project"
        {
            continue;
        }
        let router = fixture.router();
        let command_id = fixture.queue_command(&fixture.dep_a).await;
        let before = fixture.state_of(&command_id).await;
        let (status, body) = send(
            &router,
            &bearer,
            "POST",
            "/v1/commands/leases",
            lease_body(&fixture.dep_a, &operator_queue()),
        )
        .await;
        if allowed {
            assert_eq!(status, StatusCode::OK, "{name}: {body}");
            assert_eq!(
                body["leases"].as_array().map(Vec::len),
                Some(1),
                "{name} should lease its command"
            );
        } else {
            assert_eq!(status, StatusCode::FORBIDDEN, "{name}: {body}");
            assert_eq!(
                fixture.state_of(&command_id).await,
                before,
                "{name} must not have leased the command"
            );
        }
    }
}

#[tokio::test]
async fn answering_and_releasing_the_operator_queue_is_limited_the_same_way() {
    for index in 0..case_count().await {
        for action in ["respond", "release"] {
            let (fixture, name, bearer, allowed) = case(index).await;
            let router = fixture.router();
            let command_id = fixture.queue_command(&fixture.dep_a).await;
            let lease = fixture.lease_directly(&fixture.dep_a).await;
            let before = fixture.state_of(&command_id).await;
            let (status, body) = if action == "respond" {
                send(
                    &router,
                    &bearer,
                    "PUT",
                    &format!(
                        "/v1/commands/{command_id}/response?lease_id={}",
                        lease.lease_id
                    ),
                    serde_json::to_value(alien_core::SubmitResponseRequest {
                        response: alien_core::CommandResponse::success(b"done"),
                    })
                    .unwrap(),
                )
                .await
            } else {
                send(
                    &router,
                    &bearer,
                    "POST",
                    &format!("/v1/commands/leases/{}/release", lease.lease_id),
                    json!({}),
                )
                .await
            };
            if allowed {
                assert_eq!(status, StatusCode::OK, "{name} {action}: {body}");
            } else {
                assert_eq!(status, StatusCode::FORBIDDEN, "{name} {action}: {body}");
                assert_eq!(
                    fixture.state_of(&command_id).await,
                    before,
                    "{name} {action} must leave the command untouched"
                );
                assert!(
                    fixture
                        .state
                        .command_server
                        .get_lease_owner(&lease.lease_id)
                        .await
                        .unwrap()
                        .is_some(),
                    "{name} {action} must leave the lease in place"
                );
            }
        }
    }
}

#[tokio::test]
async fn the_operator_credential_reaches_only_its_own_deployments_queue() {
    let fixture = fixture().await;
    let router = fixture.router();
    let own = fixture.runtime(
        WORKSPACE,
        &fixture.dep_a,
        RuntimePurpose::OperatorOperations,
    );

    // Naming another deployment in the request is refused before any lookup.
    let other_command = fixture.queue_command(&fixture.dep_b).await;
    let before = fixture.state_of(&other_command).await;
    let (status, _) = send(
        &router,
        &own,
        "POST",
        "/v1/commands/leases",
        lease_body(&fixture.dep_b, &operator_queue()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(fixture.state_of(&other_command).await, before);

    // Another deployment's lease cannot be answered or released with it.
    let lease = fixture.lease_directly(&fixture.dep_b).await;
    let before = fixture.state_of(&other_command).await;
    let (status, _) = send(
        &router,
        &own,
        "POST",
        &format!("/v1/commands/leases/{}/release", lease.lease_id),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send(
        &router,
        &own,
        "PUT",
        &format!(
            "/v1/commands/{other_command}/response?lease_id={}",
            lease.lease_id
        ),
        serde_json::to_value(alien_core::SubmitResponseRequest {
            response: alien_core::CommandResponse::success(b"done"),
        })
        .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(fixture.state_of(&other_command).await, before);
}

#[tokio::test]
async fn the_operator_credential_never_reaches_another_reserved_or_stack_target() {
    let fixture = memory_fixture().await;
    let router = fixture.router();
    let own = fixture.runtime(
        WORKSPACE,
        &fixture.dep_a,
        RuntimePurpose::OperatorOperations,
    );
    let targets = [
        CommandTarget::new(
            OPERATIONS_WORKER_COMMAND_TARGET_ID,
            CommandTargetType::Worker,
        ),
        CommandTarget::new(
            OPERATIONS_WORKER_COMMAND_TARGET_ID,
            CommandTargetType::Daemon,
        ),
        CommandTarget::new(OPERATOR_COMMAND_TARGET_ID, CommandTargetType::Worker),
        CommandTarget::new(OPERATOR_COMMAND_TARGET_ID, CommandTargetType::Container),
        CommandTarget::new("daemon-a", CommandTargetType::Daemon),
    ];
    for target in targets {
        let (status, body) = send(
            &router,
            &own,
            "POST",
            "/v1/commands/leases",
            lease_body(&fixture.dep_a, &target),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{}/{:?}: {body}",
            target.resource_id,
            target.resource_type
        );
    }
}

#[tokio::test]
async fn a_stack_receiver_credential_still_leases_its_own_stack_target() {
    let fixture = memory_fixture().await;
    let router = fixture.router();
    let target = CommandTarget::new("daemon-a", CommandTargetType::Daemon);
    let bearer = fixture.runtime(
        WORKSPACE,
        &fixture.dep_a,
        RuntimePurpose::ReceiveCommands {
            target: target.clone(),
        },
    );
    let (status, _) = send(
        &router,
        &bearer,
        "POST",
        "/v1/commands/leases",
        lease_body(&fixture.dep_a, &target),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn the_operator_credential_cannot_answer_or_release_a_stack_targets_lease() {
    let fixture = memory_fixture().await;
    let router = fixture.router();
    let own = fixture.runtime(
        WORKSPACE,
        &fixture.dep_a,
        RuntimePurpose::OperatorOperations,
    );
    let stack_target = CommandTarget::new("daemon-a", CommandTargetType::Daemon);
    let command_id = fixture
        .state
        .command_server
        .create_command(alien_core::CreateCommandRequest {
            deployment_id: fixture.dep_a.clone(),
            command: "echo".to_owned(),
            params: alien_core::BodySpec::inline(b"hello"),
            deadline: None,
            idempotency_key: None,
            target_resource_id: Some("daemon-a".to_owned()),
            operation_result_contract: None,
        })
        .await
        .unwrap()
        .command_id;
    let mut leased = fixture
        .state
        .command_server
        .acquire_lease(
            &fixture.dep_a,
            &serde_json::from_value(lease_body(&fixture.dep_a, &stack_target)).unwrap(),
        )
        .await
        .unwrap();
    let lease = leased.leases.remove(0);
    let before = fixture.state_of(&command_id).await;

    let (status, _) = send(
        &router,
        &own,
        "PUT",
        &format!(
            "/v1/commands/{command_id}/response?lease_id={}",
            lease.lease_id
        ),
        serde_json::to_value(alien_core::SubmitResponseRequest {
            response: alien_core::CommandResponse::success(b"done"),
        })
        .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send(
        &router,
        &own,
        "POST",
        &format!("/v1/commands/leases/{}/release", lease.lease_id),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(fixture.state_of(&command_id).await, before);
    assert!(fixture
        .state
        .command_server
        .get_lease_owner(&lease.lease_id)
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn the_operator_credential_completes_a_command_it_leased() {
    let fixture = fixture().await;
    let router = fixture.router();
    let own = fixture.runtime(
        WORKSPACE,
        &fixture.dep_a,
        RuntimePurpose::OperatorOperations,
    );
    let command_id = fixture.queue_command(&fixture.dep_a).await;

    let (status, body) = send(
        &router,
        &own,
        "POST",
        "/v1/commands/leases",
        lease_body(&fixture.dep_a, &operator_queue()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let lease_id = body["leases"][0]["leaseId"].as_str().unwrap().to_owned();
    assert_eq!(body["leases"][0]["commandId"], command_id.as_str());

    // A long response asks for its upload target again; that must work.
    let (status, body) = send(
        &router,
        &own,
        "GET",
        &format!("/v1/commands/{command_id}/response-context?leaseId={lease_id}"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, _) = send(
        &router,
        &own,
        "PUT",
        &format!("/v1/commands/{command_id}/response?lease_id={lease_id}"),
        serde_json::to_value(alien_core::SubmitResponseRequest {
            response: alien_core::CommandResponse::success(b"operator-result"),
        })
        .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        fixture.state_of(&command_id).await,
        alien_core::CommandState::Succeeded
    );
    let stored = fixture
        .state
        .command_server
        .get_command_status(&command_id)
        .await
        .unwrap();
    assert!(matches!(
        stored.response,
        Some(alien_core::CommandResponse::Success { .. })
    ));
    // The lease is spent: releasing it afterwards finds nothing to release.
    let (status, _) = send(
        &router,
        &own,
        "POST",
        &format!("/v1/commands/leases/{lease_id}/release"),
        json!({}),
    )
    .await;
    assert_ne!(status, StatusCode::INTERNAL_SERVER_ERROR);
}

/// The credential is a receiver's: it does not read, create, upload or store
/// anything on the commands API beyond leasing, answering and releasing.
#[tokio::test]
async fn the_operator_credential_reaches_no_other_command_route() {
    let fixture = fixture().await;
    let router = fixture.router();
    let own = fixture.runtime(
        WORKSPACE,
        &fixture.dep_a,
        RuntimePurpose::OperatorOperations,
    );
    let command_id = fixture.queue_command(&fixture.dep_a).await;
    let before = fixture.state_of(&command_id).await;

    let create = json!({
        "deploymentId": fixture.dep_a,
        "command": "echo",
        "params": {"mode": "inline", "inlineBase64": "aGVsbG8="},
        "targetResourceId": OPERATOR_COMMAND_TARGET_ID,
    });
    for (method, path, body) in [
        ("POST", "/v1/commands".to_owned(), create),
        ("GET", format!("/v1/commands/{command_id}"), json!({})),
        (
            "GET",
            format!("/v1/commands/{command_id}/payload"),
            json!({}),
        ),
        (
            "PUT",
            format!("/v1/commands/{command_id}/payload"),
            serde_json::to_value(alien_commands::server::StorePayloadRequest {
                params: None,
                response: Some(alien_core::CommandResponse::success(b"stored-directly")),
            })
            .unwrap(),
        ),
        (
            "POST",
            format!("/v1/commands/{command_id}/upload-complete"),
            json!({"size": 1}),
        ),
    ] {
        let (status, body) = send(&router, &own, method, &path, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}: {body}");
    }
    assert_eq!(fixture.state_of(&command_id).await, before);
}

/// A signed capability that agrees with the request on a reserved id, but not
/// on the Operator queue's resource type, is refused by the policy itself: the
/// issuer would not sign it, so these subjects are placed directly.
#[tokio::test]
async fn a_capability_for_a_reserved_id_with_another_resource_type_is_refused() {
    for (id, resource_type) in confused_targets() {
        let name = confused_name(id, resource_type);
        let fixture = fixture().await;
        let router = fixture.router();
        let command_id = fixture.queue_command(&fixture.dep_a).await;
        let before = fixture.state_of(&command_id).await;

        let (status, body) = send(
            &router,
            &name,
            "POST",
            "/v1/commands/leases",
            lease_body(&fixture.dep_a, &CommandTarget::new(id, resource_type)),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{name} lease: {body}");
        assert_eq!(fixture.state_of(&command_id).await, before);

        let lease = fixture.lease_directly(&fixture.dep_a).await;
        let (status, body) = send(
            &router,
            &name,
            "POST",
            &format!("/v1/commands/leases/{}/release", lease.lease_id),
            json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{name} release: {body}");
        let (status, body) = send(
            &router,
            &name,
            "PUT",
            &format!(
                "/v1/commands/{command_id}/response?lease_id={}",
                lease.lease_id
            ),
            serde_json::to_value(alien_core::SubmitResponseRequest {
                response: alien_core::CommandResponse::success(b"done"),
            })
            .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{name} respond: {body}");
        assert!(fixture
            .state
            .command_server
            .get_lease_owner(&lease.lease_id)
            .await
            .unwrap()
            .is_some());
    }
}

fn key_hash(key: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(key.as_bytes()))
}

/// Enroll `key` as the Operator key of `deployment` through the manager's own
/// enrollment path, the way a code install does.
async fn enroll_operator(fixture: &Fixture, deployment: &str, key: &str) {
    let record = fixture
        .state
        .deployment_store
        .get_deployment(&Subject::system(), deployment)
        .await
        .unwrap()
        .unwrap();
    let (code, _) = fixture
        .identity
        .issue_enrollment_code(&record)
        .await
        .unwrap();
    fixture
        .identity
        .enroll(&IdentityEnrollmentRequest {
            code,
            initial_key_hash: key_hash(key),
        })
        .await
        .unwrap();
}

async fn exchange(
    fixture: &Fixture,
    deployment: &str,
    purpose: Value,
    proof: Value,
) -> (StatusCode, Value) {
    let router = alien_manager::auth::identity_exchange::router(
        fixture.credentials.clone(),
        fixture.identity.clone(),
    )
    .with_state(fixture.state.clone());
    send(
        &router,
        "unused",
        "POST",
        "/v1/identity/exchange",
        json!({"deploymentId": deployment, "purpose": purpose, "proof": proof}),
    )
    .await
}

const OPERATOR_KEY: &str =
    "ax_day_v1_1111111111111111111111111111111111111111111111111111111111111111";

/// Through the exchange endpoint: a daily key gets the operations credential
/// for its own deployment and uses it on the queue; the same request with a
/// cloud proof, a session proof, a key of another deployment or a rotated-away
/// key gets nothing.
#[tokio::test]
async fn only_an_operators_daily_key_is_issued_the_operations_credential() {
    let fixture = fixture().await;
    enroll_operator(&fixture, &fixture.dep_a, OPERATOR_KEY).await;
    let purpose = json!({"type": "operatorOperations"});
    let daily = |key: &str| json!({"type": "dailyKey", "key": key});

    let (status, body) = exchange(
        &fixture,
        &fixture.dep_a,
        purpose.clone(),
        daily(OPERATOR_KEY),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let issued = body["token"].as_str().unwrap().to_owned();
    let command_id = fixture.queue_command(&fixture.dep_a).await;
    let (status, leased) = send(
        &fixture.router(),
        &issued,
        "POST",
        "/v1/commands/leases",
        lease_body(&fixture.dep_a, &operator_queue()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{leased}");
    assert_eq!(leased["leases"][0]["commandId"], command_id.as_str());

    // The key belongs to deployment A. Naming B, with the same key, fails.
    let (status, _) = exchange(
        &fixture,
        &fixture.dep_b,
        purpose.clone(),
        daily(OPERATOR_KEY),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // Neither a cloud proof nor a session proof is an Operator key.
    for proof in [
        json!({"type": "aws", "presignedUrl": "https://sts.amazonaws.com/?Action=GetCallerIdentity"}),
        json!({"type": "gcp", "idToken": "token"}),
        json!({"type": "session", "token": "session"}),
    ] {
        let (status, body) =
            exchange(&fixture, &fixture.dep_a, purpose.clone(), proof.clone()).await;
        assert_ne!(status, StatusCode::OK, "{proof}: {body}");
        assert!(body.get("token").is_none(), "{proof}: {body}");
    }
    // A key that was rotated away is no longer an Operator key.
    let successor = "ax_day_v1_2222222222222222222222222222222222222222222222222222222222222222";
    fixture
        .identity
        .rotate(&IdentityRotationRequest {
            deployment_id: fixture.dep_a.clone(),
            key: OPERATOR_KEY.to_owned(),
            successor_hash: key_hash(successor),
        })
        .await
        .unwrap();
    let (status, _) = exchange(
        &fixture,
        &fixture.dep_a,
        purpose.clone(),
        daily(OPERATOR_KEY),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = exchange(&fixture, &fixture.dep_a, purpose, daily(successor)).await;
    assert_eq!(status, StatusCode::OK);
}

fn respond_body() -> Value {
    serde_json::to_value(alien_core::SubmitResponseRequest {
        response: alien_core::CommandResponse::success(b"done"),
    })
    .unwrap()
}

/// Two deployments in one group: the group's token and the first
/// deployment's own token reach nothing of the second deployment's
/// queue; the second deployment's own token reaches all of it.
#[tokio::test]
async fn a_group_token_for_one_deployment_is_refused_on_a_second_deployment_in_the_group() {
    for (bearer, allowed) in [
        ("group-a-token", false),
        ("deployment-a-token", false),
        ("deployment-a2-token", true),
    ] {
        for action in ["lease", "respond", "release"] {
            let fixture = fixture().await;
            let router = fixture.router();
            let command_id = fixture.queue_command(&fixture.dep_a2).await;
            let lease = fixture.lease_directly(&fixture.dep_a2).await;
            let before = fixture.state_of(&command_id).await;
            let (status, body) = match action {
                "lease" => {
                    send(
                        &router,
                        bearer,
                        "POST",
                        "/v1/commands/leases",
                        lease_body(&fixture.dep_a2, &operator_queue()),
                    )
                    .await
                }
                "respond" => {
                    send(
                        &router,
                        bearer,
                        "PUT",
                        &format!(
                            "/v1/commands/{command_id}/response?lease_id={}",
                            lease.lease_id
                        ),
                        respond_body(),
                    )
                    .await
                }
                _ => {
                    send(
                        &router,
                        bearer,
                        "POST",
                        &format!("/v1/commands/leases/{}/release", lease.lease_id),
                        json!({}),
                    )
                    .await
                }
            };
            if allowed {
                assert_eq!(status, StatusCode::OK, "{bearer} {action}: {body}");
            } else {
                assert_eq!(status, StatusCode::FORBIDDEN, "{bearer} {action}: {body}");
                assert_eq!(
                    fixture.state_of(&command_id).await,
                    before,
                    "{bearer} {action} must leave the command untouched"
                );
            }
        }
    }
}

/// A command that was never leased can be answered without a lease id; on
/// the Operator queue that path is closed to other callers.
#[tokio::test]
async fn a_pending_operator_command_is_not_answered_without_a_lease_by_others() {
    for index in 0..case_count().await {
        let (fixture, name, bearer, allowed) = case(index).await;
        if name.starts_with("operatorOperations, other") {
            continue;
        }
        let router = fixture.router();
        let command_id = fixture.queue_command(&fixture.dep_a).await;
        let before = fixture.state_of(&command_id).await;
        let (status, body) = send(
            &router,
            &bearer,
            "PUT",
            &format!("/v1/commands/{command_id}/response"),
            respond_body(),
        )
        .await;
        if !allowed {
            assert_eq!(status, StatusCode::FORBIDDEN, "{name}: {body}");
            assert_eq!(
                fixture.state_of(&command_id).await,
                before,
                "{name} must not have answered the command"
            );
        }
    }
}

/// `operations-worker` is delivered by push. On the lease route only its own
/// deployment's token passes the rule; other subjects are refused.
#[tokio::test]
async fn nobody_but_the_deployments_own_token_leases_the_operations_worker() {
    for index in 0..case_count().await {
        // The in-memory registry knows the worker target, so a command can be
        // queued for it and the allowed lease can be checked by command id.
        let fixture = memory_fixture().await;
        let (name, bearer, allowed) = fixture.callers().remove(index);
        let router = fixture.router();
        let worker_command = fixture
            .queue_command_for(&fixture.dep_a, OPERATIONS_WORKER_COMMAND_TARGET_ID)
            .await;
        for resource_type in [CommandTargetType::Worker, CommandTargetType::Daemon] {
            let target = CommandTarget::new(OPERATIONS_WORKER_COMMAND_TARGET_ID, resource_type);
            let (status, body) = send(
                &router,
                &bearer,
                "POST",
                "/v1/commands/leases",
                lease_body(&fixture.dep_a, &target),
            )
            .await;
            if allowed
                && !name.starts_with("operatorOperations")
                && resource_type == CommandTargetType::Worker
            {
                assert_eq!(status, StatusCode::OK, "{name}: {body}");
                let response: alien_core::LeaseResponse = serde_json::from_value(body.clone())
                    .unwrap_or_else(|error| {
                        panic!("{name}: not a lease response ({error}): {body}")
                    });
                assert_eq!(response.leases.len(), 1, "{name}: {body}");
                assert_eq!(response.leases[0].command_id, worker_command, "{name}");
                assert_eq!(
                    response.leases[0].envelope.target.resource_id,
                    OPERATIONS_WORKER_COMMAND_TARGET_ID,
                    "{name}"
                );
            } else {
                assert_eq!(
                    status,
                    StatusCode::FORBIDDEN,
                    "{name} {resource_type:?}: {body}"
                );
            }
        }
    }
}

/// A reserved id with the wrong type is not a target: every caller, the
/// deployment's own token included, is refused before a lease slot is taken
/// or an attempt is counted.
#[tokio::test]
async fn a_reserved_id_with_the_wrong_type_is_refused_for_everyone_and_touches_nothing() {
    for index in 0..case_count().await {
        let (fixture, name, bearer, _) = case(index).await;
        let router = fixture.router();
        let command_id = fixture.queue_command(&fixture.dep_a).await;
        let before = fixture.state_of(&command_id).await;
        for (id, resource_type) in confused_targets() {
            if id == OPERATIONS_WORKER_COMMAND_TARGET_ID
                && resource_type == CommandTargetType::Worker
            {
                continue; // the real worker target, not a confusion
            }
            let (status, body) = send(
                &router,
                &bearer,
                "POST",
                "/v1/commands/leases",
                lease_body(&fixture.dep_a, &CommandTarget::new(id, resource_type)),
            )
            .await;
            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "{name} {id}/{resource_type:?}: {body}"
            );
            assert_eq!(fixture.state_of(&command_id).await, before, "{name}");
        }
        // Nothing was leased or counted: the real queue still hands the
        // command out as a first attempt.
        let lease = fixture.lease_directly(&fixture.dep_a).await;
        assert_eq!(lease.command_id, command_id, "{name}");
        assert_eq!(lease.attempt, 1, "{name}: no attempt was spent");
    }
}

/// Answer and release on the operations worker: the own deployment token
/// passes, the others are refused and leave command and lease alone.
#[tokio::test]
async fn answer_and_release_on_the_operations_worker_follow_the_same_rule() {
    let worker = CommandTarget::new(
        OPERATIONS_WORKER_COMMAND_TARGET_ID,
        CommandTargetType::Worker,
    );
    for index in 0..case_count().await {
        for action in ["respond", "release"] {
            let (fixture, name, bearer, allowed) = {
                let fixture = memory_fixture().await;
                let (name, bearer, allowed) = fixture.callers().remove(index);
                (fixture, name, bearer, allowed)
            };
            // Only the deployment-manager subjects pass on the worker; the
            // operations credential is the Operator queue's alone.
            let allowed = allowed && !name.starts_with("operatorOperations");
            let router = fixture.router();
            let command_id = fixture
                .queue_command_for(&fixture.dep_a, OPERATIONS_WORKER_COMMAND_TARGET_ID)
                .await;
            let lease = fixture.lease_target(&fixture.dep_a, &worker).await;
            let before = fixture.state_of(&command_id).await;
            let (status, body) = if action == "respond" {
                send(
                    &router,
                    &bearer,
                    "PUT",
                    &format!(
                        "/v1/commands/{command_id}/response?lease_id={}",
                        lease.lease_id
                    ),
                    respond_body(),
                )
                .await
            } else {
                send(
                    &router,
                    &bearer,
                    "POST",
                    &format!("/v1/commands/leases/{}/release", lease.lease_id),
                    json!({}),
                )
                .await
            };
            if allowed {
                assert_eq!(status, StatusCode::OK, "{name} {action}: {body}");
            } else {
                assert_eq!(status, StatusCode::FORBIDDEN, "{name} {action}: {body}");
                assert_eq!(
                    fixture.state_of(&command_id).await,
                    before,
                    "{name} {action}"
                );
                assert!(
                    fixture
                        .state
                        .command_server
                        .get_lease_owner(&lease.lease_id)
                        .await
                        .unwrap()
                        .is_some(),
                    "{name} {action} must leave the lease in place"
                );
            }
        }
    }
}

/// The response-context route applies the same rule to a reserved-target
/// command.
#[tokio::test]
async fn response_context_for_a_reserved_target_command_follows_the_rule() {
    for index in 0..case_count().await {
        let (fixture, name, bearer, allowed) = case(index).await;
        let router = fixture.router();
        let command_id = fixture.queue_command(&fixture.dep_a).await;
        let lease = fixture.lease_directly(&fixture.dep_a).await;
        let (status, body) = send(
            &router,
            &bearer,
            "GET",
            &format!(
                "/v1/commands/{command_id}/response-context?leaseId={}",
                lease.lease_id
            ),
            Value::Null,
        )
        .await;
        if allowed {
            assert_eq!(status, StatusCode::OK, "{name}: {body}");
            let handling: alien_core::ResponseHandling = serde_json::from_value(body.clone())
                .unwrap_or_else(|error| {
                    panic!("{name}: not response credentials ({error}): {body}")
                });
            // The credentials belong to this command and this lease.
            assert!(
                handling
                    .submit_response_url
                    .contains(&format!("/commands/{command_id}/response?")),
                "{name}: {}",
                handling.submit_response_url
            );
            assert!(
                handling
                    .submit_response_url
                    .contains(&format!("lease_id={}", lease.lease_id)),
                "{name}: {}",
                handling.submit_response_url
            );
            assert!(
                handling
                    .storage_upload_request
                    .path
                    .contains(&format!("commands/{command_id}/response")),
                "{name}: {}",
                handling.storage_upload_request.path
            );
        } else {
            assert_eq!(status, StatusCode::FORBIDDEN, "{name}: {body}");
        }
    }
}

/// The allowed row of the no-lease answer: the deployment's own token (and the
/// operations credential) may answer a pending command without a lease id.
#[tokio::test]
async fn the_allowed_callers_pass_the_no_lease_answer() {
    for index in 0..case_count().await {
        let (fixture, name, bearer, allowed) = case(index).await;
        if !allowed || name.starts_with("operatorOperations, other") {
            continue;
        }
        let router = fixture.router();
        let command_id = fixture.queue_command(&fixture.dep_a).await;
        let before = fixture.state_of(&command_id).await;
        let (status, body) = send(
            &router,
            &bearer,
            "PUT",
            &format!("/v1/commands/{command_id}/response"),
            respond_body(),
        )
        .await;
        // Authorization passes (it is not the 403 of the refused callers), but
        // the answer itself is not accepted today: a command that was never
        // leased is Pending, and Pending -> Succeeded is not a transition.
        // Exact status and code, so a 401 or a 500 cannot pass as "allowed".
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name}: {body}");
        assert_eq!(body["code"], "INVALID_STATE_TRANSITION", "{name}: {body}");
        assert_eq!(
            fixture.state_of(&command_id).await,
            before,
            "{name}: the refused answer must leave the command as it was"
        );
    }
}

/// Every caller in the matrix, on both reserved ids, over HTTP: status, payload
/// (read and direct write) and upload-complete of a reserved command, and create
/// for a reserved target. None of these routes is open to a lease holder: reads
/// and direct writes are refused for everyone; only the sender's exact
/// operations capability (the caller that created the command) may create and
/// complete the upload.
#[tokio::test]
async fn status_payload_upload_complete_and_create_of_reserved_commands_are_closed_to_every_caller()
{
    for (target_id, resource_type) in [
        (OPERATOR_COMMAND_TARGET_ID, CommandTargetType::Daemon),
        (
            OPERATIONS_WORKER_COMMAND_TARGET_ID,
            CommandTargetType::Worker,
        ),
    ] {
        for index in 0..case_count().await {
            let fixture = memory_fixture().await;
            let (name, bearer, _) = fixture.callers().remove(index);
            // The operations capability of the sender is the one exact holder.
            let sender = name == "sender operations capability";
            let router = fixture.router();
            let command_id = fixture.queue_command_for(&fixture.dep_a, target_id).await;
            let before = fixture.state_of(&command_id).await;
            let label = format!("{name} on {target_id} ({resource_type:?})");

            for (method, path, body) in [
                ("GET", format!("/v1/commands/{command_id}"), Value::Null),
                (
                    "GET",
                    format!("/v1/commands/{command_id}/payload"),
                    Value::Null,
                ),
                (
                    "PUT",
                    format!("/v1/commands/{command_id}/payload"),
                    json!({"params": {"mode": "inline", "inlineBase64": "aGk="}}),
                ),
            ] {
                let (status, response) = send(&router, &bearer, method, &path, body).await;
                assert_eq!(
                    status,
                    StatusCode::FORBIDDEN,
                    "{label}: {method} {path}: {response}"
                );
            }

            let (status, response) = send(
                &router,
                &bearer,
                "POST",
                &format!("/v1/commands/{command_id}/upload-complete"),
                json!({"size": 2}),
            )
            .await;
            if sender {
                assert_ne!(status, StatusCode::FORBIDDEN, "{label}: {response}");
            } else {
                assert_eq!(status, StatusCode::FORBIDDEN, "{label}: {response}");
            }

            let (status, response) = send(
                &router,
                &bearer,
                "POST",
                "/v1/commands",
                json!({
                    "deploymentId": fixture.dep_a,
                    "command": "echo",
                    "params": {"mode": "inline", "inlineBase64": "aGk="},
                    "targetResourceId": target_id,
                }),
            )
            .await;
            if sender {
                assert_eq!(status, StatusCode::OK, "{label}: {response}");
            } else {
                assert_eq!(status, StatusCode::FORBIDDEN, "{label}: {response}");
            }
            assert_eq!(
                fixture.state_of(&command_id).await,
                before,
                "{label}: the command is untouched"
            );
        }
    }
}

/// The reserved names are matched exactly. A name that differs in case or by
/// whitespace is an ordinary target id: it is not the Operator's queue, so a
/// lease on it never returns the Operator's commands, and nothing normalizes it
/// into the reserved name.
#[tokio::test]
async fn case_and_whitespace_variants_of_the_reserved_names_reach_no_reserved_queue() {
    let variants = [
        "Operator",
        "OPERATOR",
        " operator",
        "operator ",
        "operator\n",
        "\toperator",
        "operat\u{6f}r\u{200b}",
        "Operations-Worker",
        "operations-worker ",
        " operations-worker",
        "OPERATIONS-WORKER",
    ];
    let fixture = memory_fixture().await;
    let router = fixture.router();
    let operator_command = fixture.queue_command(&fixture.dep_a).await;
    let worker_command = fixture
        .queue_command_for(&fixture.dep_a, OPERATIONS_WORKER_COMMAND_TARGET_ID)
        .await;
    let before = (
        fixture.state_of(&operator_command).await,
        fixture.state_of(&worker_command).await,
    );
    for bearer in [
        "deployment-a-token",
        "deployment-a-real-token",
        "workspace-key",
        "commands-key",
    ] {
        for variant in variants {
            for resource_type in [CommandTargetType::Daemon, CommandTargetType::Worker] {
                let target = CommandTarget::new(variant, resource_type);
                let (status, body) = send(
                    &router,
                    bearer,
                    "POST",
                    "/v1/commands/leases",
                    lease_body(&fixture.dep_a, &target),
                )
                .await;
                let leased = body["leases"].as_array().map_or(0, Vec::len);
                assert_eq!(
                    leased, 0,
                    "{bearer} leased through {variant:?} ({resource_type:?}): {status} {body}"
                );
            }
            // And a command cannot be created for a variant either.
            let (status, body) = send(
                &router,
                bearer,
                "POST",
                "/v1/commands",
                json!({
                    "deploymentId": fixture.dep_a,
                    "command": "echo",
                    "params": {"mode": "inline", "inlineBase64": "aGk="},
                    "targetResourceId": variant,
                }),
            )
            .await;
            assert_ne!(status, StatusCode::OK, "{bearer} created for {variant:?}");
            let _ = body;
        }
    }
    assert_eq!(
        (
            fixture.state_of(&operator_command).await,
            fixture.state_of(&worker_command).await
        ),
        before,
        "no variant touched a reserved command"
    );
}

/// An answer on the response URL of one lease is bound to that command and
/// that lease: the same token with another lease id, on another command, or
/// without a lease id is refused, and nothing changes.
#[tokio::test]
async fn a_presigned_answer_of_one_lease_does_not_answer_another_lease_or_command() {
    let fixture = fixture().await;
    let router = fixture.router();
    let first = fixture.queue_command(&fixture.dep_a).await;
    let second = fixture.queue_command(&fixture.dep_a).await;
    let first_lease = fixture.lease_directly(&fixture.dep_a).await;
    let second_lease = fixture.lease_directly(&fixture.dep_a).await;
    // Which command each lease holds, whatever order they came in.
    let (cmd_one, lease_one, cmd_two, lease_two) = if first_lease.command_id == first {
        (first, first_lease, second, second_lease)
    } else {
        (second, second_lease, first, first_lease)
    };
    let (status, body) = send(
        &router,
        "deployment-a-token",
        "GET",
        &format!(
            "/v1/commands/{cmd_one}/response-context?leaseId={}",
            lease_one.lease_id
        ),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let handling: alien_core::ResponseHandling = serde_json::from_value(body).unwrap();
    let query = handling
        .submit_response_url
        .split_once('?')
        .map(|(_, query)| query.to_owned())
        .unwrap();
    let param = |name: &str| {
        query
            .split('&')
            .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
            .unwrap()
            .to_owned()
    };
    let (token, expires) = (param("response_token"), param("expires"));

    let anonymous = |path: String| {
        let router = router.clone();
        async move {
            let response = router
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri(path)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(serde_json::to_vec(&respond_body()).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            response.status()
        }
    };
    let before = (
        fixture.state_of(&cmd_one).await,
        fixture.state_of(&cmd_two).await,
    );
    for path in [
        // Another lease's id on this command.
        format!(
            "/v1/commands/{cmd_one}/response?response_token={token}&expires={expires}&lease_id={}",
            lease_two.lease_id
        ),
        // This lease's token on the other command.
        format!(
            "/v1/commands/{cmd_two}/response?response_token={token}&expires={expires}&lease_id={}",
            lease_one.lease_id
        ),
        format!(
            "/v1/commands/{cmd_two}/response?response_token={token}&expires={expires}&lease_id={}",
            lease_two.lease_id
        ),
        // No lease id at all.
        format!("/v1/commands/{cmd_one}/response?response_token={token}&expires={expires}"),
    ] {
        assert_eq!(
            anonymous(path.clone()).await,
            StatusCode::UNAUTHORIZED,
            "{path}"
        );
    }
    assert_eq!(
        (
            fixture.state_of(&cmd_one).await,
            fixture.state_of(&cmd_two).await
        ),
        before
    );
    // The URL as issued still works, so the refusals above are the binding.
    let own = format!(
        "/v1/commands/{cmd_one}/response?response_token={token}&expires={expires}&lease_id={}",
        lease_one.lease_id
    );
    assert_eq!(anonymous(own).await, StatusCode::OK);
    assert_eq!(
        fixture.state_of(&cmd_two).await,
        before.1,
        "the other command is still running"
    );
}

/// A lease whose command record cannot be found is not released by anyone, the
/// allowed callers included, and it stays in place.
#[tokio::test]
async fn a_lease_without_its_command_record_is_released_by_no_caller() {
    for index in 0..case_count().await {
        let (fixture, name, bearer, allowed) = case(index).await;
        let router = fixture.router();
        let command_id = "cmd_without_a_record";
        let lease_id = "lease_without_a_record";
        let now = chrono::Utc::now();
        fixture
            .state
            .kv
            .put(
                &format!("lease:{lease_id}"),
                command_id.as_bytes().to_vec(),
                None,
            )
            .await
            .unwrap();
        fixture
            .state
            .kv
            .put(
                &format!("cmd:{command_id}:lease"),
                serde_json::to_vec(&json!({
                    "lease_id": lease_id,
                    "acquired_at": now,
                    "expires_at": now + chrono::Duration::minutes(5),
                    "owner": fixture.dep_a,
                    "completing": false,
                }))
                .unwrap(),
                None,
            )
            .await
            .unwrap();
        let (status, body) = send(
            &router,
            &bearer,
            "POST",
            &format!("/v1/commands/leases/{lease_id}/release"),
            json!({}),
        )
        .await;
        if allowed {
            // The rule lets this caller through, so only the missing record
            // stops it.
            assert_eq!(status, StatusCode::NOT_FOUND, "{name}: {body}");
        } else {
            assert!(
                status == StatusCode::NOT_FOUND || status == StatusCode::FORBIDDEN,
                "{name}: {status} {body}"
            );
        }
        assert!(
            fixture
                .state
                .command_server
                .get_lease_owner(lease_id)
                .await
                .unwrap()
                .is_some(),
            "{name}: the lease must stay"
        );
    }

/// A credential for reporting decisions on access requests carries a role of
/// its own. On this manager nothing accepts that role, so every route refuses
/// it: it can never be used as a sync, command, telemetry or registry credential.
#[tokio::test]
async fn an_access_request_credential_is_refused_on_every_route() {
    let fixture = fixture().await;
    let router = alien_manager::routes::create_router_inner(
        fixture.state.clone(),
        alien_manager::routes::RouterOptions {
            include_initialize: true,
            include_install: true,
        },
    );
    let own = fixture.runtime(
        WORKSPACE,
        &fixture.dep_a,
        RuntimePurpose::OperatorAccessRequests,
    );
    let deployment = fixture.dep_a.clone();
    let command_id = fixture.queue_command(&deployment).await;
    let before = fixture.state_of(&command_id).await;

    let routes: Vec<(&str, String)> = vec![
        ("POST", "/v1/sync".into()),
        ("POST", "/v1/sync/acquire".into()),
        ("POST", "/v1/sync/reconcile".into()),
        ("POST", "/v1/sync/renew".into()),
        ("POST", "/v1/sync/release".into()),
        ("POST", "/v1/initialize".into()),
        ("POST", "/v1/commands".into()),
        ("POST", "/v1/commands/leases".into()),
        ("GET", format!("/v1/commands/{command_id}")),
        ("PUT", format!("/v1/commands/{command_id}/response")),
        ("POST", "/v1/logs".into()),
        ("POST", "/v1/traces".into()),
        ("POST", "/v1/metrics".into()),
        ("GET", "/v2/alien-operator/manifests/v1".into()),
        ("GET", format!("/v1/deployments/{deployment}")),
        ("GET", format!("/v1/deployments/{deployment}/info")),
        ("POST", format!("/v1/deployments/{deployment}/status-report")),
        ("POST", format!("/v1/deployments/{deployment}/delete")),
        ("GET", "/v1/releases".into()),
        ("POST", "/v1/releases".into()),
        ("GET", "/v1/tokens".into()),
        ("POST", "/v1/bindings/resolve".into()),
        ("POST", "/v1/stack/import".into()),
    ];
    for (method, path) in routes {
        let body = if path.ends_with("/leases") {
            lease_body(&deployment, &operator_queue())
        } else {
            json!({})
        };
        let (status, text) = send(&router, &own, method, &path, body).await;
        // A list shows only what the caller may read, which is nothing.
        if method == "GET" && path == "/v1/releases" {
            assert_eq!(status, StatusCode::OK, "{text}");
            assert_eq!(text["items"], json!([]), "{text}");
            continue;
        }
        // A route that checks its body before it authorizes answers an empty
        // body with 400 or 422, which is also a refusal; none may succeed.
        assert!(
            matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
                || (method != "GET"
                    && matches!(
                        status,
                        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
                    )),
            "{method} {path}: {status} {text}"
        );
    }
    // With a well-formed body the refusal is the authorization itself.
    let (status, text) = send(
        &router,
        &own,
        "POST",
        "/v1/commands/leases",
        lease_body(&deployment, &operator_queue()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{text}");
    assert_eq!(fixture.state_of(&command_id).await, before);
}
