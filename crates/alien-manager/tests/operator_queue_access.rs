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
    DeploymentRecord, DeploymentStore, ReleaseStore, TokenStore,
};
use async_trait::async_trait;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use http::header;
use serde_json::{json, Value};
use tower::ServiceExt;

const WORKSPACE: &str = "default";
/// What a deployment-scoped viewer may do on the Operator queue today; the
/// policy for existing roles is not part of this change.
const DEPLOYMENT_VIEWER_TODAY: bool = true;
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
        "deployment-b-token",
        legacy(
            WORKSPACE,
            deployment_scope(&dep_b.id),
            Role::DeploymentManager,
            false,
        ),
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
            ("workspace key", legacy("workspace-key"), true),
            ("workspace user", legacy("workspace-user"), true),
            ("project developer", legacy("project-developer"), true),
            // The standalone manager lets every project-scoped user act on the
            // project's deployments; that policy is not part of this change.
            ("project viewer", legacy("project-viewer"), true),
            ("group token, own group", legacy("group-a-token"), true),
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
                DEPLOYMENT_VIEWER_TODAY,
            ),
            ("commands-only key", legacy("commands-key"), false),
            // The standalone manager serves one workspace and does not compare the
            // workspace of a workspace-scoped key.
            ("other workspace key", legacy("other-workspace-key"), true),
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
        self.state
            .command_server
            .create_command(alien_core::CreateCommandRequest {
                deployment_id: deployment.to_owned(),
                command: "echo".to_owned(),
                params: alien_core::BodySpec::inline(b"hello"),
                deadline: None,
                idempotency_key: None,
                target_resource_id: Some(OPERATOR_COMMAND_TARGET_ID.to_owned()),
                operation_result_contract: None,
            })
            .await
            .unwrap()
            .command_id
    }

    async fn lease_directly(&self, deployment: &str) -> alien_core::LeaseInfo {
        let mut leased = self
            .state
            .command_server
            .acquire_lease(
                deployment,
                &serde_json::from_value(lease_body(deployment, &operator_queue())).unwrap(),
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
