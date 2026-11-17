//! Keeps the machines a cluster admits up to date for a deployment whose update or refresh
//! failed.
//!
//! A failed deployment takes no step, so nothing else would refresh the list: a machine that
//! replaces a lost one would be refused until someone retries. This tick calls only each
//! cluster's membership sync. It cannot change the deployment: it takes the state by reference
//! and returns nothing from it, so there is no status, controller state or retry request to
//! write back.

use crate::{runner, transport::DeploymentLoopTransport, DeploymentState, DeploymentStatus};
use crate::{ErrorData, Result};
use alien_core::{ComputeCluster, ResourceStatus};
use alien_error::{AlienError, Context};
use alien_infra::StackExecutor;
use tracing::warn;

/// Whether a deployment in this state gets a membership tick.
///
/// - `UpdateFailed` without a retry request: no step runs for it. With a retry request the
///   normal step runs and consumes the request, so no tick.
/// - `RefreshFailed` with a compute cluster that is itself failed: the cluster's failed state
///   takes no step, though the rest of the deployment is still observed.
///
/// Every other state either takes steps that already refresh the list, never served a
/// release, or is being removed, and must not admit new machines.
pub fn membership_tick_wanted(state: &DeploymentState) -> bool {
    if state.retry_requested || state.stack_state.is_none() {
        return false;
    }
    match state.status {
        DeploymentStatus::UpdateFailed => true,
        DeploymentStatus::RefreshFailed => state.stack_state.as_ref().is_some_and(|stack| {
            stack.resources.values().any(|resource| {
                resource.config.resource_type() == ComputeCluster::RESOURCE_TYPE
                    && matches!(
                        resource.status,
                        ResourceStatus::RefreshFailed | ResourceStatus::UpdateFailed
                    )
            })
        }),
        _ => false,
    }
}

/// Runs the membership tick of one deployment while keeping its lease alive.
///
/// The caller holds the lease and releases it afterwards. `state` is not changed and nothing
/// is checkpointed.
pub async fn run_membership_tick(
    state: &DeploymentState,
    config: &crate::DeploymentConfig,
    client_config: &alien_core::ClientConfig,
    deployment_id: &str,
    transport: &dyn DeploymentLoopTransport,
    service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<()> {
    let result = runner::run_with_lease_renewal(
        deployment_id,
        transport,
        sync_membership(state, config, client_config, service_provider),
    )
    .await;
    if let Err(error) = &result {
        warn!(%deployment_id, %error, "Could not update the machines the deployment's cluster admits; keeping the list as it is");
    }
    result
}

/// Asks every resource that keeps a list of admitted machines to refresh it.
pub async fn sync_membership(
    state: &DeploymentState,
    config: &crate::DeploymentConfig,
    client_config: &alien_core::ClientConfig,
    service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<()> {
    let stack_state = state.stack_state.as_ref().ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required to update the admitted machines".to_string(),
        })
    })?;
    let stack = state
        .runtime_metadata
        .as_ref()
        .and_then(|metadata| metadata.prepared_stack.as_ref())
        .ok_or_else(|| {
            AlienError::new(ErrorData::MissingConfiguration {
                message: "Prepared stack required to update the admitted machines".to_string(),
            })
        })?;
    let executor = StackExecutor::builder(stack, client_config.clone())
        .deployment_config(config)
        .service_provider(service_provider)
        .build()
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to create executor to update the admitted machines".to_string(),
        })?;
    executor
        .sync_membership(stack_state)
        .await
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to update the admitted machines".to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{Platform, Resource, StackResourceState, StackState, Storage};

    fn state(status: DeploymentStatus, resources: &[(&str, bool, ResourceStatus)]) -> DeploymentState {
        let mut stack_state = StackState::new(Platform::Aws);
        for (id, is_cluster, resource_status) in resources {
            let config = if *is_cluster {
                Resource::new(ComputeCluster::new((*id).to_string()).build())
            } else {
                Resource::new(Storage::new((*id).to_string()).build())
            };
            let mut resource = StackResourceState::new_pending(
                config.resource_type().to_string(),
                config,
                None,
                vec![],
            );
            resource.status = *resource_status;
            stack_state.resources.insert((*id).to_string(), resource);
        }
        DeploymentState {
            status,
            platform: Platform::Aws,
            current_release: None,
            target_release: None,
            stack_state: Some(stack_state),
            error: None,
            environment_info: None,
            retry_requested: false,
            protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
            runtime_metadata: None,
        }
    }

    #[test]
    fn a_failed_update_gets_a_tick() {
        let failed = state(
            DeploymentStatus::UpdateFailed,
            &[("data", false, ResourceStatus::UpdateFailed)],
        );
        assert!(membership_tick_wanted(&failed));
    }

    #[test]
    fn a_retry_request_is_never_ticked_so_the_normal_step_consumes_it() {
        let mut failed = state(
            DeploymentStatus::UpdateFailed,
            &[("compute", true, ResourceStatus::UpdateFailed)],
        );
        failed.retry_requested = true;
        assert!(!membership_tick_wanted(&failed));
        let mut refresh = state(
            DeploymentStatus::RefreshFailed,
            &[("compute", true, ResourceStatus::RefreshFailed)],
        );
        refresh.retry_requested = true;
        assert!(!membership_tick_wanted(&refresh));
    }

    #[test]
    fn a_failed_refresh_is_ticked_only_when_the_cluster_itself_failed() {
        assert!(membership_tick_wanted(&state(
            DeploymentStatus::RefreshFailed,
            &[("compute", true, ResourceStatus::RefreshFailed)],
        )));
        // The cluster is Running, so the step the loop still takes refreshes its list.
        assert!(!membership_tick_wanted(&state(
            DeploymentStatus::RefreshFailed,
            &[
                ("compute", true, ResourceStatus::Running),
                ("data", false, ResourceStatus::RefreshFailed),
            ],
        )));
    }

    #[test]
    fn no_other_state_is_ticked() {
        for status in [
            DeploymentStatus::Running,
            DeploymentStatus::Pending,
            DeploymentStatus::InitialSetup,
            DeploymentStatus::InitialSetupFailed,
            DeploymentStatus::Provisioning,
            DeploymentStatus::ProvisioningFailed,
            DeploymentStatus::UpdatePending,
            DeploymentStatus::Updating,
            DeploymentStatus::DeletePending,
            DeploymentStatus::Deleting,
            DeploymentStatus::DeleteFailed,
            DeploymentStatus::TeardownRequired,
            DeploymentStatus::TeardownFailed,
            DeploymentStatus::Deleted,
        ] {
            assert!(
                !membership_tick_wanted(&state(
                    status,
                    &[("compute", true, ResourceStatus::UpdateFailed)]
                )),
                "{status:?}"
            );
        }
    }
}
