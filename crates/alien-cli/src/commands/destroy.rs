//! Destroy command — tears down a deployment's cloud resources via the manager.
//!
//! Flow:
//! 1. Resolve tracked deployment
//! 2. Discover manager (resolve_manager)
//! 3. Request deletion via manager
//! 4. Run deletion step loop (acquire → step → reconcile → release)

use crate::deployment_tracking::DeploymentTracker;
use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::ui::{command, contextual_heading, dim_label, success_line, FixedSteps};
use alien_core::{ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus, Platform};
use alien_deployment::loop_contract::{LoopOperation, LoopOutcome};
use alien_deployment::manager_api_transport::{
    acquire_deployment, final_reconcile, release_deployment, ManagerApiTransport,
};
use alien_deployment::runner::{RunnerPolicy, RunnerResult};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_infra::ClientConfigExt;
use clap::Parser;
use std::str::FromStr;
use tracing::info;
use uuid::Uuid;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Destroy resources from a deployment",
    long_about = "Destroy a deployment's cloud resources via the manager.",
    after_help = "EXAMPLES:
    # Destroy a tracked deployment
    alien destroy --name production --platform aws

    # Force-destroy (skip resource teardown)
    alien destroy --name production --platform aws --force"
)]
pub struct DestroyArgs {
    /// Deployment API key for authentication (optional if already tracked)
    #[arg(long)]
    pub token: Option<String>,

    /// Deployment name
    #[arg(long)]
    pub name: String,

    /// Target platform
    #[arg(long)]
    pub platform: String,

    /// Force-destroy: skip resource teardown and delete the record immediately.
    #[arg(long)]
    pub force: bool,
}

pub async fn destroy_task(args: DestroyArgs, ctx: ExecutionMode) -> Result<()> {
    info!("Starting destroy command");
    println!("{}", contextual_heading("Destroying", &args.name, &[]));
    let steps = FixedSteps::new(&["Resolve deployment", "Resolve manager", "Delete resources"]);
    steps.activate(0, Some(format!("Deployment {}", args.name)));

    let platform = Platform::from_str(&args.platform).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "platform".to_string(),
            message: e,
        })
    })?;

    // Step 1: Resolve tracked deployment
    let tracker = DeploymentTracker::new()?;
    let tracked_deployment = tracker
        .get_deployment(&args.name)
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "name".to_string(),
                message: format!(
                    "Deployment '{}' is not tracked. Deploy it first with 'alien deploy'",
                    args.name
                ),
            })
        })?
        .clone();

    steps.complete(
        0,
        Some(format!(
            "{} ({})",
            args.name, tracked_deployment.deployment_id
        )),
    );

    // Step 2: Resolve manager
    steps.activate(1, Some("Discovering manager...".to_string()));

    let manager_ctx = ctx
        .resolve_manager(&tracked_deployment.project_id, &args.platform)
        .await?;
    let manager_client = manager_ctx.client;

    steps.complete(1, Some(format!("Manager: {}", manager_ctx.manager_url)));

    // Step 3: Delete via manager
    steps.activate(2, Some(tracked_deployment.deployment_id.clone()));

    // Request deletion
    manager_client
        .delete_deployment()
        .id(&tracked_deployment.deployment_id)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to request deployment deletion".to_string(),
        })?;

    if args.force {
        steps.complete(2, Some("Force-deleted".to_string()));
        drop(steps);
        eprintln!();
        println!("{}", success_line("Deployment force-deleted."));
        return Ok(());
    }

    // Run the deletion step loop
    let client_config =
        ClientConfig::from_std_env(platform)
            .await
            .context(ErrorData::ConfigurationError {
                message: format!("Failed to build client config for platform {:?}", platform),
            })?;

    // Fetch deployment state
    let deployment = manager_client
        .get_deployment()
        .id(&tracked_deployment.deployment_id)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    let status: DeploymentStatus =
        serde_json::from_value(serde_json::Value::String(deployment.status.clone()))
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: format!("Unknown deployment status: {}", deployment.status),
            })?;

    let mut current = DeploymentState {
        status,
        platform,
        current_release: None,
        target_release: None,
        stack_state: deployment
            .stack_state
            .map(serde_json::from_value)
            .transpose()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to deserialize stack_state".to_string(),
            })?,
        environment_info: deployment
            .environment_info
            .map(serde_json::from_value)
            .transpose()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to deserialize environment_info".to_string(),
            })?,
        runtime_metadata: deployment
            .runtime_metadata
            .map(|rm| serde_json::to_value(rm).and_then(serde_json::from_value))
            .transpose()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to deserialize runtime_metadata".to_string(),
            })?,
        retry_requested: deployment.retry_requested,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    };

    let stack_settings: alien_core::StackSettings = deployment
        .stack_settings
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_settings".to_string(),
        })?
        .unwrap_or_default();

    let mut config: DeploymentConfig = serde_json::from_value(serde_json::json!({
        "stackSettings": serde_json::to_value(&stack_settings).unwrap_or_default(),
        "environmentVariables": {
            "variables": [],
            "hash": "",
            "createdAt": ""
        }
    }))
    .into_alien_error()
    .context(ErrorData::ConfigurationError {
        message: "Failed to construct deployment config".to_string(),
    })?;

    // Acquire → step loop → reconcile → release
    let session = format!("cli-destroy-{}", Uuid::new_v4());
    acquire_deployment(&manager_client, &tracked_deployment.deployment_id, &session)
        .await
        .context(ErrorData::ConfigurationError {
            message: "Failed to acquire deployment lock for deletion".to_string(),
        })?;

    // Re-fetch under lock
    let deployment = manager_client
        .get_deployment()
        .id(&tracked_deployment.deployment_id)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to re-fetch deployment under lock".to_string(),
        })?
        .into_inner();

    current.status = serde_json::from_value(serde_json::Value::String(deployment.status.clone()))
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Unknown deployment status: {}", deployment.status),
        })?;
    current.stack_state = deployment
        .stack_state
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_state".to_string(),
        })?;

    let transport = ManagerApiTransport::new(manager_client.clone(), session.clone());
    let policy = RunnerPolicy {
        max_steps: 400,
        operation: LoopOperation::Delete,
        delay_threshold: None,
    };

    let runner_result = alien_deployment::runner::run_step_loop(
        &mut current,
        &mut config,
        &client_config,
        &tracked_deployment.deployment_id,
        &policy,
        &transport,
        None,
        None,
    )
    .await;

    // Always reconcile + release
    final_reconcile(
        &manager_client,
        &tracked_deployment.deployment_id,
        &session,
        &current,
    )
    .await;
    release_deployment(&manager_client, &tracked_deployment.deployment_id, &session).await;

    let RunnerResult {
        loop_result,
        steps_executed,
    } = runner_result.context(ErrorData::GenericError {
        message: "deletion step loop failed".to_string(),
    })?;

    info!(
        steps_executed = steps_executed,
        stop_reason = ?loop_result.stop_reason,
        outcome = ?loop_result.outcome,
        final_status = ?loop_result.final_status,
        "Deletion loop finished"
    );

    match loop_result.outcome {
        LoopOutcome::Success => {
            steps.complete(2, Some("Deleted".to_string()));
            println!("{}", success_line("Deployment destroyed."));
        }
        LoopOutcome::Failure => {
            steps.fail(2, Some(format!("{:?}", loop_result.final_status)));
            return Err(AlienError::new(ErrorData::DeploymentFailed {
                message: format!("deletion failed at status {:?}", loop_result.final_status),
            }));
        }
        LoopOutcome::Neutral => {
            steps.complete(2, Some("Deletion in progress".to_string()));
        }
    }
    drop(steps);

    println!(
        "{} {} ({})",
        dim_label("Deployment"),
        args.name,
        tracked_deployment.deployment_id
    );
    println!(
        "{} {}",
        dim_label("Next"),
        command(&format!(
            "alien deployments get {}",
            tracked_deployment.deployment_id
        ))
    );

    Ok(())
}
