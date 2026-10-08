//! Shared transport for callers that reconcile deployment state via the Manager API.
//!
//! Used by every "external" deployment loop caller:
//!
//! | Caller           | How it gets the manager client                     |
//! |------------------|----------------------------------------------------|
//! | alien-deploy-cli | `--manager-url` / embedded config / tracker        |
//! | alien-cli        | `resolve_manager()` (discovers URL via platform)   |
//! | alien-terraform  | `manager_url` from SyncAcquire response            |

use alien_core::{DeploymentModel, DeploymentState, ObservedInventoryBatch, ResourceHeartbeat};
use alien_error::{AlienError, AlienErrorData, Context, ContextError, IntoAlienError};
use alien_manager_api::{Client as ManagerClient, SdkResultExt, SdkResultExtReadingBody as _};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{
    error::ErrorData,
    loop_contract::LoopStopReason,
    transport::{DeploymentLoopTransport, StepReconcileResult},
};

/// Transport that reconciles deployment state via the Manager API after each step.
///
/// Each `reconcile_step` call:
/// 1. POSTs the current state to `/v1/sync/reconcile` so the manager persists it
///    and runs server-side side-effects (e.g. cross-account registry access).
/// 2. Re-fetches the deployment to pick up any changes the manager made (e.g.
///    server-side mutations applied during reconciliation).
/// 3. Returns updated state if the manager injected server-side mutations.
pub struct ManagerApiTransport {
    client: ManagerClient,
    session: String,
    execution_claim: Option<ExecutionClaim>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionClaim {
    pub operation_id: String,
    pub attempt_id: String,
}

#[derive(Debug, Clone)]
pub struct AcquiredDeploymentPayload {
    pub deployment: serde_json::Value,
    pub execution_claim: Option<ExecutionClaim>,
}

impl ManagerApiTransport {
    pub fn new(client: ManagerClient, session: String) -> Self {
        Self {
            client,
            session,
            execution_claim: None,
        }
    }

    pub fn with_execution_claim(
        client: ManagerClient,
        session: String,
        execution_claim: Option<ExecutionClaim>,
    ) -> Self {
        Self {
            client,
            session,
            execution_claim,
        }
    }
}

#[async_trait]
impl DeploymentLoopTransport for ManagerApiTransport {
    async fn renew_lease(&self, deployment_id: &str) -> Result<(), AlienError> {
        self.client
            .renew()
            .body(alien_manager_api::types::RenewRequest {
                deployment_id: deployment_id.to_string(),
                execution_claim: self
                    .execution_claim
                    .as_ref()
                    .map(to_manager_api_execution_claim)
                    .transpose()?,
                session: self.session.clone(),
            })
            .send()
            .await
            .into_sdk_error()
            .context(alien_error::GenericError {
                message: "Failed to renew deployment lease via manager API".to_string(),
            })?;
        Ok(())
    }

    async fn reconcile_step(
        &self,
        deployment_id: &str,
        state: &DeploymentState,
        config: &alien_core::DeploymentConfig,
        update_heartbeat: bool,
        suggested_delay_ms: Option<u64>,
        heartbeats: Vec<ResourceHeartbeat>,
        observed_inventory_batches: Vec<ObservedInventoryBatch>,
    ) -> Result<StepReconcileResult, AlienError> {
        let state_json =
            serde_json::to_value(state)
                .into_alien_error()
                .context(alien_error::GenericError {
                    message: "Failed to serialize state for reconcile".to_string(),
                })?;

        let suggested_delay_ms = suggested_delay_ms
            .map(i64::try_from)
            .transpose()
            .into_alien_error()
            .context(alien_error::GenericError {
                message: "suggested_delay_ms exceeded manager API integer range".to_string(),
            })?;
        let heartbeats = to_manager_api_heartbeats(heartbeats)?;
        let observed_inventory_batches =
            to_manager_api_observed_inventory_batches(observed_inventory_batches)?;

        #[cfg(feature = "openapi")]
        let body = alien_manager_api::types::ReconcileRequest {
            deployment_id: deployment_id.to_string(),
            session: self.session.clone(),
            state: state_json,
            update_heartbeat: Some(update_heartbeat),
            suggested_delay_ms,
            resource_heartbeats: heartbeats,
            observed_inventory_batches,
            capabilities: Vec::new(),
            execution_claim: self
                .execution_claim
                .as_ref()
                .map(to_manager_api_execution_claim)
                .transpose()?,
            operator_version: None,
        };
        #[cfg(not(feature = "openapi"))]
        let body = alien_manager_api::types::ReconcileRequest {
            deployment_id: deployment_id.to_string(),
            session: self.session.clone(),
            state: state_json,
            update_heartbeat: Some(update_heartbeat),
            suggested_delay_ms,
            resource_heartbeats: heartbeats,
            observed_inventory_batches,
            capabilities: Vec::new(),
            execution_claim: self
                .execution_claim
                .as_ref()
                .map(to_manager_api_execution_claim)
                .transpose()?,
            operator_version: None,
        };

        // POST state to the manager
        let resp = self
            .client
            .reconcile()
            .body(body)
            .send()
            .await
            // Reads the body so a structured manager error keeps its own retryable flag.
            .into_sdk_error_reading_body()
            .await
            // Inherits retryable: the runner retries a checkpoint only on a retryable error,
            // and a network error reaching the manager must not fail the deployment.
            .context(crate::ErrorData::ManagerRequestFailed {
                message: "reconcile step".to_string(),
            })
            .map_err(AlienError::into_generic)?
            .into_inner();

        // If the manager returned a native_image_host, inject it into the config
        // so Lambda/Cloud Run controllers resolve proxy URIs to native ECR/GAR URIs.
        let config_update = resp.native_image_host.and_then(|host| {
            if config.native_image_host.as_deref() == Some(&host) {
                None
            } else {
                let mut updated = config.clone();
                updated.native_image_host = Some(host);
                Some(updated)
            }
        });

        // Parse the server-returned state to pick up server-side mutations
        // (e.g. registry_access_granted flag set by reconcile_registry_access).
        // Without this, the client keeps sending stale state that overwrites
        // the server's updates on subsequent reconcile calls.
        let state_update = serde_json::from_value::<DeploymentState>(resp.current)
            .ok()
            .filter(|updated| deployment_state_changed(updated, state));

        Ok(StepReconcileResult {
            state: state_update,
            config: config_update,
        })
    }
}

fn deployment_state_changed(updated: &DeploymentState, current: &DeploymentState) -> bool {
    updated.status != current.status
        || updated.platform != current.platform
        || updated.current_release != current.current_release
        || updated.target_release != current.target_release
        || serialized_values_differ(&updated.stack_state, &current.stack_state)
        || updated.error != current.error
        || updated.environment_info != current.environment_info
        || updated.runtime_metadata != current.runtime_metadata
        || updated.retry_requested != current.retry_requested
        || updated.protocol_version != current.protocol_version
}

fn to_manager_api_execution_claim(
    claim: &ExecutionClaim,
) -> Result<alien_manager_api::types::ExecutionClaim, AlienError> {
    Ok(alien_manager_api::types::ExecutionClaim {
        attempt_id: claim.attempt_id.clone(),
        operation_id: claim.operation_id.clone(),
    })
}

fn serialized_values_differ<T: Serialize>(updated: &T, current: &T) -> bool {
    serde_json::to_value(updated).ok() != serde_json::to_value(current).ok()
}

fn to_manager_api_heartbeats(
    heartbeats: Vec<ResourceHeartbeat>,
) -> Result<Vec<alien_manager_api::types::ResourceHeartbeat>, AlienError> {
    heartbeats
        .into_iter()
        .map(|heartbeat| serde_json::to_value(heartbeat).and_then(serde_json::from_value))
        .collect::<Result<Vec<alien_manager_api::types::ResourceHeartbeat>, _>>()
        .into_alien_error()
        .context(alien_error::GenericError {
            message: "Failed to convert heartbeats for manager API".to_string(),
        })
}

fn to_manager_api_observed_inventory_batches(
    snapshots: Vec<ObservedInventoryBatch>,
) -> Result<Vec<alien_manager_api::types::ObservedInventoryBatch>, AlienError> {
    snapshots
        .into_iter()
        .map(|snapshot| serde_json::to_value(snapshot).and_then(serde_json::from_value))
        .collect::<Result<Vec<alien_manager_api::types::ObservedInventoryBatch>, _>>()
        .into_alien_error()
        .context(alien_error::GenericError {
            message: "Failed to convert observed inventory snapshots for manager API".to_string(),
        })
}

// ---------------------------------------------------------------------------
// Shared helpers for deployment acquisition and finalization.
//
// Every external caller (alien-deploy-cli, alien-cli, alien-terraform) follows
// the same protocol:
//   1. acquire_deployment()   — lock the deployment with a retry loop
//   2. run_step_loop()        — step until terminal (uses ManagerApiTransport)
//   3. finalize_step_loop()   — release checkpointed terminal state, otherwise
//                              persist final state and always attempt unlock
// ---------------------------------------------------------------------------

/// Maximum number of acquire attempts (60 × 2s = 2 minutes).
const MAX_ACQUIRE_ATTEMPTS: usize = 60;
/// Maximum number of setup delete handoff attempts (1,350 × 2s = 45 minutes).
const MAX_SETUP_DELETE_ACQUIRE_ATTEMPTS: usize = 1_350;
/// Delay between acquire attempts in seconds.
const ACQUIRE_RETRY_DELAY_SECS: u64 = 2;

/// Result of waiting for setup-owned deletion work.
pub enum SetupDeleteAcquireOutcome {
    /// The setup teardown lock was acquired and must be released.
    Acquired {
        execution_claim: Option<ExecutionClaim>,
    },
    /// Runtime cleanup already deleted the deployment record.
    AlreadyDeleted,
}

/// Preserve an operation failure while also reporting a finalization failure.
pub fn combine_operation_and_finalization<T, E>(
    operation_result: Result<T, AlienError<E>>,
    finalization_result: Result<(), AlienError>,
) -> Result<T, AlienError>
where
    E: AlienErrorData + Clone + std::fmt::Debug + Serialize,
{
    match (operation_result, finalization_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Ok(_), Err(finalization_error)) => Err(finalization_error),
        (Err(operation_error), Ok(())) => Err(operation_error.into_generic()),
        (Err(operation_error), Err(finalization_error)) => {
            let mut primary = operation_error.into_generic();
            // Treat finalization as a second error boundary. In particular, do not
            // embed its arbitrary context in a public primary error: internal
            // errors may carry provider responses or credentials there.
            let mut finalization = finalization_error.into_external();
            finalization.context = None;
            finalization.source = None;
            let finalization_summary = serde_json::json!({
                "code": finalization.code,
                "message": finalization.message,
                "retryable": finalization.retryable,
            });
            let mut context = primary
                .context
                .take()
                .and_then(|value| value.as_object().cloned())
                .unwrap_or_default();
            context.insert("finalizationError".to_string(), finalization_summary);
            primary.context = Some(serde_json::Value::Object(context));

            // Also retain the sanitized secondary failure in the source chain so
            // the normal human renderer shows it instead of silently hiding it in
            // structured context.
            let mut tail = &mut primary;
            while let Some(ref mut source) = tail.source {
                tail = source;
            }
            tail.source = Some(Box::new(finalization));
            Err(primary)
        }
    }
}

/// Acquire a deployment lock for CLI-owned runtime deletion.
///
/// Local/pull-model deployments do not have a manager-side runtime that can
/// delete host-local resources. The deploy CLI must first drive
/// `delete-pending` / `deleting` to the normal runtime cleanup handoff, and
/// only then acquire setup teardown if frozen setup resources remain.
pub async fn acquire_runtime_delete_deployment(
    client: &ManagerClient,
    deployment_id: &str,
    session: &str,
    deployment_model: DeploymentModel,
) -> Result<AcquiredDeploymentPayload, AlienError> {
    acquire_deployment_with_statuses(
        client,
        deployment_id,
        session,
        deployment_model,
        Some("runtime".to_string()),
        Some("cli".to_string()),
        Some(vec![
            "delete-pending".to_string(),
            "deleting".to_string(),
            "delete-failed".to_string(),
        ]),
    )
    .await
}

/// Acquire a deployment lock from the manager, retrying until the lock is granted
/// or the timeout is reached.
///
/// Returns `Ok(())` on success. The caller must call [`release_deployment`] when
/// done, even on error.
pub async fn acquire_deployment(
    client: &ManagerClient,
    deployment_id: &str,
    session: &str,
    deployment_model: DeploymentModel,
) -> Result<AcquiredDeploymentPayload, AlienError> {
    acquire_deployment_with_statuses(
        client,
        deployment_id,
        session,
        deployment_model,
        None,
        None,
        None,
    )
    .await
}

/// Acquire a deployment lock and return the manager's acquired deployment
/// payload. Callers that need deployment-config fields that are intentionally
/// redacted from `GET /v1/deployments` should use this helper.
pub async fn acquire_deployment_with_payload(
    client: &ManagerClient,
    deployment_id: &str,
    session: &str,
    deployment_model: DeploymentModel,
) -> Result<AcquiredDeploymentPayload, AlienError> {
    acquire_deployment_with_statuses(
        client,
        deployment_id,
        session,
        deployment_model,
        None,
        None,
        None,
    )
    .await
}

/// Acquire a deployment lock for CLI-owned setup.
///
/// Runtime managers intentionally skip these setup-owned states; the customer
/// CLI must drive them with the customer's local cloud credentials until the
/// deployment reaches the provisioning handoff.
pub async fn acquire_setup_run_deployment(
    client: &ManagerClient,
    deployment_id: &str,
    session: &str,
    deployment_model: DeploymentModel,
) -> Result<AcquiredDeploymentPayload, AlienError> {
    acquire_deployment_with_statuses(
        client,
        deployment_id,
        session,
        deployment_model,
        Some("setup-run".to_string()),
        Some("cli".to_string()),
        Some(setup_run_acquire_statuses()),
    )
    .await
}

fn setup_run_acquire_statuses() -> Vec<String> {
    [
        "pending",
        "preflights-failed",
        "initial-setup",
        "initial-setup-failed",
        "waiting-for-machines",
        "waiting-for-secrets",
        "running",
        "update-failed",
        "refresh-failed",
        "provisioning-failed",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

/// Acquire a deployment lock for a caller that owns setup-time teardown.
///
/// Unlike the normal manager acquire path, this can acquire `teardown-required`
/// so setup-authority callers can resume frozen-resource teardown.
pub async fn acquire_setup_delete_deployment(
    client: &ManagerClient,
    deployment_id: &str,
    session: &str,
    deployment_model: DeploymentModel,
) -> Result<SetupDeleteAcquireOutcome, AlienError> {
    let statuses = vec![
        "teardown-required".to_string(),
        "teardown-failed".to_string(),
    ];

    for attempt in 1..=MAX_SETUP_DELETE_ACQUIRE_ATTEMPTS {
        let response = client
            .acquire()
            .body(alien_manager_api::types::AcquireRequest {
                acquire_mode: Some("setup-teardown".to_string()),
                session: session.to_string(),
                deployment_ids: Some(vec![deployment_id.to_string()]),
                setup_method: Some("cli".to_string()),
                statuses: Some(statuses.clone()),
                platforms: None,
                deployment_model: deployment_model_wire(deployment_model),
                limit: None,
            })
            .send()
            .await
            .into_sdk_error();
        let resp = match response {
            Ok(response) => response,
            Err(error) => {
                // A runtime with setup authority can finish and remove the record
                // between the delete request and this acquire request.
                if is_missing_deployment_response(&error) {
                    let lookup = client
                        .get_deployment()
                        .id(deployment_id)
                        .send()
                        .await
                        .into_sdk_error();
                    if let Err(lookup_error) = lookup {
                        if is_missing_deployment_response(&lookup_error) {
                            return Ok(SetupDeleteAcquireOutcome::AlreadyDeleted);
                        }
                        return Err(lookup_error.context(alien_error::GenericError {
                            message: "Failed to confirm completed deployment deletion".to_string(),
                        }));
                    }
                }
                return Err(error.context(alien_error::GenericError {
                    message: "Failed to acquire setup teardown sync lock".to_string(),
                }));
            }
        };

        let response = resp.into_inner();
        if let Some(acquired) = response.deployments.into_iter().next() {
            return Ok(SetupDeleteAcquireOutcome::Acquired {
                execution_claim: acquired.execution_claim.map(|claim| ExecutionClaim {
                    operation_id: claim.operation_id,
                    attempt_id: claim.attempt_id,
                }),
            });
        }

        reject_non_retryable_acquire_reason(deployment_id, &response.not_acquired, true)?;

        let status = match client.get_deployment().id(deployment_id).send().await {
            Ok(resp) => resp.into_inner().status,
            Err(err) => {
                let message = err.to_string();
                if message.contains("404") || message.contains("not found") {
                    return Ok(SetupDeleteAcquireOutcome::AlreadyDeleted);
                }
                return Err(AlienError::new(alien_error::GenericError {
                    message: format!(
                        "Failed to read deployment while waiting for setup teardown: {message}"
                    ),
                }));
            }
        };

        match status.as_str() {
            "teardown-required" | "teardown-failed" => {}
            "deleted" => return Ok(SetupDeleteAcquireOutcome::AlreadyDeleted),
            "delete-failed" => {
                return Err(AlienError::new(alien_error::GenericError {
                    message:
                        "Runtime deletion failed before setup teardown became available. Retry destroy after resolving the runtime cleanup failure."
                            .to_string(),
                }));
            }
            _ => {}
        }

        if attempt == MAX_SETUP_DELETE_ACQUIRE_ATTEMPTS {
            return Err(AlienError::new(alien_error::GenericError {
                message: "Timed out waiting for runtime cleanup to reach setup teardown handoff"
                    .to_string(),
            }));
        }

        info!(
            attempt = attempt,
            max = MAX_SETUP_DELETE_ACQUIRE_ATTEMPTS,
            status = %status,
            "Waiting for runtime cleanup handoff before setup teardown"
        );
        tokio::time::sleep(std::time::Duration::from_secs(ACQUIRE_RETRY_DELAY_SECS)).await;
    }

    unreachable!()
}

async fn acquire_deployment_with_statuses(
    client: &ManagerClient,
    deployment_id: &str,
    session: &str,
    deployment_model: DeploymentModel,
    acquire_mode: Option<String>,
    setup_method: Option<String>,
    statuses: Option<Vec<String>>,
) -> Result<AcquiredDeploymentPayload, AlienError> {
    for attempt in 1..=MAX_ACQUIRE_ATTEMPTS {
        let resp = client
            .acquire()
            .body(alien_manager_api::types::AcquireRequest {
                acquire_mode: acquire_mode.clone(),
                session: session.to_string(),
                deployment_ids: Some(vec![deployment_id.to_string()]),
                setup_method: setup_method.clone(),
                statuses: statuses.clone(),
                platforms: None,
                deployment_model: deployment_model_wire(deployment_model),
                limit: None,
            })
            .send()
            .await
            .into_sdk_error()
            .context(alien_error::GenericError {
                message: "Failed to acquire sync lock".to_string(),
            })?;

        let response = resp.into_inner();
        if let Some(acquired) = response.deployments.into_iter().next() {
            return Ok(AcquiredDeploymentPayload {
                deployment: acquired.deployment,
                execution_claim: acquired.execution_claim.map(|claim| ExecutionClaim {
                    operation_id: claim.operation_id,
                    attempt_id: claim.attempt_id,
                }),
            });
        }

        reject_non_retryable_acquire_reason(deployment_id, &response.not_acquired, false)?;

        if attempt == MAX_ACQUIRE_ATTEMPTS {
            return Err(AlienError::new(alien_error::GenericError {
                message: acquisition_timeout_message(deployment_id, &response.not_acquired),
            }));
        }

        info!(
            attempt = attempt,
            max = MAX_ACQUIRE_ATTEMPTS,
            "Waiting for deployment lock"
        );
        tokio::time::sleep(std::time::Duration::from_secs(ACQUIRE_RETRY_DELAY_SECS)).await;
    }

    unreachable!()
}

fn acquisition_timeout_message(
    deployment_id: &str,
    unavailable: &[alien_manager_api::types::UnacquiredDeployment],
) -> String {
    use alien_manager_api::types::DeploymentAcquireUnavailableReason as Reason;
    match unavailable.iter().find(|outcome| outcome.deployment_id == deployment_id) {
        Some(outcome) if outcome.reason == Reason::Contended =>
            "Timed out waiting for deployment lock: another operation still holds the deployment lease. Check the current deployment operation before retrying; this command did not acquire the lease.".to_string(),
        Some(outcome) if outcome.reason == Reason::Deferred =>
            "Timed out waiting for deployment lock: the manager deferred this deployment. Check its current operation and status before retrying; this command did not acquire the lease.".to_string(),
        _ => "Timed out waiting for deployment lock; the manager did not provide an acquisition reason. Check the current deployment operation before retrying.".to_string(),
    }
}

fn reject_non_retryable_acquire_reason(
    deployment_id: &str,
    unavailable: &[alien_manager_api::types::UnacquiredDeployment],
    allow_status_transition: bool,
) -> Result<(), AlienError> {
    let Some(outcome) = unavailable
        .iter()
        .find(|outcome| outcome.deployment_id == deployment_id)
    else {
        // Older managers do not return acquisition reasons. Preserve the
        // bounded retry behavior for compatibility with those servers.
        return Ok(());
    };

    use alien_manager_api::types::DeploymentAcquireUnavailableReason as Reason;
    match outcome.reason {
        Reason::Contended | Reason::Deferred => Ok(()),
        Reason::StatusMismatch if allow_status_transition => Ok(()),
        reason => Err(AlienError::new(ErrorData::DeploymentAcquireUnavailable {
            deployment_id: deployment_id.to_string(),
            reason: reason.to_string(),
        })
        .into_generic()),
    }
}

/// Finalize an unmodified step-loop result and release its manager lease.
///
/// A terminal `Ok` result whose state the manager recorded (the acquired state
/// or an accepted checkpoint) only releases the claim: repeating that
/// checkpoint can address a claim the server has already closed. Errors,
/// nonterminal stops and terminal states the manager has not confirmed still
/// persist final state.
/// Callers that mutate state after the loop must use `final_reconcile` instead.
pub async fn finalize_step_loop(
    client: &ManagerClient,
    deployment_id: &str,
    session: &str,
    execution_claim: Option<&ExecutionClaim>,
    state: &DeploymentState,
    result: crate::Result<crate::runner::RunnerResult>,
) -> Result<crate::runner::RunnerResult, AlienError> {
    let checkpointed_terminal = result.as_ref().is_ok_and(|result| {
        result.state_persisted
            && (result.loop_result.final_status.is_failed()
                || matches!(
                result.loop_result.stop_reason,
                LoopStopReason::Synced
                    | LoopStopReason::Failed
                    | LoopStopReason::Deleted
                    | LoopStopReason::Handoff
                ))
    });
    let finalized = if checkpointed_terminal {
        release_deployment(client, deployment_id, session, execution_claim).await
    } else {
        final_reconcile(client, deployment_id, session, execution_claim, state).await
    };
    combine_operation_and_finalization(
        crate::runner::preserve_semantic_failure(result, state),
        finalized,
    )
}

/// Persist the final deployment state and release its manager lease.
///
/// Lease release is attempted even when serialization or reconciliation fails.
/// A missing deployment is successful only for a completed deletion.
pub async fn final_reconcile(
    client: &ManagerClient,
    deployment_id: &str,
    session: &str,
    execution_claim: Option<&ExecutionClaim>,
    state: &DeploymentState,
) -> Result<(), AlienError> {
    let reconcile_result = async {
        let state_json =
            serde_json::to_value(state)
                .into_alien_error()
                .context(alien_error::GenericError {
                    message: "Failed to serialize final deployment state".to_string(),
                })?;
        client
            .reconcile()
            .body(alien_manager_api::types::ReconcileRequest {
                deployment_id: deployment_id.to_string(),
                session: session.to_string(),
                state: state_json,
                update_heartbeat: Some(false),
                suggested_delay_ms: None,
                resource_heartbeats: vec![],
                observed_inventory_batches: vec![],
                capabilities: Vec::new(),
                execution_claim: execution_claim
                    .map(to_manager_api_execution_claim)
                    .transpose()?,
                operator_version: None,
            })
            .send()
            .await
            .into_sdk_error_reading_body()
            .await
            .map(|_| ())
    }
    .await;

    let reconcile_result = match reconcile_result {
        Err(error)
            if state.status == alien_core::DeploymentStatus::Deleted
                && is_missing_deployment_response(&error) =>
        {
            info!(
                deployment_id = %deployment_id,
                "Deployment was removed before final deletion reconciliation"
            );
            Ok(())
        }
        result => result,
    };

    let release_result = release_deployment(client, deployment_id, session, execution_claim).await;
    match (reconcile_result, release_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(reconcile_error), Err(release_error)) => {
            Err(reconcile_error.context(alien_error::GenericError {
                message: format!(
                    "Final deployment reconciliation failed and lease release also failed: {release_error}"
                ),
            }))
        }
    }
}

fn deployment_model_wire(model: DeploymentModel) -> alien_manager_api::types::DeploymentModel {
    match model {
        DeploymentModel::Push => alien_manager_api::types::DeploymentModel::Push,
        DeploymentModel::Pull => alien_manager_api::types::DeploymentModel::Pull,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        loop_contract::{LoopOutcome, LoopResult},
        runner::RunnerResult,
    };
    use alien_core::{
        ContainerHeartbeatData, GcpAgentPlatformSandboxHeartbeatData, HeartbeatBackend,
        HeartbeatCollectionIssue, HeartbeatCollectionIssueReason, HeartbeatIssueSeverity,
        KubernetesContainerHeartbeatData, KubernetesWorkloadKind, ObservedHealth, Platform,
        ProviderLifecycleState, ResourceHeartbeatData, ResourceType, SandboxHeartbeatData,
        SandboxHeartbeatStatus, WorkloadHeartbeatStatus, WorkloadReplicaStatus,
    };
    use chrono::TimeZone;
    use httpmock::prelude::*;

    #[tokio::test]
    async fn setup_delete_acquire_confirms_a_concurrently_removed_record() {
        let server = MockServer::start_async().await;
        let acquire = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/sync/acquire");
                then.status(404).body("removed deployment");
            })
            .await;
        let lookup = server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/deployments/deployment-1");
                then.status(404).body("removed deployment");
            })
            .await;
        let result = acquire_setup_delete_deployment(
            &ManagerClient::new(&server.base_url()),
            "deployment-1",
            "session-1",
            DeploymentModel::Push,
        )
        .await
        .expect("confirmed removed record is completed deletion");
        assert!(matches!(result, SetupDeleteAcquireOutcome::AlreadyDeleted));
        acquire.assert_async().await;
        lookup.assert_async().await;
    }

    #[tokio::test]
    async fn setup_delete_acquire_does_not_hide_a_failed_completion_lookup() {
        let server = MockServer::start_async().await;
        let acquire = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/sync/acquire");
                then.status(404).body("missing");
            })
            .await;
        let lookup = server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/deployments/deployment-1");
                then.status(500).body("backend unavailable");
            })
            .await;
        let error = acquire_setup_delete_deployment(
            &ManagerClient::new(&server.base_url()),
            "deployment-1",
            "session-1",
            DeploymentModel::Push,
        )
        .await
        .err()
        .expect("lookup failure must not report deleted");
        assert_eq!(error.http_status_code, Some(500));
        acquire.assert_async().await;
        lookup.assert_async().await;
    }

    #[test]
    fn only_not_found_means_deleted_cleanup_is_already_complete() {
        let mut error = AlienError::new(alien_error::GenericError {
            message: "test".to_string(),
        });
        error.http_status_code = Some(404);
        assert!(is_missing_deployment_response(&error));

        error.http_status_code = Some(409);
        assert!(!is_missing_deployment_response(&error));
    }

    #[test]
    fn operation_error_is_retained_when_finalization_also_fails() {
        let operation_error = AlienError::new(alien_error::GenericError {
            message: "runner failed".to_string(),
        });
        let finalization_error = AlienError::new(alien_error::GenericError {
            message: "reconcile failed".to_string(),
        });

        let error = combine_operation_and_finalization::<(), _>(
            Err(operation_error),
            Err(finalization_error),
        )
        .expect_err("both failures must be reported");

        assert_eq!(error.message, "runner failed");
        assert_eq!(
            error
                .context
                .as_ref()
                .and_then(|context| context.get("finalizationError"))
                .and_then(|error| error.get("message"))
                .and_then(serde_json::Value::as_str),
            Some("reconcile failed")
        );
        assert!(error
            .human_report()
            .causes
            .iter()
            .any(|cause| cause.message == "reconcile failed"));
    }

    #[test]
    fn internal_finalization_details_are_not_embedded_in_external_error() {
        let operation_error = AlienError::new(alien_error::GenericError {
            message: "runner failed".to_string(),
        });
        let mut finalization_error = AlienError::new(alien_error::GenericError {
            message: "provider response contained secret-token".to_string(),
        });
        finalization_error.internal = true;
        finalization_error.context = Some(serde_json::json!({ "credential": "secret-token" }));

        let error = combine_operation_and_finalization::<(), _>(
            Err(operation_error),
            Err(finalization_error),
        )
        .expect_err("both failures must be reported");
        let serialized = serde_json::to_string(&error).expect("serialize combined error");

        assert!(!serialized.contains("secret-token"));
        assert!(serialized.contains("Internal server error"));
        assert!(error
            .human_report()
            .causes
            .iter()
            .any(|cause| cause.message == "Internal server error"));
    }

    fn running_state() -> DeploymentState {
        DeploymentState {
            status: alien_core::DeploymentStatus::Running,
            platform: alien_core::Platform::Aws,
            current_release: None,
            target_release: None,
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        }
    }

    fn deployment_config() -> alien_core::DeploymentConfig {
        alien_core::DeploymentConfig::builder()
            .stack_settings(alien_core::StackSettings::default())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(alien_core::ExternalBindings::default())
            .allow_frozen_changes(false)
            .build()
    }

    async fn reconcile_against(base_url: &str) -> AlienError {
        let transport = ManagerApiTransport::new(ManagerClient::new(base_url), "session-1".into());
        match transport
            .reconcile_step(
                "deployment-1",
                &running_state(),
                &deployment_config(),
                false,
                None,
                vec![],
                vec![],
            )
            .await
        {
            Ok(_) => panic!("the checkpoint must fail"),
            Err(error) => error,
        }
    }

    /// The runner retries a checkpoint only when its error is retryable, so an unreachable
    /// manager must come back retryable rather than fail the deployment on one network blip.
    #[tokio::test]
    async fn an_unreachable_manager_fails_the_checkpoint_as_retryable() {
        // Bound and released: nothing listens, so the request is refused.
        let address = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|listener| listener.local_addr())
            .expect("reserve a loopback port");

        let error = reconcile_against(&format!("http://{address}")).await;

        assert!(error.retryable, "{error:?}");
    }

    #[tokio::test]
    async fn a_checkpoint_the_manager_rejects_is_not_retried() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/sync/reconcile");
                // A 5xx status would be retryable by itself: the manager's own flag decides.
                then.status(500).json_body(serde_json::json!({
                    "code": "DEPLOYMENT_LEASE_LOST",
                    "message": "another session holds the lease",
                    "retryable": false,
                    "internal": false,
                }));
            })
            .await;

        let error = reconcile_against(&server.base_url()).await;

        assert!(!error.retryable, "{error:?}");
        assert_eq!(error.http_status_code, Some(500));
    }

    fn terminal_result(state: &DeploymentState) -> crate::Result<RunnerResult> {
        Ok(RunnerResult {
            loop_result: crate::loop_contract::classify_status(
                &state.status,
                crate::loop_contract::LoopOperation::Deploy,
            )
            .expect("test state must be terminal"),
            steps_executed: 1,
            state_persisted: true,
        })
    }

    fn failed_resource_state() -> DeploymentState {
        let mut state = running_state();
        state.status = alien_core::DeploymentStatus::InitialSetupFailed;
        let worker = alien_core::Worker::new("worker".to_string())
            .code(alien_core::WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("default".to_string())
            .build();
        let resource = alien_core::Resource::new(worker);
        let mut stack = alien_core::StackState::new(Platform::Test);
        stack.resources.insert(
            "worker".to_string(),
            alien_core::StackResourceState {
                resource_type: resource.resource_type().as_ref().to_string(),
                config: resource,
                status: alien_core::ResourceStatus::ProvisionFailed,
                error: Some(AlienError::new(alien_error::GenericError {
                    message: "execution profile is missing".to_string(),
                })),
                lifecycle: Some(alien_core::ResourceLifecycle::Frozen),
                internal_state: None,
                outputs: None,
                previous_config: None,
                retry_attempt: 0,
                controller_platform: None,
                dependencies: vec![],
                last_failed_state: None,
                remote_binding_params: None,
            },
        );
        state.stack_state = Some(stack);
        state
    }

    #[tokio::test]
    async fn actual_setup_loop_checkpoints_handoff_once_before_release() {
        let server = MockServer::start_async().await;
        let mut state = running_state();
        state.status = alien_core::DeploymentStatus::InitialSetup;
        state.platform = Platform::Test;
        let stack = alien_core::Stack::new("test".to_string()).build();
        state.target_release = Some(alien_core::ReleaseInfo {
            release_id: Some("release-1".to_string()),
            version: None,
            description: None,
            stack: stack.clone(),
        });
        state.stack_state = Some(alien_core::StackState::new(Platform::Test));
        state.runtime_metadata = Some(alien_core::RuntimeMetadata {
            prepared_stack: Some(stack),
            initial_setup_authority: alien_core::InitialSetupAuthority::DirectSetup,
            ..Default::default()
        });
        let mut checkpointed = state.clone();
        checkpointed.status = alien_core::DeploymentStatus::Provisioning;
        let reconcile = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/sync/reconcile")
                    .json_body_partial(r#"{"state":{"status":"provisioning"}}"#);
                then.status(200)
                    .json_body(serde_json::json!({"success":true,"current":checkpointed}));
            })
            .await;
        let release = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/sync/release");
                then.status(200);
            })
            .await;
        let client = ManagerClient::new(&server.base_url());
        let mut config = deployment_config();
        let claim = ExecutionClaim {
            operation_id: "operation-1".to_string(),
            attempt_id: "attempt-1".to_string(),
        };
        let transport = ManagerApiTransport::with_execution_claim(
            client.clone(),
            "session-1".to_string(),
            Some(claim.clone()),
        );
        let result = crate::runner::run_step_loop(
            &mut state,
            &mut config,
            &alien_core::ClientConfig::Test,
            "deployment-1",
            &crate::runner::RunnerPolicy {
                max_steps: 2,
                operation: crate::loop_contract::LoopOperation::InitialSetup,
                delay_strategy: crate::runner::DelayStrategy::Inline,
            },
            &transport,
            None,
            None,
        )
        .await;
        let result = finalize_step_loop(
            &client,
            "deployment-1",
            "session-1",
            Some(&claim),
            &state,
            result,
        )
        .await
        .expect("checkpointed setup handoff should release");
        assert_eq!(result.steps_executed, 1);
        assert_eq!(result.loop_result.stop_reason, LoopStopReason::Handoff);
        reconcile.assert_hits_async(1).await;
        release.assert_hits_async(1).await;
    }

    #[tokio::test]
    async fn acquired_terminal_state_releases_without_a_local_step() {
        let server = MockServer::start_async().await;
        let reconcile = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/sync/reconcile");
                then.status(409);
            })
            .await;
        let release = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/sync/release");
                then.status(200);
            })
            .await;
        let client = ManagerClient::new(&server.base_url());
        let mut state = failed_resource_state();
        state.target_release = Some(alien_core::ReleaseInfo {
            release_id: Some("release-1".to_string()),
            version: None,
            description: None,
            stack: alien_core::Stack::new("test".to_string()).build(),
        });
        let result = crate::runner::run_step_loop(
            &mut state,
            &mut deployment_config(),
            &alien_core::ClientConfig::Test,
            "deployment-1",
            &crate::runner::RunnerPolicy {
                max_steps: 2,
                operation: crate::loop_contract::LoopOperation::InitialSetup,
                delay_strategy: crate::runner::DelayStrategy::Inline,
            },
            &ManagerApiTransport::new(client.clone(), "session-1".to_string()),
            None,
            None,
        )
        .await;
        assert_eq!(
            result
                .as_ref()
                .expect("terminal status should stop before stepping")
                .steps_executed,
            0
        );
        let error = finalize_step_loop(&client, "deployment-1", "session-1", None, &state, result)
            .await
            .expect_err("original controller error must survive release");
        assert_eq!(error.code, "DEPLOYMENT_FAILED");
        assert!(serde_json::to_string(&error)
            .unwrap()
            .contains("execution profile is missing"));
        reconcile.assert_hits_async(0).await;
        release.assert_hits_async(1).await;
    }

    #[tokio::test]
    async fn terminal_checkpoint_is_followed_only_by_exact_claim_release() {
        for failed in [false, true] {
            let server = MockServer::start_async().await;
            let state = if failed {
                failed_resource_state()
            } else {
                running_state()
            };
            let claim = ExecutionClaim {
                operation_id: "operation-1".to_string(),
                attempt_id: "attempt-1".to_string(),
            };
            let reconcile = server.mock_async(|when, then| {
                when.method(POST).path("/v1/sync/reconcile")
                    .json_body_partial(r#"{"executionClaim":{"operationId":"operation-1","attemptId":"attempt-1"}}"#);
                then.status(200).json_body(serde_json::json!({"success":true,"current":state}));
            }).await;
            let release = server.mock_async(|when, then| {
                when.method(POST).path("/v1/sync/release")
                    .json_body(serde_json::json!({"deploymentId":"deployment-1","session":"session-1","executionClaim":claim}));
                then.status(200);
            }).await;
            let client = ManagerClient::new(&server.base_url());
            let transport = ManagerApiTransport::with_execution_claim(
                client.clone(),
                "session-1".to_string(),
                Some(claim.clone()),
            );
            let config = deployment_config();
            transport
                .reconcile_step("deployment-1", &state, &config, false, None, vec![], vec![])
                .await
                .expect("terminal checkpoint should persist");
            let result = finalize_step_loop(
                &client,
                "deployment-1",
                "session-1",
                Some(&claim),
                &state,
                terminal_result(&state),
            )
            .await;
            if failed {
                let error = result.expect_err("controller failure must remain a failure");
                assert_eq!(error.code, "DEPLOYMENT_FAILED");
                assert!(serde_json::to_string(&error)
                    .unwrap()
                    .contains("execution profile is missing"));
            } else {
                assert_eq!(
                    result
                        .expect("successful terminal result should release")
                        .loop_result
                        .outcome,
                    LoopOutcome::Success
                );
            }
            reconcile.assert_hits_async(1).await;
            release.assert_hits_async(1).await;
        }
    }

    #[tokio::test]
    async fn uncheckpointed_errors_and_nonterminal_stops_persist_then_release() {
        for stop in [
            None,
            Some(LoopStopReason::Delayed),
            Some(LoopStopReason::BudgetExceeded),
        ] {
            let server = MockServer::start_async().await;
            let state = running_state();
            let reconcile = server
                .mock_async(|when, then| {
                    when.method(POST)
                        .path("/v1/sync/reconcile")
                        .json_body_partial(r#"{"state":{"status":"running"}}"#);
                    then.status(200)
                        .json_body(serde_json::json!({"success":true,"current":state}));
                })
                .await;
            let release = server
                .mock_async(|when, then| {
                    when.method(POST).path("/v1/sync/release");
                    then.status(200);
                })
                .await;
            let result = match stop.clone() {
                Some(stop_reason) => Ok(RunnerResult {
                    loop_result: LoopResult {
                        outcome: if stop_reason == LoopStopReason::BudgetExceeded {
                            LoopOutcome::Failure
                        } else {
                            LoopOutcome::Neutral
                        },
                        stop_reason,
                        final_status: state.status,
                    },
                    steps_executed: 1,
                    state_persisted: true,
                }),
                None => Err(AlienError::new(
                    crate::ErrorData::DeploymentCheckpointFailed {
                        message: "checkpoint rejected".to_string(),
                    },
                )),
            };
            let result = finalize_step_loop(
                &ManagerClient::new(&server.base_url()),
                "deployment-1",
                "session-1",
                None,
                &state,
                result,
            )
            .await;
            if stop.is_none() {
                assert!(result
                    .expect_err("runner error must survive finalization")
                    .message
                    .contains("checkpoint rejected"));
            } else if stop == Some(LoopStopReason::BudgetExceeded) {
                assert!(result
                    .expect_err("exhausted budget remains a failure")
                    .message
                    .contains("deployment failed"));
            } else {
                result.expect("delayed result should finalize");
            }
            reconcile.assert_hits_async(1).await;
            release.assert_hits_async(1).await;
        }
    }

    #[tokio::test]
    async fn checkpointed_teardown_budget_failure_releases_without_another_terminal_write() {
        let server = MockServer::start_async().await;
        let mut state = running_state();
        state.platform = Platform::Test;
        state.status = alien_core::DeploymentStatus::TeardownRequired;
        state.stack_state = Some(alien_core::StackState::new(Platform::Test));
        state.runtime_metadata = Some(alien_core::RuntimeMetadata::default());
        let prepared = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/sync/reconcile")
                    .json_body_partial(r#"{"state":{"status":"teardown-required"}}"#);
                then.status(200)
                    .json_body(serde_json::json!({"success":true,"current":state}));
            })
            .await;
        let mut failed = state.clone();
        failed.status = alien_core::DeploymentStatus::TeardownFailed;
        failed.error = Some(
            AlienError::new(crate::ErrorData::StackExecutionFailed {
                message: "Setup-owned resource teardown did not complete within 0 steps"
                    .to_string(),
            })
            .into_generic(),
        );
        let terminal = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/sync/reconcile")
                    .json_body_partial(r#"{"state":{"status":"teardown-failed"}}"#);
                then.status(200)
                    .json_body(serde_json::json!({"success":true,"current":failed}));
            })
            .await;
        let release = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/sync/release");
                then.status(200);
            })
            .await;
        let client = ManagerClient::new(&server.base_url());
        let result = crate::setup_teardown::run_setup_teardown_after_handoff(
            &mut state,
            &mut deployment_config(),
            &alien_core::ClientConfig::Test,
            "deployment-1",
            &crate::runner::RunnerPolicy {
                max_steps: 0,
                operation: crate::loop_contract::LoopOperation::Delete,
                delay_strategy: crate::runner::DelayStrategy::Inline,
            },
            &ManagerApiTransport::new(client.clone(), "session-1".to_string()),
            None,
        )
        .await
        .map(|result| result.expect("teardown must run"));
        assert_eq!(
            result
                .as_ref()
                .expect("failure should be durably checkpointed")
                .loop_result
                .stop_reason,
            LoopStopReason::BudgetExceeded
        );
        let error = finalize_step_loop(&client, "deployment-1", "session-1", None, &state, result)
            .await
            .expect_err("teardown exhaustion must remain a semantic failure");
        assert!(error.message.contains("did not complete within 0 steps"));
        prepared.assert_hits_async(2).await;
        terminal.assert_hits_async(1).await;
        release.assert_hits_async(1).await;
    }

    /// A deletion the manager confirmed only releases the claim; one whose
    /// checkpoint failed is persisted by the final reconcile instead.
    #[tokio::test]
    async fn teardown_deletion_is_reconciled_again_only_when_its_checkpoint_failed() {
        for accepted in [true, false] {
            let server = MockServer::start_async().await;
            let mut state = running_state();
            state.platform = Platform::Test;
            state.status = alien_core::DeploymentStatus::TeardownRequired;
            state.stack_state = Some(alien_core::StackState::new(Platform::Test));
            state.runtime_metadata = Some(alien_core::RuntimeMetadata::default());
            let progress = server
                .mock_async(|when, then| {
                    when.method(POST)
                        .path("/v1/sync/reconcile")
                        .json_body_partial(r#"{"state":{"status":"teardown-required"}}"#);
                    then.status(200)
                        .json_body(serde_json::json!({"success":true,"current":null}));
                })
                .await;
            let mut checkpoint = server
                .mock_async(|when, then| {
                    when.method(POST)
                        .path("/v1/sync/reconcile")
                        .json_body_partial(r#"{"state":{"status":"deleted"}}"#);
                    if accepted {
                        then.status(200)
                            .json_body(serde_json::json!({"success":true,"current":null}));
                    } else {
                        then.status(500).json_body(serde_json::json!({
                            "code": "INTERNAL_ERROR", "message": "Internal server error",
                            "retryable": false, "internal": true
                        }));
                    }
                })
                .await;
            let client = ManagerClient::new(&server.base_url());
            let result = crate::setup_teardown::run_setup_teardown_after_handoff(
                &mut state,
                &mut deployment_config(),
                &alien_core::ClientConfig::Test,
                "deployment-1",
                &crate::runner::RunnerPolicy {
                    max_steps: 2,
                    operation: crate::loop_contract::LoopOperation::Delete,
                    delay_strategy: crate::runner::DelayStrategy::Inline,
                },
                &ManagerApiTransport::new(client.clone(), "session-1".to_string()),
                None,
            )
            .await
            .map(|result| result.expect("teardown must run"));
            let teardown = result.as_ref().expect("a finished teardown returns Ok");
            assert_eq!(teardown.loop_result.stop_reason, LoopStopReason::Deleted);
            assert_eq!(teardown.state_persisted, accepted);
            assert!(progress.hits_async().await >= 1);
            checkpoint.assert_hits_async(1).await;
            checkpoint.delete_async().await;

            let final_reconcile = server
                .mock_async(|when, then| {
                    when.method(POST)
                        .path("/v1/sync/reconcile")
                        .json_body_partial(r#"{"state":{"status":"deleted"}}"#);
                    then.status(200)
                        .json_body(serde_json::json!({"success":true,"current":null}));
                })
                .await;
            let release = server
                .mock_async(|when, then| {
                    when.method(POST).path("/v1/sync/release");
                    then.status(200);
                })
                .await;
            let result =
                finalize_step_loop(&client, "deployment-1", "session-1", None, &state, result)
                    .await
                    .expect("a completed teardown must succeed");
            assert_eq!(result.loop_result.outcome, LoopOutcome::Success);
            final_reconcile
                .assert_hits_async(usize::from(!accepted))
                .await;
            release.assert_hits_async(1).await;
        }
    }

    #[tokio::test]
    async fn controller_failure_remains_primary_when_terminal_release_fails() {
        let server = MockServer::start_async().await;
        let reconcile = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/sync/reconcile");
                then.status(409).body("attempt is no longer live");
            })
            .await;
        let release = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/sync/release");
                then.status(500).body("release unavailable");
            })
            .await;
        let state = failed_resource_state();
        let error = finalize_step_loop(
            &ManagerClient::new(&server.base_url()),
            "deployment-1",
            "session-1",
            None,
            &state,
            terminal_result(&state),
        )
        .await
        .expect_err("controller and release failures must be reported");
        assert_eq!(error.code, "DEPLOYMENT_FAILED");
        assert!(serde_json::to_string(&error)
            .unwrap()
            .contains("execution profile is missing"));
        assert!(error
            .context
            .as_ref()
            .unwrap()
            .get("finalizationError")
            .is_some());
        assert!(error
            .human_report()
            .causes
            .iter()
            .any(|cause| cause.message.contains("release")));
        reconcile.assert_hits_async(0).await;
        release.assert_hits_async(1).await;
    }

    #[tokio::test]
    async fn final_reconcile_releases_lease_after_reconcile_failure() {
        let server = MockServer::start_async().await;
        let reconcile = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/sync/reconcile");
                then.status(500).body("reconcile failed");
            })
            .await;
        let release = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/sync/release");
                then.status(200);
            })
            .await;
        let client = ManagerClient::new(&server.base_url());
        let state = DeploymentState {
            status: alien_core::DeploymentStatus::Running,
            platform: alien_core::Platform::Aws,
            current_release: None,
            target_release: None,
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        };

        let error = final_reconcile(&client, "deployment-1", "session-1", None, &state)
            .await
            .expect_err("reconcile failure must propagate");

        assert_eq!(error.http_status_code, Some(500));
        reconcile.assert_async().await;
        release.assert_async().await;
    }

    #[tokio::test]
    async fn deleted_missing_deployment_is_successful_and_still_releases() {
        let server = MockServer::start_async().await;
        let reconcile = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/sync/reconcile");
                then.status(404).body("missing");
            })
            .await;
        let release = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/sync/release");
                then.status(404).body("missing");
            })
            .await;
        let client = ManagerClient::new(&server.base_url());
        let state = DeploymentState {
            status: alien_core::DeploymentStatus::Deleted,
            platform: alien_core::Platform::Aws,
            current_release: None,
            target_release: None,
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        };

        final_reconcile(&client, "deployment-1", "session-1", None, &state)
            .await
            .expect("missing deleted deployment should be complete");
        reconcile.assert_async().await;
        release.assert_async().await;
    }

    #[test]
    fn setup_run_can_recover_failed_setup_and_provisioning_states() {
        let statuses = setup_run_acquire_statuses();

        assert!(statuses
            .iter()
            .any(|status| status == "initial-setup-failed"));
        assert!(statuses
            .iter()
            .any(|status| status == "provisioning-failed"));
    }

    #[tokio::test]
    async fn explicit_acquire_mismatch_fails_without_retrying() {
        let server = MockServer::start_async().await;
        let acquire = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/sync/acquire");
                then.status(200).json_body(serde_json::json!({
                    "deployments": [],
                    "notAcquired": [{
                        "deploymentId": "deployment-1",
                        "reason": "deploymentModelMismatch"
                    }]
                }));
            })
            .await;
        let client = ManagerClient::new(&server.base_url());

        let error = acquire_deployment(&client, "deployment-1", "session-1", DeploymentModel::Push)
            .await
            .expect_err("a permanent acquisition mismatch must fail immediately");

        assert_eq!(error.code, "DEPLOYMENT_ACQUIRE_UNAVAILABLE");
        assert!(!error.retryable);
        assert!(error.message.contains("deploymentModelMismatch"));
        acquire.assert_hits_async(1).await;
    }

    #[tokio::test]
    async fn contended_acquisition_reports_the_held_lease_after_bounded_wait() {
        let server = MockServer::start_async().await;
        let acquire = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/sync/acquire");
                then.status(200).json_body(serde_json::json!({
                    "deployments": [],
                    "notAcquired": [{"deploymentId": "deployment-1", "reason": "contended"}]
                }));
            })
            .await;
        let client = ManagerClient::new(&server.base_url());
        let error = acquire_deployment(&client, "deployment-1", "session-1", DeploymentModel::Push)
            .await
            .expect_err("a held lease must exhaust the bounded wait");
        assert!(
            error.message.contains("another operation still holds"),
            "{}",
            error.message
        );
        assert!(error.message.contains("did not acquire the lease"));
        acquire.assert_hits_async(MAX_ACQUIRE_ATTEMPTS).await;
    }

    fn sample_heartbeat() -> ResourceHeartbeat {
        ResourceHeartbeat {
            deployment_id: Some("dep_test".to_string()),
            resource_id: "api".to_string(),
            resource_type: ResourceType::from("container"),
            controller_platform: Platform::Kubernetes,
            backend: HeartbeatBackend::Kubernetes,
            observed_at: chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            data: ResourceHeartbeatData::Container(ContainerHeartbeatData::Kubernetes(
                KubernetesContainerHeartbeatData {
                    status: WorkloadHeartbeatStatus {
                        health: ObservedHealth::Healthy,
                        lifecycle: ProviderLifecycleState::Running,
                        message: None,
                        stale: false,
                        partial: true,
                        collection_issues: vec![HeartbeatCollectionIssue {
                            source: "metrics".to_string(),
                            reason: HeartbeatCollectionIssueReason::NotInstalled,
                            severity: HeartbeatIssueSeverity::Warning,
                            message: "metrics API is not installed".to_string(),
                        }],
                    },
                    namespace: "default".to_string(),
                    name: "api".to_string(),
                    workload_kind: KubernetesWorkloadKind::Deployment,
                    replicas: WorkloadReplicaStatus {
                        desired: Some(2),
                        current: Some(2),
                        ready: Some(2),
                        available: Some(2),
                        updated: Some(2),
                        misscheduled: None,
                    },
                    restarts: Some(0),
                    cpu: None,
                    memory: None,
                    workload: None,
                    pods: vec![],
                    events: vec![],
                },
            )),
            raw: vec![],
        }
    }

    fn sample_observed_inventory_batch() -> ObservedInventoryBatch {
        ObservedInventoryBatch {
            source_kind: "operator".to_string(),
            inventory_scope: "cluster/test".to_string(),
            controller_platform: Platform::Machines,
            backend: HeartbeatBackend::External,
            observed_at: chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            complete: true,
            resources: vec![],
        }
    }

    #[test]
    fn converts_core_heartbeats_to_generated_manager_request_type() {
        let heartbeats = to_manager_api_heartbeats(vec![sample_heartbeat()])
            .expect("core heartbeat should convert to generated manager API heartbeat");

        let value = serde_json::to_value(&heartbeats[0]).expect("heartbeat should serialize");

        assert_eq!(value["deploymentId"], "dep_test");
        assert_eq!(value["resourceId"], "api");
        assert_eq!(value["data"]["resourceType"], "container");
        assert!(value.get("collection").is_none());
        assert!(value["data"]["data"].get("summary").is_none());
        assert!(value["data"]["data"].get("detail").is_none());
    }

    /// The generated type is built from the manager OpenAPI spec, so a variant added to
    /// `alien-core` without regenerating it fails here — in the client, before any HTTP call.
    #[test]
    fn converts_gcp_sandbox_heartbeats_to_generated_manager_request_type() {
        let heartbeat = ResourceHeartbeat {
            deployment_id: Some("dep_test".to_string()),
            resource_id: "agent-sbx".to_string(),
            resource_type: ResourceType::from("sandbox"),
            controller_platform: Platform::Gcp,
            backend: HeartbeatBackend::Gcp,
            observed_at: chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            data: ResourceHeartbeatData::Sandbox(SandboxHeartbeatData::GcpAgentPlatform(
                GcpAgentPlatformSandboxHeartbeatData {
                    status: SandboxHeartbeatStatus::default(),
                    engine: "eng".to_string(),
                    template_id: "tpl1".to_string(),
                },
            )),
            raw: vec![],
        };

        let heartbeats = to_manager_api_heartbeats(vec![heartbeat])
            .expect("core heartbeat should convert to generated manager API heartbeat");

        let value = serde_json::to_value(&heartbeats[0]).expect("heartbeat should serialize");

        assert_eq!(value["resourceId"], "agent-sbx");
        assert_eq!(value["resourceType"], "sandbox");
        assert_eq!(value["data"]["resourceType"], "sandbox");
        assert_eq!(value["data"]["data"]["backend"], "gcpAgentPlatform");
        assert_eq!(value["data"]["data"]["engine"], "eng");
        assert_eq!(value["data"]["data"]["templateId"], "tpl1");
        assert_eq!(value["data"]["data"]["status"]["health"], "healthy");
        assert_eq!(value["data"]["data"]["status"]["lifecycle"], "running");
    }

    #[test]
    fn converts_observed_inventory_batches_to_generated_manager_request_type() {
        let batches =
            to_manager_api_observed_inventory_batches(vec![sample_observed_inventory_batch()])
                .expect("core observed inventory should convert to generated manager API type");

        let value = serde_json::to_value(&batches[0]).expect("inventory batch should serialize");

        assert_eq!(value["sourceKind"], "operator");
        assert_eq!(value["inventoryScope"], "cluster/test");
        assert_eq!(value["controllerPlatform"], "machines");
        assert_eq!(value["backend"], "external");
        assert_eq!(value["complete"], true);
        assert_eq!(
            value["resources"]
                .as_array()
                .expect("resources should be an array")
                .len(),
            0
        );
    }
}

/// Release the deployment lock.
///
pub async fn release_deployment(
    client: &ManagerClient,
    deployment_id: &str,
    session: &str,
    execution_claim: Option<&ExecutionClaim>,
) -> Result<(), AlienError> {
    if let Err(error) = client
        .release()
        .body(alien_manager_api::types::ReleaseRequest {
            deployment_id: deployment_id.to_string(),
            execution_claim: execution_claim
                .map(to_manager_api_execution_claim)
                .transpose()?,
            session: session.to_string(),
        })
        .send()
        .await
        .into_sdk_error_reading_body()
        .await
    {
        if is_missing_deployment_response(&error) {
            info!(
                deployment_id = %deployment_id,
                "Deployment was already removed; no sync lock remains to release"
            );
            return Ok(());
        }
        return Err(error.context(alien_error::GenericError {
            message: "Failed to release sync lock".to_string(),
        }));
    }
    Ok(())
}

fn is_missing_deployment_response(error: &AlienError) -> bool {
    error.http_status_code == Some(404)
}
