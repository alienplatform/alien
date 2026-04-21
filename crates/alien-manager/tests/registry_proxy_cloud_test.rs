//! Cloud integration tests for the OCI registry proxy.
//!
//! Tests the full push→pull cycle through the proxy against real cloud
//! registries (ECR, GAR, ACR). Each test:
//! 1. Starts a manager with a real cloud artifact registry binding
//! 2. Builds a minimal OCI image
//! 3. Pushes it through the proxy to the upstream cloud registry
//! 4. Creates a release + deployment with the pushed image
//! 5. Pulls the manifest back through the proxy with a deployment token
//!
//! Requires `.env.test` with cloud credentials. Skips gracefully if
//! credentials are missing (local dev without cloud access).
//!
//! Runs in `cloud-tests.yml` CI workflow.

use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use alien_core::{
    DeploymentModel, DeploymentState, DeploymentStatus, Function, FunctionCode, Ingress, Platform,
    ReadinessProbe, ReleaseInfo, Stack, StackSettings,
};
use alien_manager::config::ManagerConfig;
use alien_manager::stores::sqlite::{
    SqliteDatabase, SqliteDeploymentStore, SqliteReleaseStore, SqliteTokenStore,
};
use alien_manager::traits::{
    CreateDeploymentGroupParams, CreateDeploymentParams, CreateReleaseParams, CreateTokenParams,
    DeploymentStore, ReconcileData, ReleaseStore, TokenStore, TokenType,
};
use alien_manager::AlienManagerBuilder;
use sha2::{Digest, Sha256};
use tracing::info;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load_test_env() {
    let root = workspace_root::get_workspace_root();
    let _ = dotenvy::from_path(root.join(".env.test"));
}

/// Skip if a required env var is missing or empty.
macro_rules! require_env {
    ($var:expr) => {
        match std::env::var($var) {
            Ok(v) if !v.is_empty() => v,
            _ => {
                eprintln!("SKIP: {} not set, skipping cloud proxy test", $var);
                return;
            }
        }
    };
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn hash_token(raw: &str) -> (String, String) {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    (
        raw[..12.min(raw.len())].to_string(),
        format!("{:x}", hasher.finalize()),
    )
}

fn test_stack(function_id: &str, image_uri: &str) -> Stack {
    let function = Function::new(function_id.to_string())
        .code(FunctionCode::Image {
            image: image_uri.to_string(),
        })
        .permissions("execution".to_string())
        .ingress(Ingress::Public)
        .memory_mb(256)
        .timeout_seconds(30)
        .commands_enabled(false)
        .readiness_probe(ReadinessProbe::default())
        .build();
    Stack::new("test".to_string())
        .add(function, alien_core::ResourceLifecycle::Live)
        .build()
}

/// Build a minimal OCI image and return the path to the tarball.
async fn build_test_image(
    registry_host: &str,
    repo_name: &str,
    tag: &str,
) -> (String, std::path::PathBuf) {
    let image_name = format!("{}/{}:{}", registry_host, repo_name, tag);
    let temp_dir = tempfile::tempdir().unwrap();
    let output_file = temp_dir.path().join("image.oci.tar");

    // Use unique content large enough to force chunked upload (not monolithic).
    // Small layers use monolithic upload (single PUT) which skips Location
    // redirects. Large layers trigger POST → Location → PUT, which exercises
    // the proxy's Location header rewriting (GAR rewrites /default/ → /pkg/).
    let mut large_content = format!(
        "proxy cloud integration test — run {}\n",
        uuid::Uuid::new_v4()
    )
    .into_bytes();
    // Pad to ~400MB to reproduce large E2E image layers
    large_content.resize(400 * 1024 * 1024, b'x');
    let layer = dockdash::Layer::builder()
        .unwrap()
        .data("app/test.bin", &large_content, None)
        .unwrap()
        .build()
        .await
        .unwrap();

    let (image, _) = dockdash::Image::builder()
        .platform("linux", &dockdash::Arch::Amd64)
        .layer(layer)
        .cmd(vec!["cat".to_string(), "/app/test.txt".to_string()])
        .output_to(output_file.clone())
        .output_name_and_tag(&image_name)
        .build()
        .await
        .unwrap();

    // Keep temp dir alive
    std::mem::forget(temp_dir);
    (image_name, output_file)
}

// ---------------------------------------------------------------------------
// Test harness: start manager + push + create deployment + pull
// ---------------------------------------------------------------------------

struct CloudProxyTest {
    manager_url: String,
    admin_token: String,
    deployment_token: String,
    deployment_id: String,
    _server_handle: tokio::task::JoinHandle<()>,
    _state_dir: tempfile::TempDir,
}

impl CloudProxyTest {
    /// Start a manager with the given cloud binding and create test data.
    /// If `base_url_override` is set, the manager uses it as its public base URL
    /// (e.g., an ngrok tunnel URL). Location headers will be rewritten to this URL.
    async fn start(
        platform: Platform,
        binding_env_var: &str,
        binding_json: &str,
        env_vars: HashMap<String, String>,
    ) -> Self {
        Self::start_with_base_url(
            platform,
            binding_env_var,
            binding_json,
            env_vars,
            None::<(String, u16)>,
        )
        .await
    }

    async fn start_with_base_url(
        platform: Platform,
        binding_env_var: &str,
        binding_json: &str,
        env_vars: HashMap<String, String>,
        base_url_override: Option<(String, u16)>,
    ) -> Self {
        let port = base_url_override
            .as_ref()
            .map(|(_, p)| *p)
            .unwrap_or_else(free_port);
        let manager_url = format!("http://127.0.0.1:{}", port);
        let state_dir = tempfile::tempdir().unwrap();
        let db_path = state_dir.path().join("test.db");

        // Set cloud credentials and binding in the PROCESS environment so
        // with_standalone_defaults() picks them up.
        for (k, v) in &env_vars {
            std::env::set_var(k, v);
        }
        // Set as both the platform-specific binding AND the primary binding.
        // The primary binding is what load_artifact_registry() checks first.
        std::env::set_var(binding_env_var, binding_json);
        std::env::set_var("ALIEN_ARTIFACTS_BINDING", binding_json);

        let env_map: HashMap<String, String> = std::env::vars().collect();

        // Create DB
        let db = Arc::new(
            SqliteDatabase::new(&db_path.to_string_lossy())
                .await
                .unwrap(),
        );
        let token_store: Arc<dyn TokenStore> = Arc::new(SqliteTokenStore::new(db.clone()));
        let deployment_store: Arc<dyn DeploymentStore> =
            Arc::new(SqliteDeploymentStore::new(db.clone()));
        let release_store: Arc<dyn ReleaseStore> = Arc::new(SqliteReleaseStore::new(db.clone()));

        // Create admin token
        let admin_raw = format!(
            "ax_admin_{}",
            uuid::Uuid::new_v4().to_string().replace('-', "")
        );
        let (pfx, hash) = hash_token(&admin_raw);
        token_store
            .create_token(CreateTokenParams {
                token_type: TokenType::Admin,
                key_prefix: pfx,
                key_hash: hash,
                deployment_group_id: None,
                deployment_id: None,
            })
            .await
            .unwrap();

        // Create deployment token
        let deploy_raw = format!(
            "ax_dep_{}",
            uuid::Uuid::new_v4().to_string().replace('-', "")
        );

        // Create bindings provider from env
        let bindings_provider = Arc::new(
            alien_bindings::BindingsProvider::from_env(env_map)
                .await
                .expect("Failed to create bindings provider"),
        );

        // Build manager
        let config = ManagerConfig {
            port,
            host: "127.0.0.1".to_string(),
            db_path: Some(db_path),
            state_dir: Some(state_dir.path().to_path_buf()),
            deployment_interval_secs: 999,
            heartbeat_interval_secs: 999,
            self_heartbeat_interval_secs: 999,
            otlp_endpoint: None,
            base_url: Some(
                base_url_override
                    .as_ref()
                    .map(|(url, _)| url.clone())
                    .unwrap_or(manager_url.clone()),
            ),
            releases_url: None,
            targets: vec![platform],
            disable_deployment_loop: true,
            disable_heartbeat_loop: true,
            enable_local_log_ingest: false,
            allowed_origins: None,
            response_signing_key: b"test-signing-key".to_vec(),
        };

        let toml_config = alien_manager::standalone_config::ManagerTomlConfig::default();
        let server = AlienManagerBuilder::new(config)
            .token_store(token_store.clone())
            .bindings_provider(bindings_provider)
            .with_standalone_defaults(&toml_config)
            .await
            .unwrap()
            .build()
            .await
            .unwrap();

        let addr: std::net::SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
        let server_handle = tokio::spawn(async move {
            let _ = server.start(addr).await;
        });

        // Wait for health
        let client = reqwest::Client::new();
        for _ in 0..50 {
            if client
                .get(format!("{}/health", manager_url))
                .send()
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Create deployment group + deployment
        let dg = deployment_store
            .create_deployment_group(CreateDeploymentGroupParams {
                name: "cloud-proxy-test".to_string(),
                max_deployments: 100,
            })
            .await
            .unwrap();

        let dep = deployment_store
            .create_deployment(CreateDeploymentParams {
                name: "cloud-proxy-deploy".to_string(),
                deployment_group_id: dg.id.clone(),
                platform,
                stack_settings: StackSettings {
                    deployment_model: DeploymentModel::Pull,
                    ..Default::default()
                },
                environment_variables: None,
                deployment_token: Some(deploy_raw.clone()),
            })
            .await
            .unwrap();

        // Register the deployment token in the token store
        let (dep_pfx, dep_hash) = hash_token(&deploy_raw);
        token_store
            .create_token(CreateTokenParams {
                token_type: TokenType::Deployment,
                key_prefix: dep_pfx,
                key_hash: dep_hash,
                deployment_group_id: Some(dg.id.clone()),
                deployment_id: Some(dep.id.clone()),
            })
            .await
            .unwrap();

        Self {
            manager_url,
            admin_token: admin_raw,
            deployment_token: deploy_raw,
            deployment_id: dep.id.clone(),
            _server_handle: server_handle,
            _state_dir: state_dir,
        }
    }

    /// Push an image through the proxy, create a release, assign it to the
    /// deployment, then pull the manifest back with the deployment token.
    async fn push_and_pull(&self, repo_name: &str, db_path: &std::path::Path) {
        let tag = format!("proxy-cloud-{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let manager_host = self.manager_url.trim_start_matches("http://");

        // Build and push through proxy
        let (image_name, _tar_path) = build_test_image(manager_host, repo_name, &tag).await;

        let image = dockdash::Image::from_tarball(&_tar_path).unwrap();
        image
            .push(
                &image_name,
                &dockdash::PushOptions {
                    auth: dockdash::RegistryAuth::Basic(
                        "token".to_string(),
                        self.admin_token.clone(),
                    ),
                    protocol: dockdash::ClientProtocol::Http,
                    ..Default::default()
                },
            )
            .await
            .expect("Push through proxy to cloud registry should succeed");

        info!(%image_name, "Push through proxy succeeded");

        // Create release with the pushed image URI
        let db = Arc::new(
            SqliteDatabase::new(&db_path.to_string_lossy())
                .await
                .unwrap(),
        );
        let release_store: Arc<dyn ReleaseStore> = Arc::new(SqliteReleaseStore::new(db.clone()));
        let deployment_store: Arc<dyn DeploymentStore> =
            Arc::new(SqliteDeploymentStore::new(db.clone()));

        let stack = test_stack("test-fn", &image_name);
        let release = release_store
            .create_release(CreateReleaseParams {
                project: None,
                caller_token: None,
                stacks: HashMap::from([(Platform::Local, stack.clone())]),
                git_commit_sha: None,
                git_commit_ref: None,
                git_commit_message: None,
            })
            .await
            .unwrap();

        // Assign release to deployment
        deployment_store
            .set_desired_release(&release.id, None)
            .await
            .unwrap();

        // Reconcile to set current_release_id
        let state = DeploymentState {
            status: DeploymentStatus::Running,
            platform: Platform::Local,
            current_release: Some(ReleaseInfo {
                release_id: release.id.clone(),
                version: None,
                description: None,
                stack,
            }),
            target_release: None,
            stack_state: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: 0,
        };
        deployment_store
            .reconcile(ReconcileData {
                deployment_id: self.deployment_id.clone(),
                session: "cloud-proxy-test".to_string(),
                state,
                update_heartbeat: false,
                error: None,
            })
            .await
            .unwrap();

        // Pull manifest through proxy with deployment token
        let client = reqwest::Client::new();
        let manifest_url = format!("{}/v2/{}/manifests/{}", self.manager_url, repo_name, tag);

        let resp = client
            .get(&manifest_url)
            .header(
                "Authorization",
                format!(
                    "Basic {}",
                    base64_encode(&format!("deployment:{}", self.deployment_token))
                ),
            )
            .header(
                "Accept",
                "application/vnd.oci.image.manifest.v1+json, \
                 application/vnd.oci.image.index.v1+json, \
                 application/vnd.docker.distribution.manifest.v2+json, \
                 */*",
            )
            .send()
            .await
            .unwrap();

        assert!(
            resp.status().is_success(),
            "Pull manifest through proxy with deployment token should succeed (got {}). \
             URL: {}, Body: {}",
            resp.status(),
            manifest_url,
            resp.text().await.unwrap_or_default(),
        );

        info!(%repo_name, %tag, "Pull through proxy with deployment token succeeded");
    }
}

fn base64_encode(input: &str) -> String {
    use base64::engine::{general_purpose::STANDARD, Engine};
    STANDARD.encode(input.as_bytes())
}

// ---------------------------------------------------------------------------
// AWS ECR
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_proxy_push_pull_ecr() {
    load_test_env();

    let region = require_env!("AWS_MANAGEMENT_REGION");
    let access_key = require_env!("AWS_MANAGEMENT_ACCESS_KEY_ID");
    let secret_key = require_env!("AWS_MANAGEMENT_SECRET_ACCESS_KEY");
    let account_id = require_env!("AWS_MANAGEMENT_ACCOUNT_ID");
    let push_role = require_env!("E2E_AWS_AR_PUSH_ROLE_ARN");
    let pull_role = require_env!("E2E_AWS_AR_PULL_ROLE_ARN");

    let binding = serde_json::json!({
        "service": "ecr",
        "repositoryPrefix": "alien-e2e",
        "pullRoleArn": pull_role,
        "pushRoleArn": push_role,
    });

    let mut env_vars = HashMap::new();
    env_vars.insert("AWS_REGION".into(), region);
    env_vars.insert("AWS_ACCESS_KEY_ID".into(), access_key);
    env_vars.insert("AWS_SECRET_ACCESS_KEY".into(), secret_key);
    env_vars.insert("AWS_ACCOUNT_ID".into(), account_id);
    env_vars.insert("ALIEN_DEPLOYMENT_TYPE".into(), "aws".into());

    let test = CloudProxyTest::start(
        Platform::Aws,
        "ALIEN_AWS_ARTIFACTS_BINDING",
        &binding.to_string(),
        env_vars,
    )
    .await;

    let db_path = test._state_dir.path().join("test.db");
    test.push_and_pull("alien-e2e", &db_path).await;
}

// ---------------------------------------------------------------------------
// GCP GAR
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_proxy_push_pull_gar() {
    load_test_env();

    let sa_key = require_env!("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY");
    let project_id = require_env!("GOOGLE_MANAGEMENT_PROJECT_ID");
    let region = require_env!("GOOGLE_MANAGEMENT_REGION");
    let gar_repo_url = require_env!("E2E_GCP_GAR_REPOSITORY");
    let push_sa = require_env!("E2E_GCP_AR_PUSH_SA_EMAIL");
    let pull_sa = require_env!("E2E_GCP_AR_PULL_SA_EMAIL");

    let gar_repo_name = gar_repo_url
        .rsplit('/')
        .next()
        .unwrap_or("alien-e2e")
        .to_string();

    let binding = serde_json::json!({
        "service": "gar",
        "repositoryName": gar_repo_name,
        "pullServiceAccountEmail": pull_sa,
        "pushServiceAccountEmail": push_sa,
    });

    let mut env_vars = HashMap::new();
    env_vars.insert("GOOGLE_SERVICE_ACCOUNT_KEY".into(), sa_key);
    env_vars.insert("GCP_PROJECT_ID".into(), project_id.clone());
    env_vars.insert("GCP_REGION".into(), region);
    env_vars.insert("ALIEN_DEPLOYMENT_TYPE".into(), "gcp".into());

    let test = CloudProxyTest::start(
        Platform::Gcp,
        "ALIEN_GCP_ARTIFACTS_BINDING",
        &binding.to_string(),
        env_vars,
    )
    .await;

    // GAR requires {project_id}/{gar_repo_name}/{image_name} as the OCI repo path
    let repo_name = format!("{}/{}/default", project_id, gar_repo_name);
    let db_path = test._state_dir.path().join("test.db");
    test.push_and_pull(&repo_name, &db_path).await;
}

/// Same as test_proxy_push_pull_gar but pushes through an ngrok HTTPS tunnel.
/// The manager's base_url is the ngrok URL, so Location header rewrites point
/// back through ngrok — reproducing the E2E flow exactly.
#[tokio::test]
async fn test_proxy_push_gar_via_ngrok() {
    load_test_env();

    let ngrok_token = require_env!("NGROK_AUTHTOKEN");
    let sa_key = require_env!("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY");
    let project_id = require_env!("GOOGLE_MANAGEMENT_PROJECT_ID");
    let region = require_env!("GOOGLE_MANAGEMENT_REGION");
    let gar_repo_url = require_env!("E2E_GCP_GAR_REPOSITORY");
    let push_sa = require_env!("E2E_GCP_AR_PUSH_SA_EMAIL");
    let pull_sa = require_env!("E2E_GCP_AR_PULL_SA_EMAIL");

    let gar_repo_name = gar_repo_url
        .rsplit('/')
        .next()
        .unwrap_or("alien-e2e")
        .to_string();

    let binding = serde_json::json!({
        "service": "gar",
        "repositoryName": gar_repo_name,
        "pullServiceAccountEmail": pull_sa,
        "pushServiceAccountEmail": push_sa,
    });

    let mut env_vars = HashMap::new();
    env_vars.insert("GOOGLE_SERVICE_ACCOUNT_KEY".into(), sa_key);
    env_vars.insert("GCP_PROJECT_ID".into(), project_id.clone());
    env_vars.insert("GCP_REGION".into(), region);
    env_vars.insert("ALIEN_DEPLOYMENT_TYPE".into(), "gcp".into());

    // Start manager first, then set up ngrok to forward to its port.
    // The manager's base_url is overridden to the ngrok URL so Location
    // headers are rewritten through the tunnel (reproducing E2E flow).

    // We need to know the ngrok URL before starting the manager (for base_url),
    // but we need the manager's port for the ngrok tunnel. Resolve by
    // pre-allocating a port, starting ngrok, then starting the manager.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let port = free_port();
    let forward_url = url::Url::parse(&format!("http://localhost:{}", port)).unwrap();

    let session = ngrok::Session::builder()
        .authtoken(ngrok_token)
        .connect()
        .await
        .expect("Failed to connect ngrok session");

    let ephemeral_domain = format!(
        "e2e-{}.ngrok.dev",
        uuid::Uuid::new_v4().to_string().replace('-', "")
    );

    use ngrok::config::ForwarderBuilder;
    use ngrok::tunnel::EndpointInfo;
    let forwarder = session
        .http_endpoint()
        .domain(&ephemeral_domain)
        .listen_and_forward(forward_url)
        .await
        .expect("Failed to start ngrok listener");

    let ngrok_url = forwarder.url().to_string();
    let ngrok_host = ngrok_url.trim_start_matches("https://");
    info!(%ngrok_url, %port, "Ngrok tunnel started for GAR push test");

    // Start manager on the same port, with ngrok URL as base_url.
    let test = CloudProxyTest::start_with_base_url(
        Platform::Gcp,
        "ALIEN_GCP_ARTIFACTS_BINDING",
        &binding.to_string(),
        env_vars,
        Some((ngrok_url.clone(), port)),
    )
    .await;

    let repo_name = format!("{}/{}/default", project_id, gar_repo_name);
    let tag = format!("ngrok-{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let (image_name, tar_path) = build_test_image(ngrok_host, &repo_name, &tag).await;

    let image = dockdash::Image::from_tarball(&tar_path).unwrap();
    image
        .push(
            &image_name,
            &dockdash::PushOptions {
                auth: dockdash::RegistryAuth::Basic("token".to_string(), test.admin_token.clone()),
                protocol: dockdash::ClientProtocol::Https,
                ..Default::default()
            },
        )
        .await
        .expect("Push through ngrok proxy to GAR should succeed");

    info!(%image_name, "Push through ngrok proxy to GAR succeeded");

    // Keep ngrok alive until here
    drop(forwarder);
}

// ---------------------------------------------------------------------------
// Azure ACR
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_proxy_push_pull_acr() {
    load_test_env();

    let subscription_id = require_env!("AZURE_MANAGEMENT_SUBSCRIPTION_ID");
    let tenant_id = require_env!("AZURE_MANAGEMENT_TENANT_ID");
    let client_id = require_env!("AZURE_MANAGEMENT_CLIENT_ID");
    let client_secret = require_env!("AZURE_MANAGEMENT_CLIENT_SECRET");
    let registry_name = require_env!("ALIEN_TEST_AZURE_REGISTRY_NAME");
    let resource_group = require_env!("ALIEN_TEST_AZURE_RESOURCE_GROUP");

    let binding = serde_json::json!({
        "service": "acr",
        "registryName": registry_name,
        "resourceGroupName": resource_group,
    });

    let mut env_vars = HashMap::new();
    env_vars.insert("AZURE_SUBSCRIPTION_ID".into(), subscription_id);
    env_vars.insert("AZURE_TENANT_ID".into(), tenant_id);
    env_vars.insert("AZURE_CLIENT_ID".into(), client_id);
    env_vars.insert("AZURE_CLIENT_SECRET".into(), client_secret);
    env_vars.insert("ALIEN_DEPLOYMENT_TYPE".into(), "azure".into());

    // OIDC for CI
    if let Ok(v) = std::env::var("AZURE_MANAGEMENT_OIDC_ISSUER") {
        env_vars.insert("AZURE_OIDC_ISSUER".into(), v);
    }
    if let Ok(v) = std::env::var("AZURE_MANAGEMENT_OIDC_SUBJECT") {
        env_vars.insert("AZURE_OIDC_SUBJECT".into(), v);
    }
    if let Ok(v) = std::env::var("AZURE_FEDERATED_TOKEN_FILE") {
        env_vars.insert("AZURE_FEDERATED_TOKEN_FILE".into(), v);
    }

    let test = CloudProxyTest::start(
        Platform::Azure,
        "ALIEN_AZURE_ARTIFACTS_BINDING",
        &binding.to_string(),
        env_vars,
    )
    .await;

    let db_path = test._state_dir.path().join("test.db");
    test.push_and_pull("alien-e2e", &db_path).await;
}
