//! Azure ServiceActivation — `azurerm_resource_provider_registration`.
//!
//! `ServiceActivation` resources land in the stack via the
//! `AzureServiceActivationMutation` preflight when the stack contains a
//! workload that depends on a particular Azure resource provider
//! (Microsoft.App for Container Apps, Microsoft.Storage, Microsoft.KeyVault,
//! Microsoft.DocumentDB, …). Each one becomes a registration block so
//! `terraform apply` waits for the RP to reach `Registered` before the
//! dependent resources start.
//!
//! Customers who already have these providers registered (most do)
//! still get a no-op apply: the AzureRM provider's
//! `resource_provider_registration` is idempotent on existing
//! registrations.

use crate::{
    block::{attr, resource_block},
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
        let activation = downcast::<ServiceActivation>(ctx, ServiceActivation::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;

        Ok(TfFragment::default().with_resource(resource_block(
            "azurerm_resource_provider_registration",
            label,
            [attr(
                "name",
                Expression::String(activation.service_name.clone()),
            )],
        )))
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
            ("registered", Expression::Bool(true)),
        ]))
    }
}
