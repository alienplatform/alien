//! Azure ServiceActivation - import metadata for Azure resource providers.
//!
//! `ServiceActivation` resources land in the stack via the
//! `AzureServiceActivationMutation` preflight when the stack contains a
//! workload that depends on a particular Azure resource provider
//! (Microsoft.App for Container Apps, Microsoft.Storage, Microsoft.KeyVault,
//! Microsoft.DocumentDB, ...).
//! Setup registers nothing: the rendered `providers.tf` sets
//! `resource_provider_registrations = "none"`, and the runtime holds no Azure grant for
//! `providers/register/action` — the sets carry a GCP block alone. So the module names the
//! namespace as a prerequisite the operator satisfies, and this reports it unverified. The
//! controller reads the real state on every refresh and raises drift if it is not registered.

use crate::{
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{downcast, required_label},
    expr,
};
use alien_core::{import::EmitContext, Result, ServiceActivation};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureServiceActivationEmitter;

impl TfEmitter for AzureServiceActivationEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let _ = downcast::<ServiceActivation>(ctx, ServiceActivation::RESOURCE_TYPE)?;
        Ok(TfFragment::empty())
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let activation = downcast::<ServiceActivation>(ctx, ServiceActivation::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let _ = label;
        Ok(expr::object([
            ("subscriptionId", expr::raw("var.azure_subscription_id")),
            (
                "providerNamespace",
                Expression::String(activation.service_name.clone()),
            ),
            ("registered", Expression::Bool(false)),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{ResourceLifecycle, Stack, StackSettings};
    use indexmap::IndexMap;

    /// The payload is what the manager records, and setup registers nothing — the rendered
    /// provider block says `resource_provider_registrations = "none"` and no Azure grant covers
    /// `providers/register/action`. Reporting a registration here would tell the runtime a
    /// subscription state nothing established, and the first resource needing the namespace would
    /// fail with `MissingSubscriptionRegistration` while the payload said otherwise.
    #[test]
    fn the_import_payload_does_not_claim_a_registration_setup_never_made() {
        let stack = Stack::new("acme".to_string())
            .add(
                ServiceActivation::new("enable-app".to_string())
                    .service_name("Microsoft.App".to_string())
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let resource = stack
            .resources
            .get("enable-app")
            .expect("the activation is in the stack");
        let names = IndexMap::from([("enable-app".to_string(), "enable_app".to_string())]);
        let settings = StackSettings::default();
        let ctx = EmitContext {
            stack: &stack,
            resource,
            resource_id: "enable-app",
            platform: alien_core::Platform::Azure,
            targets_kubernetes: false,
            stack_settings: &settings,
            names: &names,
        };

        let rendered = AzureServiceActivationEmitter
            .emit_import_ref(&ctx)
            .expect("the activation renders an import ref")
            .to_string();

        assert!(
            rendered.contains("registered = false"),
            "the payload must not assert a registration setup did not perform: {rendered}"
        );
        assert!(rendered.contains("Microsoft.App"));
    }
}
