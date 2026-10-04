//! Destroy command — tears down a deployment's cloud resources via the manager.
//!
//! Flow:
//! 1. Resolve tracked deployment
//! 2. Discover manager (resolve_manager)
//! 3. Request deletion via manager
//! 4. Run deletion step loop (acquire → step → reconcile → release)

use crate::commands::deploy::deployment_manager_http_client;
use crate::deployment_tracking::{DeploymentTracker, TrackedDeployment};
use crate::error::{ErrorData, Result};
use crate::execution_context::{ExecutionMode, ManagerContext};
use crate::ui::{command, contextual_heading, dim_label, success_line, FixedSteps};
use alien_core::{ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus, Platform};
use alien_deployment::loop_contract::{LoopOperation, LoopOutcome};
use alien_deployment::manager_api_transport::{
    acquire_setup_delete_deployment, combine_operation_and_finalization, final_reconcile,
    ManagerApiTransport, SetupDeleteAcquireOutcome,
};
use alien_deployment::runner::{preserve_semantic_failure, RunnerPolicy, RunnerResult};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
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

    /// Target platform (required by `alien destroy`; `alien dev destroy` is always local)
    #[arg(long)]
    pub platform: Option<String>,

    /// Force-destroy: skip resource teardown and delete the record immediately.
    #[arg(long)]
    pub force: bool,
}

pub async fn destroy_task(args: DestroyArgs, ctx: ExecutionMode) -> Result<()> {
    info!("Starting destroy command");
    println!("{}", contextual_heading("Destroying", &args.name, &[]));
    let steps = FixedSteps::new(&["Resolve deployment", "Resolve manager", "Delete resources"]);
    steps.activate(0, Some(format!("Deployment {}", args.name)));

    let platform_name = args.platform.clone().ok_or_else(|| {
        AlienError::new(ErrorData::ValidationError {
            field: "platform".to_string(),
            message: "--platform is required".to_string(),
        })
    })?;
    let platform = Platform::from_str(&platform_name).map_err(|e| {
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
        .resolve_manager(&tracked_deployment.project_id, &platform_name)
        .await?;

    steps.complete(1, Some(format!("Manager: {}", manager_ctx.manager_url)));

    destroy_tracked_deployment(&args, platform, &tracked_deployment, manager_ctx, steps).await
}

/// Delete a tracked deployment through its resolved manager.
async fn destroy_tracked_deployment(
    args: &DestroyArgs,
    platform: Platform,
    tracked_deployment: &TrackedDeployment,
    manager_ctx: ManagerContext,
    steps: FixedSteps,
) -> Result<()> {
    // Manager discovery may authenticate as the user, but teardown drives the
    // manager's sync endpoints, which only accept the deployment's own token.
    let manager_client = alien_manager_api::Client::new_with_client(
        &manager_ctx.manager_url,
        deployment_manager_http_client(
            &tracked_deployment.api_key,
            manager_ctx.workspace.as_deref(),
        )?,
    );
    let operator_client = manager_ctx.client;

    // Step 3: Delete via manager
    steps.activate(2, Some(tracked_deployment.deployment_id.clone()));

    if args.force {
        operator_client
            .delete_deployment()
            .id(&tracked_deployment.deployment_id)
            .body(alien_manager_api::types::DeleteDeploymentRequest {
                action: alien_manager_api::types::DeleteDeploymentAction::Forget,
            })
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to force-delete deployment record".to_string(),
            })?;

        steps.complete(2, Some("Force-deleted".to_string()));
        drop(steps);
        eprintln!();
        println!("{}", success_line("Deployment force-deleted."));
        return Ok(());
    }

    let pre_delete_deployment = manager_client
        .get_deployment()
        .id(&tracked_deployment.deployment_id)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    if matches!(
        pre_delete_deployment.status.as_str(),
        "teardown-required" | "teardown-failed"
    ) {
        steps.activate(2, Some("Setup teardown required".to_string()));
    } else {
        // Request deletion
        operator_client
            .delete_deployment()
            .id(&tracked_deployment.deployment_id)
            .body(alien_manager_api::types::DeleteDeploymentRequest {
                action: alien_manager_api::types::DeleteDeploymentAction::Cleanup,
            })
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to request deployment deletion".to_string(),
            })?;
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
        error: None,
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

    // Settling an adopted resource's deletion needs its binding, and the manager keeps
    // bindings in stack settings rather than on the deployment config the executor reads.
    if let Some(external_bindings) = stack_settings.external_bindings.clone() {
        config.external_bindings = external_bindings;
    }

    // Acquire → step loop → reconcile → release
    let session = format!("cli-destroy-{}", Uuid::new_v4());
    let acquire_outcome = acquire_setup_delete_deployment(
        &manager_client,
        &tracked_deployment.deployment_id,
        &session,
        stack_settings.deployment_model,
    )
    .await
    .context(ErrorData::ConfigurationError {
        message: "Failed to acquire deployment lock for deletion".to_string(),
    })?;
    let execution_claim = match acquire_outcome {
        SetupDeleteAcquireOutcome::Acquired { execution_claim } => execution_claim,
        SetupDeleteAcquireOutcome::AlreadyDeleted => return Ok(()),
    };

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

    let transport = ManagerApiTransport::with_execution_claim(
        manager_client.clone(),
        session.clone(),
        execution_claim.clone(),
    );
    let policy = RunnerPolicy {
        max_steps: 400,
        operation: LoopOperation::Delete,
        delay_strategy: alien_deployment::runner::DelayStrategy::Inline,
    };

    let runner_result = match alien_deployment::runner::run_step_loop(
        &mut current,
        &mut config,
        &client_config,
        &tracked_deployment.deployment_id,
        &policy,
        &transport,
        None,
        None,
    )
    .await
    {
        Ok(result)
            if result.loop_result.outcome == LoopOutcome::Success
                && current.status == DeploymentStatus::TeardownRequired =>
        {
            alien_deployment::setup_teardown::run_setup_teardown_after_handoff(
                &mut current,
                &mut config,
                &client_config,
                &tracked_deployment.deployment_id,
                &policy,
                &transport,
                None,
            )
            .await
            .map(|setup_result| setup_result.unwrap_or(result))
        }
        other => other,
    };
    let semantic_failure_status = runner_result.as_ref().ok().and_then(|result| {
        (result.loop_result.outcome == LoopOutcome::Failure)
            .then(|| result.loop_result.final_status.clone())
    });

    // Always reconcile + release
    let runner_result = combine_operation_and_finalization(
        preserve_semantic_failure(runner_result, &current),
        final_reconcile(
            &manager_client,
            &tracked_deployment.deployment_id,
            &session,
            execution_claim.as_ref(),
            &current,
        )
        .await,
    );

    if let Some(status) = semantic_failure_status {
        steps.fail(2, Some(format!("{status:?}")));
    }

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
            let failed = ErrorData::DeploymentFailed {
                message: format!("deletion failed at status {:?}", loop_result.final_status),
            };
            // The final state's headline error names each failed resource and its cause.
            return Err(
                match alien_deployment::deployment_headline_error_from_state(&current) {
                    Some(cause) => cause.context(failed),
                    None => AlienError::new(failed),
                },
            );
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::State,
        http::{HeaderMap, StatusCode},
        response::{IntoResponse, Response},
        routing::{get, post},
        Json, Router,
    };
    use std::sync::{Arc, Mutex};

    const DEPLOYMENT_TOKEN: &str = "deployment-secret";

    #[derive(Default)]
    struct ManagerState {
        acquire_authorizations: Vec<String>,
        deleted: bool,
    }

    type Shared = Arc<Mutex<ManagerState>>;

    async fn get_deployment(State(state): State<Shared>) -> Response {
        if state.lock().unwrap().deleted {
            return StatusCode::NOT_FOUND.into_response();
        }
        Json(serde_json::json!({
            "id": "dep_test",
            "name": "test",
            "platform": "test",
            "status": "teardown-required",
            "deploymentGroupId": "dg_test",
            "deploymentProtocolVersion": 1,
            "projectId": "proj_test",
            "workspaceId": "ws_test",
            "retryRequested": false,
            "createdAt": "2026-10-05T00:00:00Z"
        }))
        .into_response()
    }

    /// Mirrors the platform: sync acquire only accepts deployment-scoped tokens.
    /// A granted acquire finds nothing left to tear down, so the deployment is gone.
    async fn acquire(State(state): State<Shared>, headers: HeaderMap) -> Response {
        let authorization = headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let mut state = state.lock().unwrap();
        state.acquire_authorizations.push(authorization.clone());
        if authorization != format!("Bearer {DEPLOYMENT_TOKEN}") {
            return StatusCode::FORBIDDEN.into_response();
        }
        state.deleted = true;
        Json(serde_json::json!({ "deployments": [] })).into_response()
    }

    #[tokio::test]
    async fn teardown_required_destroy_acquires_with_the_deployment_token() {
        let state = Shared::default();
        let app = Router::new()
            .route("/v1/deployments/{id}", get(get_deployment))
            .route("/v1/sync/acquire", post(acquire))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind manager");
        let manager_url = format!("http://{}", listener.local_addr().expect("manager address"));
        tokio::spawn(async move { axum::serve(listener, app).await.expect("serve manager") });

        // Discovery authenticated as the user, as `alien destroy` does with a CLI login.
        let user_http_client =
            crate::auth::client_with_auth_and_workspace("Bearer user-session", "ws-name")
                .expect("user client");
        let manager_ctx = ManagerContext {
            manager_url: manager_url.clone(),
            manager_name: None,
            manager_is_system: None,
            manager_cloud: None,
            client: alien_manager_api::Client::new_with_client(
                &manager_url,
                user_http_client.clone(),
            ),
            http_client: user_http_client,
            auth_token: Some("user-session".to_string()),
            repository_name: None,
            repository_uri: None,
            workspace: Some("ws-name".to_string()),
        };
        let tracked = TrackedDeployment {
            name: "test".to_string(),
            deployment_id: "dep_test".to_string(),
            api_key: DEPLOYMENT_TOKEN.to_string(),
            workspace_id: "ws_test".to_string(),
            project_id: "proj_test".to_string(),
        };
        let args = DestroyArgs {
            token: None,
            name: "test".to_string(),
            platform: Some("test".to_string()),
            force: false,
        };

        destroy_tracked_deployment(
            &args,
            Platform::Test,
            &tracked,
            manager_ctx,
            FixedSteps::new(&["Resolve deployment", "Resolve manager", "Delete resources"]),
        )
        .await
        .expect("destroy should finish once the deployment is gone");

        assert_eq!(
            state.lock().unwrap().acquire_authorizations,
            vec![format!("Bearer {DEPLOYMENT_TOKEN}")]
        );
    }
}
