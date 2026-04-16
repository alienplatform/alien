//! Alien Deployment - Unified deployment system
//!
//! This crate provides a single, resumable state machine for deploying infrastructure
//! across all platforms. It provides one function: `step()` that performs incremental
//! deployment steps.

mod deleting;
mod error;
mod helpers;
mod initial_setup;
pub mod loop_contract;
pub mod manager_api_transport;
mod pending;
mod provisioning;
pub mod runner;
mod running;
pub mod transport;
mod updating;

pub use error::{ErrorData, ResourceError, Result};
// Re-export types from alien-core
pub use alien_core::{
    AwsEnvironmentInfo, AzureEnvironmentInfo, DeploymentConfig, DeploymentState, DeploymentStatus,
    DeploymentStepResult, EnvironmentInfo, GcpEnvironmentInfo, Stack,
};

// Re-export helper functions
pub use helpers::collect_environment_info;
pub use helpers::create_aggregated_error_from_stack_state;

use tracing::{debug, info, warn};

/// Resolve the GCP project number if the client config is GCP and project_number is not yet set.
/// GCP IAM condition expressions use project number (numeric) in resource.name, not project ID.
async fn resolve_gcp_project_number(
    client_config: alien_core::ClientConfig,
) -> alien_core::ClientConfig {
    if let alien_core::ClientConfig::Gcp(mut gcp_config) = client_config {
        if gcp_config.project_number.is_none() {
            let rm_client = alien_gcp_clients::ResourceManagerClient::new(
                reqwest::Client::new(),
                *gcp_config.clone(),
            );
            match alien_gcp_clients::ResourceManagerApi::get_project_metadata(
                &rm_client,
                gcp_config.project_id.clone(),
            )
            .await
            {
                Ok(project) => {
                    gcp_config.project_number = project.project_number;
                }
                Err(e) => {
                    warn!(error = %e, "Failed to resolve GCP project number; IAM conditions may not work");
                }
            }
        }
        alien_core::ClientConfig::Gcp(gcp_config)
    } else {
        client_config
    }
}

/// Execute one deployment step based on the current deployment state.
///
/// This function:
/// - Takes the current deployment state (which includes target release with stack)
/// - Takes the deployment configuration (management config, capabilities, env vars)
/// - Takes cloud credentials for executing the deployment
/// - Takes an optional service provider for platform services (defaults to DefaultPlatformServiceProvider)
/// - Does ONE incremental step based on `current.status`
/// - Returns the complete next state (not a delta)
/// - Works identically whether called from CLI, controller, or manager
///
/// The caller is responsible for:
/// - Acquiring a lock on the deployment before calling
/// - Saving the returned state
/// - Releasing the lock after updating
///
/// # Arguments
/// * `current` - Current deployment state (includes current_release and target_release)
/// * `config` - Deployment configuration (management config, capabilities, env vars)
/// * `client_config` - Cloud credentials for deployment execution
/// * `service_provider` - Optional platform service provider (uses default if None)
///
/// # Returns
/// `DeploymentStepResult` containing the complete next state and platform hints
pub async fn step(
    current: DeploymentState,
    config: DeploymentConfig,
    client_config: alien_core::ClientConfig,
    service_provider: Option<std::sync::Arc<dyn alien_infra::PlatformServiceProvider>>,
) -> Result<DeploymentStepResult> {
    info!(
        "Executing deployment step (status: {:?}, platform: {:?})",
        current.status, current.platform
    );

    // Resolve GCP project number for IAM condition expressions
    let client_config = resolve_gcp_project_number(client_config).await;

    // Extract target stack from target_release (optional — only required by
    // Pending, UpdatePending, Delete*, and *Failed handlers that receive the
    // stack directly; InitialSetup/Provisioning/Running/Updating get it from
    // runtime_metadata.prepared_stack instead).
    let target_stack = current.target_release.as_ref().map(|r| r.stack.clone());

    // Use provided service provider or default
    let service_provider = service_provider.unwrap_or_else(|| {
        std::sync::Arc::new(alien_infra::DefaultPlatformServiceProvider::default())
    });

    // Dispatch to appropriate handler based on status
    // Mutation and injection strategy:
    // - Pending: Applies mutations, stores prepared_stack in runtime_metadata, validates env var injection
    // - UpdatePending: Applies mutations, stores prepared_stack in runtime_metadata
    // - InitialSetup: Use prepared_stack, deploy all resources with env vars injected
    // - Provisioning/Updating: Use prepared_stack with env vars injected for functions/services
    // - Running: Use prepared_stack with env vars for health checks (read-only, no config changes)
    // - Delete phases: Use prepared_stack for deletion (no env var injection needed)
    //
    // This ensures:
    // 1. Mutations are applied once per deployment phase (in Pending/UpdatePending)
    // 2. Env vars are only injected when deploying resources that use them (functions/services)
    // 3. Health checks and deletions don't modify resource configurations
    // Helper to require target_stack for handlers that need it
    let require_target_stack = || -> Result<Stack> {
        target_stack.clone().ok_or_else(|| {
            alien_error::AlienError::new(ErrorData::MissingConfiguration {
                message: "Target release required for deployment".to_string(),
            })
        })
    };

    let update = match current.status {
        DeploymentStatus::Pending => {
            pending::handle_pending(
                current,
                require_target_stack()?,
                config,
                client_config,
                service_provider,
            )
            .await?
        }
        DeploymentStatus::InitialSetup => {
            initial_setup::handle_initial_setup(current, config, client_config, service_provider)
                .await?
        }
        DeploymentStatus::Provisioning => {
            provisioning::handle_provisioning(current, config, client_config, service_provider)
                .await?
        }
        DeploymentStatus::Running => {
            running::handle_running(current, config, client_config, service_provider).await?
        }
        DeploymentStatus::UpdatePending => {
            updating::handle_update_pending(
                current,
                require_target_stack()?,
                config,
                client_config,
                service_provider,
            )
            .await?
        }
        DeploymentStatus::Updating => {
            updating::handle_updating(current, config, client_config, service_provider).await?
        }
        DeploymentStatus::DeletePending => {
            deleting::handle_delete_pending(current, config, client_config, service_provider)
                .await?
        }
        DeploymentStatus::Deleting => {
            deleting::handle_deleting(current, config, client_config, service_provider).await?
        }
        // Failed states - retry failed resources and transition back to active status
        DeploymentStatus::InitialSetupFailed => {
            initial_setup::handle_initial_setup_failed(
                current,
                require_target_stack()?,
                config,
                client_config,
                service_provider,
            )
            .await?
        }
        DeploymentStatus::ProvisioningFailed => {
            provisioning::handle_provisioning_failed(
                current,
                require_target_stack()?,
                config,
                client_config,
                service_provider,
            )
            .await?
        }
        DeploymentStatus::UpdateFailed => {
            updating::handle_update_failed(
                current,
                require_target_stack()?,
                config,
                client_config,
                service_provider,
            )
            .await?
        }
        DeploymentStatus::DeleteFailed => {
            deleting::handle_delete_failed(current, config, client_config, service_provider).await?
        }
        DeploymentStatus::RefreshFailed => {
            running::handle_refresh_failed(
                current,
                require_target_stack()?,
                config,
                client_config,
                service_provider,
            )
            .await?
        }
        DeploymentStatus::Deleted => {
            debug!("Deployment is deleted, no action");
            DeploymentStepResult {
                state: current,
                error: None,
                suggested_delay_ms: None,
                update_heartbeat: false,
            }
        }
        DeploymentStatus::Error => {
            debug!("Deployment is in error state, no action");
            DeploymentStepResult {
                state: current,
                error: None,
                suggested_delay_ms: None,
                update_heartbeat: false,
            }
        }
    };

    Ok(update)
}
