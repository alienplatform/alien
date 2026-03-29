//! Build and push test app stacks to cloud registries.
//!
//! Mirrors the production `alien build` + `alien release` flow:
//! 1. `build_stack()` compiles source code into OCI image tarballs
//! 2. `push_stack()` pushes images to the platform's container registry
//!
//! Registry credentials are constructed using the same cloud SDK clients
//! that production uses (alien-aws-clients for ECR, standard Docker auth
//! for GAR and ACR).

use std::path::Path;

use alien_build::settings::{BuildSettings, PlatformBuildSettings, PushSettings};
use alien_core::{Function, FunctionCode, Platform};
use anyhow::Context;
use dockdash::{ClientProtocol, PushOptions, RegistryAuth};
use tracing::info;

use crate::config::TestConfig;

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

/// Build and push a test app stack, returning the stack with pushed image URIs.
///
/// This is the E2E equivalent of running `alien build` + `alien release --image-repo ...`.
/// `app_dir` is the path to the test app source directory — relative `src` paths
/// in the stack will be resolved against it.
pub async fn build_and_push_stack(
    mut stack: alien_core::Stack,
    platform: Platform,
    config: &TestConfig,
    app_dir: &Path,
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

    let push_settings = create_push_settings(platform, config).await?;
    info!(repository = %push_settings.repository, "Pushing stack to registry");

    let pushed_stack = alien_build::push_stack(built_stack, platform, &push_settings)
        .await
        .map_err(|e| anyhow::anyhow!("push_stack failed: {}", e))?;
    info!("Stack pushed successfully");

    Ok(pushed_stack)
}

/// Construct [`BuildSettings`] for the given platform.
///
/// Uses platform defaults for target architecture (AWS=arm64, GCP/Azure=x64)
/// and enables debug mode for faster test builds.
fn create_build_settings(
    platform: Platform,
    config: &TestConfig,
) -> anyhow::Result<BuildSettings> {
    let output_dir = tempfile::tempdir()
        .context("Failed to create temp dir for build output")?
        .keep();

    let platform_settings = match platform {
        Platform::Aws => PlatformBuildSettings::Aws {
            managing_account_id: config
                .aws_mgmt
                .as_ref()
                .and_then(|m| m.account_id.clone()),
        },
        Platform::Gcp => PlatformBuildSettings::Gcp {},
        Platform::Azure => PlatformBuildSettings::Azure {},
        other => anyhow::bail!("Unsupported platform for build: {:?}", other),
    };

    let override_base_image = std::env::var("ALIEN_TEST_OVERRIDE_BASE_IMAGE").ok();

    Ok(BuildSettings {
        platform: platform_settings,
        output_directory: output_dir.to_string_lossy().to_string(),
        targets: None,
        cache_url: None,
        override_base_image,
        debug_mode: true,
    })
}

/// Construct [`PushSettings`] with registry credentials for the given platform.
async fn create_push_settings(
    platform: Platform,
    config: &TestConfig,
) -> anyhow::Result<PushSettings> {
    match platform {
        Platform::Aws => create_aws_push_settings(config).await,
        Platform::Gcp => create_gcp_push_settings(config),
        Platform::Azure => create_azure_push_settings(config),
        other => anyhow::bail!("Unsupported platform for push: {:?}", other),
    }
}

/// AWS ECR push settings: get an authorization token via the ECR API
/// (same code path as the production ECR artifact registry provider).
async fn create_aws_push_settings(config: &TestConfig) -> anyhow::Result<PushSettings> {
    use alien_aws_clients::{EcrApi, EcrClient};
    use alien_core::{AwsClientConfig, AwsCredentials};

    let mgmt = config
        .aws_mgmt
        .as_ref()
        .context("AWS management credentials not configured")?;

    let repository = config
        .aws_resources
        .ecr_repository
        .as_ref()
        .context(
            "ALIEN_TEST_AWS_ECR_REPOSITORY not set. \
             Set it to your ECR repository URL, e.g. 123456789012.dkr.ecr.us-east-1.amazonaws.com/repo-name",
        )?
        .clone();

    let account_id = mgmt
        .account_id
        .as_ref()
        .context("AWS_MANAGEMENT_ACCOUNT_ID required for ECR auth")?
        .clone();

    let aws_config = AwsClientConfig {
        region: mgmt.region.clone(),
        account_id,
        credentials: AwsCredentials::AccessKeys {
            access_key_id: mgmt.access_key_id.clone(),
            secret_access_key: mgmt.secret_access_key.clone(),
            session_token: mgmt.session_token.clone(),
        },
        service_overrides: None,
    };

    let cred_provider = alien_aws_clients::AwsCredentialProvider::from_config(aws_config)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create AWS credential provider: {}", e))?;

    let ecr_client = EcrClient::new(reqwest::Client::new(), cred_provider);
    let auth_response = ecr_client
        .get_authorization_token(
            alien_aws_clients::aws::ecr::GetAuthorizationTokenRequest::builder().build(),
        )
        .await
        .map_err(|e| anyhow::anyhow!("ECR get_authorization_token failed: {}", e))?;

    let auth_data = auth_response
        .authorization_data
        .first()
        .context("No authorization data in ECR response")?;

    // ECR returns a base64-encoded "username:password" string
    let decoded = String::from_utf8(
        base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &auth_data.authorization_token,
        )
        .context("Failed to base64-decode ECR auth token")?,
    )
    .context("ECR auth token is not valid UTF-8")?;

    let (username, password) = decoded
        .split_once(':')
        .context("Invalid ECR token format (expected username:password)")?;

    info!("ECR authorization token obtained");

    Ok(PushSettings {
        repository,
        options: PushOptions {
            auth: RegistryAuth::Basic(username.to_string(), password.to_string()),
            protocol: ClientProtocol::Https,
            ..Default::default()
        },
    })
}

/// GCP Artifact Registry push settings: use `_json_key` auth with the
/// service account key (standard Docker auth for GAR).
fn create_gcp_push_settings(config: &TestConfig) -> anyhow::Result<PushSettings> {
    let mgmt = config
        .gcp_mgmt
        .as_ref()
        .context("GCP management credentials not configured")?;

    let repository = config
        .gcp_resources
        .gar_repository
        .as_ref()
        .context(
            "ALIEN_TEST_GCP_GAR_REPOSITORY not set. \
             Set it to your GAR repository URL, e.g. us-central1-docker.pkg.dev/project/repo/image",
        )?
        .clone();

    let sa_key = mgmt
        .credentials_json
        .as_ref()
        .context("GCP service account key not configured")?;

    Ok(PushSettings {
        repository,
        options: PushOptions {
            auth: RegistryAuth::Basic("_json_key".to_string(), sa_key.clone()),
            protocol: ClientProtocol::Https,
            ..Default::default()
        },
    })
}

/// Azure Container Registry push settings: use service principal
/// client_id/client_secret as basic auth credentials.
fn create_azure_push_settings(config: &TestConfig) -> anyhow::Result<PushSettings> {
    let mgmt = config
        .azure_mgmt
        .as_ref()
        .context("Azure management credentials not configured")?;

    let repository = config
        .azure_resources
        .acr_repository
        .as_ref()
        .context(
            "ALIEN_TEST_AZURE_ACR_REPOSITORY not set. \
             Set it to your ACR repository URL, e.g. myregistry.azurecr.io/image",
        )?
        .clone();

    Ok(PushSettings {
        repository,
        options: PushOptions {
            auth: RegistryAuth::Basic(mgmt.client_id.clone(), mgmt.client_secret.clone()),
            protocol: ClientProtocol::Https,
            ..Default::default()
        },
    })
}
