//! The registry revoke a delete owes, driven through `POST /v1/sync/reconcile`.
//!
//! A deploy CLI tearing down setup reconciles every step through this route. The final step
//! persists `Deleted`, which removes the record a still-running caller renews its lease against,
//! so the revoke has to be over before that state is saved. A revoke the registry denies must come
//! back as a failed delete the caller adopts, not as an error it retries.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use http::header;
use sha2::{Digest, Sha256};
use tower::ServiceExt;

use alien_bindings::error::{ErrorData as BindingErrorData, Result as BindingResult};
use alien_bindings::providers::{kv::local::LocalKv, storage::local::LocalStorage};
use alien_bindings::traits::{
    ArtifactRegistry, ArtifactRegistryCredentials, ArtifactRegistryPermissions, Binding,
    BindingsProviderApi, Build, Container, CrossAccountAccess, CrossAccountPermissions, Kv,
    Postgres, Queue, RepositoryResponse, Sandbox, ServiceAccount, Storage, Vault, Worker,
};
use alien_commands::dispatchers::NullCommandDispatcher;
use alien_commands::server::{CommandDispatcher, CommandRegistry, CommandServer};
use alien_commands::InMemoryCommandRegistry;
use alien_core::{
    AwsEnvironmentInfo, DeploymentState, DeploymentStatus, EnvironmentInfo, Platform,
    ResourceLifecycle, RuntimeMetadata, Stack, Worker as WorkerResource, WorkerCode,
};
use alien_error::AlienError;
use alien_manager::auth::{Authz, Subject};
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
    AuthValidator, CreateDeploymentGroupParams, CreateDeploymentParams, CreateTokenParams,
    CredentialResolver, DeploymentStore, ReconcileData, ReleaseStore, TelemetryBackend, TokenStore,
    TokenType,
};

const IMAGE: &str = "123456789012.dkr.ecr.us-east-2.amazonaws.com/alien-artifacts-prj_test:latest";
const SESSION: &str = "setup-teardown";

#[derive(Debug, Clone, Copy, PartialEq)]
enum Answer {
    Revoke,
    Deny,
}

/// A registry that records the deployment's persisted status at each revoke.
struct RecordingRegistry {
    answer: Mutex<Answer>,
    store: Arc<dyn DeploymentStore>,
    deployment_id: Mutex<String>,
    statuses_at_revoke: Mutex<Vec<String>>,
}

impl std::fmt::Debug for RecordingRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecordingRegistry").finish_non_exhaustive()
    }
}

impl Binding for RecordingRegistry {}

#[async_trait]
impl ArtifactRegistry for RecordingRegistry {
    fn registry_endpoint(&self) -> String {
        "https://123456789012.dkr.ecr.us-east-2.amazonaws.com".to_string()
    }

    fn upstream_repository_prefix(&self) -> String {
        "alien-artifacts".to_string()
    }

    async fn create_repository(&self, _repo_name: &str) -> BindingResult<RepositoryResponse> {
        unimplemented!("not needed for revoke tests")
    }

    async fn get_repository(&self, _repo_id: &str) -> BindingResult<RepositoryResponse> {
        unimplemented!("not needed for revoke tests")
    }

    async fn add_cross_account_access(
        &self,
        _repo_id: &str,
        _access: CrossAccountAccess,
    ) -> BindingResult<()> {
        unimplemented!("not needed for revoke tests")
    }

    async fn remove_cross_account_access(
        &self,
        repo_id: &str,
        _access: CrossAccountAccess,
    ) -> BindingResult<()> {
        let deployment_id = self.deployment_id.lock().expect("id lock").clone();
        let status = self
            .store
            .get_deployment(&Subject::system(), &deployment_id)
            .await
            .expect("deployment read")
            .expect("deployment exists")
            .status;
        self.statuses_at_revoke
            .lock()
            .expect("statuses lock")
            .push(status);
        match *self.answer.lock().expect("answer lock") {
            Answer::Revoke => Ok(()),
            Answer::Deny => Err(AlienError::new(BindingErrorData::RemoteAccessDenied {
                operation_context: format!(
                    "Failed to delete ECR repository policy for '{repo_id}'"
                ),
                resource_type: "ECR Repository".to_string(),
                resource_name: repo_id.to_string(),
            })),
        }
    }

    async fn get_cross_account_access(
        &self,
        _repo_id: &str,
    ) -> BindingResult<CrossAccountPermissions> {
        unimplemented!("not needed for revoke tests")
    }

    async fn generate_credentials(
        &self,
        _repo_id: &str,
        _permissions: ArtifactRegistryPermissions,
        _ttl_seconds: Option<u32>,
    ) -> BindingResult<ArtifactRegistryCredentials> {
        unimplemented!("not needed for revoke tests")
    }

    async fn delete_repository(&self, _repo_id: &str) -> BindingResult<()> {
        unimplemented!("not needed for revoke tests")
    }
}

/// A provider with only the artifact registry configured.
#[derive(Debug)]
struct RegistryOnlyProvider {
    registry: Arc<RecordingRegistry>,
}

fn not_configured(binding_name: &str) -> alien_bindings::error::Error {
    AlienError::new(BindingErrorData::BindingNotConfigured {
        binding_name: binding_name.to_string(),
        env_var: alien_core::bindings::binding_env_var_name(binding_name),
    })
}

#[async_trait]
impl BindingsProviderApi for RegistryOnlyProvider {
    async fn load_storage(&self, binding_name: &str) -> BindingResult<Arc<dyn Storage>> {
        Err(not_configured(binding_name))
    }

    async fn load_build(&self, binding_name: &str) -> BindingResult<Arc<dyn Build>> {
        Err(not_configured(binding_name))
    }

    async fn load_artifact_registry(
        &self,
        binding_name: &str,
    ) -> BindingResult<Arc<dyn ArtifactRegistry>> {
        if binding_name == "artifact-registry" {
            Ok(self.registry.clone())
        } else {
            Err(not_configured(binding_name))
        }
    }

    async fn load_vault(&self, binding_name: &str) -> BindingResult<Arc<dyn Vault>> {
        Err(not_configured(binding_name))
    }

    async fn load_kv(&self, binding_name: &str) -> BindingResult<Arc<dyn Kv>> {
        Err(not_configured(binding_name))
    }

    async fn load_postgres(&self, binding_name: &str) -> BindingResult<Arc<dyn Postgres>> {
        Err(not_configured(binding_name))
    }

    async fn load_queue(&self, binding_name: &str) -> BindingResult<Arc<dyn Queue>> {
        Err(not_configured(binding_name))
    }

    async fn load_worker(&self, binding_name: &str) -> BindingResult<Arc<dyn Worker>> {
        Err(not_configured(binding_name))
    }

    async fn load_container(&self, binding_name: &str) -> BindingResult<Arc<dyn Container>> {
        Err(not_configured(binding_name))
    }

    async fn load_service_account(
        &self,
        binding_name: &str,
    ) -> BindingResult<Arc<dyn ServiceAccount>> {
        Err(not_configured(binding_name))
    }

    async fn load_sandbox(&self, binding_name: &str) -> BindingResult<Arc<dyn Sandbox>> {
        Err(not_configured(binding_name))
    }
}

struct Fixture {
    state: AppState,
    store: Arc<dyn DeploymentStore>,
    registry: Arc<RecordingRegistry>,
    admin_token: String,
    deployment_id: String,
}

async fn mint_admin_token(token_store: &Arc<dyn TokenStore>) -> String {
    let raw = format!("ax_admin_{}", uuid::Uuid::new_v4().simple());
    let mut hash = Sha256::new();
    hash.update(raw.as_bytes());
    token_store
        .create_token(CreateTokenParams {
            token_type: TokenType::Admin,
            key_prefix: raw[..12].to_string(),
            key_hash: format!("{:x}", hash.finalize()),
            deployment_group_id: None,
            deployment_id: None,
        })
        .await
        .expect("admin token");
    raw
}

fn worker_stack() -> Stack {
    Stack::new("test-stack".to_string())
        .add(
            WorkerResource::new("test-worker".to_string())
                .code(WorkerCode::Image {
                    image: IMAGE.to_string(),
                })
                .permissions("execution".to_string())
                .build(),
            ResourceLifecycle::Live,
        )
        .build()
}

fn state(status: DeploymentStatus) -> DeploymentState {
    DeploymentState {
        status,
        platform: Platform::Aws,
        current_release: None,
        target_release: None,
        stack_state: None,
        error: None,
        environment_info: Some(EnvironmentInfo::Aws(AwsEnvironmentInfo {
            account_id: "123456789012".to_string(),
            region: "us-east-2".to_string(),
        })),
        runtime_metadata: Some(RuntimeMetadata {
            prepared_stack: Some(worker_stack()),
            ..RuntimeMetadata::default()
        }),
        retry_requested: false,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    }
}

/// A deployment the manager has handed to setup teardown, as a deploy CLI finds it.
async fn handed_off_deployment(answer: Answer) -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir").keep();
    let db = Arc::new(
        SqliteDatabase::new(&dir.join("manager.db").to_string_lossy())
            .await
            .expect("database"),
    );
    let store: Arc<dyn DeploymentStore> = Arc::new(SqliteDeploymentStore::new(db.clone()));
    let release_store: Arc<dyn ReleaseStore> = Arc::new(SqliteReleaseStore::new(db.clone()));
    let token_store: Arc<dyn TokenStore> = Arc::new(SqliteTokenStore::new(db.clone()));
    let admin_token = mint_admin_token(&token_store).await;

    let system = Subject::system();
    let group = store
        .create_deployment_group(
            &system,
            CreateDeploymentGroupParams {
                name: "group".to_string(),
                max_deployments: 10,
                setup: Default::default(),
            },
        )
        .await
        .expect("group");
    let deployment = store
        .create_deployment(
            &system,
            CreateDeploymentParams {
                deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                name: "worker".to_string(),
                deployment_group_id: group.id,
                platform: Platform::Aws,
                base_platform: None,
                stack_settings: Default::default(),
                stack_state: None,
                environment_variables: None,
                public_subdomain: None,
                input_values: Default::default(),
                setup_item: None,
                deployment_token: None,
            },
        )
        .await
        .expect("deployment");
    store
        .set_delete_pending(&system, &deployment.id)
        .await
        .expect("delete pending");
    for status in [
        DeploymentStatus::Deleting,
        DeploymentStatus::TeardownRequired,
    ] {
        store
            .reconcile(
                &system,
                ReconcileData {
                    deployment_id: deployment.id.clone(),
                    session: "manager-loop".to_string(),
                    state: state(status),
                    update_heartbeat: false,
                    suggested_delay_ms: None,
                    heartbeats: Vec::new(),
                    observed_inventory_batches: Vec::new(),
                    capabilities: Vec::new(),
                    operator_version: None,
                    execution_claim: None,
                    operations_report: None,
                },
            )
            .await
            .expect("runtime cleanup checkpoint");
    }

    let registry = Arc::new(RecordingRegistry {
        answer: Mutex::new(answer),
        store: store.clone(),
        deployment_id: Mutex::new(deployment.id.clone()),
        statuses_at_revoke: Mutex::new(Vec::new()),
    });
    let provider: Arc<dyn BindingsProviderApi> = Arc::new(RegistryOnlyProvider {
        registry: registry.clone(),
    });

    let auth_validator: Arc<dyn AuthValidator> = Arc::new(
        alien_manager::providers::token_db_validator::TokenDbValidator::new(token_store.clone()),
    );
    let authz: Arc<dyn Authz> = Arc::new(OssAuthz);
    let telemetry_backend: Arc<dyn TelemetryBackend> = Arc::new(NullTelemetryBackend);
    let credential_resolver: Arc<dyn CredentialResolver> = Arc::new(
        alien_manager::providers::local_credentials::LocalCredentialResolver::new(dir.clone()),
    );
    let kv: Arc<dyn Kv> = Arc::new(LocalKv::new(dir.join("kv")).await.expect("kv"));
    let command_storage: Arc<dyn Storage> = Arc::new(
        LocalStorage::new(dir.join("storage").to_string_lossy().to_string()).expect("storage"),
    );
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
        deployment_store: store.clone(),
        release_store,
        token_store,
        auth_validator,
        authz,
        telemetry_backend,
        credential_resolver,
        command_server,
        config: Arc::new(ManagerConfig::default()),
        bindings_provider: Some(provider),
        target_bindings_providers: HashMap::new(),
        kv,
        http_client: reqwest::Client::new(),
        credential_cache: Arc::new(CredentialCache::new()),
        pull_validation_cache: Arc::new(PullValidationCache::new()),
        registry_routing_table: Arc::new(
            RegistryRoutingTable::new(vec![]).expect("empty routing table should build"),
        ),
        import_registry: Arc::new(alien_infra::ImporterRegistry::built_in()),
        tunnels: None,
        charts: None,
        release_channels: None,
        bundle_signing_key: None,
        bundle_sources: None,
        log_buffer: Arc::new(alien_manager::LogBuffer::new()),
    };

    Fixture {
        state,
        store,
        registry,
        admin_token,
        deployment_id: deployment.id,
    }
}

impl Fixture {
    /// One setup-teardown checkpoint, as the deploy CLI's transport sends it.
    async fn reconcile(&self, status: DeploymentStatus) -> (StatusCode, serde_json::Value) {
        let body = serde_json::json!({
            "deploymentId": self.deployment_id,
            "session": SESSION,
            "state": serde_json::to_value(state(status)).expect("state json"),
        });
        let request = Request::builder()
            .method("POST")
            .uri("/v1/sync/reconcile")
            .header(header::CONTENT_TYPE, "application/json")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", self.admin_token),
            )
            .body(Body::from(serde_json::to_vec(&body).expect("body")))
            .expect("request");
        let response = alien_manager::routes::sync::router()
            .with_state(self.state.clone())
            .oneshot(request)
            .await
            .expect("response");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("body bytes");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    async fn persisted_status(&self) -> String {
        self.store
            .get_deployment(&Subject::system(), &self.deployment_id)
            .await
            .expect("deployment read")
            .expect("deployment exists")
            .status
    }

    fn statuses_at_revoke(&self) -> Vec<String> {
        self.registry
            .statuses_at_revoke
            .lock()
            .expect("statuses lock")
            .clone()
    }
}

#[tokio::test]
async fn the_final_teardown_step_revokes_before_the_deletion_is_saved() {
    let fixture = handed_off_deployment(Answer::Revoke).await;

    let (status, body) = fixture.reconcile(DeploymentStatus::Deleted).await;

    assert_eq!(status, StatusCode::OK, "body = {body:#}");
    assert_eq!(
        fixture.statuses_at_revoke(),
        vec!["teardown-required".to_string()],
        "the revoke must finish before the deletion is saved: a caller renewing its lease \
         during the revoke must still find the deployment"
    );
    assert_eq!(fixture.persisted_status().await, "deleted");
}

#[tokio::test]
async fn a_denied_revoke_answers_the_deploy_cli_with_a_failed_delete() {
    let fixture = handed_off_deployment(Answer::Deny).await;

    let (status, body) = fixture.reconcile(DeploymentStatus::Deleted).await;

    assert_eq!(
        status,
        StatusCode::OK,
        "a denied revoke is a delete outcome, not a retryable error: {body:#}"
    );
    let current: DeploymentState =
        serde_json::from_value(body["current"].clone()).expect("current state");
    assert_eq!(
        current.status,
        DeploymentStatus::DeleteFailed,
        "the CLI adopts the returned state and stops"
    );
    let error = current.error.expect("the failed delete carries the denial");
    assert_eq!(error.code, "REGISTRY_ACCESS_REVOKE_DENIED");
    assert!(
        error.message.contains("alien-artifacts-prj_test"),
        "the error names the repository: {}",
        error.message
    );
    assert_eq!(fixture.persisted_status().await, "delete-failed");
    assert_eq!(
        fixture.statuses_at_revoke(),
        vec!["teardown-required".to_string()]
    );
}
