//! Canonical deployment-loop contract.
//!
//! Every deployment-loop caller (alien-cli, alien-deploy-cli, alien-manager,
//! alien-agent, alien-terraform) must use these types so loop semantics are
//! consistent across push, pull, and platform paths.

use crate::DeploymentStatus;
use serde::{Deserialize, Serialize};

/// Why the loop stopped running.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LoopStopReason {
    /// Deployment reached a synced state (is_synced() returned true).
    Synced,
    /// Deployment reached a failed state.
    Failed,
    /// Deployment was deleted (status == Deleted).
    Deleted,
    /// Deployment entered a handoff status — another actor takes over.
    /// For push initial setup: Provisioning/Updating means the manager takes over.
    Handoff,
    /// No work was available (no target release, no config, etc.).
    NoWork,
    /// Step budget exhausted without reaching a terminal state.
    BudgetExceeded,
    /// Loop was cancelled by an external signal.
    Cancelled,
}

/// Caller-facing outcome of the loop run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LoopOutcome {
    /// The operation completed successfully.
    Success,
    /// The operation failed.
    Failure,
    /// The loop stopped without a definitive success/failure verdict.
    Neutral,
}

/// The result of a deployment loop run.
#[derive(Debug, Clone)]
pub struct LoopResult {
    pub stop_reason: LoopStopReason,
    pub outcome: LoopOutcome,
    pub final_status: DeploymentStatus,
}

/// The operation being performed, which determines success criteria.
///
/// Different callers use different operations to express their intent:
///
/// - **`Deploy`**: Drive the deployment all the way to Running. Used by
///   alien-manager, alien-agent, alien-cli — any actor that should continue
///   through Provisioning/Updating to Running.
///
/// - **`InitialSetup`**: Drive only the initial setup phase (Pending →
///   InitialSetup → Provisioning). Stops with `Handoff` once the deployment
///   reaches Provisioning or Updating, because another actor (the manager)
///   takes over from there. Used by alien-deploy-cli's `push_initial_setup`.
///
/// - **`Delete`**: Drive deletion — success is Deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopOperation {
    /// Full deploy — success is Running. Provisioning/Updating are NOT
    /// terminal; the loop continues stepping through them.
    Deploy,
    /// Push initial setup only — stops at Provisioning/Updating with Handoff
    /// so the manager can take over. Used by alien-deploy-cli.
    InitialSetup,
    /// Delete — success is Deleted.
    Delete,
}

/// Classify a deployment status into a stop reason and outcome
/// based on the operation being performed.
pub fn classify_status(status: &DeploymentStatus, operation: LoopOperation) -> Option<LoopResult> {
    // Running is success for both Deploy and InitialSetup (though InitialSetup
    // normally stops earlier at Provisioning via Handoff).
    match (status, operation) {
        (DeploymentStatus::Running, LoopOperation::Deploy | LoopOperation::InitialSetup) => {
            return Some(LoopResult {
                stop_reason: LoopStopReason::Synced,
                outcome: LoopOutcome::Success,
                final_status: *status,
            });
        }
        (DeploymentStatus::Deleted, LoopOperation::Delete) => {
            return Some(LoopResult {
                stop_reason: LoopStopReason::Deleted,
                outcome: LoopOutcome::Success,
                final_status: *status,
            });
        }
        _ => {}
    }

    if status.is_failed() {
        return Some(LoopResult {
            stop_reason: LoopStopReason::Failed,
            outcome: LoopOutcome::Failure,
            final_status: *status,
        });
    }

    // Handoff: only for InitialSetup. The push CLI stops here because the
    // manager takes over at Provisioning/Updating. The manager and agent use
    // Deploy, which does NOT stop here — they continue stepping through
    // Provisioning/Updating to reach Running.
    if operation == LoopOperation::InitialSetup
        && matches!(
            status,
            DeploymentStatus::Provisioning | DeploymentStatus::Updating
        )
    {
        return Some(LoopResult {
            stop_reason: LoopStopReason::Handoff,
            outcome: LoopOutcome::Neutral,
            final_status: *status,
        });
    }

    None
}

// Test ownership: Loop contract behavior tests live HERE (alien-deployment).
// Callers (alien-manager, alien-agent, alien-deploy-cli) should NOT duplicate
// these tests. They can test their own integration with classify_status
// (e.g. skip logic, operation selection) but not re-test the contract itself.

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_STATUSES: [DeploymentStatus; 15] = [
        DeploymentStatus::Pending,
        DeploymentStatus::InitialSetup,
        DeploymentStatus::InitialSetupFailed,
        DeploymentStatus::Provisioning,
        DeploymentStatus::ProvisioningFailed,
        DeploymentStatus::Running,
        DeploymentStatus::RefreshFailed,
        DeploymentStatus::UpdatePending,
        DeploymentStatus::Updating,
        DeploymentStatus::UpdateFailed,
        DeploymentStatus::DeletePending,
        DeploymentStatus::Deleting,
        DeploymentStatus::DeleteFailed,
        DeploymentStatus::Deleted,
        DeploymentStatus::Error,
    ];

    #[test]
    fn test_deploy_running_is_success() {
        let result = classify_status(&DeploymentStatus::Running, LoopOperation::Deploy);
        let r = result.unwrap();
        assert_eq!(r.stop_reason, LoopStopReason::Synced);
        assert_eq!(r.outcome, LoopOutcome::Success);
    }

    #[test]
    fn test_delete_deleted_is_success() {
        let result = classify_status(&DeploymentStatus::Deleted, LoopOperation::Delete);
        let r = result.unwrap();
        assert_eq!(r.stop_reason, LoopStopReason::Deleted);
        assert_eq!(r.outcome, LoopOutcome::Success);
    }

    #[test]
    fn test_failed_statuses_are_failure() {
        for status in [
            DeploymentStatus::InitialSetupFailed,
            DeploymentStatus::ProvisioningFailed,
            DeploymentStatus::UpdateFailed,
            DeploymentStatus::DeleteFailed,
            DeploymentStatus::RefreshFailed,
        ] {
            let result = classify_status(&status, LoopOperation::Deploy).unwrap();
            assert_eq!(
                result.outcome,
                LoopOutcome::Failure,
                "Expected failure for {:?}",
                status
            );
        }
    }

    #[test]
    fn test_provisioning_is_not_terminal_for_deploy() {
        // Deploy (used by manager/agent) continues through Provisioning.
        let result = classify_status(&DeploymentStatus::Provisioning, LoopOperation::Deploy);
        assert!(
            result.is_none(),
            "Provisioning should not be terminal for Deploy"
        );
    }

    #[test]
    fn test_provisioning_is_handoff_for_initial_setup() {
        // InitialSetup (used by push CLI) stops at Provisioning.
        let result = classify_status(&DeploymentStatus::Provisioning, LoopOperation::InitialSetup);
        let r = result.unwrap();
        assert_eq!(r.stop_reason, LoopStopReason::Handoff);
        assert_eq!(r.outcome, LoopOutcome::Neutral);
    }

    #[test]
    fn test_pending_is_not_terminal() {
        let result = classify_status(&DeploymentStatus::Pending, LoopOperation::Deploy);
        assert!(result.is_none());
    }

    #[test]
    fn test_running_for_delete_is_not_terminal() {
        let result = classify_status(&DeploymentStatus::Running, LoopOperation::Delete);
        assert!(result.is_none());
    }

    // ---- Exhaustive coverage: every DeploymentStatus × all operations ----

    #[test]
    fn classify_status_covers_every_status_for_deploy() {
        for status in ALL_STATUSES {
            let _ = classify_status(&status, LoopOperation::Deploy);
        }
    }

    #[test]
    fn classify_status_covers_every_status_for_initial_setup() {
        for status in ALL_STATUSES {
            let _ = classify_status(&status, LoopOperation::InitialSetup);
        }
    }

    #[test]
    fn classify_status_covers_every_status_for_delete() {
        for status in ALL_STATUSES {
            let _ = classify_status(&status, LoopOperation::Delete);
        }
    }

    #[test]
    fn deploy_operation_expected_results() {
        // Deploy is used by manager/agent/cli — continues through Provisioning/Updating.
        let expectations: Vec<(DeploymentStatus, Option<(LoopStopReason, LoopOutcome)>)> = vec![
            (DeploymentStatus::Pending, None),
            (DeploymentStatus::InitialSetup, None),
            (
                DeploymentStatus::InitialSetupFailed,
                Some((LoopStopReason::Failed, LoopOutcome::Failure)),
            ),
            (DeploymentStatus::Provisioning, None), // NOT terminal — manager continues
            (
                DeploymentStatus::ProvisioningFailed,
                Some((LoopStopReason::Failed, LoopOutcome::Failure)),
            ),
            (
                DeploymentStatus::Running,
                Some((LoopStopReason::Synced, LoopOutcome::Success)),
            ),
            (
                DeploymentStatus::RefreshFailed,
                Some((LoopStopReason::Failed, LoopOutcome::Failure)),
            ),
            (DeploymentStatus::UpdatePending, None),
            (DeploymentStatus::Updating, None), // NOT terminal — manager continues
            (
                DeploymentStatus::UpdateFailed,
                Some((LoopStopReason::Failed, LoopOutcome::Failure)),
            ),
            (DeploymentStatus::DeletePending, None),
            (DeploymentStatus::Deleting, None),
            (
                DeploymentStatus::DeleteFailed,
                Some((LoopStopReason::Failed, LoopOutcome::Failure)),
            ),
            (DeploymentStatus::Deleted, None),
        ];

        for (status, expected) in expectations {
            let result = classify_status(&status, LoopOperation::Deploy);
            match expected {
                None => assert!(
                    result.is_none(),
                    "Expected None for Deploy + {:?}, got {:?}",
                    status,
                    result.map(|r| (r.stop_reason, r.outcome))
                ),
                Some((stop, outcome)) => {
                    let r =
                        result.unwrap_or_else(|| panic!("Expected Some for Deploy + {:?}", status));
                    assert_eq!(
                        r.stop_reason, stop,
                        "Wrong stop_reason for Deploy + {:?}",
                        status
                    );
                    assert_eq!(
                        r.outcome, outcome,
                        "Wrong outcome for Deploy + {:?}",
                        status
                    );
                }
            }
        }
    }

    #[test]
    fn delete_operation_expected_results() {
        let expectations: Vec<(DeploymentStatus, Option<(LoopStopReason, LoopOutcome)>)> = vec![
            (DeploymentStatus::Pending, None),
            (DeploymentStatus::InitialSetup, None),
            (
                DeploymentStatus::InitialSetupFailed,
                Some((LoopStopReason::Failed, LoopOutcome::Failure)),
            ),
            (DeploymentStatus::Provisioning, None),
            (
                DeploymentStatus::ProvisioningFailed,
                Some((LoopStopReason::Failed, LoopOutcome::Failure)),
            ),
            (DeploymentStatus::Running, None),
            (
                DeploymentStatus::RefreshFailed,
                Some((LoopStopReason::Failed, LoopOutcome::Failure)),
            ),
            (DeploymentStatus::UpdatePending, None),
            (DeploymentStatus::Updating, None),
            (
                DeploymentStatus::UpdateFailed,
                Some((LoopStopReason::Failed, LoopOutcome::Failure)),
            ),
            (DeploymentStatus::DeletePending, None),
            (DeploymentStatus::Deleting, None),
            (
                DeploymentStatus::DeleteFailed,
                Some((LoopStopReason::Failed, LoopOutcome::Failure)),
            ),
            (
                DeploymentStatus::Deleted,
                Some((LoopStopReason::Deleted, LoopOutcome::Success)),
            ),
        ];

        for (status, expected) in expectations {
            let result = classify_status(&status, LoopOperation::Delete);
            match expected {
                None => assert!(
                    result.is_none(),
                    "Expected None for Delete + {:?}, got {:?}",
                    status,
                    result.map(|r| (r.stop_reason, r.outcome))
                ),
                Some((stop, outcome)) => {
                    let r =
                        result.unwrap_or_else(|| panic!("Expected Some for Delete + {:?}", status));
                    assert_eq!(
                        r.stop_reason, stop,
                        "Wrong stop_reason for Delete + {:?}",
                        status
                    );
                    assert_eq!(
                        r.outcome, outcome,
                        "Wrong outcome for Delete + {:?}",
                        status
                    );
                }
            }
        }
    }

    #[test]
    fn initial_setup_operation_expected_results() {
        // InitialSetup is used by alien-deploy-cli's push_initial_setup.
        // It stops at Provisioning/Updating with Handoff (manager takes over).
        let expectations: Vec<(DeploymentStatus, Option<(LoopStopReason, LoopOutcome)>)> = vec![
            (DeploymentStatus::Pending, None),
            (DeploymentStatus::InitialSetup, None),
            (
                DeploymentStatus::InitialSetupFailed,
                Some((LoopStopReason::Failed, LoopOutcome::Failure)),
            ),
            (
                DeploymentStatus::Provisioning,
                Some((LoopStopReason::Handoff, LoopOutcome::Neutral)),
            ),
            (
                DeploymentStatus::ProvisioningFailed,
                Some((LoopStopReason::Failed, LoopOutcome::Failure)),
            ),
            (
                DeploymentStatus::Running,
                Some((LoopStopReason::Synced, LoopOutcome::Success)),
            ),
            (
                DeploymentStatus::RefreshFailed,
                Some((LoopStopReason::Failed, LoopOutcome::Failure)),
            ),
            (DeploymentStatus::UpdatePending, None),
            (
                DeploymentStatus::Updating,
                Some((LoopStopReason::Handoff, LoopOutcome::Neutral)),
            ),
            (
                DeploymentStatus::UpdateFailed,
                Some((LoopStopReason::Failed, LoopOutcome::Failure)),
            ),
            (DeploymentStatus::DeletePending, None),
            (DeploymentStatus::Deleting, None),
            (
                DeploymentStatus::DeleteFailed,
                Some((LoopStopReason::Failed, LoopOutcome::Failure)),
            ),
            (DeploymentStatus::Deleted, None),
        ];

        for (status, expected) in expectations {
            let result = classify_status(&status, LoopOperation::InitialSetup);
            match expected {
                None => assert!(
                    result.is_none(),
                    "Expected None for InitialSetup + {:?}, got {:?}",
                    status,
                    result.map(|r| (r.stop_reason, r.outcome))
                ),
                Some((stop, outcome)) => {
                    let r = result
                        .unwrap_or_else(|| panic!("Expected Some for InitialSetup + {:?}", status));
                    assert_eq!(
                        r.stop_reason, stop,
                        "Wrong stop_reason for InitialSetup + {:?}",
                        status
                    );
                    assert_eq!(
                        r.outcome, outcome,
                        "Wrong outcome for InitialSetup + {:?}",
                        status
                    );
                }
            }
        }
    }

    // ---- Regression: failed-but-synced statuses must be Failure, never Success ----

    #[test]
    fn failed_synced_statuses_map_to_failure_not_success() {
        let failed_synced_statuses = [
            DeploymentStatus::InitialSetupFailed,
            DeploymentStatus::ProvisioningFailed,
            DeploymentStatus::UpdateFailed,
            DeploymentStatus::DeleteFailed,
            DeploymentStatus::RefreshFailed,
        ];

        for status in failed_synced_statuses {
            assert!(
                status.is_synced(),
                "{:?} should be synced (is_synced() == true)",
                status
            );
            assert!(
                status.is_failed(),
                "{:?} should be failed (is_failed() == true)",
                status
            );

            for operation in [
                LoopOperation::Deploy,
                LoopOperation::InitialSetup,
                LoopOperation::Delete,
            ] {
                let result = classify_status(&status, operation).unwrap_or_else(|| {
                    panic!(
                        "classify_status should return Some for failed status {:?} with {:?}",
                        status, operation
                    )
                });
                assert_eq!(
                    result.outcome,
                    LoopOutcome::Failure,
                    "REGRESSION: {:?} with {:?} mapped to {:?} instead of Failure. \
                     Failed statuses must NEVER map to Success even though is_synced() is true.",
                    status,
                    operation,
                    result.outcome
                );
                assert_ne!(
                    result.outcome,
                    LoopOutcome::Success,
                    "REGRESSION: {:?} reached Success outcome",
                    status
                );
            }
        }
    }

    // ---- Budget exceeded produces Failure ----

    #[test]
    fn budget_exceeded_result_is_failure() {
        let result = LoopResult {
            stop_reason: LoopStopReason::BudgetExceeded,
            outcome: LoopOutcome::Failure,
            final_status: DeploymentStatus::Pending,
        };
        assert_eq!(result.outcome, LoopOutcome::Failure);
        assert_eq!(result.stop_reason, LoopStopReason::BudgetExceeded);
    }

    // ---- Handoff only applies to InitialSetup, not Deploy or Delete ----

    #[test]
    fn provisioning_and_updating_are_handoff_only_for_initial_setup() {
        for status in [DeploymentStatus::Provisioning, DeploymentStatus::Updating] {
            // InitialSetup (push CLI) stops here with Handoff.
            let initial_setup_result = classify_status(&status, LoopOperation::InitialSetup);
            assert_eq!(
                initial_setup_result.as_ref().map(|r| &r.stop_reason),
                Some(&LoopStopReason::Handoff),
                "{:?} should be Handoff for InitialSetup",
                status
            );

            // Deploy (manager/agent) does NOT stop here — continues to Running.
            let deploy_result = classify_status(&status, LoopOperation::Deploy);
            assert!(
                deploy_result.is_none(),
                "{:?} should be None (non-terminal) for Deploy, got {:?}",
                status,
                deploy_result.map(|r| (r.stop_reason, r.outcome))
            );

            // Delete also does not stop here.
            let delete_result = classify_status(&status, LoopOperation::Delete);
            assert!(
                delete_result.is_none(),
                "{:?} should be None (non-terminal) for Delete, got {:?}",
                status,
                delete_result.map(|r| (r.stop_reason, r.outcome))
            );
        }
    }

    // ---- Deleted is only success for Delete, not for Deploy/InitialSetup ----

    #[test]
    fn deleted_is_only_success_for_delete_operation() {
        for op in [LoopOperation::Deploy, LoopOperation::InitialSetup] {
            let result = classify_status(&DeploymentStatus::Deleted, op);
            assert!(
                result.is_none(),
                "Deleted should not be terminal for {:?} operation",
                op
            );
        }

        let delete_result = classify_status(&DeploymentStatus::Deleted, LoopOperation::Delete);
        let r = delete_result.expect("Deleted should be terminal for Delete");
        assert_eq!(r.outcome, LoopOutcome::Success);
    }
}
