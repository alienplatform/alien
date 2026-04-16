//! `TestManager` -- start and stop a standalone alien-manager in-process.
//!
//! Each test gets its own manager with an ephemeral SQLite database, a random
//! port, and a freshly bootstrapped admin token.

use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::sync::Arc;

use alien_core::Platform;
use sha2::{Digest, Sha256};
use tracing::info;

use alien_manager::{
    stores::sqlite::{SqliteDatabase, SqliteTokenStore},
    traits::{CreateTokenParams, TokenStore, TokenType},
    AlienManagerBuilder, ManagerConfig, ManagerTomlConfig,
};

use crate::config::TestConfig;

/// Find a free TCP port by binding to port 0 and reading back the assigned port.
fn find_free_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("failed to bind to port 0");
    let port = listener.local_addr().expect("no local addr").port();
    drop(listener);
    port
}

/// A running alien-manager instance for E2E tests.
///
/// Call [`TestManager::start`] to spin one up. The manager runs in a background
/// tokio task and is stopped when the value is dropped (the task is aborted) or
/// when [`TestManager::stop`] is called explicitly.
pub struct TestManager {
    /// The TCP port the manager is listening on.
    pub port: u16,
    /// Full base URL, e.g. `http://127.0.0.1:12345`.
    pub url: String,
    /// Public base URL (ngrok URL when tunnel is active, otherwise same as `url`).
    /// Use this for image URI rewriting — it must be reachable from cloud platforms.
    pub public_url: String,
    /// The raw admin token (unhashed). Use this for `Authorization: Bearer`.
    pub admin_token: String,
    /// Pre-built SDK client pointing at this manager with the admin token
    /// already configured via a custom reqwest client.
    client: alien_manager_api::Client,
    /// Snapshot of the `TestConfig` used to start this manager (if provided).
    config: Option<TestConfig>,
    /// Management config for cross-account deployment (if configured).
    management_config: Option<alien_core::ManagementConfig>,
    /// Token store for creating deployment tokens in tests.
    token_store: Arc<dyn TokenStore>,
    /// Temp directory backing the SQLite database. Dropped after the manager
    /// shuts down so the directory is cleaned up.
    _state_dir: tempfile::TempDir,
    /// Handle to the background task running the server.
    server_handle: Option<tokio::task::JoinHandle<()>>,
    /// Ngrok tunnel handle. Kept alive so the tunnel stays open for the
    /// duration of the test. Cloud-deployed functions poll this URL for commands.
    _ngrok_tunnel: Option<crate::ngrok::NgrokTunnel>,
}

impl TestManager {
    /// Start a standalone manager on a random available port.
    ///
    /// The manager is backed by an ephemeral SQLite database in a temp dir and
    /// has a freshly-bootstrapped admin token. Cloud credentials from `config`
    /// are set as environment variables so the manager's credential resolver can
    /// pick them up. `platforms` lists the target platforms to configure; if
    /// empty, the manager starts with no configured targets.
    pub async fn start_with_config(
        config: &TestConfig,
        platforms: &[Platform],
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Self::start_inner(Some(config), platforms).await
    }

    /// Start a standalone manager with no cloud credentials.
    ///
    /// Useful for tests that only exercise the manager API surface without
    /// deploying to a real cloud environment.
    pub async fn start() -> Result<Self, Box<dyn std::error::Error>> {
        Self::start_inner(None, &[]).await
    }

    /// Internal constructor shared by both `start()` and `start_with_config()`.
    async fn start_inner(
        config: Option<&TestConfig>,
        platforms: &[Platform],
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // 1. Ephemeral state directory
        let state_dir = tempfile::tempdir()?;
        let db_path = state_dir.path().join("test.db");

        // 2. Find a free port
        let port = find_free_port();
        let url = format!("http://127.0.0.1:{}", port);

        // 2b. Start ngrok tunnel if configured. Cloud-deployed functions
        //     (Lambda, Cloud Run, Container Apps) need to reach the local
        //     manager for commands polling. The ngrok URL becomes the
        //     manager's `base_url` so `ALIEN_COMMANDS_POLLING_URL` points
        //     to the publicly reachable tunnel.
        let has_ngrok_token = std::env::var("NGROK_AUTHTOKEN")
            .ok()
            .filter(|s| !s.is_empty())
            .is_some();
        let ngrok_tunnel = if has_ngrok_token {
            info!(%port, "Starting ephemeral ngrok tunnel for commands polling");
            match crate::ngrok::start_tunnel(port).await {
                Ok(tunnel) => {
                    info!(tunnel_url = %tunnel.url, "Ngrok tunnel ready");
                    Some(tunnel)
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to start ngrok tunnel, commands polling will use localhost");
                    None
                }
            }
        } else {
            None
        };

        let base_url = if let Some(ref tunnel) = ngrok_tunnel {
            tunnel.url.clone()
        } else {
            url.clone()
        };

        // 3. Generate admin token
        let raw_token = format!(
            "ax_admin_{}",
            uuid::Uuid::new_v4().to_string().replace('-', "")
        );
        let key_hash = {
            let mut hasher = Sha256::new();
            hasher.update(raw_token.as_bytes());
            hex::encode(hasher.finalize())
        };
        let key_prefix = raw_token[..12.min(raw_token.len())].to_string();

        // 4. Open the database and bootstrap the token
        let db = Arc::new(SqliteDatabase::new(&db_path.to_string_lossy()).await?);
        let token_store: Arc<dyn TokenStore> = Arc::new(SqliteTokenStore::new(db.clone()));

        token_store
            .create_token(CreateTokenParams {
                token_type: TokenType::Admin,
                key_prefix,
                key_hash,
                deployment_group_id: None,
                deployment_id: None,
            })
            .await?;

        info!(%port, %url, "TestManager: starting");

        // 5. Set ALIEN_API_KEY so the manager's preflight checks
        //    (DnsTlsRequiredCheck, HorizonRequiredCheck) skip themselves.
        //    These checks block public ingress and containers on cloud
        //    platforms unless the alien.dev platform is available. In E2E
        //    tests, cloud providers supply their own URLs (Lambda function
        //    URLs, Cloud Run URLs, etc.) so the checks are unnecessary.
        //    The standalone manager does not use this env var for anything
        //    else — it only affects preflight `should_run()` logic.
        std::env::set_var("ALIEN_API_KEY", &raw_token);

        // 6. Inject cloud credential env vars so the manager's
        //    EnvironmentCredentialResolver can discover them.
        if let Some(cfg) = config {
            Self::inject_credential_env_vars(cfg, platforms);
        }

        // 6b. Build ManagementConfig (used by setup_target to simulate alien-deploy).
        //     The manager itself does not need ManagementConfig at init time —
        //     it resolves per-deployment via the CredentialResolver trait.
        let management_config =
            config.and_then(|cfg| Self::build_management_config(cfg, platforms.first().copied()?));

        // 6c. Build typed bindings for the TOML config from test resources.
        //     This replaces the old env var injection pattern — all binding
        //     configuration now flows through ManagerTomlConfig, matching
        //     what `alien serve` does in production.
        let artifact_registry = Self::build_artifact_registry_config(config, platforms);
        let commands = Self::build_commands_config(config, platforms);
        let impersonation = Self::build_impersonation_config(config, platforms);

        // 7. Build the manager configuration via ManagerTomlConfig, validating
        //    the TOML config → ManagerConfig conversion path end-to-end.
        let toml_config = ManagerTomlConfig {
            server: alien_manager::standalone_config::ServerConfig {
                port,
                host: "127.0.0.1".to_string(),
                base_url: Some(base_url.clone()),
                releases_url: None,
                deployment_interval_secs: 2,
                heartbeat_interval_secs: 60,
            },
            database: alien_manager::standalone_config::DatabaseConfig {
                path: Some(db_path),
                state_dir: state_dir.path().to_path_buf(),
                encryption_key: None,
            },
            telemetry: alien_manager::standalone_config::TelemetryConfig {
                otlp_endpoint: None,
                headers: Default::default(),
            },
            artifact_registry,
            commands,
            impersonation,
            ..ManagerTomlConfig::default()
        };

        // Validate round-trip: the TOML config we built must serialize and
        // deserialize without loss. This catches schema drift early.
        let toml_str = toml::to_string_pretty(&toml_config)
            .expect("ManagerTomlConfig should serialize to TOML");
        let _round_tripped: ManagerTomlConfig =
            toml::from_str(&toml_str).expect("serialized TOML should parse back");

        let targets: Vec<Platform> = platforms.to_vec();
        let mut manager_config = toml_config.to_manager_config();
        // Override fields that ManagerTomlConfig.to_manager_config() doesn't set
        // or sets differently for production use.
        manager_config.targets = targets;
        manager_config.disable_heartbeat_loop = true;

        // 7. Build the server (reuses the pre-created token store).
        //    with_standalone_defaults() reads typed bindings from toml_config
        //    and starts an embedded local registry when no cloud AR is configured.
        let server = AlienManagerBuilder::new(manager_config)
            .token_store(token_store.clone())
            .with_standalone_defaults(&toml_config)
            .await?
            .build()
            .await?;

        // 8. Spawn the server in a background task
        let addr: SocketAddr = format!("127.0.0.1:{}", port).parse()?;
        let server_handle = tokio::spawn(async move {
            if let Err(e) = server.start(addr).await {
                tracing::error!(error = %e, "TestManager server error");
            }
        });

        // 9. Wait for the health endpoint to respond
        let http_client = reqwest::Client::new();
        let health_url = format!("{}/health", url);
        let mut attempts = 0;
        loop {
            match http_client.get(&health_url).send().await {
                Ok(resp) if resp.status().is_success() => break,
                _ => {
                    attempts += 1;
                    if attempts > 50 {
                        server_handle.abort();
                        return Err("TestManager: health check timed out after 50 attempts".into());
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }

        info!(%url, "TestManager: healthy and ready");

        // 10. Build an authenticated SDK client
        let bearer_client = reqwest::Client::builder()
            .default_headers({
                let mut headers = reqwest::header::HeaderMap::new();
                headers.insert(
                    reqwest::header::AUTHORIZATION,
                    reqwest::header::HeaderValue::from_str(&format!("Bearer {}", raw_token))
                        .expect("valid header value"),
                );
                headers
            })
            .build()?;

        let sdk_client = alien_manager_api::Client::new_with_client(&url, bearer_client);

        Ok(Self {
            port,
            url,
            public_url: base_url,
            admin_token: raw_token,
            client: sdk_client,
            config: config.cloned(),
            management_config,
            token_store,
            _state_dir: state_dir,
            server_handle: Some(server_handle),
            _ngrok_tunnel: ngrok_tunnel,
        })
    }

    /// Get a reference to the pre-authenticated SDK client.
    pub fn client(&self) -> &alien_manager_api::Client {
        &self.client
    }

    /// Get a reference to the test configuration used to start this manager, if
    /// one was provided.
    pub fn test_config(&self) -> Option<&TestConfig> {
        self.config.as_ref()
    }

    /// Get the management config for cross-account deployment, if configured.
    pub fn management_config(&self) -> Option<alien_core::ManagementConfig> {
        self.management_config.clone()
    }

    /// Create a deployment-scoped token for proxy auth.
    pub async fn create_deployment_token(
        &self,
        deployment_group_id: &str,
        deployment_id: &str,
    ) -> anyhow::Result<String> {
        let raw_token = format!("dep-{}", uuid::Uuid::new_v4().simple());
        let key_hash = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(raw_token.as_bytes());
            hex::encode(hasher.finalize())
        };
        let key_prefix = raw_token[..12.min(raw_token.len())].to_string();

        self.token_store
            .create_token(CreateTokenParams {
                token_type: TokenType::Deployment,
                key_prefix,
                key_hash,
                deployment_group_id: Some(deployment_group_id.to_string()),
                deployment_id: Some(deployment_id.to_string()),
            })
            .await?;

        Ok(raw_token)
    }

    /// Build an authenticated `reqwest::Client` with the admin token set as the
    /// `Authorization: Bearer` header. Useful for hitting raw HTTP endpoints
    /// that are not covered by the SDK.
    pub fn http_client(&self) -> reqwest::Client {
        reqwest::Client::builder()
            .default_headers({
                let mut h = reqwest::header::HeaderMap::new();
                h.insert(
                    reqwest::header::AUTHORIZATION,
                    reqwest::header::HeaderValue::from_str(&format!("Bearer {}", self.admin_token))
                        .expect("valid header value"),
                );
                h
            })
            .build()
            .expect("failed to build reqwest client")
    }

    /// Gracefully stop the manager.
    pub async fn stop(mut self) {
        if let Some(handle) = self.server_handle.take() {
            handle.abort();
            let _ = handle.await;
        }
    }

    /// Build a `ManagementConfig` from the management credentials for the given platform.
    ///
    /// For AWS: extracts a role ARN from `AWS_MANAGEMENT_ROLE_ARN` env var (set by Terraform).
    /// For GCP: extracts the SA email from the management key JSON.
    fn build_management_config(
        config: &TestConfig,
        platform: Platform,
    ) -> Option<alien_core::ManagementConfig> {
        match platform {
            Platform::Aws => {
                let role_arn = std::env::var("AWS_MANAGEMENT_ROLE_ARN").ok()?;
                Some(alien_core::ManagementConfig::Aws(
                    alien_core::AwsManagementConfig {
                        managing_role_arn: role_arn,
                    },
                ))
            }
            Platform::Gcp => {
                let mgmt = config.gcp_mgmt.as_ref()?;
                let email = mgmt.management_identity_email.as_ref()?;
                Some(alien_core::ManagementConfig::Gcp(
                    alien_core::GcpManagementConfig {
                        service_account_email: email.clone(),
                    },
                ))
            }
            Platform::Azure => {
                let mgmt = config.azure_mgmt.as_ref()?;
                Some(alien_core::ManagementConfig::Azure(
                    alien_core::AzureManagementConfig {
                        managing_tenant_id: mgmt.tenant_id.clone(),
                        oidc_issuer: mgmt.oidc_issuer.clone(),
                        oidc_subject: mgmt.oidc_subject.clone(),
                        management_principal_id: if mgmt.oidc_issuer.is_none() {
                            mgmt.management_sp_object_id.clone()
                        } else {
                            None
                        },
                    },
                ))
            }
            _ => None,
        }
    }

    /// Build artifact registry config section from test resources.
    fn build_artifact_registry_config(
        config: Option<&TestConfig>,
        platforms: &[Platform],
    ) -> alien_manager::standalone_config::ArtifactRegistrySection {
        use alien_core::bindings::{
            AcrArtifactRegistryBinding, ArtifactRegistryBinding, BindingValue,
            EcrArtifactRegistryBinding, GarArtifactRegistryBinding,
        };

        let mut section = alien_manager::standalone_config::ArtifactRegistrySection::default();

        let config = match config {
            Some(c) => c,
            None => return section,
        };

        let e2e = &config.e2e_artifact_registry;

        for platform in platforms {
            match platform {
                Platform::Aws => {
                    if let (Some(push_role), Some(pull_role)) = (
                        e2e.aws_ar_push_role_arn.as_ref(),
                        e2e.aws_ar_pull_role_arn.as_ref(),
                    ) {
                        section.aws =
                            Some(ArtifactRegistryBinding::Ecr(EcrArtifactRegistryBinding {
                                repository_prefix: BindingValue::Value("alien-e2e".to_string()),
                                pull_role_arn: Some(BindingValue::Value(pull_role.clone())),
                                push_role_arn: Some(BindingValue::Value(push_role.clone())),
                            }));
                    }
                }
                Platform::Gcp => {
                    let repo_name = e2e
                        .gcp_gar_repository
                        .as_ref()
                        .and_then(|url| url.rsplit('/').next().map(|s| s.to_string()))
                        .unwrap_or_else(|| "alien-e2e".to_string());
                    section.gcp = Some(ArtifactRegistryBinding::Gar(GarArtifactRegistryBinding {
                        repository_name: BindingValue::Value(repo_name),
                        pull_service_account_email: e2e
                            .gcp_ar_pull_sa_email
                            .clone()
                            .map(BindingValue::Value),
                        push_service_account_email: e2e
                            .gcp_ar_push_sa_email
                            .clone()
                            .map(BindingValue::Value),
                    }));
                }
                Platform::Azure => {
                    if let (Some(registry_name), Some(resource_group)) = (
                        config.azure_resources.registry_name.as_ref(),
                        config.azure_resources.resource_group.as_ref(),
                    ) {
                        section.azure =
                            Some(ArtifactRegistryBinding::Acr(AcrArtifactRegistryBinding {
                                registry_name: BindingValue::Value(registry_name.clone()),
                                resource_group_name: BindingValue::Value(resource_group.clone()),
                                repository_prefix: Some(BindingValue::Value(
                                    "azure-e2e".to_string(),
                                )),
                            }));
                    }
                }
                _ => {}
            }
        }

        section
    }

    /// Build commands config section.
    /// Returns default (local filesystem) — cloud-backed commands storage
    /// is configured per-platform via the TOML config in production.
    fn build_commands_config(
        config: Option<&TestConfig>,
        platforms: &[Platform],
    ) -> alien_manager::standalone_config::CommandsSection {
        use alien_core::bindings::{
            BindingValue, BlobStorageBinding, GcsStorageBinding, KvBinding, S3StorageBinding,
            StorageBinding,
        };

        let config = match config {
            Some(c) => c,
            None => return Default::default(),
        };

        // Cloud platforms need cloud-backed storage for commands. Functions submit
        // responses via the manager's API (through ngrok), and the manager reads/writes
        // to its own commands storage. We use DynamoDB + S3 for ALL cloud platforms
        // because the manager always runs with AWS credentials in the test setup.
        // This avoids needing per-platform storage tables (Azure Table, GCP Firestore).
        if !platforms.is_empty() && !platforms.iter().all(|p| *p == Platform::Local) {
            if let (Some(table), Some(bucket)) = (
                config.aws_resources.command_kv_table.as_ref(),
                config.aws_resources.s3_bucket.as_ref(),
            ) {
                let region = config
                    .aws_mgmt
                    .as_ref()
                    .map(|m| m.region.clone())
                    .unwrap_or_else(|| "us-east-1".to_string());

                return alien_manager::standalone_config::CommandsSection {
                    kv: Some(KvBinding::dynamodb(table.clone(), region)),
                    storage: Some(StorageBinding::S3(S3StorageBinding {
                        bucket_name: BindingValue::Value(bucket.clone()),
                    })),
                };
            }
        }

        // Local platform: local filesystem is fine
        Default::default()
    }

    /// Build impersonation config section from management credentials.
    ///
    /// Maps test management credentials into `ServiceAccountBinding` entries
    /// so the manager's target bindings providers can load the management
    /// identity for cross-account access.
    fn build_impersonation_config(
        config: Option<&TestConfig>,
        platforms: &[Platform],
    ) -> alien_manager::standalone_config::ImpersonationSection {
        use alien_core::bindings::{BindingValue, ServiceAccountBinding};

        let mut section = alien_manager::standalone_config::ImpersonationSection::default();

        let config = match config {
            Some(c) => c,
            None => return section,
        };

        for platform in platforms {
            match platform {
                Platform::Aws => {
                    let role_arn = match std::env::var("AWS_MANAGEMENT_ROLE_ARN") {
                        Ok(arn) => arn,
                        Err(_) => continue,
                    };
                    let role_name = std::env::var("AWS_MANAGEMENT_ROLE_NAME")
                        .unwrap_or_else(|_| "alien-test-management".to_string());
                    section.aws = Some(ServiceAccountBinding::aws_iam(
                        BindingValue::value(role_name),
                        BindingValue::value(role_arn),
                    ));
                }
                Platform::Gcp => {
                    let mgmt = match config.gcp_mgmt.as_ref() {
                        Some(m) => m,
                        None => continue,
                    };
                    let email = match mgmt.management_identity_email.as_ref() {
                        Some(e) => e.clone(),
                        None => continue,
                    };
                    let unique_id = match mgmt.management_identity_unique_id.as_ref() {
                        Some(u) => u.clone(),
                        None => continue,
                    };
                    section.gcp = Some(ServiceAccountBinding::gcp_service_account(
                        BindingValue::value(email),
                        BindingValue::value(unique_id),
                    ));
                }
                Platform::Azure => {
                    let mgmt = match config.azure_mgmt.as_ref() {
                        Some(m) => m,
                        None => continue,
                    };
                    let (client_id, object_id, resource_id) = if mgmt.oidc_issuer.is_some() {
                        ("oidc".to_string(), "oidc".to_string(), "oidc".to_string())
                    } else {
                        let client_id = match mgmt.management_sp_client_id.as_ref() {
                            Some(id) => id.clone(),
                            None => continue,
                        };
                        let object_id = match mgmt.management_sp_object_id.as_ref() {
                            Some(id) => id.clone(),
                            None => continue,
                        };
                        let resource_id = format!(
                            "/subscriptions/{}/resourceGroups/alien-test/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{}",
                            mgmt.subscription_id, client_id
                        );
                        (client_id, object_id, resource_id)
                    };
                    section.azure = Some(ServiceAccountBinding::azure_managed_identity(
                        BindingValue::value(client_id),
                        BindingValue::value(resource_id),
                        BindingValue::value(object_id),
                    ));
                }
                _ => {}
            }
        }

        section
    }

    /// Set environment variables from the test config for the given platforms so
    /// the manager's `EnvironmentCredentialResolver` can discover them.
    fn inject_credential_env_vars(config: &TestConfig, platforms: &[Platform]) {
        // Commands KV/storage always uses DynamoDB+S3 for cloud platforms,
        // so AWS credentials must be set even when AWS isn't a target platform.
        let has_cloud_platform = platforms.iter().any(|p| !matches!(p, Platform::Local));
        let has_aws_platform = platforms.iter().any(|p| matches!(p, Platform::Aws));

        if has_cloud_platform && !has_aws_platform {
            if let Some(ref mgmt) = config.aws_mgmt {
                std::env::set_var("AWS_ACCESS_KEY_ID", &mgmt.access_key_id);
                std::env::set_var("AWS_SECRET_ACCESS_KEY", &mgmt.secret_access_key);
                std::env::set_var("AWS_REGION", &mgmt.region);
                if let Some(ref token) = mgmt.session_token {
                    std::env::set_var("AWS_SESSION_TOKEN", token);
                }
                if let Some(ref account_id) = mgmt.account_id {
                    std::env::set_var("AWS_ACCOUNT_ID", account_id);
                }
            }
        }

        for platform in platforms {
            match platform {
                Platform::Aws => {
                    if let Some(ref mgmt) = config.aws_mgmt {
                        std::env::set_var("AWS_ACCESS_KEY_ID", &mgmt.access_key_id);
                        std::env::set_var("AWS_SECRET_ACCESS_KEY", &mgmt.secret_access_key);
                        std::env::set_var("AWS_REGION", &mgmt.region);
                        if let Some(ref token) = mgmt.session_token {
                            std::env::set_var("AWS_SESSION_TOKEN", token);
                        }
                        if let Some(ref account_id) = mgmt.account_id {
                            std::env::set_var("AWS_ACCOUNT_ID", account_id);
                        }
                    }
                }
                Platform::Gcp => {
                    if let Some(ref mgmt) = config.gcp_mgmt {
                        std::env::set_var("GCP_PROJECT_ID", &mgmt.project_id);
                        std::env::set_var("GCP_REGION", &mgmt.region);
                        if let Some(ref creds) = mgmt.credentials_json {
                            std::env::set_var("GOOGLE_SERVICE_ACCOUNT_KEY", creds);
                        }
                    }
                }
                Platform::Azure => {
                    if let Some(ref mgmt) = config.azure_mgmt {
                        // Execution identity: Terraform SP (has ACR push, storage access)
                        std::env::set_var("AZURE_SUBSCRIPTION_ID", &mgmt.subscription_id);
                        std::env::set_var("AZURE_TENANT_ID", &mgmt.tenant_id);
                        std::env::set_var("AZURE_CLIENT_ID", &mgmt.client_id);
                        std::env::set_var("AZURE_CLIENT_SECRET", &mgmt.client_secret);
                        std::env::set_var("AZURE_REGION", &mgmt.region);
                        std::env::set_var(
                            "AZURE_MANAGEMENT_OIDC_ISSUER",
                            mgmt.oidc_issuer.as_deref().unwrap_or(""),
                        );
                        std::env::set_var(
                            "AZURE_MANAGEMENT_OIDC_SUBJECT",
                            mgmt.oidc_subject.as_deref().unwrap_or(""),
                        );
                        // Management SP secret for local-development fallback impersonation
                        if let Some(ref sp_secret) = mgmt.management_sp_client_secret {
                            std::env::set_var("ALIEN_AZURE_MANAGEMENT_CLIENT_SECRET", sp_secret);
                        } else {
                            std::env::remove_var("ALIEN_AZURE_MANAGEMENT_CLIENT_SECRET");
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

impl Drop for TestManager {
    fn drop(&mut self) {
        // Best-effort: abort the background task so the port is freed.
        if let Some(handle) = self.server_handle.take() {
            handle.abort();
        }
    }
}
