//! AWS Email (SES) controller.
//!
//! Email is a **Frozen-only** resource: the setup stack (CloudFormation) owns
//! the configuration set, the seed identities, and the inbound/event wiring
//! end to end, and deployments only ever receive it through stack import
//! (see [`crate::email::AwsEmailImporter`]). This controller therefore has no
//! provisioning flow — its create entry fails fast — and exists to carry the
//! imported state through the deployment loop and expose
//! [`alien_core::EmailOutputs`].
//!
//! The `Ready` handler is passive: there is no SES client in
//! `alien-aws-clients` yet, so no liveness probe (`ses:GetConfigurationSet`,
//! covered by the `email/management` permission set) is performed. Adding an
//! active heartbeat is a follow-up that lands together with an SES client.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{Email, EmailDomainOutputs, EmailOutputs, ResourceOutputs, ResourceStatus};
use alien_error::AlienError;
use alien_macros::controller;

/// How often the passive `Ready` handler re-runs. There is no cloud probe
/// behind it, so a long interval avoids pointless churn in the refresh loop.
const READY_REFRESH_INTERVAL: Duration = Duration::from_secs(300);

#[controller]
pub struct AwsEmailController {
    /// SES configuration set name (used when sending).
    pub(crate) configuration_set: Option<String>,
    /// Per-seed-domain DNS records the operator must create, keyed by domain.
    pub(crate) domains: BTreeMap<String, EmailDomainOutputs>,
    /// SES receipt rule set name, when inbound mail is configured.
    pub(crate) rule_set_name: Option<String>,
    /// Deployment region recorded at import time. Absent on states imported
    /// before this field existed; the binding then omits `region` and
    /// consumers fall back to their runtime's own region.
    #[serde(default)]
    pub(crate) region: Option<String>,
}

#[controller]
impl AwsEmailController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    //
    // Email is setup-owned; runtime provisioning is not supported. The flow
    // entry exists because every controller needs one, and it fails fast so
    // a mis-lifecycled resource surfaces as a clear error instead of a
    // half-provisioned SES topology.
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Email>()?;
        Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            resource_id: Some(config.id.clone()),
            message: "email resources are setup-owned (Frozen): they are provisioned by the \
                      setup stack and enter a deployment only through stack import"
                .to_string(),
        }))
    }

    // ─────────────── READY STATE ────────────────────────────────
    /// Passive refresh: no SES client exists in `alien-aws-clients` yet, so
    /// this performs no liveness probe. See the module docs.
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
        self.configuration_set.as_ref().map(|configuration_set| {
            ResourceOutputs::new(EmailOutputs {
                domains: self.domains.clone(),
                configuration_set: configuration_set.clone(),
                rule_set_name: self.rule_set_name.clone(),
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        // Mirrors the CloudFormation emitter's binding ref: service "ses" plus
        // the configuration set; `region` is included when known.
        let Some(configuration_set) = &self.configuration_set else {
            return Ok(None);
        };
        let mut binding = serde_json::json!({
            "service": "ses",
            "configurationSet": configuration_set,
        });
        if let Some(region) = &self.region {
            binding["region"] = serde_json::Value::String(region.clone());
        }
        Ok(Some(binding))
    }
}
