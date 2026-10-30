//! Build and push test app stacks through the manager proxy.
//!
//! Mirrors the production `alien build` + `alien release` flow:
//! 1. `build_stack()` compiles source code into OCI image tarballs
//! 2. `push_stack()` pushes images through the manager's OCI proxy
//!
//! The manager proxy forwards to the upstream cloud registry (ECR/GAR/ACR)
//! transparently, using credentials from its artifact registry binding.

use std::path::Path;

use alien_aws_clients::{
    aws::ecr::{BatchGetImageRequest, ImageIdentifier},
    EcrApi, EcrClient,
};
use alien_build::settings::{BuildSettings, PlatformBuildSettings, PushSettings};
use alien_core::{
    AwsClientConfig, AwsCredentials, Container, ContainerCode, Daemon, DaemonCode, Platform,
    Worker, WorkerCode,
};
use anyhow::Context;
use dockdash::{ClientProtocol, PushOptions, RegistryAuth};
use tracing::info;

use crate::config::TestConfig;
use crate::manager::TestManager;

/// Resolve relative `src` paths in source-backed compute resources against `app_dir`.
///
/// The production CLI runs from the project directory, so relative paths in
/// the Stack JSON resolve correctly. In tests, the working directory is the
/// workspace root, so we must absolutize them before calling `build_stack`.
fn resolve_source_paths(stack: &mut alien_core::Stack, app_dir: &Path) {
    for (_id, entry) in stack.resources_mut() {
        if let Some(func) = entry.config.downcast_mut::<Worker>() {
            if let WorkerCode::Source { ref mut src, .. } = func.code {
                let resolved = app_dir.join(&*src);
                *src = resolved.to_string_lossy().to_string();
            }
        }

        if let Some(container) = entry.config.downcast_mut::<Container>() {
            if let ContainerCode::Source { ref mut src, .. } = container.code {
                let resolved = app_dir.join(&*src);
                *src = resolved.to_string_lossy().to_string();
            }
        }

        if let Some(daemon) = entry.config.downcast_mut::<Daemon>() {
            if let DaemonCode::Source { ref mut src, .. } = daemon.code {
                let resolved = app_dir.join(&*src);
                *src = resolved.to_string_lossy().to_string();
            }
        }
    }
}

/// Build and push a test app stack through the manager proxy.
///
/// This is the E2E equivalent of running `alien build` + `alien release`.
/// Images are pushed through the manager's OCI proxy, so the stack ends up
/// with proxy URIs (e.g., `manager-host:port/image:tag`).
///
/// `app_dir` is the path to the test app source directory — relative `src` paths
/// in the stack will be resolved against it.
pub async fn build_and_push_stack(
    stack: alien_core::Stack,
    platform: Platform,
    config: &TestConfig,
    app_dir: &Path,
    manager: &TestManager,
) -> anyhow::Result<alien_core::Stack> {
    build_and_push_stack_for_registry(stack, platform, platform, None, config, app_dir, manager)
        .await
}

/// Build for one runtime platform and push through a possibly different cloud
/// registry binding. Managed Kubernetes uses this because the runtime platform
/// is Kubernetes, while the registry proxy is backed by AWS/GCP/Azure.
pub async fn build_and_push_stack_for_registry(
    mut stack: alien_core::Stack,
    build_platform: Platform,
    registry_platform: Platform,
    base_platform: Option<Platform>,
    config: &TestConfig,
    app_dir: &Path,
    manager: &TestManager,
) -> anyhow::Result<alien_core::Stack> {
    resolve_source_paths(&mut stack, app_dir);

    let build_settings = create_build_settings(build_platform, base_platform, config)?;
    info!(
        build_platform = %build_platform.as_str(),
        registry_platform = %registry_platform.as_str(),
        output_dir = %build_settings.output_directory,
        "Building stack"
    );

    let built_stack = alien_build::build_stack(stack, &build_settings)
        .await
        .map_err(|e| anyhow::anyhow!("build_stack failed: {}", e))?;
    info!("Stack built successfully");

    let push_settings = create_proxy_push_settings(registry_platform, manager).await?;
    info!(repository = %push_settings.repository, "Pushing stack through manager proxy");

    let pushed_stack = alien_build::push_stack(built_stack, build_platform, &push_settings)
        .await
        .map_err(|e| anyhow::anyhow!("push_stack failed: {}", e))?;
    info!("Stack pushed successfully");

    Ok(pushed_stack)
}

/// Provision the standalone project's cloud repository and push through the manager's OCI proxy.
async fn create_proxy_push_settings(
    platform: Platform,
    manager: &TestManager,
) -> anyhow::Result<PushSettings> {
    // Use the public URL (ngrok) so the image URI in the release is reachable
    // from cloud platforms (Azure Container Apps, etc). The ngrok tunnel
    // forwards to localhost, so the push actually goes to the local manager.
    let manager_url = &manager.public_url;
    let registry_host = alien_core::image_rewrite::strip_url_scheme(manager_url);

    let protocol = if manager_url.starts_with("https://") {
        ClientProtocol::Https
    } else {
        ClientProtocol::Http
    };

    let repo_name = if matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure) {
        let response: serde_json::Value = reqwest::Client::new()
            .post(format!("{}/v1/image-repositories", manager.url))
            .bearer_auth(&manager.admin_token)
            .json(&serde_json::json!({ "projectId": "default", "platform": platform }))
            .send()
            .await
            .context("Failed to provision the test project's image repository")?
            .error_for_status()
            .context("The manager could not provision the test project's image repository")?
            .json()
            .await
            .context("Invalid image repository response")?;
        response["repository"]
            .as_str()
            .context("Image repository response has no repository path")?
            .to_string()
    } else {
        "artifacts/default".to_string()
    };

    Ok(PushSettings {
        repository: format!("{}/{}", registry_host, repo_name),
        destination_label: Some(format!("{:?} test manager", platform)),
        options: PushOptions {
            auth: RegistryAuth::Basic("token".to_string(), manager.admin_token.clone()),
            protocol,
            // Always use monolithic push through the proxy. The proxy doesn't
            // know the upstream registry type, and some registries (GAR) reject
            // chunked PATCH uploads. Monolithic works with all registries.
            monolithic_push: dockdash::MonolithicPushPolicy::Always,
            ..Default::default()
        },
    })
}

/// Construct [`BuildSettings`] for the given platform.
///
/// Uses platform defaults for target architecture. Kubernetes can receive a
/// base cloud platform so managed cluster tests build for the node pool arch.
fn create_build_settings(
    platform: Platform,
    base_platform: Option<Platform>,
    config: &TestConfig,
) -> anyhow::Result<BuildSettings> {
    let output_dir = tempfile::tempdir()
        .context("Failed to create temp dir for build output")?
        .keep();

    let platform_settings = match platform {
        Platform::Aws => PlatformBuildSettings::Aws {
            managing_account_id: config.aws_mgmt.as_ref().and_then(|m| m.account_id.clone()),
        },
        Platform::Gcp => PlatformBuildSettings::Gcp {},
        Platform::Azure => PlatformBuildSettings::Azure {},
        Platform::Kubernetes => PlatformBuildSettings::Kubernetes { base_platform },
        Platform::Local => PlatformBuildSettings::Local {},
        other => anyhow::bail!("Unsupported platform for build: {:?}", other),
    };

    // `None` resolves to the platform defaults — the host target for Local's
    // Workers/Daemons — while still letting source Containers build their
    // Linux image. An explicit host-only list means "host-binary CI job" to
    // the build and skips containers entirely — see
    // `requested_host_binary_only` in alien-build (and the identical fix in
    // alien-cli's dev flow).
    let targets = None;

    let override_base_image = std::env::var("ALIEN_OVERRIDE_BASE_IMAGE")
        .or_else(|_| std::env::var("ALIEN_TEST_OVERRIDE_BASE_IMAGE"))
        .ok()
        .filter(|s| !s.is_empty());

    Ok(BuildSettings {
        platform: platform_settings,
        output_directory: output_dir.to_string_lossy().to_string(),
        targets,
        cache_url: None,
        override_base_image,
        debug_mode: true,
        rebuild: false,
        pull_base_images: false,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the AWS region from an ECR repository URL.
/// e.g. "219193354193.dkr.ecr.us-east-2.amazonaws.com/alien-test-lambda" -> "us-east-2"
#[allow(dead_code)]
fn extract_ecr_region(ecr_url: &str) -> anyhow::Result<String> {
    // Format: {account_id}.dkr.ecr.{region}.amazonaws.com/{repo}
    let host = ecr_url.split('/').next().unwrap_or(ecr_url);
    let parts: Vec<&str> = host.split('.').collect();
    // parts: [account_id, "dkr", "ecr", region, "amazonaws", "com"]
    if parts.len() >= 4 && parts[1] == "dkr" && parts[2] == "ecr" {
        Ok(parts[3].to_string())
    } else {
        anyhow::bail!("Cannot extract region from ECR URL: {}", ecr_url)
    }
}

/// Wait for ECR image replication from the source region to the target region.
///
/// ECR replication copies images asynchronously. When Lambda is deployed in a
/// different region from the ECR source, it uses the replicated image. This
/// worker polls the target-region ECR until the image tag is available.
pub async fn wait_for_ecr_replication(
    config: &TestConfig,
    repo_name: &str,
    image_tags: &[String],
) -> anyhow::Result<()> {
    let mgmt = config
        .aws_mgmt
        .as_ref()
        .context("AWS management credentials required")?;
    let target = config
        .aws_target
        .as_ref()
        .context("AWS target credentials required")?;
    let ecr_region = mgmt.region.clone();

    // If Lambda deploys in the same region as ECR, no replication needed.
    if target.region == ecr_region {
        return Ok(());
    }

    let account_id = mgmt
        .account_id
        .as_ref()
        .context("AWS_MANAGEMENT_ACCOUNT_ID required")?
        .clone();

    // ECR client for the target (replica) region, using management credentials.
    let target_ecr_config = AwsClientConfig {
        region: target.region.clone(),
        account_id: account_id.clone(),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: mgmt.access_key_id.clone(),
            secret_access_key: mgmt.secret_access_key.clone(),
            session_token: mgmt.session_token.clone(),
        },
        service_overrides: None,
    };

    let cred_provider = alien_aws_clients::AwsCredentialProvider::from_config(target_ecr_config)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create ECR credential provider: {}", e))?;
    let ecr_client = EcrClient::new(reqwest::Client::new(), cred_provider);

    let image_ids: Vec<ImageIdentifier> = image_tags
        .iter()
        .map(|tag| ImageIdentifier {
            image_tag: Some(tag.clone()),
            image_digest: None,
        })
        .collect();

    let max_wait = std::time::Duration::from_secs(300);
    let poll_interval = std::time::Duration::from_secs(5);
    let start = std::time::Instant::now();

    info!(
        repo = %repo_name,
        source_region = %ecr_region,
        target_region = %target.region,
        tags = ?image_tags,
        "Waiting for ECR image replication to target region"
    );

    loop {
        let resp = ecr_client
            .batch_get_image(
                BatchGetImageRequest::builder()
                    .repository_name(repo_name.to_string())
                    .image_ids(image_ids.clone())
                    .build(),
            )
            .await;

        match resp {
            Ok(r) if r.failures.is_empty() => {
                info!(
                    elapsed_secs = start.elapsed().as_secs(),
                    "ECR image replication complete"
                );
                return Ok(());
            }
            Ok(r) => {
                let missing: Vec<_> = r
                    .failures
                    .iter()
                    .filter_map(|f| f.image_id.image_tag.as_deref())
                    .collect();
                if start.elapsed() > max_wait {
                    anyhow::bail!(
                        "ECR replication timed out after {}s. Missing tags: {:?}",
                        max_wait.as_secs(),
                        missing
                    );
                }
                info!(
                    elapsed_secs = start.elapsed().as_secs(),
                    missing = ?missing,
                    "ECR replication in progress, waiting..."
                );
            }
            Err(e) => {
                if start.elapsed() > max_wait {
                    anyhow::bail!(
                        "ECR replication check failed after {}s: {}",
                        max_wait.as_secs(),
                        e
                    );
                }
                info!(
                    elapsed_secs = start.elapsed().as_secs(),
                    error = %e,
                    "ECR replication check error, retrying..."
                );
            }
        }

        tokio::time::sleep(poll_interval).await;
    }
}
