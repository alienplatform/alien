//! Build and push test app stacks through the manager proxy.
//!
//! Mirrors the production `alien build` + `alien release` flow:
//! 1. `build_stack()` compiles source code into OCI image tarballs
//! 2. `push_stack()` pushes images through the manager's OCI proxy
//!
//! The manager proxy forwards to the upstream cloud registry (ECR/GAR/ACR)
//! transparently, using credentials from its artifact registry binding.

use std::path::Path;

use alien_build::settings::{BuildSettings, PlatformBuildSettings, PushSettings};
use alien_core::{Function, FunctionCode, Platform};
use anyhow::Context;
use dockdash::{ClientProtocol, PushOptions, RegistryAuth};
use tracing::info;

use crate::config::TestConfig;
use crate::manager::TestManager;

/// Resolve relative `src` paths in FunctionCode::Source entries against `app_dir`.
///
/// The production CLI runs from the project directory, so relative paths in
/// the Stack JSON resolve correctly. In tests, the working directory is the
/// workspace root, so we must absolutize them before calling `build_stack`.
fn resolve_source_paths(stack: &mut alien_core::Stack, app_dir: &Path) {
    for (_id, entry) in stack.resources_mut() {
        if let Some(func) = entry.config.downcast_mut::<Function>() {
            if let FunctionCode::Source { ref mut src, .. } = func.code {
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
    mut stack: alien_core::Stack,
    platform: Platform,
    config: &TestConfig,
    app_dir: &Path,
    manager: &TestManager,
) -> anyhow::Result<alien_core::Stack> {
    resolve_source_paths(&mut stack, app_dir);

    let build_settings = create_build_settings(platform, config)?;
    info!(
        platform = %platform.as_str(),
        output_dir = %build_settings.output_directory,
        "Building stack"
    );

    let built_stack = alien_build::build_stack(stack, &build_settings)
        .await
        .map_err(|e| anyhow::anyhow!("build_stack failed: {}", e))?;
    info!("Stack built successfully");

    let push_settings = create_proxy_push_settings(platform, config, manager);
    info!(repository = %push_settings.repository, "Pushing stack through manager proxy");

    let pushed_stack = alien_build::push_stack(built_stack, platform, &push_settings)
        .await
        .map_err(|e| anyhow::anyhow!("push_stack failed: {}", e))?;
    info!("Stack pushed successfully");

    Ok(pushed_stack)
}

/// Create push settings that push through the manager's OCI proxy.
///
/// The repo name is the real upstream repo path — no proxying mapping.
/// Each platform has its own path convention:
/// - ECR: flat repo name (e.g., `alien-e2e`)
/// - GAR: `{project_id}/{gar_repo_name}` (e.g., `alien-test-mgmt/alien-e2e`)
/// - ACR: flat repo name (e.g., `alien-e2e`)
fn create_proxy_push_settings(
    platform: Platform,
    config: &TestConfig,
    manager: &TestManager,
) -> PushSettings {
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

    // Derive the upstream repo name per platform — matches build_artifact_registry_config().
    let repo_name = upstream_repo_name(platform, config);

    PushSettings {
        repository: format!("{}/{}", registry_host, repo_name),
        options: PushOptions {
            auth: RegistryAuth::Basic("token".to_string(), manager.admin_token.clone()),
            protocol,
            // Always use monolithic push through the proxy. The proxy doesn't
            // know the upstream registry type, and some registries (GAR) reject
            // chunked PATCH uploads. Monolithic works with all registries.
            monolithic_push: dockdash::MonolithicPushPolicy::Always,
            ..Default::default()
        },
    }
}

/// Derive the upstream repo name for the given platform.
///
/// Must match what `build_artifact_registry_config()` configured in the manager's
/// artifact registry binding — the proxy forwards the path unchanged, so the
/// repo name here must be the exact OCI path the upstream expects.
fn upstream_repo_name(platform: Platform, config: &TestConfig) -> String {
    match platform {
        Platform::Aws => {
            // ECR: flat repo name from binding's repositoryPrefix
            "alien-e2e".to_string()
        }
        Platform::Gcp => {
            // GAR requires 3 segments: {gcp_project}/{gar_repo}/{image_name}
            // "gcp_project" = GCP Cloud project (not Alien project)
            // "gar_repo" = the Artifact Registry repository resource
            // "image_name" = the image within the repo (our Alien project namespace)
            let project_id = config
                .gcp_mgmt
                .as_ref()
                .map(|m| m.project_id.as_str())
                .unwrap_or("alien-test-mgmt");
            let gar_repo = config
                .e2e_artifact_registry
                .gcp_gar_repository
                .as_ref()
                .and_then(|url| url.rsplit('/').next().map(|s| s.to_string()))
                .unwrap_or_else(|| "alien-e2e".to_string());
            format!("{}/{}/default", project_id, gar_repo)
        }
        Platform::Azure => {
            // ACR: flat repo name — must match repository_prefix in build_artifact_registry_config
            "azure-e2e".to_string()
        }
        // Local platform uses the embedded local registry whose prefix is
        // "artifacts/default" — matching the CLI dev mode and the local AR's
        // upstream_repository_prefix().
        _ => "artifacts/default".to_string(),
    }
}

/// Construct [`BuildSettings`] for the given platform.
///
/// Uses platform defaults for target architecture (AWS=arm64, GCP/Azure=x64)
/// and enables debug mode for faster test builds.
fn create_build_settings(platform: Platform, config: &TestConfig) -> anyhow::Result<BuildSettings> {
    let output_dir = tempfile::tempdir()
        .context("Failed to create temp dir for build output")?
        .keep();

    let platform_settings = match platform {
        Platform::Aws => PlatformBuildSettings::Aws {
            managing_account_id: config.aws_mgmt.as_ref().and_then(|m| m.account_id.clone()),
        },
        Platform::Gcp => PlatformBuildSettings::Gcp {},
        Platform::Azure => PlatformBuildSettings::Azure {},
        Platform::Kubernetes => PlatformBuildSettings::Kubernetes {},
        Platform::Local => PlatformBuildSettings::Local {},
        other => anyhow::bail!("Unsupported platform for build: {:?}", other),
    };

    // Local platform builds for the host OS/arch
    let targets = if platform == Platform::Local {
        Some(vec![alien_core::BinaryTarget::current_os()])
    } else {
        None
    };

    let override_base_image = std::env::var("ALIEN_TEST_OVERRIDE_BASE_IMAGE")
        .ok()
        .filter(|s| !s.is_empty());

    Ok(BuildSettings {
        platform: platform_settings,
        output_directory: output_dir.to_string_lossy().to_string(),
        targets,
        cache_url: None,
        override_base_image,
        debug_mode: true,
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
/// function polls the target-region ECR until the image tag is available.
pub async fn wait_for_ecr_replication(
    config: &TestConfig,
    image_tags: &[String],
) -> anyhow::Result<()> {
    use alien_aws_clients::aws::ecr::{BatchGetImageRequest, ImageIdentifier};
    use alien_aws_clients::{EcrApi, EcrClient};
    use alien_core::{AwsClientConfig, AwsCredentials};

    let mgmt = config
        .aws_mgmt
        .as_ref()
        .context("AWS management credentials required")?;
    let target = config
        .aws_target
        .as_ref()
        .context("AWS target credentials required")?;
    // Use the E2E repo name — same one that create_aws_push_settings pushes to.
    let repo_name = "alien-e2e";
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
