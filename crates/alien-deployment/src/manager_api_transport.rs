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
use serde::Serialize;
use tracing::info;

use crate::transport::{DeploymentLoopTransport, StepReconcileResult};

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
}

impl ManagerApiTransport {
    pub fn new(client: ManagerClient, session: String) -> Self {
        Self { client, session }
    }
}

#[async_trait]
impl DeploymentLoopTransport for ManagerApiTransport {
    async fn renew_lease(&self, deployment_id: &str) -> Result<(), AlienError> {
        self.client
            .renew()
            .body(alien_manager_api::types::RenewRequest {
                deployment_id: deployment_id.to_string(),
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
            operator_version: None,
        };

        // POST state to the manager
        let resp = self
            .client
            .reconcile()
            .body(body)
            .send()
            .await
            .into_sdk_error()
            .context(alien_error::GenericError {
                message: "Failed to reconcile step via manager API".to_string(),
            })?
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
// Shared helpers for the acquire / final-reconcile / release pattern.
//
// Every external caller (alien-deploy-cli, alien-cli, alien-terraform) follows
// the same protocol:
//   1. acquire_deployment()   — lock the deployment with a retry loop
//   2. run_step_loop()        — step until terminal (uses ManagerApiTransport)
//   3. final_reconcile()      — persist terminal state (even on error)
//   4. release_deployment()   — unlock (even on error)
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
    Acquired,
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
        (Err(operation_error), Err(finalization_error)) => Err(operation_error
            .into_generic()
            .context(alien_error::GenericError {
                message: format!(
                    "Operation failed and deployment finalization also failed: {finalization_error}"
                ),
            })),
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
) -> Result<(), AlienError> {
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
    .map(|_| ())
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
) -> Result<(), AlienError> {
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
    .map(|_| ())
}

/// Acquire a deployment lock and return the manager's acquired deployment
/// payload. Callers that need deployment-config fields that are intentionally
/// redacted from `GET /v1/deployments` should use this helper.
pub async fn acquire_deployment_with_payload(
    client: &ManagerClient,
    deployment_id: &str,
    session: &str,
    deployment_model: DeploymentModel,
) -> Result<serde_json::Value, AlienError> {
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
) -> Result<serde_json::Value, AlienError> {
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
        let resp = client
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
            .into_sdk_error()
            .context(alien_error::GenericError {
                message: "Failed to acquire setup teardown sync lock".to_string(),
            })?;

        if resp.into_inner().deployments.into_iter().next().is_some() {
            return Ok(SetupDeleteAcquireOutcome::Acquired);
        }

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
) -> Result<serde_json::Value, AlienError> {
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

        if let Some(deployment) = resp.into_inner().deployments.into_iter().next() {
            return Ok(deployment.deployment);
        }

        if attempt == MAX_ACQUIRE_ATTEMPTS {
            return Err(AlienError::new(alien_error::GenericError {
                message: "Timed out waiting for deployment lock".to_string(),
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

/// Persist the final deployment state and release its manager lease.
///
/// Lease release is attempted even when serialization or reconciliation fails.
/// A missing deployment is successful only for a completed deletion.
pub async fn final_reconcile(
    client: &ManagerClient,
    deployment_id: &str,
    session: &str,
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

    let release_result = release_deployment(client, deployment_id, session).await;
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
    use alien_core::{
        ContainerHeartbeatData, GcpAgentPlatformSandboxHeartbeatData, HeartbeatBackend,
        HeartbeatCollectionIssue, HeartbeatCollectionIssueReason, HeartbeatIssueSeverity,
        KubernetesContainerHeartbeatData, KubernetesWorkloadKind, ObservedHealth, Platform,
        ProviderLifecycleState, ResourceHeartbeatData, ResourceType, SandboxHeartbeatData,
        SandboxHeartbeatStatus, WorkloadHeartbeatStatus, WorkloadReplicaStatus,
    };
    use chrono::TimeZone;
    use httpmock::prelude::*;

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

        assert!(error.message.contains("reconcile failed"));
        assert_eq!(
            error
                .source
                .as_deref()
                .map(|source| source.message.as_str()),
            Some("runner failed")
        );
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

        let error = final_reconcile(&client, "deployment-1", "session-1", &state)
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

        final_reconcile(&client, "deployment-1", "session-1", &state)
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
) -> Result<(), AlienError> {
    if let Err(error) = client
        .release()
        .body(alien_manager_api::types::ReleaseRequest {
            deployment_id: deployment_id.to_string(),
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
