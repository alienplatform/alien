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

    // Extract ECR region from the repository URL (e.g. "...dkr.ecr.us-east-2.amazonaws.com/...")
    let ecr_region = extract_ecr_region(&repository)?;

    let aws_config = AwsClientConfig {
        region: ecr_region,
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the AWS region from an ECR repository URL.
/// e.g. "219193354193.dkr.ecr.us-east-2.amazonaws.com/alien-test-lambda" -> "us-east-2"
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

// ---------------------------------------------------------------------------
// Cross-account registry access
// ---------------------------------------------------------------------------

/// Ensure the management account's container registry allows the target
/// account to pull images. This mirrors production infrastructure setup
/// where cross-account access is configured before deploying functions.
///
/// Returns `Some(ImagePullCredentials)` when the platform requires explicit
/// registry credentials for cross-account image pulls (Azure), or `None`
/// when IAM-based access is sufficient (AWS, GCP).
pub async fn ensure_cross_account_registry_access(
    platform: Platform,
    config: &TestConfig,
) -> anyhow::Result<Option<alien_core::ImagePullCredentials>> {
    match platform {
        Platform::Aws => {
            ensure_aws_ecr_cross_account_access(config).await?;
            Ok(None)
        }
        Platform::Gcp => {
            ensure_gcp_gar_cross_account_access(config).await?;
            Ok(None)
        }
        Platform::Azure => ensure_azure_acr_cross_account_access(config).await,
        _ => Ok(None),
    }
}

/// Grant the RSM service account (created during InitialSetup) read access to the
/// management project's Artifact Registry. Must be called AFTER setup_target completes.
///
/// During Provisioning, the manager impersonates the RSM SA to make API calls.
/// Cloud Run validates that the CALLER has `artifactregistry.repositories.downloadArtifacts`
/// when updating a service with a cross-project image. The RSM SA needs this access
/// on the management project's AR repo.
pub async fn grant_rsm_gar_access(
    config: &TestConfig,
    rsm_sa_email: &str,
) -> anyhow::Result<()> {
    use alien_gcp_clients::artifactregistry::{ArtifactRegistryApi, ArtifactRegistryClient};

    let mgmt = config
        .gcp_mgmt
        .as_ref()
        .context("GCP management credentials required")?;
    let gar_repo_url = config
        .gcp_resources
        .gar_repository
        .as_ref()
        .context("ALIEN_TEST_GCP_GAR_REPOSITORY required")?;

    let parts: Vec<&str> = gar_repo_url.split('/').collect();
    if parts.len() < 3 {
        anyhow::bail!("Invalid GAR repository URL format: {}", gar_repo_url);
    }
    let location = parts[0]
        .strip_suffix("-docker.pkg.dev")
        .context("Invalid GAR host format")?;
    let repo_id = parts[2];

    let sa_key = mgmt
        .credentials_json
        .as_ref()
        .context("GCP management service account key required")?;

    let gcp_config = alien_core::GcpClientConfig {
        project_id: mgmt.project_id.clone(),
        region: mgmt.region.clone(),
        credentials: alien_core::GcpCredentials::ServiceAccountKey {
            json: sa_key.clone(),
        },
        service_overrides: None,
        project_number: None,
    };

    let ar_client = ArtifactRegistryClient::new(reqwest::Client::new(), gcp_config);
    let mut policy = ar_client
        .get_repository_iam_policy(
            mgmt.project_id.clone(),
            location.to_string(),
            repo_id.to_string(),
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get GAR repository IAM policy: {}", e))?;

    let rsm_member = format!("serviceAccount:{}", rsm_sa_email);
    let reader_role = "roles/artifactregistry.reader";

    let mut found = false;
    for binding in &mut policy.bindings {
        if binding.role == reader_role {
            if !binding.members.contains(&rsm_member) {
                binding.members.push(rsm_member.clone());
            }
            found = true;
            break;
        }
    }
    if !found {
        policy.bindings.push(alien_gcp_clients::Binding {
            role: reader_role.to_string(),
            members: vec![rsm_member.clone()],
            condition: None,
        });
    }

    ar_client
        .set_repository_iam_policy(
            mgmt.project_id.clone(),
            location.to_string(),
            repo_id.to_string(),
            policy,
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to set GAR repository IAM policy: {}", e))?;

    info!(
        repo = %repo_id,
        rsm_sa = %rsm_sa_email,
        "Granted RSM service account AR reader access"
    );

    Ok(())
}

/// Azure ACR cross-subscription access: create a repository-scoped pull
/// token on the management ACR and return its credentials.
///
/// In cross-subscription deployments, the managed identity created in the
/// target subscription cannot pull from the management subscription's ACR
/// (AcrPull role is scoped to the target subscription). Instead, we create
/// a narrowly-scoped ACR token that can only read images from this registry,
/// and pass it as `ImagePullCredentials` to the Container App.
async fn ensure_azure_acr_cross_account_access(
    config: &TestConfig,
) -> anyhow::Result<Option<alien_core::ImagePullCredentials>> {
    use alien_azure_clients::containerregistry::{
        AzureContainerRegistryClient, ContainerRegistryApi,
    };
    use alien_azure_clients::long_running_operation::{
        LongRunningOperationApi, LongRunningOperationClient, OperationResult,
    };
    use alien_azure_clients::models::containerregistry::{
        GenerateCredentialsParameters, GenerateCredentialsResult, ScopeMapProperties,
        TokenProperties, TokenPropertiesStatus,
    };
    use alien_azure_clients::{AzureClientConfig, AzureCredentials, AzureTokenCache};

    let mgmt = config
        .azure_mgmt
        .as_ref()
        .context("Azure management credentials required for cross-subscription ACR access")?;
    let resource_group = config
        .azure_resources
        .resource_group
        .as_ref()
        .context("ALIEN_TEST_AZURE_RESOURCE_GROUP required for ACR token creation")?;
    let registry_name = config
        .azure_resources
        .registry_name
        .as_ref()
        .context("ALIEN_TEST_AZURE_REGISTRY_NAME required for ACR token creation")?;

    let azure_config = AzureClientConfig {
        subscription_id: mgmt.subscription_id.clone(),
        tenant_id: mgmt.tenant_id.clone(),
        region: Some(mgmt.region.clone()),
        credentials: AzureCredentials::ServicePrincipal {
            client_id: mgmt.client_id.clone(),
            client_secret: mgmt.client_secret.clone(),
        },
        service_overrides: None,
    };

    let http_client = reqwest::Client::new();
    let acr_client = AzureContainerRegistryClient::new(
        http_client.clone(),
        AzureTokenCache::new(azure_config.clone()),
    );
    let lro_client = LongRunningOperationClient::new(
        http_client,
        AzureTokenCache::new(azure_config),
    );

    // Use a deterministic name so repeated runs reuse the same token.
    let scope_map_name = "alien-e2e-pull-scope";
    let token_name = "alien-e2e-pull-token";

    // 1. Create (or update) a scope map allowing pull from all repositories.
    info!(
        registry = %registry_name,
        scope_map = scope_map_name,
        "Creating ACR pull scope map"
    );
    let scope_map_result = acr_client
        .create_scope_map(
            resource_group,
            registry_name,
            scope_map_name,
            &ScopeMapProperties {
                description: Some(
                    "E2E test pull-only scope map for cross-subscription access".to_string(),
                ),
                actions: vec!["repositories/*/content/read".to_string()],
                creation_date: None,
                provisioning_state: None,
                type_: None,
            },
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create ACR scope map: {}", e))?;

    scope_map_result
        .wait_for_operation_completion(&lro_client, "CreateScopeMap", scope_map_name)
        .await
        .map_err(|e| anyhow::anyhow!("Failed waiting for scope map creation: {}", e))?;

    // Get the scope map ID.
    let scope_map = acr_client
        .get_scope_map(resource_group, registry_name, scope_map_name)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get scope map: {}", e))?;
    let scope_map_id = scope_map.id.context("Scope map missing ID")?;

    // 2. Create (or update) a token linked to the scope map.
    info!(
        registry = %registry_name,
        token = token_name,
        "Creating ACR pull token"
    );
    let token_result = acr_client
        .create_token(
            resource_group,
            registry_name,
            token_name,
            &TokenProperties {
                scope_map_id: Some(scope_map_id.clone()),
                status: Some(TokenPropertiesStatus::Enabled),
                credentials: None,
                creation_date: None,
                provisioning_state: None,
            },
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create ACR token: {}", e))?;

    token_result
        .wait_for_operation_completion(&lro_client, "CreateToken", token_name)
        .await
        .map_err(|e| anyhow::anyhow!("Failed waiting for token creation: {}", e))?;

    // Get the token to retrieve its resource ID.
    let token = acr_client
        .get_token(resource_group, registry_name, token_name)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get token: {}", e))?;
    let token_id = token.id.context("Token missing ID")?;

    // 3. Generate credentials (username + password) for the token.
    info!(
        registry = %registry_name,
        token = token_name,
        "Generating ACR token credentials"
    );
    let cred_op = acr_client
        .generate_credentials(
            resource_group,
            registry_name,
            &GenerateCredentialsParameters {
                token_id: Some(token_id),
                expiry: None,
                name: None,
            },
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to generate ACR credentials: {}", e))?;

    // Extract the password from the generateCredentials response directly.
    // Azure only returns the password `value` in the generateCredentials response —
    // subsequent GET on the token does NOT include the password value (by design).
    let cred_result = match cred_op {
        OperationResult::Completed(result) => result,
        OperationResult::LongRunning(lro) => {
            // Poll Azure-AsyncOperation until status is "Succeeded".
            lro_client
                .wait_for_completion(&lro, "GenerateCredentials", token_name)
                .await
                .map_err(|e| {
                    anyhow::anyhow!("Failed waiting for credential generation: {}", e)
                })?;

            // For POST LROs, Azure-AsyncOperation returns only status metadata.
            // The actual result must be fetched from the Location URL.
            lro_client
                .fetch_location_result::<GenerateCredentialsResult>(
                    &lro,
                    "GenerateCredentials",
                    token_name,
                )
                .await
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to fetch credential result from Location URL: {}",
                        e
                    )
                })?
        }
    };

    let password = cred_result
        .passwords
        .into_iter()
        .find_map(|p| p.value)
        .context("generateCredentials returned no password value")?;

    let username = token_name.to_string();

    info!(
        registry = %registry_name,
        token = token_name,
        "ACR repository-scoped pull token created successfully"
    );

    Ok(Some(alien_core::ImagePullCredentials { username, password }))
}

/// Set an ECR repository policy allowing Lambda in the target account to
/// pull images from the management account's ECR repository.
async fn ensure_aws_ecr_cross_account_access(config: &TestConfig) -> anyhow::Result<()> {
    use alien_aws_clients::aws::ecr::SetRepositoryPolicyRequest;
    use alien_aws_clients::{EcrApi, EcrClient};
    use alien_core::{AwsClientConfig, AwsCredentials};

    let mgmt = config
        .aws_mgmt
        .as_ref()
        .context("AWS management credentials required for cross-account ECR access")?;
    let target = config
        .aws_target
        .as_ref()
        .context("AWS target credentials required for cross-account ECR access")?;
    let target_account_id = target
        .account_id
        .as_ref()
        .context("AWS_TARGET_ACCOUNT_ID required for cross-account ECR access")?;
    let ecr_repo_url = config
        .aws_resources
        .ecr_repository
        .as_ref()
        .context("ALIEN_TEST_AWS_ECR_REPOSITORY required for cross-account ECR access")?;

    // Extract repository name from URL:
    // "219193354193.dkr.ecr.us-east-1.amazonaws.com/alien-test-lambda" -> "alien-test-lambda"
    let repo_name = ecr_repo_url
        .split('/')
        .last()
        .context("Invalid ECR repository URL format")?;

    let account_id = mgmt
        .account_id
        .as_ref()
        .context("AWS_MANAGEMENT_ACCOUNT_ID required")?
        .clone();

    // Extract ECR region from the repository URL
    let ecr_region = extract_ecr_region(ecr_repo_url)?;

    let aws_config = AwsClientConfig {
        region: ecr_region.clone(),
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

    // Policy: allow Lambda service to pull images cross-account.
    // Two principals are needed:
    // 1. Lambda service principal — for image pull during function deployment
    // 2. Target account root — for ECR authorization and cache management
    // The Lambda service principal statement intentionally omits the sourceArn
    // condition because the function doesn't exist yet during CreateFunction.
    let policy = serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [
            {
                "Sid": "LambdaCrossAccountPull",
                "Effect": "Allow",
                "Principal": {
                    "Service": "lambda.amazonaws.com"
                },
                "Action": [
                    "ecr:BatchGetImage",
                    "ecr:GetDownloadUrlForLayer",
                    "ecr:SetRepositoryPolicy",
                    "ecr:DeleteRepositoryPolicy",
                    "ecr:GetRepositoryPolicy"
                ]
            },
            {
                "Sid": "TargetAccountAccess",
                "Effect": "Allow",
                "Principal": {
                    "AWS": format!("arn:aws:iam::{}:root", target_account_id)
                },
                "Action": [
                    "ecr:BatchGetImage",
                    "ecr:GetDownloadUrlForLayer",
                    "ecr:GetAuthorizationToken"
                ]
            }
        ]
    });

    ecr_client
        .set_repository_policy(SetRepositoryPolicyRequest {
            repository_name: repo_name.to_string(),
            policy_text: policy.to_string(),
            registry_id: None,
            force: Some(true),
        })
        .await
        .map_err(|e| anyhow::anyhow!("Failed to set ECR repository policy: {}", e))?;

    info!(
        repo = %repo_name,
        target_account = %target_account_id,
        "ECR cross-account pull policy set"
    );

    // If the target account is in a different region, also set the policy on
    // the replicated ECR repository in that region. ECR private image
    // replication copies images but NOT repository policies, so Lambda in
    // the target region won't be able to pull without an explicit policy.
    if target.region != ecr_region {
        info!(
            target_region = %target.region,
            ecr_region = %ecr_region,
            "Setting ECR cross-account policy on replicated repo in target region"
        );

        let target_region_config = AwsClientConfig {
            region: target.region.clone(),
            account_id: mgmt
                .account_id
                .as_ref()
                .context("AWS_MANAGEMENT_ACCOUNT_ID required")?
                .clone(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: mgmt.access_key_id.clone(),
                secret_access_key: mgmt.secret_access_key.clone(),
                session_token: mgmt.session_token.clone(),
            },
            service_overrides: None,
        };

        let target_cred_provider =
            alien_aws_clients::AwsCredentialProvider::from_config(target_region_config)
                .await
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to create AWS credential provider for target region: {}",
                        e
                    )
                })?;

        let target_ecr_client =
            EcrClient::new(reqwest::Client::new(), target_cred_provider);

        target_ecr_client
            .set_repository_policy(SetRepositoryPolicyRequest {
                repository_name: repo_name.to_string(),
                policy_text: policy.to_string(),
                registry_id: None,
                force: Some(true),
            })
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to set ECR repository policy in target region {}: {}",
                    target.region,
                    e
                )
            })?;

        info!(
            repo = %repo_name,
            target_region = %target.region,
            "ECR cross-account pull policy set on replicated repo"
        );
    }

    Ok(())
}

/// Add an IAM binding on the management project's GAR repository allowing
/// the target project's Cloud Run service agent to pull images.
async fn ensure_gcp_gar_cross_account_access(config: &TestConfig) -> anyhow::Result<()> {
    use alien_gcp_clients::artifactregistry::{ArtifactRegistryApi, ArtifactRegistryClient};
    use alien_gcp_clients::resource_manager::{ResourceManagerApi, ResourceManagerClient};
    use alien_gcp_clients::Binding;

    let mgmt = config
        .gcp_mgmt
        .as_ref()
        .context("GCP management credentials required for cross-account GAR access")?;
    let target = config
        .gcp_target
        .as_ref()
        .context("GCP target credentials required for cross-account GAR access")?;
    let gar_repo_url = config
        .gcp_resources
        .gar_repository
        .as_ref()
        .context("ALIEN_TEST_GCP_GAR_REPOSITORY required for cross-account GAR access")?;

    // Parse GAR URL: "us-central1-docker.pkg.dev/alien-test-mgmt/alien-test/http-server"
    // -> location=us-central1, project=alien-test-mgmt, repo=alien-test
    let parts: Vec<&str> = gar_repo_url.split('/').collect();
    if parts.len() < 3 {
        anyhow::bail!("Invalid GAR repository URL format: {}", gar_repo_url);
    }
    // parts[0] = "us-central1-docker.pkg.dev"
    // parts[1] = "alien-test-mgmt" (project)
    // parts[2] = "alien-test" (repository)
    let location_host = parts[0]; // "us-central1-docker.pkg.dev"
    let location = location_host
        .strip_suffix("-docker.pkg.dev")
        .context("Invalid GAR host format")?;
    let repo_id = parts[2];

    let sa_key = mgmt
        .credentials_json
        .as_ref()
        .context("GCP management service account key required")?;

    // Build GcpClientConfig for management project
    let gcp_config = alien_core::GcpClientConfig {
        project_id: mgmt.project_id.clone(),
        region: mgmt.region.clone(),
        credentials: alien_core::GcpCredentials::ServiceAccountKey {
            json: sa_key.clone(),
        },
        service_overrides: None,
        project_number: None,
    };

    let http = reqwest::Client::new();

    // Step 1: Get the target project number
    let target_sa_key = target
        .credentials_json
        .as_ref()
        .context("GCP target service account key required")?;
    let target_gcp_config = alien_core::GcpClientConfig {
        project_id: target.project_id.clone(),
        region: target.region.clone(),
        credentials: alien_core::GcpCredentials::ServiceAccountKey {
            json: target_sa_key.clone(),
        },
        service_overrides: None,
        project_number: None,
    };

    let rm_client = ResourceManagerClient::new(http.clone(), target_gcp_config);
    let project_meta = rm_client
        .get_project_metadata(target.project_id.clone())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get target project metadata: {}", e))?;
    let project_number = project_meta
        .project_number
        .context("Target project metadata missing project_number")?;

    info!(
        target_project = %target.project_id,
        target_project_number = %project_number,
        "Retrieved target project number"
    );

    // Step 2: Get the current IAM policy on the GAR repository
    let ar_client = ArtifactRegistryClient::new(http, gcp_config);
    let mut policy = ar_client
        .get_repository_iam_policy(
            mgmt.project_id.clone(),
            location.to_string(),
            repo_id.to_string(),
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get GAR repository IAM policy: {}", e))?;

    // Step 3: Add bindings for cross-project image access.
    // Cloud Run v2 validates image accessibility using the caller's credentials
    // during both CreateService and UpdateService. We need three principals:
    //   a) The Cloud Run service agent — for pulling images at deploy time
    //   b) The target SA — caller during InitialSetup (CreateService)
    //   c) The management SA — caller during Provisioning (UpdateService)
    let service_agent_member = format!(
        "serviceAccount:service-{}@serverless-robot-prod.iam.gserviceaccount.com",
        project_number
    );

    // Extract the target SA email from its credentials JSON
    let target_sa_key_json: serde_json::Value =
        serde_json::from_str(target_sa_key).context("Failed to parse target SA key JSON")?;
    let target_sa_email = target_sa_key_json["client_email"]
        .as_str()
        .context("Target SA key missing client_email")?;
    let target_sa_member = format!("serviceAccount:{}", target_sa_email);

    // Extract the management SA email — this SA is used by the manager's deployment
    // loop during Provisioning to update Cloud Run services with the real image.
    let mgmt_sa_key_json: serde_json::Value =
        serde_json::from_str(sa_key).context("Failed to parse management SA key JSON")?;
    let mgmt_sa_email = mgmt_sa_key_json["client_email"]
        .as_str()
        .context("Management SA key missing client_email")?;
    let mgmt_sa_member = format!("serviceAccount:{}", mgmt_sa_email);

    let reader_role = "roles/artifactregistry.reader";
    let members_to_add = [&service_agent_member, &target_sa_member, &mgmt_sa_member];

    // Merge all members into the existing binding
    let mut found = false;
    for binding in &mut policy.bindings {
        if binding.role == reader_role {
            for member in &members_to_add {
                if !binding.members.contains(member) {
                    binding.members.push((*member).clone());
                }
            }
            found = true;
            break;
        }
    }
    if !found {
        policy.bindings.push(Binding {
            role: reader_role.to_string(),
            members: members_to_add.iter().map(|m| (*m).clone()).collect(),
            condition: None,
        });
    }

    // Step 4: Set the updated policy
    ar_client
        .set_repository_iam_policy(
            mgmt.project_id.clone(),
            location.to_string(),
            repo_id.to_string(),
            policy,
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to set GAR repository IAM policy: {}", e))?;

    info!(
        repo = %repo_id,
        service_agent = %service_agent_member,
        target_sa = %target_sa_member,
        mgmt_sa = %mgmt_sa_member,
        "GAR cross-project reader bindings set on repository"
    );

    // Step 5: Also add project-level Artifact Registry Reader on the management project.
    // Cloud Run cross-project image access may require project-level IAM in addition
    // to repository-level IAM for the service agent to discover and access the repository.
    let rm_mgmt_client = ResourceManagerClient::new(reqwest::Client::new(), {
        alien_core::GcpClientConfig {
            project_id: mgmt.project_id.clone(),
            region: mgmt.region.clone(),
            credentials: alien_core::GcpCredentials::ServiceAccountKey {
                json: sa_key.clone(),
            },
            service_overrides: None,
            project_number: None,
        }
    });

    let mut project_policy = rm_mgmt_client
        .get_project_iam_policy(mgmt.project_id.clone(), None)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get management project IAM policy: {}", e))?;

    // Merge bindings into project-level policy (same members as repo-level)
    let mut project_binding_found = false;
    for binding in &mut project_policy.bindings {
        if binding.role == reader_role {
            for member in &members_to_add {
                if !binding.members.contains(member) {
                    binding.members.push((*member).clone());
                }
            }
            project_binding_found = true;
            break;
        }
    }
    if !project_binding_found {
        project_policy.bindings.push(Binding {
            role: reader_role.to_string(),
            members: members_to_add.iter().map(|m| (*m).clone()).collect(),
            condition: None,
        });
    }

    rm_mgmt_client
        .set_project_iam_policy(mgmt.project_id.clone(), project_policy, None)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to set management project IAM policy: {}", e))?;

    info!(
        project = %mgmt.project_id,
        service_agent = %service_agent_member,
        target_sa = %target_sa_member,
        mgmt_sa = %mgmt_sa_member,
        "GAR cross-project reader bindings set on project"
    );

    Ok(())
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
