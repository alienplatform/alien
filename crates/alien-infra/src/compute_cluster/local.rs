//! Local ComputeCluster controller.
//!
//! On the Local platform, ComputeCluster is minimal - it just ensures
//! the Docker network exists. There's no machine provisioning, autoscaling,
//! or managed container control plane integration.

use std::time::Duration;
use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{
    ComputeCluster, ComputeClusterHeartbeatData, ComputeClusterHeartbeatStatus,
    ComputeClusterOutputs, HeartbeatBackend, LocalComputeClusterHeartbeatData, ObservedCounts,
    ObservedHealth, ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData,
    ResourceOutputs as CoreResourceOutputs, ResourceStatus,
};
use alien_error::{AlienError, Context};
use alien_local::LocalRuntimeStatus;
use alien_macros::controller;
use chrono::Utc;

const LOCAL_NETWORK_NAME: &str = "deployment-network";

/// Local ComputeCluster controller.
///
/// On the Local platform, this controller:
/// - Ensures the Docker network exists (via LocalContainerManager)
/// - Stores the network name for container controllers to use
///
/// Unlike cloud platforms, there's no:
/// - Machine provisioning (ASGs, VMs)
/// - Managed container cluster creation
/// - Machine autoscaling
#[controller]
pub struct LocalComputeClusterController {
    /// Docker network name (always "deployment-network" for local)
    pub(crate) network_name: Option<String>,
}

#[controller]
impl LocalComputeClusterController {
    // ─────────────── CREATE FLOW ───────────────────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = EnsureNetwork,
        on_failure = ProvisionFailed,
        status = ResourceStatus::Provisioning
    )]
    async fn ensure_network(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ComputeCluster>()?;

        info!(cluster_id = %config.id, "Setting up local container cluster");

        // Get the container manager
        let container_mgr = ctx
            .service_provider
            .get_local_container_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "LocalContainerManager".to_string(),
                })
            })?;

        // Ensure Docker network exists
        container_mgr
            .ensure_network()
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to ensure Docker network".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.network_name = Some(LOCAL_NETWORK_NAME.to_string());

        info!(
            cluster_id = %config.id,
            network = LOCAL_NETWORK_NAME,
            "Local container cluster ready"
        );

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ComputeCluster>()?;

        // Verify Docker daemon is still accessible
        let container_mgr = ctx
            .service_provider
            .get_local_container_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "LocalContainerManager".to_string(),
                })
            })?;

        // Re-ensure network exists (idempotent)
        container_mgr
            .ensure_network()
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Docker network health check failed".to_string(),
                resource_id: Some(config.id.clone()),
            })?;
        let runtime_status =
            container_mgr
                .runtime_status()
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Docker runtime status check failed".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;
        emit_local_compute_cluster_heartbeat(ctx, &config.id, runtime_status);

        debug!(cluster_id = %config.id, "Container cluster health check passed");

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────────────────

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdatingCluster,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating
    )]
    async fn updating_cluster(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ComputeCluster>()?;

        info!(cluster_id = %config.id, "Updating local container cluster (no-op on local)");

        // On local platform, updates are no-op - just ensure network still exists
        let container_mgr = ctx
            .service_provider
            .get_local_container_manager()
            .ok_or_else(|| {
                AlienError::new(ErrorData::LocalServicesNotAvailable {
                    service_name: "LocalContainerManager".to_string(),
                })
            })?;

        container_mgr
            .ensure_network()
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to ensure Docker network during update".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────────────────

    #[flow_entry(Delete)]
    #[handler(
        state = Deleting,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting
    )]
    async fn deleting(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ComputeCluster>()?;

        info!(cluster_id = %config.id, "Deleting local container cluster");

        // On local platform, we don't remove the Docker network
        // because other containers/resources might be using it.
        // The network is shared across all agents.

        debug!(
            cluster_id = %config.id,
            "Docker network preserved (shared resource)"
        );

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── TERMINAL STATES ──────────────────────────────────────

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);
    terminal_state!(
        state = ProvisionFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );

    // ─────────────── HELPER METHODS ──────────────────────────────────────

    fn build_outputs(&self) -> Option<CoreResourceOutputs> {
        self.network_name.as_ref().map(|_network| {
            CoreResourceOutputs::new(ComputeClusterOutputs {
                // On local platform, we use Docker directly.
                cluster_id: "local-docker".to_string(),
                horizon_ready: true,                 // Local is always "ready"
                capacity_group_statuses: Vec::new(), // No capacity groups on local
                total_machines: 1,                   // Single Docker host
            })
        })
    }
}

fn emit_local_compute_cluster_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    runtime_status: LocalRuntimeStatus,
) {
    let host_identifier = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok();

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: ComputeCluster::RESOURCE_TYPE,
        controller_platform: alien_core::Platform::Local,
        backend: HeartbeatBackend::Local,
        source: Default::default(),
        alien_resource_id: None,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::ComputeCluster(ComputeClusterHeartbeatData::Local(
            LocalComputeClusterHeartbeatData {
                status: ComputeClusterHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: None,
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                nodes: ObservedCounts {
                    desired: Some(1),
                    current: Some(1),
                    ready: Some(1),
                },
                name: resource_id.to_string(),
                host_identifier,
                docker_available: true,
                docker_version: runtime_status.docker_version,
                docker_api_version: runtime_status.docker_api_version,
                docker_os: runtime_status.docker_os,
                docker_arch: runtime_status.docker_arch,
                network_name: Some(LOCAL_NETWORK_NAME.to_string()),
                network_available: true,
                tracked_containers: Some(runtime_status.tracked_containers),
                running_containers: Some(runtime_status.running_containers),
            },
        )),
        raw: vec![],
    });
}
