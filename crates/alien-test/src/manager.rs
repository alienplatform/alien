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
    AlienManagerBuilder, ManagerConfig,
    stores::sqlite::{SqliteDatabase, SqliteTokenStore},
    traits::{CreateTokenParams, TokenStore, TokenType},
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
    /// The raw admin token (unhashed). Use this for `Authorization: Bearer`.
    pub admin_token: String,
    /// Pre-built SDK client pointing at this manager with the admin token
    /// already configured via a custom reqwest client.
    client: alien_manager_api::Client,
    /// Snapshot of the `TestConfig` used to start this manager (if provided).
    config: Option<TestConfig>,
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
        let ngrok_domain = config.and_then(|c| c.ngrok_domain.as_deref().map(str::to_string));
        let ngrok_tunnel = if std::env::var("NGROK_AUTHTOKEN").is_ok() && ngrok_domain.is_some() {
            let domain = ngrok_domain.as_deref().unwrap();
            info!(%domain, %port, "Starting ngrok tunnel for commands polling");
            match crate::ngrok::start_tunnel(port, Some(domain)).await {
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

        // 6. Build the manager configuration
        let targets: Vec<Platform> = platforms.to_vec();
        let manager_config = ManagerConfig {
            port,
            host: "127.0.0.1".to_string(),
            db_path: Some(db_path),
            state_dir: Some(state_dir.path().to_path_buf()),
            deployment_interval_secs: 2,
            heartbeat_interval_secs: 60,
            self_heartbeat_interval_secs: 60,
            otlp_endpoint: None,
            base_url: Some(base_url),
            releases_url: None,
            targets,
            disable_deployment_loop: false,
            disable_heartbeat_loop: true,
            enable_local_log_ingest: false,
        };

        // 7. Build the server (reuses the pre-created token store)
        let server = AlienManagerBuilder::new(manager_config)
            .token_store(token_store)
            .with_standalone_defaults()
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
                        return Err(
                            "TestManager: health check timed out after 50 attempts".into()
                        );
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
                    reqwest::header::HeaderValue::from_str(&format!(
                        "Bearer {}",
                        raw_token
                    ))
                    .expect("valid header value"),
                );
                headers
            })
            .build()?;

        let sdk_client = alien_manager_api::Client::new_with_client(&url, bearer_client);

        Ok(Self {
            port,
            url,
            admin_token: raw_token,
            client: sdk_client,
            config: config.cloned(),
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

    /// Build an authenticated `reqwest::Client` with the admin token set as the
    /// `Authorization: Bearer` header. Useful for hitting raw HTTP endpoints
    /// that are not covered by the SDK.
    pub fn http_client(&self) -> reqwest::Client {
        reqwest::Client::builder()
            .default_headers({
                let mut h = reqwest::header::HeaderMap::new();
                h.insert(
                    reqwest::header::AUTHORIZATION,
                    reqwest::header::HeaderValue::from_str(&format!(
                        "Bearer {}",
                        self.admin_token
                    ))
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

    /// Set environment variables from the test config for the given platforms so
    /// the manager's `EnvironmentCredentialResolver` can discover them.
    fn inject_credential_env_vars(config: &TestConfig, platforms: &[Platform]) {
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
                        std::env::set_var("AZURE_SUBSCRIPTION_ID", &mgmt.subscription_id);
                        std::env::set_var("AZURE_TENANT_ID", &mgmt.tenant_id);
                        std::env::set_var("AZURE_CLIENT_ID", &mgmt.client_id);
                        std::env::set_var("AZURE_CLIENT_SECRET", &mgmt.client_secret);
                        std::env::set_var("AZURE_REGION", &mgmt.region);
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
