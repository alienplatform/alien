//! AWS OpenSearch (`experimental/aws-opensearch`) controller.
//!
//! The OpenSearch Serverless collection is a **Frozen-only** resource: the
//! setup stack (CloudFormation) owns the collection group, the collection,
//! and its network/data-access policies end to end, and deployments only
//! ever receive it through stack import (see
//! [`crate::open_search::AwsOpenSearchImporter`]). This controller therefore
//! has no provisioning flow — its create entry fails fast — and exists to
//! carry the imported state through the deployment loop and expose
//! [`alien_core::AwsOpenSearchOutputs`].
//!
//! The `Ready` handler is passive: there is no OpenSearch Serverless client
//! in `alien-aws-clients` yet, so no liveness probe
//! (`aoss:BatchGetCollection`) is performed. Adding an active heartbeat is a
//! follow-up that lands together with an AOSS client.

use std::time::Duration;

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{AwsOpenSearch, AwsOpenSearchOutputs, ResourceOutputs, ResourceStatus};
use alien_error::AlienError;
use alien_macros::controller;

/// How often the passive `Ready` handler re-runs. There is no cloud probe
/// behind it, so a long interval avoids pointless churn in the refresh loop.
const READY_REFRESH_INTERVAL: Duration = Duration::from_secs(300);

#[controller]
pub struct AwsOpenSearchController {
    /// Physical collection name (`{id}-{stack-suffix}`).
    pub(crate) collection_name: Option<String>,
    /// Server-assigned collection id.
    pub(crate) collection_id: Option<String>,
    /// ARN of the collection.
    pub(crate) collection_arn: Option<String>,
    /// Collection endpoint (SigV4-signed with service name `aoss`).
    pub(crate) endpoint: Option<String>,
}

#[controller]
impl AwsOpenSearchController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    //
    // The collection is setup-owned; runtime provisioning is not supported.
    // The flow entry exists because every controller needs one, and it fails
    // fast so a mis-lifecycled resource surfaces as a clear error.
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<AwsOpenSearch>()?;
        Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            resource_id: Some(config.id.clone()),
            message: "experimental/aws-opensearch resources are setup-owned (Frozen): they are \
                      provisioned by the setup stack and enter a deployment only through stack \
                      import"
                .to_string(),
        }))
    }

    // ─────────────── READY STATE ────────────────────────────────
    /// Passive refresh: no OpenSearch Serverless client exists in
    /// `alien-aws-clients` yet, so this performs no liveness probe. See the
    /// module docs.
    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, _ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(READY_REFRESH_INTERVAL),
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

    // Unreachable through any flow (there is no Delete flow — setup owns
    // deletion of this Frozen resource), but the controller macro's
    // `get_outputs` matches on a `Deleted` variant, so it must exist.
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        if let (Some(endpoint), Some(collection_arn)) = (&self.endpoint, &self.collection_arn) {
            Some(ResourceOutputs::new(AwsOpenSearchOutputs {
                endpoint: endpoint.clone(),
                collection_arn: collection_arn.clone(),
            }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        // Mirrors the CloudFormation emitter's binding ref: service "aoss",
        // the SigV4-signed collection endpoint, and the physical name.
        let (Some(endpoint), Some(collection_name)) = (&self.endpoint, &self.collection_name)
        else {
            return Ok(None);
        };
        Ok(Some(serde_json::json!({
            "service": "aoss",
            "endpoint": endpoint,
            "collectionName": collection_name,
        })))
    }
}
