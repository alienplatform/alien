//! E2E test orchestration.
//!
//! Provides the high-level `setup()` entry point that each E2E test calls,
//! plus the support matrix, deployment helpers, and stack evaluation logic.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use alien_core::{ClientConfig, Platform, Stack};
use anyhow::Context;
use tracing::info;

use crate::build_push::build_and_push_stack;
use crate::config::TestConfig;
use crate::deployment::TestDeployment;
use crate::managed_secret::provision_managed_test_secret;
use crate::manager::TestManager;

const LOCAL_DELETION_TIMEOUT: Duration = Duration::from_secs(120);
const DISTRIBUTION_DELETION_HANDOFF_TIMEOUT: Duration = Duration::from_secs(600);
const DEFAULT_DEPLOYMENT_RUNNING_TIMEOUT: Duration = Duration::from_secs(600);
const AZURE_DEPLOYMENT_RUNNING_TIMEOUT: Duration = Duration::from_secs(1_800);

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// Deployment model: push (serverless worker) or pull (container / agent).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentModel {
    /// Serverless / worker-based deployment (Lambda, Cloud Run worker, etc.)
    Push,
    /// Container-based deployment (managed cloud, Kubernetes, local Docker)
    Pull,
}

/// Infrastructure artifact used for initial setup in distribution E2E.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistributionFlow {
    /// CloudFormation creates AWS infrastructure and registers the stack.
    CloudFormationAwsPush,
    /// CloudFormation creates AWS-side infra, Helm installs the K8s runtime on EKS.
    CloudFormationEksHelmPull,
    /// Terraform creates AWS infrastructure and registers the stack.
    TerraformAwsPush,
    /// Terraform creates GCP infrastructure and registers the stack.
    TerraformGcpPush,
    /// Terraform creates Azure infrastructure and registers the stack.
    TerraformAzurePush,
    /// Terraform creates AWS-side infra, Helm installs the K8s runtime on EKS.
    TerraformEksHelmPull,
    /// Terraform creates GCP-side infra, Helm installs the K8s runtime on GKE.
    TerraformGkeHelmPull,
    /// Terraform creates Azure-side infra, Helm installs the K8s runtime on AKS.
    TerraformAksHelmPull,
    /// Terraform/external values feed Helm's local-import path for on-prem K8s.
    TerraformOnpremHelmPull,
}

impl DistributionFlow {
    /// Platform used by the running deployment after import.
    pub fn platform(self) -> Platform {
        match self {
            DistributionFlow::CloudFormationAwsPush | DistributionFlow::TerraformAwsPush => {
                Platform::Aws
            }
            DistributionFlow::TerraformGcpPush => Platform::Gcp,
            DistributionFlow::TerraformAzurePush => Platform::Azure,
            DistributionFlow::CloudFormationEksHelmPull
            | DistributionFlow::TerraformEksHelmPull
            | DistributionFlow::TerraformGkeHelmPull
            | DistributionFlow::TerraformAksHelmPull
            | DistributionFlow::TerraformOnpremHelmPull => Platform::Kubernetes,
        }
    }

    /// Base cloud for managed Kubernetes setup targets.
    ///
    /// This is not the runtime platform. The runtime platform remains
    /// Kubernetes; the base cloud only selects cloud setup emitters, registry
    /// access, credentials, and managed-cluster architecture defaults.
    pub fn kubernetes_base_platform(self) -> Option<Platform> {
        match self {
            DistributionFlow::CloudFormationEksHelmPull
            | DistributionFlow::TerraformEksHelmPull => Some(Platform::Aws),
            DistributionFlow::TerraformGkeHelmPull => Some(Platform::Gcp),
            DistributionFlow::TerraformAksHelmPull => Some(Platform::Azure),
            DistributionFlow::CloudFormationAwsPush
            | DistributionFlow::TerraformAwsPush
            | DistributionFlow::TerraformGcpPush
            | DistributionFlow::TerraformAzurePush
            | DistributionFlow::TerraformOnpremHelmPull => None,
        }
    }

    /// Deployment model used by the running deployment after import.
    pub fn deployment_model(self) -> DeploymentModel {
        match self {
            DistributionFlow::CloudFormationAwsPush
            | DistributionFlow::TerraformAwsPush
            | DistributionFlow::TerraformGcpPush
            | DistributionFlow::TerraformAzurePush => DeploymentModel::Push,
            DistributionFlow::CloudFormationEksHelmPull
            | DistributionFlow::TerraformEksHelmPull
            | DistributionFlow::TerraformGkeHelmPull
            | DistributionFlow::TerraformAksHelmPull
            | DistributionFlow::TerraformOnpremHelmPull => DeploymentModel::Pull,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            DistributionFlow::CloudFormationAwsPush => "cloudformation_aws_push",
            DistributionFlow::CloudFormationEksHelmPull => "cloudformation_eks_helm_pull",
            DistributionFlow::TerraformAwsPush => "terraform_aws_push",
            DistributionFlow::TerraformGcpPush => "terraform_gcp_push",
            DistributionFlow::TerraformAzurePush => "terraform_azure_push",
            DistributionFlow::TerraformEksHelmPull => "terraform_eks_helm_pull",
            DistributionFlow::TerraformGkeHelmPull => "terraform_gke_helm_pull",
            DistributionFlow::TerraformAksHelmPull => "terraform_aks_helm_pull",
            DistributionFlow::TerraformOnpremHelmPull => "terraform_onprem_helm_pull",
        }
    }
}

impl std::fmt::Display for DeploymentModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeploymentModel::Push => write!(f, "push"),
            DeploymentModel::Pull => write!(f, "pull"),
        }
    }
}

/// Supported E2E application fixtures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestApp {
    ComprehensiveRust,
    ComprehensiveTs,
    FullStackMicroservices,
    /// Worker + Daemon registering overlapping command names, routed by target
    /// (`examples/command-routing-ts`).
    CommandRoutingTs,
    /// Rust SOURCE Container running the app-owned pull receiver with a
    /// direct in-process KV binding (`tests/e2e/test-apps/container-rust`).
    ContainerRust,
    /// TypeScript SOURCE Container + Rust SOURCE Daemon sharing a direct KV
    /// binding and registering the same target-scoped command.
    RuntimeLessMixed,
}

impl std::fmt::Display for TestApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestApp::ComprehensiveRust => write!(f, "comprehensive-rust"),
            TestApp::ComprehensiveTs => write!(f, "comprehensive-ts"),
            TestApp::FullStackMicroservices => write!(f, "full-stack-microservices"),
            TestApp::CommandRoutingTs => write!(f, "command-routing-ts"),
            TestApp::ContainerRust => write!(f, "container-rust"),
            TestApp::RuntimeLessMixed => write!(f, "runtime-less-mixed"),
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
    /// Managed Postgres database (Aurora, Cloud SQL, Flexible Server, embedded pgvector on Local)
    Postgres,
    /// Message queue (SQS, Pub/Sub, Service Bus)
    Queue,
    /// Direct worker-to-worker invocation
    Worker,
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
    /// Managed secret retrieval from the internal `secrets` vault (cloud only)
    ManagedSecret,
    /// Queue trigger delivery: a send-only message must reach `on_queue_message`
    QueueEvent,
    /// Storage trigger delivery: an object write must reach `on_storage_event`
    StorageEvent,
    /// Cron trigger delivery: the schedule must fire `on_cron_event`
    CronEvent,
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
            Binding::Postgres => write!(f, "postgres"),
            Binding::Queue => write!(f, "queue"),
            Binding::Worker => write!(f, "worker"),
            Binding::Container => write!(f, "container"),
            Binding::WaitUntil => write!(f, "wait-until"),
            Binding::Health => write!(f, "health"),
            Binding::Hello => write!(f, "hello"),
            Binding::Sse => write!(f, "sse"),
            Binding::Environment => write!(f, "environment"),
            Binding::Inspect => write!(f, "inspect"),
            Binding::ManagedSecret => write!(f, "managed-secret"),
            Binding::QueueEvent => write!(f, "queue-event"),
            Binding::StorageEvent => write!(f, "storage-event"),
            Binding::CronEvent => write!(f, "cron-event"),
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
            bindings.push(Binding::QueueEvent);
            bindings.push(Binding::StorageEvent);
            bindings.push(Binding::CronEvent);
            bindings.push(Binding::Build);
            bindings.push(Binding::ArtifactRegistry);
            bindings.push(Binding::ServiceAccount);
            // Test infrastructure limitation: in pull mode the manager does not
            // seed the internal `secrets` vault; the runtime path manages its
            // own secret sync.
            if model == DeploymentModel::Push {
                bindings.push(Binding::ManagedSecret);
            }
        }
        Platform::Kubernetes => {
            bindings.push(Binding::Kv);
            bindings.push(Binding::Container);
        }
        Platform::Local => {
            bindings.push(Binding::Kv);
            bindings.push(Binding::Queue);
            bindings.push(Binding::QueueEvent);
            bindings.push(Binding::StorageEvent);
            bindings.push(Binding::CronEvent);
            bindings.push(Binding::ArtifactRegistry);
            bindings.push(Binding::ServiceAccount);
            // Only the embedded Local controller ships in this repo, so Postgres is
            // exercised on Local only.
            bindings.push(Binding::Postgres);
        }
        _ => {}
    }

    // Worker binding only for push (serverless) deployments on cloud
    if model == DeploymentModel::Push {
        match platform {
            Platform::Aws | Platform::Gcp | Platform::Azure => {
                bindings.push(Binding::Worker);
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
    app: TestApp,
) -> Option<&'static str> {
    match binding {
        Binding::Worker => Some("Worker binding test app endpoint not yet implemented"),
        Binding::Container => Some("Container binding requires managed container infrastructure"),
        // The TypeScript comprehensive app's surface is storage/kv/queue/vault.
        // The manager/controller-internal artifact-registry and
        // service-account flows were split out of the TS SDK and are not
        // reachable through any handler the TS app can serve. The Rust
        // comprehensive app still exercises these two bindings. Build is not
        // listed here because it is excluded for every app by the arm below.
        Binding::ArtifactRegistry | Binding::ServiceAccount
            if app == TestApp::ComprehensiveTs =>
        {
            Some(
                "TS comprehensive app has no artifact-registry/service-account handlers after the SDK facade split",
            )
        }
        Binding::Build => Some("Build binding not yet stable across all platforms"),
        Binding::ServiceAccount if platform == Platform::Local => {
            Some("Local service account binding not yet wired up")
        }
        // Bun-compiled TypeScript binaries on Windows have a runtime issue where
        // setTimeout/async tasks in detached promises (waitUntil) don't execute.
        // All direct binding checks work; only background tasks are affected.
        Binding::WaitUntil
            if platform == Platform::Local
                && app == TestApp::ComprehensiveTs
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

/// Returns the relative path to the test app directory for a given app.
pub(crate) fn test_app_path(app: TestApp) -> &'static str {
    match app {
        TestApp::ComprehensiveRust => "test-apps/comprehensive-rust",
        TestApp::ComprehensiveTs => "test-apps/comprehensive-typescript",
        TestApp::FullStackMicroservices => "../../examples/full-stack-microservices",
        TestApp::CommandRoutingTs => "../../examples/command-routing-ts",
        TestApp::ContainerRust => "test-apps/container-rust",
        TestApp::RuntimeLessMixed => "test-apps/runtime-less-mixed",
    }
}

/// Returns the alien config file name for a given deployment model and platform.
///
/// The default alien config file name used by all test apps.
const CONFIG_FILE: &str = "alien.ts";

fn deployment_environment_variables(
    app: TestApp,
) -> Option<Vec<alien_manager_api::types::EnvironmentVariable>> {
    match app {
        TestApp::ComprehensiveRust
        | TestApp::ComprehensiveTs
        | TestApp::CommandRoutingTs
        | TestApp::ContainerRust
        | TestApp::RuntimeLessMixed => None,
        TestApp::FullStackMicroservices => {
            Some(vec![alien_manager_api::types::EnvironmentVariable {
                name: "APP_SECRET".to_string(),
                value: "e2e-full-stack-internal-token".to_string(),
                type_: alien_manager_api::types::EnvironmentVariableType::Secret,
                target_resources: Some(vec![
                    "api".to_string(),
                    "worker".to_string(),
                    "scheduler".to_string(),
                    "dashboard".to_string(),
                ]),
            }])
        }
    }
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
        Platform::Machines => alien_manager_api::types::Platform::Machines,
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
    /// Test app app.
    pub app: TestApp,
    /// Alien-agent handle (pull model only).
    pub agent: Option<crate::operator::TestAlienOperator>,
    /// Distribution artifacts that must be destroyed outside the native
    /// deployment state machine (Terraform state, CFN stack, Helm release).
    pub distribution_cleanups: Vec<crate::distribution::DistributionArtifactCleanup>,
}

impl TestContext {
    /// Destroy the deployment, stop its operator, and remove external artifacts.
    ///
    /// Designed to be called from `AsyncTestContext::teardown()` so that
    /// resources are released even when a test panics. Unsafe handoffs and
    /// artifact teardown failures are returned so the E2E cannot pass while
    /// leaking resources or discarding recovery state.
    pub async fn cleanup(mut self) -> anyhow::Result<()> {
        let has_distribution_cleanups = !self.distribution_cleanups.is_empty();
        self.distribution_cleanups
            .sort_by_key(|cleanup| cleanup.cleanup_order());

        // Keep a pull operator alive until runtime deletion reaches a terminal
        // handoff. Distribution artifacts must not be removed while that
        // operator may still need their setup-created access path.
        let defer_operator_cleanup = self.platform == Platform::Local || has_distribution_cleanups;
        let mut agent = self.agent.take();
        if !defer_operator_cleanup {
            if let Some(agent) = agent.take() {
                cleanup_operator(agent).await;
            }
        }

        // Mark the deployment as delete-pending via the manager API.
        // The manager chooses the resource cleanup set from deployment ownership.
        let destroy_enqueued = match self.deployment.destroy().await {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!(
                    deployment = %self.deployment.id,
                    error = %e,
                    "cleanup: failed to trigger destroy (may already be destroyed)"
                );
                false
            }
        };

        // Drive the deletion state machine with target credentials so cloud
        // resources are actually torn down before the test exits.
        let mut distribution_cleanup_ready = true;
        if has_distribution_cleanups {
            // If enqueueing deletion failed, make one immediate status check.
            // A 404 or terminal teardown status proves external cleanup is
            // safe; an active/unknown deployment means setup artifacts must be
            // retained so live resources do not lose their credentials or
            // scaffolding.
            let handoff_timeout = if destroy_enqueued {
                DISTRIBUTION_DELETION_HANDOFF_TIMEOUT
            } else {
                Duration::ZERO
            };
            match self.deployment.wait_until_deleted(handoff_timeout).await {
                Ok(outcome) => info!(
                    deployment = %self.deployment.id,
                    ?outcome,
                    "cleanup: runtime deletion handed off to setup artifact owner"
                ),
                Err(error) => {
                    distribution_cleanup_ready = false;
                    tracing::warn!(
                        deployment = %self.deployment.id,
                        %error,
                        "cleanup: retaining distribution artifacts because runtime deletion did not reach a safe handoff"
                    );
                }
            }
        } else if destroy_enqueued
            && matches!(
                self.platform,
                Platform::Aws | Platform::Gcp | Platform::Azure
            )
        {
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
            } else {
                tracing::warn!(
                    deployment = %self.deployment.id,
                    platform = %self.platform,
                    "cleanup: target credentials unavailable for deployment teardown"
                );
            }
        } else if destroy_enqueued && self.platform == Platform::Local {
            if let Err(error) = complete_local_deletion(&self.deployment, &self.manager).await {
                tracing::warn!(
                    deployment = %self.deployment.id,
                    %error,
                    "cleanup: local operator did not complete deployment deletion"
                );
            }
        }

        // Kill the foreground agent process and wait for it to exit.
        // This prevents orphaned agent processes from spamming logs after the test.
        self.deployment.kill_foreground_agent().await;
        if let Some(mut agent) = agent.take() {
            let helm_cleanup_is_tracked = agent
                .helm_release
                .as_ref()
                .zip(agent.helm_namespace.as_ref())
                .is_some_and(|(release, namespace)| {
                    self.distribution_cleanups.iter().any(|cleanup| {
                        matches!(
                            cleanup,
                            crate::distribution::DistributionArtifactCleanup::Helm {
                                release: tracked_release,
                                namespace: tracked_namespace,
                                ..
                            } if tracked_release == release && tracked_namespace == namespace
                        )
                    })
                });
            if helm_cleanup_is_tracked {
                // The tracked artifact owns uninstall and namespace deletion so
                // failures are propagated and the release is handled exactly once.
                agent.helm_release = None;
                agent.helm_namespace = None;
            }
            cleanup_operator(agent).await;
        }

        if !distribution_cleanup_ready {
            let recovery = self
                .distribution_cleanups
                .into_iter()
                .map(|cleanup| cleanup.preserve_for_recovery())
                .collect::<Vec<_>>()
                .join("\n");
            anyhow::bail!(
                "Distribution cleanup stopped before setup teardown because live-resource deletion did not reach a safe handoff. Recovery artifacts:\n{recovery}"
            );
        }

        let mut cleanups = self.distribution_cleanups.into_iter();
        while let Some(cleanup) = cleanups.next() {
            if let Err(error) = cleanup.cleanup().await {
                let retained = cleanups
                    .map(|cleanup| cleanup.preserve_for_recovery())
                    .collect::<Vec<_>>()
                    .join("\n");
                if retained.is_empty() {
                    return Err(error);
                }
                return Err(error.context(format!(
                    "Subsequent distribution artifacts were retained to preserve cleanup order:\n{retained}"
                )));
            }
        }

        info!(deployment = %self.deployment.id, "cleanup: complete");
        Ok(())
    }
}

async fn cleanup_operator(agent: crate::operator::TestAlienOperator) {
    if let Some(ref cid) = agent.container_id {
        let logs = crate::operator::docker_container_logs(cid).await;
        tracing::info!(container_id = %cid, "Agent container logs:\n{}", logs);
    }
    if agent.installed_as_service {
        let logs = crate::operator::collect_service_logs().await;
        tracing::info!("Agent service logs:\n{}", logs);
    }
    agent.cleanup().await;
}

async fn complete_local_deletion(
    deployment: &TestDeployment,
    manager: &Arc<TestManager>,
) -> anyhow::Result<()> {
    let outcome = deployment
        .wait_until_deleted(LOCAL_DELETION_TIMEOUT)
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    if outcome == crate::deployment::DeletionOutcome::Deleted {
        return Ok(());
    }

    let state_directory = deployment
        .local_state_directory()
        .context("Local deployment state directory is unavailable for setup teardown")?;
    info!(
        deployment = %deployment.id,
        state_directory = %state_directory.display(),
        "runtime deletion handed off to client-side setup teardown"
    );

    alien_deploy_cli::commands::push_deletion(
        manager.client(),
        &deployment.id,
        Platform::Local,
        ClientConfig::Local {
            state_directory: state_directory.to_string_lossy().into_owned(),
        },
    )
    .await
    .map_err(|error| anyhow::anyhow!("Local setup teardown failed: {error}"))
}

// ---------------------------------------------------------------------------
// Stack evaluation
// ---------------------------------------------------------------------------

/// Evaluate a TypeScript alien config file using `bun` and return the Stack JSON.
///
/// The config file (alien.ts) uses the `@alienplatform/core` SDK to define
/// stacks. This worker evaluates it via bun and captures the serialized
/// JSON output.
pub(crate) async fn load_stack_json(
    app_dir: &std::path::Path,
    config_file: &str,
    platform: Platform,
) -> anyhow::Result<serde_json::Value> {
    if !app_dir.is_dir() {
        anyhow::bail!("Test app directory does not exist: {}", app_dir.display());
    }

    let bun_binary = std::env::var_os("ALIEN_TEST_BUN_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("bun"));
    let script = format!(
        r#"
const mod = await import('./{config_file}');
const stack = mod.default;
console.log(JSON.stringify(stack));
"#,
    );

    let output = tokio::process::Command::new(&bun_binary)
        .current_dir(app_dir)
        // Expose the target platform to config evaluation so an app can declare a resource only
        // where its controller exists. This is the single point the stack is materialized, so a
        // platform-conditional resource cannot drift between load and deploy.
        .env("ALIEN_TARGET_PLATFORM", platform.as_str())
        .args(["-e", &script])
        .output()
        .await
        .with_context(|| {
            format!(
                "Failed to run bun to evaluate config file using {}",
                bun_binary.display()
            )
        })?;

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
pub(crate) fn e2e_test_apps_root() -> anyhow::Result<PathBuf> {
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

/// Deploy a test app to the given platform with the specified model and app.
///
/// Extract ECR image tags from a pushed stack's worker resources.
pub(crate) fn extract_ecr_image_tags(stack: &Stack) -> Vec<String> {
    use alien_core::Worker;

    stack
        .resources()
        .filter_map(|(_, entry)| {
            let func = entry.config.downcast_ref::<Worker>()?;
            if let alien_core::WorkerCode::Image { ref image } = func.code {
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
    app: TestApp,
) -> anyhow::Result<(TestDeployment, Stack)> {
    let e2e_root = e2e_test_apps_root()?;
    let app_path = e2e_root.join(test_app_path(app));
    let cfg_file = CONFIG_FILE;

    let deployment_name = format!(
        "e2e-{}-{}-{}-{}",
        model,
        platform.as_str(),
        app,
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
    // After push, the stack has WorkerCode::Image with pushed URIs.
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
    if let Some(network) = config.e2e_network_settings(platform)? {
        stack_settings.network = Some(
            serde_json::from_value(
                serde_json::to_value(network)
                    .context("Failed to serialize E2E network settings")?,
            )
            .context("Failed to convert E2E network settings to SDK type")?,
        );
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

    let resource_prefix = crate::config::e2e_resource_prefix()?;
    info!(%resource_prefix, "Using E2E resource prefix");

    let create_body = alien_manager_api::types::CreateDeploymentRequest {
        name: deployment_name.clone(),
        platform: api_platform,
        deployment_group_id: Some(group_id.to_string()),
        stack_settings: Some(stack_settings),
        environment_variables: deployment_environment_variables(app),
        resource_prefix: Some(resource_prefix),
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
// Developer + customer flow (mirrors real alien-deploy deploy)
// ---------------------------------------------------------------------------

/// Result of the developer-side setup.
pub struct DeveloperSetupResult {
    /// Deployment group ID.
    pub group_id: String,
    /// Deployment group token (the token the customer uses with `alien-deploy deploy`).
    pub dg_token: String,
}

/// Developer-side setup: build, push, release, create deployment group + token.
///
/// This mirrors what the developer does before handing off to a customer:
/// `alien build` → `alien release` → `alien onboard` (creates DG + token).
///
/// The customer then uses the DG token with `alien-deploy deploy`.
pub async fn developer_setup(
    manager: &Arc<TestManager>,
    platform: Platform,
    app: TestApp,
) -> anyhow::Result<DeveloperSetupResult> {
    let e2e_root = e2e_test_apps_root()?;
    let app_path = e2e_root.join(test_app_path(app));
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

/// Resolve the alien-operator binary path for tests.
///
/// 1. ALIEN_OPERATOR_BINARY env var (explicit override)
/// 2. Auto-detect from cargo build output (target/debug/alien-operator)
fn resolve_agent_binary() -> Option<std::path::PathBuf> {
    let binary_name = if cfg!(windows) {
        "alien-operator.exe"
    } else {
        "alien-operator"
    };

    // Check explicit env var
    if let Ok(path) = std::env::var("ALIEN_OPERATOR_BINARY") {
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

/// Run `alien-deploy deploy` as the customer would, then discover the deployment ID.
///
/// This is the real customer flow: `alien-deploy deploy --token <dg_token> --platform <platform>`.
/// For push model, it reads cloud credentials from the environment.
/// For local pull, it installs alien-operator as an OS service.
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
    let foreground_data_dir = if foreground {
        Some(tempfile::tempdir().context("Failed to create foreground operator data directory")?)
    } else {
        None
    };

    // Service mode requires root on Linux/macOS (systemd/launchd).
    let use_sudo =
        !foreground && !cfg!(target_os = "windows") && matches!(platform, Platform::Local);

    let mut cmd = if use_sudo {
        let mut c = tokio::process::Command::new("sudo");
        // Preserve environment variables (cloud credentials, ALIEN_OPERATOR_BINARY, etc.)
        c.arg("--preserve-env");
        c.arg(deploy_binary.as_os_str());
        c
    } else {
        tokio::process::Command::new(&deploy_binary)
    };
    cmd.arg("deploy")
        .arg("--token")
        .arg(dg_token)
        .arg("--manager-url")
        .arg(&manager.url)
        .arg("--platform")
        .arg(platform.as_str())
        .arg("-y");

    if foreground {
        // In foreground mode the process runs indefinitely. Inherit stdio
        // so agent logs are visible in test output and pipes don't fill up.
        cmd.stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit());
        // Isolate the complete foreground customer flow. alien-deploy starts
        // alien-operator, so Unix teardown must signal the group rather than
        // killing only the wrapper and orphaning its child.
        #[cfg(unix)]
        cmd.process_group(0);
    } else {
        // In service mode the process exits quickly. Capture output for error reporting.
        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
    }

    if foreground {
        cmd.arg("--foreground");
        // Use a temp directory for agent state so tests don't interfere with
        // each other or with a user's real agent data at ~/.alien/agent-data.
        cmd.arg("--data-dir").arg(
            foreground_data_dir
                .as_ref()
                .context("Foreground operator data directory was not initialized")?
                .path(),
        );
    }

    // Ensure the locally-built alien-operator binary is used instead of
    // downloading from releases.alien.dev. Resolution order:
    // 1. ALIEN_OPERATOR_BINARY env var (explicit override)
    // 2. Auto-detect from cargo build output (target/debug/alien-operator)
    if let Some(operator_path) = resolve_agent_binary() {
        cmd.env("ALIEN_OPERATOR_BINARY", &operator_path);
    }

    info!(
        platform = %platform.as_str(),
        manager_url = %manager.url,
        foreground = %foreground,
        "Running alien-deploy deploy"
    );

    // In foreground mode, the agent runs as a child process that never exits.
    // Spawn it in the background, give it time to initialize the deployment,
    // then discover the deployment ID from the manager.
    let _foreground_child = if foreground {
        let mut child = cmd
            .spawn()
            .context("Failed to spawn alien-deploy deploy in foreground mode")?;

        // Wait for the deployment to be created. alien-deploy deploy initializes
        // with the manager first (creates the deployment record), then starts
        // the agent loop. Poll the manager until the deployment appears.
        let max_wait = std::time::Duration::from_secs(60);
        let start = std::time::Instant::now();
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;

            // Check if the process died early (error in initialization)
            if let Some(status) = child.try_wait()? {
                anyhow::bail!(
                    "alien-deploy deploy exited early ({}). Check test output above for agent logs.",
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
                if let Err(error) = crate::deployment::terminate_foreground_agent(&mut child).await
                {
                    tracing::warn!(%error, "Failed to stop timed-out foreground agent");
                }
                anyhow::bail!("Timed out waiting for foreground agent to create deployment");
            }
        }

        Some(child)
    } else {
        // Service mode: alien-deploy deploy installs the service and exits.
        let output = cmd
            .output()
            .await
            .context("Failed to execute alien-deploy deploy")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !stdout.is_empty() {
            info!("alien-deploy deploy stdout:\n{}", stdout);
        }
        if !stderr.is_empty() {
            info!("alien-deploy deploy stderr:\n{}", stderr);
        }

        if !output.status.success() {
            anyhow::bail!(
                "alien-deploy deploy failed (exit {})\nstdout:\n{}\nstderr:\n{}",
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
        .context("No deployment found in group after alien-deploy deploy")?;

    let deployment_id = dep.id.clone();
    let deployment_name = dep.name.clone();
    info!(%deployment_id, %deployment_name, "Discovered deployment created by alien-deploy deploy");

    // The deployment token was created during deployment creation and is stored
    // on the record. Create a fresh one for test usage since we can't access
    // the original (it was returned to alien-deploy deploy).
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
        deployment.set_foreground_agent(
            child,
            foreground_data_dir.context("Foreground operator data directory ownership was lost")?,
        );
    } else if platform == Platform::Local {
        deployment.set_local_service_state_directory(std::path::PathBuf::from(
            alien_deploy_cli::commands::operator::default_service_data_dir(),
        ));
    }

    Ok(deployment)
}

// ---------------------------------------------------------------------------
// Platform availability
// ---------------------------------------------------------------------------

/// Check if a platform is available and supported for the given deployment model and app.
pub fn is_platform_available(
    config: &TestConfig,
    platform: Platform,
    model: DeploymentModel,
    _app: TestApp,
) -> bool {
    match platform {
        Platform::Local => {
            // Local platform uses pull model only: alien-operator runs as a native
            // OS process and pulls from a cloud artifact registry. Requires at
            // least one cloud platform configured for registry access.
            model == DeploymentModel::Pull
                && (config.has_platform(Platform::Aws)
                    || config.has_platform(Platform::Gcp)
                    || config.has_platform(Platform::Azure))
        }
        Platform::Aws | Platform::Gcp | Platform::Azure => {
            // `setup` currently owns the cloud push path. Kubernetes pull is
            // exercised through `setup_distribution`, where Terraform creates
            // the setup resources and Helm installs the operator.
            model == DeploymentModel::Push && config.has_platform(platform)
        }
        _ => false,
    }
}

fn deployment_running_timeout(platform: Platform) -> Duration {
    match platform {
        Platform::Azure => AZURE_DEPLOYMENT_RUNNING_TIMEOUT,
        _ => DEFAULT_DEPLOYMENT_RUNNING_TIMEOUT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn azure_readiness_budget_accounts_for_slow_control_plane_propagation() {
        assert_eq!(
            deployment_running_timeout(Platform::Azure),
            Duration::from_secs(1_800)
        );
        assert_eq!(
            deployment_running_timeout(Platform::Aws),
            Duration::from_secs(600)
        );
        assert_eq!(
            deployment_running_timeout(Platform::Gcp),
            Duration::from_secs(600)
        );
    }

    #[test]
    fn managed_kubernetes_flows_use_kubernetes_runtime_with_cloud_base() {
        for (flow, base_platform) in [
            (DistributionFlow::CloudFormationEksHelmPull, Platform::Aws),
            (DistributionFlow::TerraformEksHelmPull, Platform::Aws),
            (DistributionFlow::TerraformGkeHelmPull, Platform::Gcp),
            (DistributionFlow::TerraformAksHelmPull, Platform::Azure),
        ] {
            assert_eq!(flow.platform(), Platform::Kubernetes);
            assert_eq!(flow.kubernetes_base_platform(), Some(base_platform));
        }
    }

    #[test]
    fn onprem_kubernetes_flow_has_no_managed_cloud_base() {
        assert_eq!(
            DistributionFlow::TerraformOnpremHelmPull.platform(),
            Platform::Kubernetes
        );
        assert_eq!(
            DistributionFlow::TerraformOnpremHelmPull.kubernetes_base_platform(),
            None
        );
    }

    #[test]
    fn event_delivery_bindings_cover_every_trigger_capable_platform() {
        // Trigger delivery must be proven wherever the platform wires
        // queue/storage/schedule triggers: the three clouds and Local.
        for platform in [
            Platform::Aws,
            Platform::Gcp,
            Platform::Azure,
            Platform::Local,
        ] {
            for model in [DeploymentModel::Push, DeploymentModel::Pull] {
                let supported = supported_bindings(platform, model);
                for binding in [
                    Binding::QueueEvent,
                    Binding::StorageEvent,
                    Binding::CronEvent,
                ] {
                    assert!(
                        supported.contains(&binding),
                        "expected {:?} in supported bindings for {:?}/{:?}",
                        binding,
                        platform,
                        model
                    );
                    assert!(
                        exclusion_reason(platform, model, binding, TestApp::ComprehensiveRust)
                            .is_none(),
                        "{:?} must not be excluded for the Rust app on {:?}/{:?}",
                        binding,
                        platform,
                        model
                    );
                    assert!(
                        exclusion_reason(platform, model, binding, TestApp::ComprehensiveTs)
                            .is_none(),
                        "{:?} must not be excluded for the TS app on {:?}/{:?}",
                        binding,
                        platform,
                        model
                    );
                }
            }
        }
        // Kubernetes worker deployments don't wire triggers in this repo.
        let k8s = supported_bindings(Platform::Kubernetes, DeploymentModel::Pull);
        for binding in [
            Binding::QueueEvent,
            Binding::StorageEvent,
            Binding::CronEvent,
        ] {
            assert!(!k8s.contains(&binding));
        }
    }

    #[test]
    fn ts_app_excludes_manager_internal_bindings_on_every_platform() {
        // Build is intentionally omitted: it is excluded for every app, not the
        // TS app specifically, so asserting its exclusion here would be
        // tautological. artifact-registry and service-account are the genuinely
        // TS-specific exclusions (the Rust app still serves them — see
        // `rust_app_keeps_service_account_and_artifact_registry_coverage`).
        for platform in [
            Platform::Aws,
            Platform::Gcp,
            Platform::Azure,
            Platform::Kubernetes,
            Platform::Local,
        ] {
            for model in [DeploymentModel::Push, DeploymentModel::Pull] {
                for binding in [Binding::ArtifactRegistry, Binding::ServiceAccount] {
                    assert!(
                        exclusion_reason(platform, model, binding, TestApp::ComprehensiveTs)
                            .is_some(),
                        "expected {:?}/{:?} to be excluded for the TS app on {:?}/{:?}",
                        binding,
                        TestApp::ComprehensiveTs,
                        platform,
                        model
                    );
                }
            }
        }
    }

    #[test]
    fn rust_app_keeps_service_account_and_artifact_registry_coverage() {
        // The Rust comprehensive app still serves these handlers, so only the
        // pre-existing, platform-specific exclusions should apply to it.
        assert!(exclusion_reason(
            Platform::Aws,
            DeploymentModel::Push,
            Binding::ArtifactRegistry,
            TestApp::ComprehensiveRust
        )
        .is_none());
        assert!(exclusion_reason(
            Platform::Aws,
            DeploymentModel::Push,
            Binding::ServiceAccount,
            TestApp::ComprehensiveRust
        )
        .is_none());
        // Local service account remains excluded regardless of app.
        assert!(exclusion_reason(
            Platform::Local,
            DeploymentModel::Pull,
            Binding::ServiceAccount,
            TestApp::ComprehensiveRust
        )
        .is_some());
    }
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

/// Run the full E2E test flow for a given platform, model, and app.
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
pub fn setup(
    platform: Platform,
    model: DeploymentModel,
    app: TestApp,
) -> Pin<Box<dyn Future<Output = anyhow::Result<TestContext>> + Send>> {
    // E2E setup composes every platform and deployment path into one large
    // future. Allocate it on the heap so nextest's test-thread stack does not
    // depend on the largest generated SDK or cloud-controller future.
    Box::pin(async move {
        init_tracing();

        let test_name = format!("{}_{}_{}", model, platform.as_str(), app);
        info!(%test_name, "Starting E2E test setup");

        // Skip if platform credentials are not available
        let config = TestConfig::from_env();
        if !is_platform_available(&config, platform, model, app) {
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
        // Push (AWS/GCP/Azure) and Local pull use separate real product flows.
        //   Developer role: build, push, release, create DG + token.
        //   Customer role: `alien-deploy deploy --token <dg_token> --platform <platform>`.
        // Kubernetes pull belongs to `setup_distribution`; it needs the setup
        // artifact outputs to select a cloud registry and configure Helm.

        // Only local pull uses `alien-deploy deploy` for now.
        // Push model has an auth issue: alien-deploy deploy uses the DG token for
        // sync/acquire, but the manager only accepts admin/deployment tokens there.
        // TODO: fix alien-deploy-cli to re-create client with deployment token
        // after initialize, then enable push model here too.
        let uses_alien_deploy_up = model == DeploymentModel::Pull && platform == Platform::Local;

        let (mut deployment, agent) = if uses_alien_deploy_up {
            // ── alien-deploy deploy flow (local pull) ─────────────────────────
            //
            // Developer side: build, push, release, create DG + DG token.
            let dev = developer_setup(&manager, platform, app).await?;

            // Customer side: alien-deploy deploy installs alien-operator as OS service.
            let deployment =
                run_alien_deploy_up(&manager, &dev.dg_token, platform, &dev.group_id).await?;
            info!(
                deployment_id = %deployment.id,
                "Deployment created via alien-deploy deploy"
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
                Some(crate::operator::TestAlienOperator::from_service(
                    deploy_binary,
                ))
            };

            (deployment, agent)
        } else {
            // ── Direct cloud push flow ──────────────────────────────────
            //
            // Push model: test harness calls push_initial_setup() directly
            // with the admin token (alien-deploy deploy auth not yet supported).
            if model == DeploymentModel::Pull {
                anyhow::bail!(
                "direct pull is not supported by this E2E path; use local pull or a Terraform+Helm Kubernetes distribution test"
            );
            }

            let (deployment, stack) = deploy_test_app(&manager, platform, model, app).await?;
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
                        &stack,
                        &manager,
                        management_config,
                    )
                    .await?;
                }
            }

            (deployment, None)
        };

        // Capture agent container ID for debug logging (avoids holding non-Send
        // types across the wait_until_running await boundary).
        let agent_container_id = agent.as_ref().and_then(|a| a.container_id.clone());

        // Wait for the deployment to be running (populates URL).
        // For push: the manager's deployment loop drives this after alien-deploy deploy completes.
        // For pull: the alien-operator drives this via sync + deployment loop.
        let wait_result = deployment
            .wait_until_running(deployment_running_timeout(platform))
            .await
            .map_err(|e| e.to_string());

        if let Err(err_msg) = wait_result {
            if let Some(ref cid) = agent_container_id {
                let logs = crate::operator::docker_container_logs(cid).await;
                tracing::error!(container_id = %cid, "Agent container logs on timeout:\n{}", logs);
            }
            if let Some(ref agent) = agent {
                if agent.installed_as_service {
                    let logs = crate::operator::collect_service_logs().await;
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

        // Provision the managed test secret via the manager vault API.
        // Only for push mode — pull-mode agents manage secrets directly.
        if model == DeploymentModel::Push
            && matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
        {
            if let Err(e) = provision_managed_test_secret(&manager, &deployment).await {
                tracing::warn!(error = %e, "Failed to provision managed test secret, cleaning up");
                cleanup_failed_setup(&mut deployment, agent, &manager, platform).await;
                return Err(e);
            }
        }

        Ok(TestContext {
            deployment,
            manager,
            platform,
            model,
            app,
            agent,
            distribution_cleanups: Vec::new(),
        })
    })
}

/// Run the full distribution E2E setup for a given artifact flow.
///
/// This intentionally does not call [`setup`]. Distribution tests must not
/// silently fall back to native controller provisioning; each flow has to prove
/// its own initial infrastructure path before reusing the common assertions.
pub async fn setup_distribution(
    flow: DistributionFlow,
    app: TestApp,
) -> anyhow::Result<TestContext> {
    init_tracing();

    crate::distribution::setup_distribution(flow, app).await
}

/// Best-effort cleanup when setup() fails after creating a deployment/agent.
/// Mirrors TestContext::cleanup() but operates on individual components since
/// the TestContext was never fully constructed.
async fn cleanup_failed_setup(
    deployment: &mut TestDeployment,
    agent: Option<crate::operator::TestAlienOperator>,
    manager: &Arc<TestManager>,
    platform: Platform,
) {
    tracing::warn!(deployment_id = %deployment.id, "Running cleanup after setup failure");

    let defer_operator_cleanup = platform == Platform::Local;
    let mut agent = agent;
    if !defer_operator_cleanup {
        if let Some(agent) = agent.take() {
            cleanup_operator(agent).await;
        }
    }

    // Mark deployment as delete-pending
    let destroy_enqueued = match deployment.destroy().await {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(
                deployment = %deployment.id,
                %error,
                "cleanup: failed to trigger destroy"
            );
            false
        }
    };

    // Drive cloud resource deletion with target credentials
    if destroy_enqueued && matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure) {
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
    } else if destroy_enqueued && platform == Platform::Local {
        if let Err(error) = complete_local_deletion(deployment, manager).await {
            tracing::warn!(
                deployment = %deployment.id,
                %error,
                "cleanup: local operator did not complete deployment deletion after setup failure"
            );
        }
    }

    deployment.kill_foreground_agent().await;
    if let Some(agent) = agent.take() {
        cleanup_operator(agent).await;
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
    app: TestApp,
) -> anyhow::Result<TestContext> {
    setup(platform, model, app).await
}
