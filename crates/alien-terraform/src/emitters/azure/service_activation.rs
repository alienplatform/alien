//! Azure ServiceActivation - import metadata for Azure resource providers.
//!
//! `ServiceActivation` resources land in the stack via the
//! `AzureServiceActivationMutation` preflight when the stack contains a
//! workload that depends on a particular Azure resource provider
//! (Microsoft.App for Container Apps, Microsoft.Storage, Microsoft.KeyVault,
//! Microsoft.DocumentDB, ...).
//! The AzureRM provider registers required resource providers itself. Emitting
//! `azurerm_resource_provider_registration` conflicts with subscriptions where
//! the provider is already registered, so Terraform distributions only report
//! the activation in Alien import metadata.

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
            ("registered", Expression::Bool(true)),
        ]))
    }
}
