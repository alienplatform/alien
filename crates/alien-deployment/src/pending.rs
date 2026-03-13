use crate::{
    DeploymentConfig, DeploymentState, DeploymentStatus, DeploymentStepResult, ErrorData, Result,
};
use alien_core::{Stack, StackState};
use alien_error::Context;
use tracing::info;

/// Handle Pending → InitialSetup transition
///
/// This step:
/// 1. Initializes stack state with platform-specific settings
/// 2. Collects environment information from the cloud platform
/// 3. Runs preflight checks (mutations are applied in subsequent phases)
pub async fn handle_pending(
    current: DeploymentState,
    target_stack: Stack,
    config: DeploymentConfig,
    client_config: alien_core::ClientConfig,
    _service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<DeploymentStepResult> {
    info!("Handling Pending status");

    // Step 1: Initialize stack state
    let stack_state = StackState::new(current.platform);
    info!(
        "Initialized stack state for platform {:?}",
        current.platform
    );

    // Step 2: Collect environment information
    let environment_info =
        crate::helpers::collect_environment_info(current.platform, &client_config)
            .await
            .context(ErrorData::EnvironmentInfoCollectionFailed {
                platform: format!("{:?}", current.platform),
                reason: "Failed to collect cloud environment details".to_string(),
            })?;

    info!(
        "Collected environment info for platform {:?}",
        current.platform
    );

    // Step 3: Run deployment-time preflights (compile-time + mutations + runtime checks)
    // Store the mutated stack for use in subsequent phases (InitialSetup, Provisioning)
    let runner = alien_preflights::runner::PreflightRunner::new();
    let (mutated_stack, _deployment_summary) = runner
        .run_deployment_time_preflights(
            target_stack.clone(),
            &stack_state,
            &config,
            &client_config,
            None,  // No old stack for initial deployment
            false, // Never skip frozen check on initial deployment
        )
        .await
        .context(ErrorData::PreflightChecksFailed)?;

    info!("Deployment-time preflight checks completed successfully");

    // Step 4: Store prepared stack and inject environment variables
    let mut runtime_metadata = alien_core::RuntimeMetadata::default();
    runtime_metadata.prepared_stack = Some(mutated_stack.clone());

    // Inject environment variables into the prepared stack for validation
    let mut mutated_stack_with_env = mutated_stack;
    crate::helpers::inject_environment_variables(&mut mutated_stack_with_env, &config)?;
    if let Some(monitoring) = &config.monitoring {
        crate::helpers::inject_monitoring_environment_variables(
            &mut mutated_stack_with_env,
            monitoring,
        )?;
    }

    // Note: We don't store the stack with env vars injected, just validate it works
    // Each phase will inject env vars fresh from the prepared stack

    // Step 5: Return update to transition to InitialSetup
    let mut next = current.clone();
    next.status = DeploymentStatus::InitialSetup;
    next.stack_state = Some(stack_state);
    next.environment_info = Some(environment_info);
    next.runtime_metadata = Some(runtime_metadata);
    // Error handled in DeploymentStepResult

    Ok(DeploymentStepResult {
        state: next,
        error: None,
        suggested_delay_ms: None,
        update_heartbeat: false,
    })
}
