//! Target account setup for cross-account E2E tests.
//!
//! Mirrors the production `alien-deploy-cli` push model flow: steps the
//! deployment through InitialSetup using **target credentials** until it reaches
//! Provisioning status, then reconciles the state (including RSM outputs) back
//! to the manager DB. After this, the manager's deployment loop takes over using
//! management SA impersonation + RSM cross-account role.

use std::sync::Arc;
use std::time::Duration;

use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus, EnvironmentVariablesSnapshot,
    ExternalBindings, ManagementConfig, Platform, ReleaseInfo, StackSettings,
};
use anyhow::Context;
use tracing::info;

use crate::config::TestConfig;
use crate::deployment::TestDeployment;
use crate::manager::TestManager;

/// Maximum number of step() calls before giving up.
const MAX_STEPS: usize = 200;

/// Build a `ClientConfig` from the test config's **target** credentials.
fn build_target_client_config(
    config: &TestConfig,
    platform: Platform,
) -> anyhow::Result<ClientConfig> {
    match platform {
        Platform::Aws => {
            let target = config
                .aws_target
                .as_ref()
                .context("Missing AWS target credentials")?;
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
/// This mirrors what `alien-deploy-cli` does in production:
/// 1. Acquires the deployment lock via `/v1/sync/acquire`
/// 2. Runs `alien_deployment::step()` with **target credentials** until
///    the deployment reaches `Provisioning` status
/// 3. Reconciles state (including RSM outputs) back to the manager
/// 4. Releases the lock
///
/// After this function returns, the manager's deployment loop will resume
/// from `Provisioning` using its own management SA impersonation chain.
pub async fn setup_target(
    config: &TestConfig,
    platform: Platform,
    deployment: &TestDeployment,
    manager: &Arc<TestManager>,
    management_config: Option<ManagementConfig>,
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
        "setup_target: stepping deployment through InitialSetup with target credentials"
    );

    let http = manager.http_client();
    let target_config = build_target_client_config(config, platform)?;

    // 1. Fetch the current deployment state from the manager via raw HTTP
    //    (the SDK returns serde_json::Value for stack_state/environment_info,
    //    so we deserialize the full JSON ourselves for type safety).
    let dep_resp: serde_json::Value = http
        .get(format!("{}/v1/deployments/{}", manager.url, deployment.id))
        .send()
        .await
        .context("Failed to get deployment")?
        .json()
        .await
        .context("Failed to parse deployment response")?;

    let release_id = dep_resp
        .get("desiredReleaseId")
        .and_then(|v| v.as_str())
        .context("Deployment has no desired release")?
        .to_string();

    // Also fetch the release stack.
    let release_resp: serde_json::Value = http
        .get(format!("{}/v1/releases/{}", manager.url, release_id))
        .send()
        .await
        .context("Failed to fetch release")?
        .json()
        .await
        .context("Failed to parse release response")?;

    let stack_json = release_resp
        .get("stack")
        .and_then(|s| s.get(platform.as_str()))
        .context("Release missing stack for platform")?;

    let stack: alien_core::Stack =
        serde_json::from_value(stack_json.clone()).context("Failed to deserialize stack")?;

    let target_release = ReleaseInfo {
        release_id: release_id.clone(),
        version: None,
        description: None,
        stack,
    };

    // Deserialize stack_state and environment_info from the deployment response.
    let stack_state = dep_resp
        .get("stackState")
        .and_then(|v| if v.is_null() { None } else { Some(v) })
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()
        .context("Failed to deserialize stack_state")?;

    let environment_info = dep_resp
        .get("environmentInfo")
        .and_then(|v| if v.is_null() { None } else { Some(v) })
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()
        .context("Failed to deserialize environment_info")?;

    let stack_settings: StackSettings = dep_resp
        .get("stackSettings")
        .and_then(|v| if v.is_null() { None } else { Some(v) })
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()
        .context("Failed to deserialize stack_settings")?
        .unwrap_or_default();

    let mut state = DeploymentState {
        status: DeploymentStatus::Pending,
        platform,
        current_release: None,
        target_release: Some(target_release),
        stack_state,
        environment_info,
        runtime_metadata: None,
        retry_requested: false,
    };

    let deploy_config = DeploymentConfig {
        stack_settings,
        management_config,
        environment_variables: EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: String::new(),
        },
        allow_frozen_changes: false,
        artifact_registry: None,
        compute_backend: None,
        external_bindings: ExternalBindings::new(),
        image_pull_credentials: None,
        public_urls: None,
        domain_metadata: None,
        monitoring: None,
    };

    // 2. Acquire the deployment lock.
    let session_id = format!("e2e-setup-target-{}", uuid::Uuid::new_v4());
    let acquire_body = serde_json::json!({
        "deploymentId": deployment.id,
        "session": session_id,
    });

    let acquire_resp = http
        .post(format!("{}/v1/sync/acquire", manager.url))
        .json(&acquire_body)
        .send()
        .await
        .context("Failed to acquire deployment lock")?;

    if !acquire_resp.status().is_success() {
        let body = acquire_resp.text().await.unwrap_or_default();
        anyhow::bail!("Failed to acquire lock: {}", body);
    }

    info!("Lock acquired, stepping deployment with target credentials");

    // 3. Step the deployment until Provisioning.
    let mut steps = 0;
    loop {
        if steps >= MAX_STEPS {
            // Release lock before failing
            let _ = http
                .post(format!("{}/v1/sync/release", manager.url))
                .json(&serde_json::json!({
                    "deploymentId": deployment.id,
                    "session": session_id,
                }))
                .send()
                .await;
            anyhow::bail!(
                "setup_target exceeded {} steps without reaching Provisioning",
                MAX_STEPS
            );
        }

        let result = alien_deployment::step(
            state.clone(),
            deploy_config.clone(),
            target_config.clone(),
            None,
        )
        .await
        .map_err(|e| anyhow::anyhow!("Deployment step failed: {}", e))?;

        state = result.state;
        steps += 1;

        info!(
            status = ?state.status,
            step = steps,
            "setup_target step completed"
        );

        if state.status == DeploymentStatus::Provisioning {
            info!("Target setup complete — deployment reached Provisioning");
            break;
        }

        // If step suggests a delay, wait for it
        if let Some(delay_ms) = result.suggested_delay_ms {
            if delay_ms > 100 {
                tokio::time::sleep(Duration::from_millis(delay_ms.min(5000))).await;
            }
        }
    }

    // 4. Reconcile state back to the manager.
    let reconcile_body = serde_json::json!({
        "deploymentId": deployment.id,
        "session": session_id,
        "status": format!("{:?}", state.status).to_lowercase(),
        "stackState": state.stack_state,
        "environmentInfo": state.environment_info,
        "currentReleaseId": state.current_release.as_ref().map(|r| &r.release_id),
    });

    let reconcile_resp = http
        .post(format!("{}/v1/sync/reconcile", manager.url))
        .json(&reconcile_body)
        .send()
        .await
        .context("Failed to reconcile deployment state")?;

    if !reconcile_resp.status().is_success() {
        let body = reconcile_resp.text().await.unwrap_or_default();
        anyhow::bail!("Failed to reconcile state: {}", body);
    }

    info!("State reconciled to manager");

    // 5. Release the lock.
    let release_resp = http
        .post(format!("{}/v1/sync/release", manager.url))
        .json(&serde_json::json!({
            "deploymentId": deployment.id,
            "session": session_id,
        }))
        .send()
        .await
        .context("Failed to release deployment lock")?;

    if !release_resp.status().is_success() {
        let body = release_resp.text().await.unwrap_or_default();
        tracing::warn!("Failed to release lock (continuing): {}", body);
    }

    info!(
        deployment_id = %deployment.id,
        "setup_target complete — manager will continue from Provisioning"
    );

    Ok(())
}
