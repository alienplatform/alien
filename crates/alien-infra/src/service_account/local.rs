use std::time::Duration;
use tracing::info;

use crate::core::ResourceControllerContext;
use crate::error::Result;
use alien_core::{ResourceOutputs, ResourceStatus, ServiceAccount, ServiceAccountOutputs};
use alien_macros::{controller, flow_entry, handler, terminal_state};

/// Local platform ServiceAccount controller.
///
/// According to ALIEN_LOCAL.md section "4. No Permission System":
/// - All resources are accessible without permission checks
/// - No service accounts or roles needed
/// - No authentication required
///
/// This controller is a no-op that immediately succeeds to satisfy the resource
/// lifecycle but doesn't create any actual infrastructure.
#[controller]
pub struct LocalServiceAccountController {
    /// Mock identity for compatibility with outputs (not used for actual access control)
    pub(crate) identity: Option<String>,
}

#[controller]
impl LocalServiceAccountController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        info!(
            service_account_id=%config.id,
            "Creating local service account (no-op - local platform has no permission system)"
        );

        // Generate mock identity for outputs compatibility
        self.identity = Some(format!("local-sa-{}", config.id));

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ────────────────────────────────
    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, _ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        // No-op: local platform doesn't need permission checks or heartbeats
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        info!(
            service_account_id=%config.id,
            "Updating local service account (no-op)"
        );

        // No actual updates needed for local service accounts
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────
    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        info!(
            service_account_id=%config.id,
            "Deleting local service account (no-op)"
        );

        self.identity = None;

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── TERMINALS ────────────────────────────────
    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );

    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );

    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);

    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        self.identity.as_ref().map(|identity| {
            ResourceOutputs::new(ServiceAccountOutputs {
                identity: identity.clone(),
                resource_id: identity.clone(),
            })
        })
    }

    fn get_binding_params(&self) -> Option<serde_json::Value> {
        // Local platform doesn't use service account bindings
        // All resources are accessible without authentication
        None
    }
}

impl LocalServiceAccountController {
    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(service_account_id: &str) -> Self {
        Self {
            state: LocalServiceAccountState::Ready,
            identity: Some(format!("local-sa-{}", service_account_id)),
            _internal_stay_count: None,
        }
    }
}
