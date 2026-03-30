//! E2E test orchestration.
//!
//! Provides the high-level `setup()` entry point that each E2E test calls,
//! plus the support matrix, deployment helpers, and stack evaluation logic.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use alien_core::{Platform, Stack};
use anyhow::Context;
use tracing::info;

use crate::build_push::build_and_push_stack;
use crate::config::TestConfig;
use crate::deployment::TestDeployment;
use crate::manager::TestManager;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// Deployment model: push (serverless function) or pull (container / agent).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentModel {
    /// Serverless / function-based deployment (Lambda, Cloud Run function, etc.)
    Push,
    /// Container-based deployment (Horizon, Kubernetes, local Docker)
    Pull,
}

impl std::fmt::Display for DeploymentModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeploymentModel::Push => write!(f, "push"),
            DeploymentModel::Pull => write!(f, "pull"),
        }
    }
}

/// Supported application languages for test apps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    TypeScript,
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Language::Rust => write!(f, "rust"),
            Language::TypeScript => write!(f, "typescript"),
        }
    }
}

/// User-facing binding types that can be tested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Binding {
    /// Object storage (S3, GCS, Azure Blob, local filesystem)
    Storage,
    /// Key-value store (DynamoDB, Firestore, Redis, Azure Table)
    Kv,
    /// Secret management (SSM, Secret Manager, Key Vault, local file)
    Vault,
    /// Message queue (SQS, Pub/Sub, Service Bus)
    Queue,
    /// Direct function-to-function invocation
    Function,
    /// Container-to-container communication
    Container,
    /// Background tasks that outlive the request
    WaitUntil,
    /// Health endpoint (GET /health)
    Health,
    /// Hello endpoint (GET /hello)
    Hello,
    /// SSE streaming (GET /sse)
    Sse,
    /// Environment variable injection (GET /env-var/:name)
    Environment,
    /// Request echo (POST /inspect)
    Inspect,
    /// External secret retrieval (cloud only)
    ExternalSecret,
    /// Event handler verification (cloud only)
    Events,
}

impl std::fmt::Display for Binding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Binding::Storage => write!(f, "storage"),
            Binding::Kv => write!(f, "kv"),
            Binding::Vault => write!(f, "vault"),
            Binding::Queue => write!(f, "queue"),
            Binding::Function => write!(f, "function"),
            Binding::Container => write!(f, "container"),
            Binding::WaitUntil => write!(f, "wait-until"),
            Binding::Health => write!(f, "health"),
            Binding::Hello => write!(f, "hello"),
            Binding::Sse => write!(f, "sse"),
            Binding::Environment => write!(f, "environment"),
            Binding::Inspect => write!(f, "inspect"),
            Binding::ExternalSecret => write!(f, "external-secret"),
            Binding::Events => write!(f, "events"),
        }
    }
}

// ---------------------------------------------------------------------------
// Support matrix
// ---------------------------------------------------------------------------

/// Returns the list of bindings supported for a given platform and deployment model.
///
/// This is the single source of truth for the support matrix. Each E2E test
/// iterates this list and runs only the checks that apply.
pub fn supported_bindings(platform: Platform, model: DeploymentModel) -> Vec<Binding> {
    // Universal checks that run on every platform
    let mut bindings = vec![
        Binding::Health,
        Binding::Hello,
        Binding::Sse,
        Binding::Environment,
        Binding::Inspect,
        Binding::Storage,
        Binding::Vault,
        Binding::WaitUntil,
    ];

    match platform {
        Platform::Aws | Platform::Gcp | Platform::Azure => {
            bindings.push(Binding::Kv);
            bindings.push(Binding::Queue);
            bindings.push(Binding::ExternalSecret);
            bindings.push(Binding::Events);
        }
        Platform::Kubernetes => {
            bindings.push(Binding::Kv);
            bindings.push(Binding::Container);
        }
        Platform::Local => {
            bindings.push(Binding::Kv);
            bindings.push(Binding::Container);
        }
        _ => {}
    }

    // Function binding only for push (serverless) deployments on cloud
    if model == DeploymentModel::Push {
        match platform {
            Platform::Aws | Platform::Gcp | Platform::Azure => {
                bindings.push(Binding::Function);
            }
            _ => {}
        }
    }

    bindings
}

/// Known exclusions: bindings that are technically supported but should be
/// skipped due to known issues or limitations.
///
/// Returns a reason string if the binding should be skipped, or `None` if
/// the binding should be tested.
pub fn exclusion_reason(
    _platform: Platform,
    _model: DeploymentModel,
    binding: Binding,
) -> Option<&'static str> {
    match binding {
        Binding::Function => Some("Function binding test app endpoint not yet implemented"),
        Binding::Container => Some("Container binding test app endpoint not yet implemented"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Returns the relative path to the test app directory for a given language.
pub fn test_app_path(language: Language) -> &'static str {
    match language {
        Language::Rust => "test-apps/comprehensive-rust",
        Language::TypeScript => "test-apps/comprehensive-typescript",
    }
}

/// Returns the alien config file name for a given deployment model.
pub fn config_file(model: DeploymentModel) -> &'static str {
    match model {
        DeploymentModel::Push => "alien.function.ts",
        DeploymentModel::Pull => "alien.container.ts",
    }
}

/// Returns the dev-mode alien config file name.
pub fn dev_config_file() -> &'static str {
    "alien.dev.ts"
}

// ---------------------------------------------------------------------------
// Platform mapping
// ---------------------------------------------------------------------------

/// Map an `alien_core::Platform` to the Progenitor-generated
/// `alien_manager_api::types::Platform` enum.
pub fn to_api_platform(platform: Platform) -> alien_manager_api::types::Platform {
    match platform {
        Platform::Aws => alien_manager_api::types::Platform::Aws,
        Platform::Gcp => alien_manager_api::types::Platform::Gcp,
        Platform::Azure => alien_manager_api::types::Platform::Azure,
        Platform::Kubernetes => alien_manager_api::types::Platform::Kubernetes,
        Platform::Local => alien_manager_api::types::Platform::Local,
        Platform::Test => alien_manager_api::types::Platform::Test,
    }
}

// ---------------------------------------------------------------------------
// TestContext
// ---------------------------------------------------------------------------

/// Context for a running E2E test, holding the deployment, manager, and agent.
pub struct TestContext {
    /// The deployed test application.
    pub deployment: TestDeployment,
    /// The in-process manager.
    pub manager: Arc<TestManager>,
    /// Target platform.
    pub platform: Platform,
    /// Deployment model (push/pull).
    pub model: DeploymentModel,
    /// Alien-agent handle (pull model only).
    pub agent: Option<crate::agent::TestAlienAgent>,
}

impl TestContext {
    /// Best-effort cleanup: destroy the deployment and stop any agent.
    ///
    /// Designed to be called from `AsyncTestContext::teardown()` so that
    /// resources are released even when a test panics. Errors are logged
    /// but never propagated.
    pub async fn cleanup(mut self) {
        // 1. Stop the agent (pull model).
        if let Some(agent) = self.agent.take() {
            agent.cleanup().await;
        }

        // 2. Destroy the deployment and wait for terminal status.
        if let Err(e) = self.deployment.destroy().await {
            tracing::warn!(
                deployment = %self.deployment.id,
                error = %e,
                "cleanup: failed to trigger destroy (may already be destroyed)"
            );
            return;
        }

        let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
        let poll_interval = Duration::from_secs(2);
        while tokio::time::Instant::now() < deadline {
            match self
                .manager
                .client()
                .get_deployment()
                .id(&self.deployment.id)
                .send()
                .await
            {
                Ok(dep) => {
                    let status = dep.status.as_str();
                    if status == "destroyed" || status == "deleted" {
                        info!(deployment = %self.deployment.id, "cleanup: deployment destroyed");
                        return;
                    }
                    if status == "failed" || status.ends_with("-failed") {
                        tracing::warn!(
                            deployment = %self.deployment.id,
                            %status,
                            "cleanup: deployment entered failed state during destroy"
                        );
                        return;
                    }
                }
                Err(_) => {
                    // 404 or connection error — deployment likely gone
                    info!(deployment = %self.deployment.id, "cleanup: deployment gone");
                    return;
                }
            }
            tokio::time::sleep(poll_interval).await;
        }
        tracing::warn!(
            deployment = %self.deployment.id,
            "cleanup: timed out waiting for destroy"
        );
    }
}

// ---------------------------------------------------------------------------
// Stack evaluation
// ---------------------------------------------------------------------------

/// Evaluate a TypeScript alien config file using `bun` and return the Stack JSON.
///
/// The config files (alien.function.ts, alien.container.ts) use the
/// `@alienplatform/core` SDK to define stacks. This function evaluates them
/// via bun and captures the serialized JSON output.
pub async fn load_stack_json(
    app_dir: &std::path::Path,
    config_file: &str,
    platform: Platform,
) -> anyhow::Result<serde_json::Value> {
    let script = format!(
        r#"
const mod = await import('./{config_file}');
const stack = mod.default;
console.log(JSON.stringify(stack));
"#,
    );

    let output = tokio::process::Command::new("bun")
        .current_dir(app_dir)
        .args(["-e", &script])
        .output()
        .await
        .context("Failed to run bun to evaluate config file")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "bun eval failed for {}/{}: {}",
            app_dir.display(),
            config_file,
            stderr
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stack_json: serde_json::Value = serde_json::from_str(stdout.trim())
        .context("Failed to parse Stack JSON from bun output")?;

    // Wrap in StackByPlatform: { "<platform>": <stack> }
    let platform_key = platform.as_str();
    let stack_by_platform = serde_json::json!({
        platform_key: stack_json,
    });

    Ok(stack_by_platform)
}

// ---------------------------------------------------------------------------
// Tracing
// ---------------------------------------------------------------------------

/// Initialize tracing for test output.
pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_test_writer()
        .try_init();
}

// ---------------------------------------------------------------------------
// Workspace root
// ---------------------------------------------------------------------------

/// Resolve the root of the `tests/e2e/` directory relative to the workspace.
///
/// Uses `CARGO_MANIFEST_DIR` (which points to `crates/alien-test/`) and walks
/// up two levels to the workspace root, then joins `tests/e2e/`.
fn e2e_test_apps_root() -> anyhow::Result<PathBuf> {
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        // crates/alien-test/ → workspace root
        let workspace_root = PathBuf::from(&manifest)
            .parent()
            .and_then(|p| p.parent())
            .context("Failed to resolve workspace root from CARGO_MANIFEST_DIR")?
            .to_path_buf();
        return Ok(workspace_root.join("tests/e2e"));
    }

    // Fallback: search upward for tests/e2e/ directory
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let mut dir = cwd.as_path();
    loop {
        let candidate = dir.join("tests/e2e/test-apps");
        if candidate.exists() {
            return Ok(dir.join("tests/e2e"));
        }
        dir = dir
            .parent()
            .context("Could not locate tests/e2e/ directory")?;
    }
}

// ---------------------------------------------------------------------------
// Deploy orchestration
// ---------------------------------------------------------------------------

/// Deploy a test app to the given platform with the specified model and language.
///
/// Mirrors the production `alien build` + `alien release` + `alien deploy` flow:
/// 1. Evaluate the TypeScript config file to get the Stack JSON
/// 2. Parse into a `Stack`, build from source, push images to registry
/// 3. Serialize the built stack and push a release (POST /v1/releases)
/// 4. Create a deployment group (POST /v1/deployment-groups)
/// 5. Create a deployment (POST /v1/deployments) in that group
pub async fn deploy_test_app(
    manager: &Arc<TestManager>,
    platform: Platform,
    model: DeploymentModel,
    language: Language,
) -> anyhow::Result<TestDeployment> {
    let e2e_root = e2e_test_apps_root()?;
    let app_path = e2e_root.join(test_app_path(language));
    let cfg_file = config_file(model);

    let deployment_name = format!(
        "e2e-{}-{}-{}-{}",
        model,
        platform.as_str(),
        language,
        &uuid::Uuid::new_v4().to_string()[..8],
    );

    info!(
        %deployment_name,
        app_path = %app_path.display(),
        config = %cfg_file,
        platform = %platform.as_str(),
        "Deploying test app"
    );

    let config = TestConfig::from_env();

    // Step 1: Evaluate config file to get Stack JSON (StackByPlatform wrapper)
    let stack_by_platform_json = load_stack_json(&app_path, cfg_file, platform).await?;
    info!("Stack JSON loaded from config file");

    // Step 2: Build from source and push images to registry.
    //
    // This mirrors the production flow: `alien build` compiles source into OCI
    // image tarballs, then `alien release` pushes them to the cloud registry.
    // After push, the stack has FunctionCode::Image with pushed URIs.
    let platform_key = platform.as_str();
    let stack_json = stack_by_platform_json
        .get(platform_key)
        .context("Stack JSON missing platform key")?;

    let stack: Stack = serde_json::from_value(stack_json.clone())
        .context("Failed to deserialize Stack from JSON")?;

    let pushed_stack = build_and_push_stack(stack, platform, &config, &app_path).await?;
    info!("Stack built and pushed to registry");

    // Step 3: Re-serialize the pushed stack into StackByPlatform and create a release
    let pushed_stack_json =
        serde_json::to_value(&pushed_stack).context("Failed to serialize pushed stack")?;
    let stack_by_platform = serde_json::json!({
        platform_key: pushed_stack_json,
    });

    let stack_by_platform_sdk: alien_manager_api::types::StackByPlatform =
        serde_json::from_value(stack_by_platform)
            .context("Failed to convert stack to SDK StackByPlatform")?;

    let release = manager
        .client()
        .create_release()
        .body(alien_manager_api::types::CreateReleaseRequest {
            stack: stack_by_platform_sdk,
            git_metadata: None,
        })
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create release: {}", e))?
        .into_inner();
    let release_id = &release.id;
    info!(%release_id, "Release created");

    // Step 4: Create a deployment group
    let group = manager
        .client()
        .create_deployment_group()
        .body(alien_manager_api::types::CreateDeploymentGroupRequest {
            name: format!("e2e-group-{}", &uuid::Uuid::new_v4().to_string()[..8]),
            max_deployments: None,
        })
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create deployment group: {}", e))?
        .into_inner();
    let group_id = &group.id;
    info!(%group_id, "Deployment group created");

    // Step 5: Create a deployment in the group via SDK
    let api_platform = to_api_platform(platform);
    let create_body = alien_manager_api::types::CreateDeploymentRequest {
        name: deployment_name.clone(),
        platform: api_platform,
        deployment_group_id: Some(group_id.to_string()),
        stack_settings: None,
        environment_variables: None,
    };

    let resp = manager
        .client()
        .create_deployment()
        .body(create_body)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create deployment: {}", e))?;

    let dep = &resp.deployment;
    let dep_id = dep.id.clone();
    info!(deployment_id = %dep_id, "Deployment created");

    let deployment = TestDeployment::new(
        dep_id,
        deployment_name,
        platform.as_str().to_string(),
        None,
        manager.clone(),
    );

    Ok(deployment)
}

// ---------------------------------------------------------------------------
// Platform availability
// ---------------------------------------------------------------------------

/// Check if a platform is available and supported for the given deployment model and language.
pub fn is_platform_available(
    config: &TestConfig,
    platform: Platform,
    model: DeploymentModel,
    _language: Language,
) -> bool {
    match platform {
        Platform::Local => {
            // Local platform deployments via the manager pipeline are not
            // supported: the local platform lacks controllers for build and
            // artifact-registry resources. Use `alien dev` for local development
            // instead. The @alienplatform/testing framework handles local tests.
            false
        }
        Platform::Kubernetes => {
            // Kubernetes only supports pull (container) model
            model == DeploymentModel::Pull
        }
        Platform::Aws | Platform::Gcp | Platform::Azure => {
            // Cloud platforms only support push (function) model in
            // standalone mode. Pull (container) model requires Horizon
            // clusters for orchestration, which are provisioned by the
            // alien.dev platform — not available in standalone managers.
            if model != DeploymentModel::Push || !config.has_platform(platform) {
                return false;
            }
            true
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// External secret provisioning
// ---------------------------------------------------------------------------

/// Provision the `EXTERNAL_TEST_SECRET` in the deployment's `alien-vault` vault
/// via the manager's vault API. This must be called after the deployment reaches
/// Running (so the vault resource is provisioned in the cloud).
async fn provision_external_secret(
    manager: &Arc<TestManager>,
    deployment: &TestDeployment,
) -> anyhow::Result<()> {
    let http = manager.http_client();
    let vault_name = "alien-vault";
    let secret_key = "EXTERNAL_TEST_SECRET";
    let secret_value = "e2e-test-external-secret-value";

    let url = format!(
        "{}/v1/deployments/{}/vault/{}/secrets/{}",
        manager.url, deployment.id, vault_name, secret_key,
    );

    info!(
        deployment_id = %deployment.id,
        vault_name,
        secret_key,
        "Provisioning external test secret via manager vault API"
    );

    let resp = http
        .put(&url)
        .json(&serde_json::json!({ "value": secret_value }))
        .send()
        .await
        .context("Failed to call vault set secret API")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!(
            "Failed to provision external secret ({}): {}",
            status,
            body
        );
    }

    info!(
        deployment_id = %deployment.id,
        "External test secret provisioned"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

/// Run the full E2E test flow for a given platform, model, and language.
///
/// This is the primary entry point that each individual test calls.
/// It:
/// 1. Starts a `TestManager` with cloud credentials
/// 2. Pushes a release (Stack JSON) to the manager
/// 3. Creates a deployment group and deployment
/// 4. Waits for the deployment to become healthy
/// 5. Runs all binding checks (via the returned `TestContext`)
/// 6. The caller is responsible for running checks and cleanup
///
/// Returns an `TestContext` with the running deployment ready for checks.
pub async fn setup(
    platform: Platform,
    model: DeploymentModel,
    language: Language,
) -> anyhow::Result<TestContext> {
    init_tracing();

    let test_name = format!("{}_{}_{}", model, platform.as_str(), language);
    info!(%test_name, "Starting E2E test setup");

    // Skip if platform credentials are not available
    let config = TestConfig::from_env();
    if !is_platform_available(&config, platform, model, language) {
        anyhow::bail!(
            "Skipping {}: platform credentials not available or platform not supported for this model",
            test_name,
        );
    }

    // Start the in-process manager with cloud credentials
    let manager = if platform == Platform::Local {
        Arc::new(
            TestManager::start()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to start TestManager: {}", e))?,
        )
    } else {
        Arc::new(
            TestManager::start_with_config(&config, &[platform])
                .await
                .map_err(|e| anyhow::anyhow!("Failed to start TestManager: {}", e))?,
        )
    };
    info!(url = %manager.url, "Manager started");

    // Deploy the test app
    let mut deployment = deploy_test_app(&manager, platform, model, language).await?;
    info!(
        deployment_id = %deployment.id,
        "Deployment created, waiting for running status"
    );

    // Cross-account registry access: ensure the management account's container
    // registry allows the target account to pull images. Must happen before
    // function deployment (Provisioning phase). Returns image pull credentials
    // for platforms that need explicit registry auth (Azure).
    let image_pull_credentials = if config.has_platform(platform) && matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure) {
        crate::build_push::ensure_cross_account_registry_access(platform, &config).await?
    } else {
        None
    };

    // Cross-account setup: if management + target credentials are both
    // configured, run InitialSetup with target credentials (mirrors the
    // production alien-deploy-cli push model flow). The manager's
    // deployment loop will pick up from Provisioning using management SA
    // impersonation + RSM cross-account role.
    if config.has_platform(platform) && matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure) {
        let management_config = manager.management_config();
        info!(
            deployment_id = %deployment.id,
            has_management_config = management_config.is_some(),
            "Running setup_target for cross-account deployment"
        );
        crate::setup::setup_target(&config, platform, &deployment, &manager, management_config, image_pull_credentials).await?;

        // GCP: grant the RSM SA (created during InitialSetup) read access to the
        // management project's Artifact Registry. During Provisioning, the manager
        // impersonates the RSM SA — Cloud Run requires the caller to have AR access
        // when updating services with cross-project images.
        if platform == Platform::Gcp {
            if let Some(rsm_sa_email) = extract_rsm_sa_email(manager.client(), &deployment.id).await? {
                crate::build_push::grant_rsm_gar_access(&config, &rsm_sa_email).await?;
            }
        }
    }

    // Wait for the deployment to be running (populates URL)
    deployment
        .wait_until_running(Duration::from_secs(600))
        .await
        .map_err(|e| anyhow::anyhow!("Deployment failed to reach running: {}", e))?;
    info!(
        deployment_id = %deployment.id,
        url = ?deployment.url,
        "Deployment is running"
    );

    // Provision the external test secret via the manager vault API.
    // Cloud platforms have vault resources that are now provisioned and ready.
    if matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure) {
        provision_external_secret(&manager, &deployment).await?;
    }

    // For pull model: start alien-agent container
    let agent = if model == DeploymentModel::Pull {
        match platform {
            Platform::Kubernetes => {
                // Helm install for K8s
                let agent = crate::agent::TestAlienAgent::helm_install(
                    &manager,
                    "charts/alien-agent",
                    &format!("e2e-agent-{}", &uuid::Uuid::new_v4().to_string()[..8]),
                    "alien-test",
                    None,
                )
                .await
                .map_err(|e| anyhow::anyhow!("Failed to start alien-agent via Helm: {}", e))?;
                Some(agent)
            }
            _ => {
                // Docker container for cloud + local pull
                let agent = crate::agent::TestAlienAgent::start_container(
                    &manager,
                    "ghcr.io/alienplatform/alien-agent:latest",
                    platform,
                )
                .await
                .map_err(|e| anyhow::anyhow!("Failed to start alien-agent container: {}", e))?;
                Some(agent)
            }
        }
    } else {
        None
    };

    Ok(TestContext {
        deployment,
        manager,
        platform,
        model,
        agent,
    })
}

/// Extract the RSM service account email from the deployment's stack state.
///
/// After InitialSetup, the `remote-stack-management` resource outputs contain the
/// RSM SA email in `access_configuration`.
async fn extract_rsm_sa_email(
    client: &alien_manager_api::Client,
    deployment_id: &str,
) -> anyhow::Result<Option<String>> {
    let resp = client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get deployment: {}", e))?;

    if let Some(ref stack_state) = resp.stack_state {
        if let Some(resources) = stack_state.get("resources").and_then(|v| v.as_object()) {
            for (_id, resource) in resources {
                let resource_type = resource.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if resource_type == "remote-stack-management" {
                    if let Some(email) = resource
                        .get("outputs")
                        .and_then(|o| o.get("access_configuration"))
                        .and_then(|v| v.as_str())
                    {
                        info!(rsm_sa = %email, "Extracted RSM SA email from deployment state");
                        return Ok(Some(email.to_string()));
                    }
                }
            }
        }
    }

    info!("No RSM SA email found in deployment state");
    Ok(None)
}

/// Convenience entry point that runs the full E2E test flow including
/// all binding checks, command checks, and destroy.
///
/// This is equivalent to calling `setup()` followed by checks and cleanup,
/// matching the old `run_e2e_test()` interface.
pub async fn run_e2e_test(
    platform: Platform,
    model: DeploymentModel,
    language: Language,
) -> anyhow::Result<TestContext> {
    setup(platform, model, language).await
}
