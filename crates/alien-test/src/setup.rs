//! Target account setup for cross-account E2E tests.
//!
//! Delegates to `alien_deploy_cli::commands::push_initial_setup` which implements
//! the full push-model flow: fetch deployment state, acquire sync lock, step
//! through InitialSetup, reconcile state back, release lock.

use std::sync::Arc;

use alien_core::{ClientConfig, ManagementConfig, Platform};
use anyhow::Context;
use tracing::info;

use crate::config::TestConfig;
use crate::deployment::TestDeployment;
use crate::manager::TestManager;

/// Build a `ClientConfig` from the test config's **target** credentials.
///
/// For AWS, if `AWS_TARGET_PROVISIONING_ROLE_ARN` is set, assumes that role
/// via STS first. This is required because `lambda:CreateFunction` with a
/// cross-account ECR image requires the **calling principal** (not just the
/// execution role) to have `ecr:BatchGetImage` and `ecr:GetDownloadUrlForLayer`
/// on the management account's ECR repository.
async fn build_target_client_config(
    config: &TestConfig,
    platform: Platform,
) -> anyhow::Result<ClientConfig> {
    match platform {
        Platform::Aws => {
            let target = config
                .aws_target
                .as_ref()
                .context("Missing AWS target credentials")?;

            // If a provisioning role is configured, assume it to get credentials
            // with cross-account ECR access for Lambda image deployment.
            if let Some(ref role_arn) = target.provisioning_role_arn {
                info!(
                    role_arn = %role_arn,
                    "Assuming provisioning role for cross-account ECR access"
                );

                let aws_config = alien_core::AwsClientConfig {
                    account_id: target.account_id.clone().unwrap_or_default(),
                    region: target.region.clone(),
                    credentials: alien_core::AwsCredentials::AccessKeys {
                        access_key_id: target.access_key_id.clone(),
                        secret_access_key: target.secret_access_key.clone(),
                        session_token: target.session_token.clone(),
                    },
                    service_overrides: None,
                };

                let sts_client =
                    alien_aws_clients::StsClient::new(reqwest::Client::new(), aws_config);

                use alien_aws_clients::StsApi;
                let assume_response = sts_client
                    .assume_role(
                        alien_aws_clients::aws::sts::AssumeRoleRequest::builder()
                            .role_arn(role_arn.clone())
                            .role_session_name("alien-e2e-provisioning".to_string())
                            .duration_seconds(3600)
                            .build(),
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to assume provisioning role: {}", e))?;

                let creds = assume_response.assume_role_result.credentials;

                return Ok(ClientConfig::Aws(Box::new(alien_core::AwsClientConfig {
                    account_id: target.account_id.clone().unwrap_or_default(),
                    region: target.region.clone(),
                    credentials: alien_core::AwsCredentials::AccessKeys {
                        access_key_id: creds.access_key_id,
                        secret_access_key: creds.secret_access_key,
                        session_token: Some(creds.session_token),
                    },
                    service_overrides: None,
                })));
            }

            Ok(ClientConfig::Aws(Box::new(alien_core::AwsClientConfig {
                account_id: target.account_id.clone().unwrap_or_default(),
                region: target.region.clone(),
                credentials: alien_core::AwsCredentials::AccessKeys {
                    access_key_id: target.access_key_id.clone(),
                    secret_access_key: target.secret_access_key.clone(),
                    session_token: target.session_token.clone(),
                },
                service_overrides: None,
            })))
        }
        Platform::Gcp => {
            let target = config
                .gcp_target
                .as_ref()
                .context("Missing GCP target credentials")?;
            let credentials = if let Some(ref json) = target.credentials_json {
                alien_core::GcpCredentials::ServiceAccountKey {
                    json: json.clone(),
                }
            } else {
                anyhow::bail!("GCP target credentials must include service account key JSON");
            };
            Ok(ClientConfig::Gcp(Box::new(alien_core::GcpClientConfig {
                project_id: target.project_id.clone(),
                region: target.region.clone(),
                credentials,
                service_overrides: None,
                project_number: None,
            })))
        }
        Platform::Azure => {
            let target = config
                .azure_target
                .as_ref()
                .context("Missing Azure target credentials")?;
            Ok(ClientConfig::Azure(Box::new(alien_core::AzureClientConfig {
                subscription_id: target.subscription_id.clone(),
                tenant_id: target.tenant_id.clone(),
                region: Some(target.region.clone()),
                credentials: alien_core::AzureCredentials::ServicePrincipal {
                    client_id: target.client_id.clone(),
                    client_secret: target.client_secret.clone(),
                },
                service_overrides: None,
            })))
        }
        other => anyhow::bail!("setup_target not supported for platform: {}", other),
    }
}

/// Run the target-side setup for cross-account deployment.
///
/// Delegates to `alien_deploy_cli::commands::push_initial_setup` which:
/// 1. Fetches deployment + release state from the manager
/// 2. Acquires the deployment sync lock
/// 3. Steps the deployment through InitialSetup with **target credentials**
///    until it reaches Provisioning (or fails)
/// 4. Reconciles state (including RSM outputs) back to the manager
/// 5. Releases the lock
///
/// After this function returns, the manager's deployment loop will resume
/// from `Provisioning` using its own management SA impersonation chain.
pub async fn setup_target(
    config: &TestConfig,
    platform: Platform,
    deployment: &TestDeployment,
    manager: &Arc<TestManager>,
    management_config: Option<ManagementConfig>,
    image_pull_credentials: Option<alien_core::ImagePullCredentials>,
) -> anyhow::Result<()> {
    if !config.has_platform(platform) {
        anyhow::bail!(
            "Cannot set up target for {}: missing management or target credentials",
            platform.as_str()
        );
    }

    info!(
        platform = %platform.as_str(),
        deployment_id = %deployment.id,
        "setup_target: delegating to push_initial_setup"
    );

    let target_config = build_target_client_config(config, platform).await?;

    alien_deploy_cli::commands::push_initial_setup(
        manager.client(),
        &deployment.id,
        platform,
        target_config,
        management_config,
        image_pull_credentials,
    )
    .await
    .map_err(|e| anyhow::anyhow!("push_initial_setup failed: {}", e))?;

    info!(
        deployment_id = %deployment.id,
        "setup_target complete — manager will continue from Provisioning"
    );

    Ok(())
}

/// Tears down a deployment by running the deletion state machine locally
/// with target-environment credentials.
///
/// Mirrors `setup_target` but drives DeletePending → Deleting → Deleted
/// via `alien_deploy_cli::commands::push_deletion`.
pub async fn teardown_target(
    config: &TestConfig,
    platform: Platform,
    deployment_id: &str,
    manager: &Arc<TestManager>,
) -> anyhow::Result<()> {
    info!(
        platform = %platform.as_str(),
        %deployment_id,
        "teardown_target: delegating to push_deletion"
    );

    let target_config = build_target_client_config(config, platform).await?;

    alien_deploy_cli::commands::push_deletion(
        manager.client(),
        deployment_id,
        platform,
        target_config,
    )
    .await
    .map_err(|e| anyhow::anyhow!("push_deletion failed: {}", e))?;

    info!(
        %deployment_id,
        "teardown_target complete — deployment resources deleted"
    );

    Ok(())
}
