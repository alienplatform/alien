//! The config a runner gets from `POST /v1/sync/acquire`.
//!
//! The manager's loop and `alien deploy` can each run part of one operation:
//! the manager starts an update, the CLI takes the lease and finishes it. Both
//! must apply the same config. When the CLI's config lacks the variables the
//! manager injects, every workload rolls a second time and keeps running
//! without `ALIEN_DEPLOYMENT_ID`, `ALIEN_DEPLOYMENT_NAME` and its command
//! receiver variables until the manager runs another update.
//!
//! The test drives the CLI's own acquire helper against the real route over
//! TCP and reads `deploymentConfig` the way `alien deploy` does.

use std::collections::HashMap;
use std::sync::Arc;

use sha2::{Digest, Sha256};

use alien_bindings::providers::{kv::local::LocalKv, storage::local::LocalStorage};
use alien_bindings::traits::{Kv, Storage};
use alien_commands::dispatchers::NullCommandDispatcher;
use alien_commands::server::{CommandDispatcher, CommandRegistry, CommandServer};
use alien_commands::InMemoryCommandRegistry;
use alien_core::{
    Container, ContainerCode, DeploymentConfig, DeploymentModel, EnvironmentVariable,
    EnvironmentVariableType, Platform, ResourceLifecycle, ResourceSpec, Stack,
    ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID, ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE,
    ENV_ALIEN_COMMANDS_TOKEN, ENV_ALIEN_COMMANDS_URL, ENV_ALIEN_DEPLOYMENT_ID,
    ENV_ALIEN_DEPLOYMENT_NAME,
};
use alien_deployment::manager_api_transport::acquire_deployment_with_payload;
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
    AuthValidator, CreateDeploymentGroupParams, CreateDeploymentParams, CreateReleaseParams,
    CreateTokenParams, CredentialResolver, DeploymentStore, ReleaseStore, TelemetryBackend,
    TokenStore, TokenType,
};

const MANAGER_URL: &str = "https://manager.example.test";

/// An app container that receives commands, as a release declares it.
fn release_stack() -> Stack {
    let api = Container::new("api".to_string())
        .code(ContainerCode::Image {
            image: "api:latest".to_string(),
        })
        .cpu(ResourceSpec {
            min: "0.5".to_string(),
            desired: "1".to_string(),
        })
        .memory(ResourceSpec {
            min: "512Mi".to_string(),
            desired: "1Gi".to_string(),
        })
        .port(8080)
        .permissions("execution".to_string())
        .commands_enabled(true)
        .build();
    Stack::new("app".to_string())
        .add(api, ResourceLifecycle::Live)
        .build()
}

fn variable(
    name: &str,
    value: &str,
    var_type: EnvironmentVariableType,
    target_resources: Option<Vec<String>>,
) -> EnvironmentVariable {
    EnvironmentVariable {
        name: name.to_string(),
        value: value.to_string(),
        var_type,
        target_resources,
    }
}

struct Fixture {
    state: AppState,
    store: Arc<dyn DeploymentStore>,
    deployment_id: String,
    deployment_token: String,
    user_variable: EnvironmentVariable,
}

/// A deployment the manager has an update queued for: a desired release that
/// carries a command-enabled container, and one variable of its own. The
/// record keeps the deployment token the command variables are built from
/// unless `record_token` is false.
async fn deployment_with_queued_update(record_token: bool) -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir").keep();
    let db = Arc::new(
        SqliteDatabase::new(&dir.join("manager.db").to_string_lossy())
            .await
            .expect("database"),
    );
    let store: Arc<dyn DeploymentStore> = Arc::new(SqliteDeploymentStore::new(db.clone()));
    let release_store: Arc<dyn ReleaseStore> = Arc::new(SqliteReleaseStore::new(db.clone()));
    let token_store: Arc<dyn TokenStore> = Arc::new(SqliteTokenStore::new(db.clone()));

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
    let deployment_token = format!("ax_dep_{}", uuid::Uuid::new_v4().simple());
    let user_variable = variable("LOG_LEVEL", "debug", EnvironmentVariableType::Plain, None);
    let deployment = store
        .create_deployment(
            &system,
            CreateDeploymentParams {
                deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                name: "app-1".to_string(),
                deployment_group_id: group.id.clone(),
                platform: Platform::Aws,
                base_platform: None,
                stack_settings: Default::default(),
                stack_state: None,
                environment_variables: Some(vec![user_variable.clone()]),
                public_subdomain: None,
                input_values: Default::default(),
                setup_item: None,
                deployment_token: record_token.then(|| deployment_token.clone()),
            },
        )
        .await
        .expect("deployment");
    let mut hash = Sha256::new();
    hash.update(deployment_token.as_bytes());
    token_store
        .create_token(CreateTokenParams {
            token_type: TokenType::Deployment,
            key_prefix: deployment_token[..12].to_string(),
            key_hash: format!("{:x}", hash.finalize()),
            deployment_group_id: Some(group.id),
            deployment_id: Some(deployment.id.clone()),
        })
        .await
        .expect("deployment token");

    let release = release_store
        .create_release(
            &system,
            CreateReleaseParams {
                project_id: deployment.project_id.clone(),
                stacks: HashMap::from([(Platform::Aws, release_stack())]),
                git_commit_sha: None,
                git_commit_ref: None,
                git_commit_message: None,
            },
        )
        .await
        .expect("release");
    store
        .set_deployment_desired_release(&system, &deployment.id, &release.id)
        .await
        .expect("desired release");

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
        format!("{MANAGER_URL}/v1"),
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
        config: Arc::new(ManagerConfig {
            base_url: Some(MANAGER_URL.to_string()),
            ..Default::default()
        }),
        bindings_provider: None,
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
        deployment_id: deployment.id,
        deployment_token,
        user_variable,
    }
}

impl Fixture {
    /// `alien deploy`'s acquire, authenticated with the deployment's own token,
    /// against the real route over TCP.
    async fn acquire_as_cli(
        &self,
    ) -> Result<alien_deployment::manager_api_transport::AcquiredDeploymentPayload, AlienError>
    {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let manager_addr = listener.local_addr().expect("local addr");
        let router = alien_manager::routes::sync::router().with_state(self.state.clone());
        tokio::spawn(async move { axum::serve(listener, router).await.expect("serve") });

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", self.deployment_token)
                .parse()
                .expect("header"),
        );
        let client = alien_manager_api::Client::new_with_client(
            &format!("http://{manager_addr}"),
            reqwest::Client::builder()
                .default_headers(headers)
                .build()
                .expect("http client"),
        );
        acquire_deployment_with_payload(
            &client,
            &self.deployment_id,
            "cli-deploy-test",
            DeploymentModel::Push,
        )
        .await
    }
}

#[tokio::test]
async fn a_runner_that_acquires_an_update_gets_the_variables_the_manager_injects() {
    let fixture = deployment_with_queued_update(true).await;

    let acquired = fixture
        .acquire_as_cli()
        .await
        .expect("the CLI acquires the queued update");

    let config: DeploymentConfig = serde_json::from_value(
        acquired
            .deployment
            .get("deploymentConfig")
            .cloned()
            .expect("acquire returns the config to run with"),
    )
    .expect("deploymentConfig deserializes as the CLI reads it");

    let api = Some(vec!["api".to_string()]);
    assert_eq!(
        config.environment_variables.variables,
        vec![
            variable(
                ENV_ALIEN_DEPLOYMENT_ID,
                &fixture.deployment_id,
                EnvironmentVariableType::Plain,
                None,
            ),
            variable(
                ENV_ALIEN_DEPLOYMENT_NAME,
                "app-1",
                EnvironmentVariableType::Plain,
                None,
            ),
            variable(
                ENV_ALIEN_COMMANDS_URL,
                &format!("{MANAGER_URL}/v1"),
                EnvironmentVariableType::Plain,
                api.clone(),
            ),
            variable(
                ENV_ALIEN_COMMANDS_TOKEN,
                &fixture.deployment_token,
                EnvironmentVariableType::Secret,
                api.clone(),
            ),
            variable(
                ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE,
                "container",
                EnvironmentVariableType::Plain,
                api.clone(),
            ),
            variable(
                ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID,
                "api",
                EnvironmentVariableType::Plain,
                api,
            ),
            fixture.user_variable,
        ],
        "the acquired config must carry the same variables the manager's loop injects"
    );
    assert!(
        config.environment_variables.hash.starts_with("env-"),
        "the snapshot hash is the manager's content hash, not a placeholder: {}",
        config.environment_variables.hash
    );
    assert_eq!(config.deployment_name.as_deref(), Some("app-1"));
    assert_eq!(
        config.deployment_token.as_deref(),
        Some(fixture.deployment_token.as_str())
    );
    assert_eq!(config.manager_url.as_deref(), Some(MANAGER_URL));
}

/// A config that cannot be built fails the acquire, and the lease it took is
/// given back: the caller never learned it held it, and the manager must not
/// wait out the lease before it can run the deployment.
#[tokio::test]
async fn an_acquire_whose_config_cannot_be_built_gives_back_the_lease() {
    // Without a deployment token on the record, the command-enabled container
    // has no commands token.
    let fixture = deployment_with_queued_update(false).await;

    let error = fixture.acquire_as_cli().await.expect_err(
        "the acquire must fail instead of returning a config without command variables",
    );
    assert!(
        format!("{error:?}").contains("COMMAND_TOKEN_MISSING"),
        "the error names the missing commands token: {error:?}"
    );

    let record = fixture
        .store
        .get_deployment(&Subject::system(), &fixture.deployment_id)
        .await
        .expect("deployment read")
        .expect("deployment exists");
    assert_eq!(record.locked_by, None, "the acquire's lease was released");
}
