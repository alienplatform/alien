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
    /// Event handler verification
    Events,
    /// Build execution (CodeBuild, Cloud Build, ACA Jobs)
    Build,
    /// Artifact registry (ECR, GAR, ACR)
    ArtifactRegistry,
    /// Service account identity and impersonation
    ServiceAccount,
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
            Binding::Build => write!(f, "build"),
            Binding::ArtifactRegistry => write!(f, "artifact-registry"),
            Binding::ServiceAccount => write!(f, "service-account"),
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
            bindings.push(Binding::Events);
            bindings.push(Binding::Build);
            bindings.push(Binding::ArtifactRegistry);
            bindings.push(Binding::ServiceAccount);
            // Test infrastructure limitation: in pull mode the test harness can't
            // provision User Vault secrets from the management account. In real
            // deployments the agent has vault access via its own credentials.
            if model == DeploymentModel::Push {
                bindings.push(Binding::ExternalSecret);
            }
        }
        Platform::Kubernetes => {
            bindings.push(Binding::Kv);
            bindings.push(Binding::Container);
        }
        Platform::Local => {
            bindings.push(Binding::Kv);
            bindings.push(Binding::Queue);
            bindings.push(Binding::Events);
            bindings.push(Binding::ArtifactRegistry);
            bindings.push(Binding::ServiceAccount);
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
    platform: Platform,
    _model: DeploymentModel,
    binding: Binding,
    language: Language,
) -> Option<&'static str> {
    match binding {
        Binding::Function => Some("Function binding test app endpoint not yet implemented"),
        Binding::Container => Some("Container binding requires Horizon (not OSS)"),
        Binding::Build => Some("Build binding not yet stable across all platforms"),
        Binding::ServiceAccount if platform == Platform::Local => {
            Some("Local service account binding not yet wired up")
        }
        // Bun-compiled TypeScript binaries on Windows have a runtime issue where
        // setTimeout/async tasks in detached promises (waitUntil) don't execute.
        // All other gRPC bindings work; only background tasks are affected.
        Binding::WaitUntil
            if platform == Platform::Local
                && language == Language::TypeScript
                && cfg!(target_os = "windows") =>
        {
            Some("Bun-on-Windows runtime issue: detached async tasks in waitUntil don't execute")
        }
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

/// Returns the alien config file name for a given deployment model and platform.
///
/// The default alien config file name used by all test apps.
const CONFIG_FILE: &str = "alien.ts";

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
    /// Test app language.
    pub language: Language,
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
        // 1. Dump agent logs for debugging, then stop the agent.
        if let Some(agent) = self.agent.take() {
            if let Some(ref cid) = agent.container_id {
                let logs = crate::agent::docker_container_logs(cid).await;
                tracing::info!(container_id = %cid, "Agent container logs:\n{}", logs);
            }
            if agent.installed_as_service {
                let logs = crate::agent::collect_service_logs().await;
                tracing::info!("Agent service logs:\n{}", logs);
            }
            agent.cleanup().await;
        }

        // 2. Mark the deployment as delete-pending via the manager API.
        if let Err(e) = self.deployment.destroy().await {
            tracing::warn!(
                deployment = %self.deployment.id,
                error = %e,
                "cleanup: failed to trigger destroy (may already be destroyed)"
            );
            return;
        }

        // 3. Drive the deletion state machine with target credentials so
        //    cloud resources are actually torn down before the test exits.
        if matches!(
            self.platform,
            Platform::Aws | Platform::Gcp | Platform::Azure
        ) {
            let config = crate::config::TestConfig::from_env();
            if config.has_platform(self.platform) {
                if let Err(e) = crate::setup::teardown_target(
                    &config,
                    self.platform,
                    &self.deployment.id,
                    &self.manager,
                )
                .await
                {
                    tracing::warn!(
                        deployment = %self.deployment.id,
                        error = %e,
                        "cleanup: teardown_target failed (resources may be orphaned)"
                    );
                }
            }
        }

        // Kill the foreground agent process and wait for it to exit.
        // This prevents orphaned agent processes from spamming logs after the test.
        self.deployment.kill_foreground_agent().await;

        info!(deployment = %self.deployment.id, "cleanup: complete");
    }
}

// ---------------------------------------------------------------------------
// Stack evaluation
// ---------------------------------------------------------------------------

/// Evaluate a TypeScript alien config file using `bun` and return the Stack JSON.
///
/// The config file (alien.ts) uses the `@alienplatform/core` SDK to define
/// stacks. This function evaluates it via bun and captures the serialized
/// JSON output.
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
/// Extract ECR image tags from a pushed stack's function resources.
fn extract_ecr_image_tags(stack: &Stack) -> Vec<String> {
    use alien_core::Function;

    stack
        .resources()
        .filter_map(|(_, entry)| {
            let func = entry.config.downcast_ref::<Function>()?;
            if let alien_core::FunctionCode::Image { ref image } = func.code {
                // Image URI: "123.dkr.ecr.us-east-1.amazonaws.com/repo:tag"
                image.split(':').last().map(|t| t.to_string())
            } else {
                None
            }
        })
        .collect()
}

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
) -> anyhow::Result<(TestDeployment, Stack)> {
    let e2e_root = e2e_test_apps_root()?;
    let app_path = e2e_root.join(test_app_path(language));
    let cfg_file = CONFIG_FILE;

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

    let pushed_stack = build_and_push_stack(stack, platform, &config, &app_path, &manager).await?;
    info!("Stack built and pushed to registry");

    // AWS cross-region: wait for ECR replication before deployment starts.
    // Lambda requires images in the same region. When the ECR source is in a
    // different region, images are replicated asynchronously and may not be
    // available immediately after push.
    if platform == Platform::Aws && config.aws_target.is_some() {
        let ecr_tags: Vec<String> = extract_ecr_image_tags(&pushed_stack);
        if !ecr_tags.is_empty() {
            crate::build_push::wait_for_ecr_replication(&config, &ecr_tags).await?;
        }
    }

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
            project_id: "default".to_string(),
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
    let mut stack_settings = alien_manager_api::types::StackSettings::default();
    if model == DeploymentModel::Pull {
        stack_settings.deployment_model = Some(alien_manager_api::types::DeploymentModel::Pull);
    }

    // Inject shared Container Apps Environment as an external binding (Azure only).
    // This avoids creating a new environment per test, preventing quota exhaustion.
    if platform == Platform::Azure {
        if let Some(ref shared_env) = config.azure_resources.shared_container_env {
            let binding = alien_core::ContainerAppsEnvironmentBinding::new(
                shared_env.environment_name.as_str(),
                shared_env.resource_id.as_str(),
                shared_env.resource_group.as_str(),
                shared_env.default_domain.as_str(),
            );
            let binding = if let Some(ref ip) = shared_env.static_ip {
                binding.with_static_ip(ip.as_str())
            } else {
                binding
            };
            let mut external_bindings = alien_core::ExternalBindings::new();
            external_bindings.insert(
                "default-container-env",
                alien_core::ExternalBinding::ContainerAppsEnvironment(binding),
            );
            // Serialize to JSON map for the SDK type
            let bindings_json = serde_json::to_value(&external_bindings)
                .context("Failed to serialize external bindings")?;
            stack_settings.external_bindings = bindings_json.as_object().cloned();
            info!("Injected shared Container Apps Environment as external binding");
        }
    }

    let create_body = alien_manager_api::types::CreateDeploymentRequest {
        name: deployment_name.clone(),
        platform: api_platform,
        deployment_group_id: Some(group_id.to_string()),
        stack_settings: Some(stack_settings),
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
    let dep_token = resp.token.clone().unwrap_or_default();
    info!(deployment_id = %dep_id, "Deployment created");

    let deployment = TestDeployment::new(
        dep_id,
        deployment_name,
        platform.as_str().to_string(),
        None,
        dep_token,
        manager.clone(),
    );

    Ok((deployment, pushed_stack))
}

// ---------------------------------------------------------------------------
// Developer + customer flow (mirrors real alien-deploy up)
// ---------------------------------------------------------------------------

/// Result of the developer-side setup.
pub struct DeveloperSetupResult {
    /// Deployment group ID.
    pub group_id: String,
    /// Deployment group token (the token the customer uses with `alien-deploy up`).
    pub dg_token: String,
}

/// Developer-side setup: build, push, release, create deployment group + token.
///
/// This mirrors what the developer does before handing off to a customer:
/// `alien build` → `alien release` → `alien onboard` (creates DG + token).
///
/// The customer then uses the DG token with `alien-deploy up`.
pub async fn developer_setup(
    manager: &Arc<TestManager>,
    platform: Platform,
    language: Language,
) -> anyhow::Result<DeveloperSetupResult> {
    let e2e_root = e2e_test_apps_root()?;
    let app_path = e2e_root.join(test_app_path(language));
    let cfg_file = CONFIG_FILE;

    info!(
        app_path = %app_path.display(),
        config = %cfg_file,
        platform = %platform.as_str(),
        "Developer setup: building and releasing test app"
    );

    let config = TestConfig::from_env();

    // Step 1: Evaluate config file to get Stack JSON
    let stack_by_platform_json = load_stack_json(&app_path, cfg_file, platform).await?;
    info!("Stack JSON loaded from config file");

    // Step 2: Build from source and push images to registry
    let platform_key = platform.as_str();
    let stack_json = stack_by_platform_json
        .get(platform_key)
        .context("Stack JSON missing platform key")?;

    let stack: Stack = serde_json::from_value(stack_json.clone())
        .context("Failed to deserialize Stack from JSON")?;

    let pushed_stack = build_and_push_stack(stack, platform, &config, &app_path, &manager).await?;
    info!("Stack built and pushed to registry");

    // AWS cross-region: wait for ECR replication
    if platform == Platform::Aws && config.aws_target.is_some() {
        let ecr_tags: Vec<String> = extract_ecr_image_tags(&pushed_stack);
        if !ecr_tags.is_empty() {
            crate::build_push::wait_for_ecr_replication(&config, &ecr_tags).await?;
        }
    }

    // Step 3: Create a release
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
            project_id: "default".to_string(),
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
    let group_id = group.id.clone();
    info!(%group_id, "Deployment group created");

    // Step 5: Create a deployment group token (what the developer gives to the customer)
    let token_resp = manager
        .client()
        .create_deployment_group_token()
        .id(&group_id)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create deployment group token: {}", e))?
        .into_inner();
    let dg_token = token_resp.token;
    info!("Deployment group token created");

    Ok(DeveloperSetupResult { group_id, dg_token })
}

/// Find the alien-deploy binary.
///
/// Resolution order:
/// 1. `ALIEN_DEPLOY_BINARY` environment variable
/// 2. `target/debug/alien-deploy` walking up from CWD
/// 3. `alien-deploy` from PATH
fn find_deploy_binary() -> anyhow::Result<std::path::PathBuf> {
    // 1. Explicit env var
    if let Ok(path) = std::env::var("ALIEN_DEPLOY_BINARY") {
        let p = std::path::PathBuf::from(&path);
        if p.exists() {
            return Ok(p.canonicalize().unwrap_or(p));
        }
        // Try resolving relative to workspace root
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let workspace_root = std::path::Path::new(&manifest_dir)
                .parent()
                .and_then(|p| p.parent());
            if let Some(root) = workspace_root {
                let resolved = root.join(&path);
                if resolved.exists() {
                    return Ok(resolved);
                }
            }
        }
        anyhow::bail!(
            "ALIEN_DEPLOY_BINARY set to '{}' but file does not exist",
            path
        );
    }

    // 2. Search upward for target/debug/alien-deploy
    let binary_name = if cfg!(windows) {
        "alien-deploy.exe"
    } else {
        "alien-deploy"
    };

    let mut dir = std::env::current_dir().ok();
    while let Some(d) = dir {
        let candidate = d.join("target").join("debug").join(binary_name);
        if candidate.exists() {
            return Ok(candidate);
        }
        dir = d.parent().map(|p| p.to_path_buf());
    }

    anyhow::bail!(
        "Could not find alien-deploy binary. Set ALIEN_DEPLOY_BINARY or build with `cargo build -p alien-deploy`"
    )
}

/// Resolve the alien-agent binary path for tests.
///
/// 1. ALIEN_AGENT_BINARY env var (explicit override)
/// 2. Auto-detect from cargo build output (target/debug/alien-agent)
fn resolve_agent_binary() -> Option<std::path::PathBuf> {
    let binary_name = if cfg!(windows) {
        "alien-agent.exe"
    } else {
        "alien-agent"
    };

    // Check explicit env var
    if let Ok(path) = std::env::var("ALIEN_AGENT_BINARY") {
        let p = std::path::PathBuf::from(&path);
        if p.is_absolute() && p.exists() {
            return Some(p);
        }
        // Try resolving relative path against workspace root
        if let Some(root) = workspace_root() {
            let resolved = root.join(&p);
            if resolved.exists() {
                return Some(resolved);
            }
        }
    }

    // Auto-detect from cargo build output
    let mut dir = std::env::current_dir().ok();
    while let Some(d) = dir {
        let candidate = d.join("target").join("debug").join(binary_name);
        if candidate.exists() {
            return Some(candidate);
        }
        dir = d.parent().map(|p| p.to_path_buf());
    }

    None
}

fn workspace_root() -> Option<std::path::PathBuf> {
    std::env::var("CARGO_MANIFEST_DIR").ok().and_then(|d| {
        std::path::Path::new(&d)
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
    })
}

/// Run `alien-deploy up` as the customer would, then discover the deployment ID.
///
/// This is the real customer flow: `alien-deploy up --token <dg_token> --platform <platform>`.
/// For push model, it reads cloud credentials from the environment.
/// For local pull, it installs alien-agent as an OS service.
pub async fn run_alien_deploy_up(
    manager: &Arc<TestManager>,
    dg_token: &str,
    platform: Platform,
    group_id: &str,
) -> anyhow::Result<TestDeployment> {
    let deploy_binary = find_deploy_binary()?;
    info!(binary = %deploy_binary.display(), "Found alien-deploy binary");

    // Foreground mode: run agent as a child process instead of installing as
    // a system service. Default: foreground (avoids sudo requirement).
    // Set ALIEN_E2E_FOREGROUND=0 to install as a system service instead.
    let foreground = std::env::var("ALIEN_E2E_FOREGROUND")
        .ok()
        .filter(|v| v == "0" || v == "false")
        .is_none();

    // Service mode requires root on Linux/macOS (systemd/launchd).
    let use_sudo =
        !foreground && !cfg!(target_os = "windows") && matches!(platform, Platform::Local);

    let mut cmd = if use_sudo {
        let mut c = tokio::process::Command::new("sudo");
        // Preserve environment variables (cloud credentials, ALIEN_AGENT_BINARY, etc.)
        c.arg("--preserve-env");
        c.arg(deploy_binary.as_os_str());
        c
    } else {
        tokio::process::Command::new(&deploy_binary)
    };
    cmd.arg("up")
        .arg("--token")
        .arg(dg_token)
        .arg("--manager-url")
        .arg(&manager.url)
        .arg("--platform")
        .arg(platform.as_str())
        .arg("-y")
        .arg("--experimental");

    if foreground {
        // In foreground mode the process runs indefinitely. Inherit stdio
        // so agent logs are visible in test output and pipes don't fill up.
        cmd.stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit());
    } else {
        // In service mode the process exits quickly. Capture output for error reporting.
        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
    }

    if foreground {
        cmd.arg("--foreground");
        // Use a temp directory for agent state so tests don't interfere with
        // each other or with a user's real agent data at ~/.alien/agent-data.
        let agent_data_dir = tempfile::tempdir().expect("failed to create temp dir for agent data");
        cmd.arg("--data-dir").arg(agent_data_dir.path());
        // Keep the tempdir alive — it'll be cleaned up when the test ends.
        // Leak it intentionally; OS cleans up /tmp on reboot anyway.
        std::mem::forget(agent_data_dir);
    }

    // Ensure the locally-built alien-agent binary is used instead of
    // downloading from releases.alien.dev. Resolution order:
    // 1. ALIEN_AGENT_BINARY env var (explicit override)
    // 2. Auto-detect from cargo build output (target/debug/alien-agent)
    if let Some(agent_path) = resolve_agent_binary() {
        cmd.env("ALIEN_AGENT_BINARY", &agent_path);
    }

    info!(
        platform = %platform.as_str(),
        manager_url = %manager.url,
        foreground = %foreground,
        "Running alien-deploy up"
    );

    // In foreground mode, the agent runs as a child process that never exits.
    // Spawn it in the background, give it time to initialize the deployment,
    // then discover the deployment ID from the manager.
    let _foreground_child = if foreground {
        let mut child = cmd
            .spawn()
            .context("Failed to spawn alien-deploy up in foreground mode")?;

        // Wait for the deployment to be created. alien-deploy up initializes
        // with the manager first (creates the deployment record), then starts
        // the agent loop. Poll the manager until the deployment appears.
        let max_wait = std::time::Duration::from_secs(60);
        let start = std::time::Instant::now();
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;

            // Check if the process died early (error in initialization)
            if let Some(status) = child.try_wait()? {
                anyhow::bail!(
                    "alien-deploy up exited early ({}). Check test output above for agent logs.",
                    status,
                );
            }

            // Check if deployment appeared in the group
            let deps = manager
                .client()
                .list_deployments()
                .deployment_group_id(group_id)
                .send()
                .await;
            if let Ok(resp) = deps {
                if !resp.items.is_empty() {
                    info!("Deployment created by foreground agent, continuing");
                    break;
                }
            }

            if start.elapsed() > max_wait {
                child.kill().await.ok();
                anyhow::bail!("Timed out waiting for foreground agent to create deployment");
            }
        }

        Some(child)
    } else {
        // Service mode: alien-deploy up installs the service and exits.
        let output = cmd
            .output()
            .await
            .context("Failed to execute alien-deploy up")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !stdout.is_empty() {
            info!("alien-deploy up stdout:\n{}", stdout);
        }
        if !stderr.is_empty() {
            info!("alien-deploy up stderr:\n{}", stderr);
        }

        if !output.status.success() {
            anyhow::bail!(
                "alien-deploy up failed (exit {})\nstdout:\n{}\nstderr:\n{}",
                output.status,
                stdout,
                stderr,
            );
        }

        None
    };

    // Discover the deployment ID by listing deployments in the group.
    let deployments = manager
        .client()
        .list_deployments()
        .deployment_group_id(group_id)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to list deployments: {}", e))?
        .into_inner();

    let dep = deployments
        .items
        .first()
        .context("No deployment found in group after alien-deploy up")?;

    let deployment_id = dep.id.clone();
    let deployment_name = dep.name.clone();
    info!(%deployment_id, %deployment_name, "Discovered deployment created by alien-deploy up");

    // The deployment token was created during deployment creation and is stored
    // on the record. Create a fresh one for test usage since we can't access
    // the original (it was returned to alien-deploy up).
    let dep_token = manager
        .create_deployment_token(group_id, &deployment_id)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create deployment token: {}", e))?;

    let mut deployment = TestDeployment::new(
        deployment_id,
        deployment_name,
        platform.as_str().to_string(),
        None,
        dep_token,
        manager.clone(),
    );

    if let Some(child) = _foreground_child {
        deployment.set_foreground_agent(child);
    }

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
            // Local platform uses pull model only: alien-agent runs as a native
            // OS process and pulls from a cloud artifact registry. Requires at
            // least one cloud platform configured for registry access.
            model == DeploymentModel::Pull
                && (config.has_platform(Platform::Aws)
                    || config.has_platform(Platform::Gcp)
                    || config.has_platform(Platform::Azure))
        }
        Platform::Kubernetes => {
            // Kubernetes only supports pull (container) model
            model == DeploymentModel::Pull
        }
        Platform::Aws | Platform::Gcp | Platform::Azure => {
            // Cloud platforms support both push and pull models.
            //
            // Push: manager deploys using cross-account impersonation (RSM).
            // Pull: alien-agent runs in the target environment and deploys
            //       directly using target credentials — no cross-account IAM.
            //
            // Both require management + target credentials to be configured.
            config.has_platform(platform)
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
        anyhow::bail!("Failed to provision external secret ({}): {}", status, body);
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

    // Start the in-process manager with cloud credentials.
    // For Local platform, we still need cloud credentials for the artifact
    // registry (images are pushed to a cloud registry). Use all available
    // cloud platforms so the manager has registry access configured.
    let manager_platforms: Vec<Platform> = if platform == Platform::Local {
        [Platform::Aws, Platform::Gcp, Platform::Azure]
            .into_iter()
            .filter(|p| config.has_platform(*p))
            .collect()
    } else {
        vec![platform]
    };

    let manager = if manager_platforms.is_empty() {
        Arc::new(
            TestManager::start()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to start TestManager: {}", e))?,
        )
    } else {
        Arc::new(
            TestManager::start_with_config(&config, &manager_platforms)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to start TestManager: {}", e))?,
        )
    };
    info!(url = %manager.url, "Manager started");

    // ── Route to the correct flow based on platform + model ─────────
    //
    // Push (AWS/GCP/Azure) and Local pull: use the real `alien-deploy up` flow.
    //   Developer role: build, push, release, create DG + token.
    //   Customer role: `alien-deploy up --token <dg_token> --platform <platform>`.
    //
    // Cloud pull (AWS/GCP/Azure pull) and K8s pull: use existing direct flows.
    //   These match the real customer flow (deploy Docker image / helm install).

    // Only local pull uses `alien-deploy up` for now.
    // Push model has an auth issue: alien-deploy up uses the DG token for
    // sync/acquire, but the manager only accepts admin/deployment tokens there.
    // TODO: fix alien-deploy-cli to re-create client with deployment token
    // after initialize, then enable push model here too.
    let uses_alien_deploy_up = model == DeploymentModel::Pull && platform == Platform::Local;

    let (mut deployment, agent) = if uses_alien_deploy_up {
        // ── alien-deploy up flow (local pull) ─────────────────────────
        //
        // Developer side: build, push, release, create DG + DG token.
        let dev = developer_setup(&manager, platform, language).await?;

        // Customer side: alien-deploy up installs alien-agent as OS service.
        let deployment =
            run_alien_deploy_up(&manager, &dev.dg_token, platform, &dev.group_id).await?;
        info!(
            deployment_id = %deployment.id,
            "Deployment created via alien-deploy up"
        );

        // Track agent for cleanup. In foreground mode the agent runs as a
        // child process owned by the deployment (no OS service installed).
        // Only create a service tracker when not using foreground mode.
        let foreground = std::env::var("ALIEN_E2E_FOREGROUND")
            .ok()
            .filter(|v| v == "0" || v == "false")
            .is_none();
        let agent = if foreground {
            None
        } else {
            let deploy_binary = find_deploy_binary().ok();
            Some(crate::agent::TestAlienAgent::from_service(deploy_binary))
        };

        (deployment, agent)
    } else {
        // ── Direct flow (push + cloud pull + K8s pull) ───────────────
        //
        // Push model: test harness calls push_initial_setup() directly
        // with the admin token (alien-deploy up auth not yet supported).
        //
        // Cloud pull: deploy alien-agent Docker image with injected creds.
        // K8s pull: helm install alien-agent chart.
        let (deployment, _stack) = deploy_test_app(&manager, platform, model, language).await?;
        info!(
            deployment_id = %deployment.id,
            "Deployment created, waiting for running status"
        );

        // Push model: run initial setup with scoped credentials
        if model == DeploymentModel::Push {
            if config.has_platform(platform)
                && matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
            {
                let management_config = manager.management_config();
                info!(
                    deployment_id = %deployment.id,
                    has_management_config = management_config.is_some(),
                    "Running setup_target for cross-account deployment"
                );
                crate::setup::setup_target(
                    &config,
                    platform,
                    &deployment,
                    &manager,
                    management_config,
                )
                .await?;
            }
        }

        // Pull model: start agent
        let agent = if model == DeploymentModel::Pull {
            let agent_image = std::env::var("ALIEN_TEST_OVERRIDE_AGENT_IMAGE")
                .unwrap_or_else(|_| "ghcr.io/alienplatform/alien-agent:latest".to_string());

            match platform {
                Platform::Kubernetes => {
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
                    // Docker container for cloud pull (AWS/GCP/Azure pull)
                    let agent = crate::agent::TestAlienAgent::start_container(
                        &manager,
                        &agent_image,
                        platform,
                        Some(&config),
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to start alien-agent container: {}", e))?;

                    // Give the agent a moment to start, then verify it's still running
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    if let Some(ref cid) = agent.container_id {
                        let status = crate::agent::docker_container_status(cid).await;
                        if status != "running" {
                            let logs = crate::agent::docker_container_logs(cid).await;
                            anyhow::bail!(
                                "Agent container {} exited (status: {}). Logs:\n{}",
                                cid,
                                status,
                                logs
                            );
                        }
                        info!(container_id = %cid, %status, "Agent container health check passed");
                    }

                    Some(agent)
                }
            }
        } else {
            None
        };

        (deployment, agent)
    };

    // Capture agent container ID for debug logging (avoids holding non-Send
    // types across the wait_until_running await boundary).
    let agent_container_id = agent.as_ref().and_then(|a| a.container_id.clone());

    // Wait for the deployment to be running (populates URL).
    // For push: the manager's deployment loop drives this after alien-deploy up completes.
    // For pull: the alien-agent drives this via sync + deployment loop.
    let wait_result = deployment
        .wait_until_running(Duration::from_secs(600))
        .await
        .map_err(|e| e.to_string());

    if let Err(err_msg) = wait_result {
        if let Some(ref cid) = agent_container_id {
            let logs = crate::agent::docker_container_logs(cid).await;
            tracing::error!(container_id = %cid, "Agent container logs on timeout:\n{}", logs);
        }
        if let Some(ref agent) = agent {
            if agent.installed_as_service {
                let logs = crate::agent::collect_service_logs().await;
                tracing::error!("Agent service logs on timeout:\n{}", logs);
            }
        }

        // Clean up partially-created resources before returning the error.
        // Without this, the test macro's .expect() panics and teardown() never
        // runs because Self was never constructed — leaking cloud resources.
        cleanup_failed_setup(&mut deployment, agent, &manager, platform).await;

        return Err(anyhow::anyhow!(
            "Deployment failed to reach running: {}",
            err_msg
        ));
    }
    info!(
        deployment_id = %deployment.id,
        url = ?deployment.url,
        "Deployment is running"
    );

    // Provision the external test secret via the manager vault API.
    // Only for push mode — pull-mode agents manage secrets directly.
    if model == DeploymentModel::Push
        && matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
    {
        if let Err(e) = provision_external_secret(&manager, &deployment).await {
            tracing::warn!(error = %e, "Failed to provision external secret, cleaning up");
            cleanup_failed_setup(&mut deployment, agent, &manager, platform).await;
            return Err(e);
        }
    }

    Ok(TestContext {
        deployment,
        manager,
        platform,
        model,
        language,
        agent,
    })
}

/// Best-effort cleanup when setup() fails after creating a deployment/agent.
/// Mirrors TestContext::cleanup() but operates on individual components since
/// the TestContext was never fully constructed.
async fn cleanup_failed_setup(
    deployment: &mut TestDeployment,
    agent: Option<crate::agent::TestAlienAgent>,
    manager: &Arc<TestManager>,
    platform: Platform,
) {
    tracing::warn!(deployment_id = %deployment.id, "Running cleanup after setup failure");

    // Stop the agent first
    if let Some(agent) = agent {
        if let Some(ref cid) = agent.container_id {
            let logs = crate::agent::docker_container_logs(cid).await;
            tracing::info!(container_id = %cid, "Agent container logs:\n{}", logs);
        }
        agent.cleanup().await;
    }

    // Mark deployment as delete-pending
    if let Err(e) = deployment.destroy().await {
        tracing::warn!(
            deployment = %deployment.id,
            error = %e,
            "cleanup: failed to trigger destroy"
        );
        return;
    }

    // Drive cloud resource deletion with target credentials
    if matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure) {
        let config = TestConfig::from_env();
        if config.has_platform(platform) {
            if let Err(e) =
                crate::setup::teardown_target(&config, platform, &deployment.id, manager).await
            {
                tracing::warn!(
                    deployment = %deployment.id,
                    error = %e,
                    "cleanup: teardown_target failed (resources may be orphaned)"
                );
            }
        }
    }

    tracing::info!(deployment = %deployment.id, "cleanup after setup failure: complete");
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
