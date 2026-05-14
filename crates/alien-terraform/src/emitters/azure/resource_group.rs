//! Azure ResourceGroup (auxiliary) — `azurerm_resource_group`.
//!
//! Preflight-injected as `default-resource-group`; every Azure resource
//! in the stack lives inside this group. Customers who manage their own
//! resource groups out-of-band point `var.azure_resource_group_name` at
//! the existing one — but in the canonical "Alien provisions everything"
//! mode the generator materialises this resource so the rest of the
//! module compiles cleanly.
//!
//! The emitted block uses the customer-supplied
//! `var.azure_resource_group_name` for the group name itself: that's the
//! same value [`super::helpers::permission_context`] threads into role
//! scopes, so `terraform apply` and any downstream RBAC stay aligned on
//! a single source of truth.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{downcast, required_label, tags},
    expr,
};
use alien_core::{import::EmitContext, AzureResourceGroup, Result};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureResourceGroupEmitter;

impl TfEmitter for AzureResourceGroupEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let _ = downcast::<AzureResourceGroup>(ctx, AzureResourceGroup::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;

        Ok(TfFragment::default().with_resource(resource_block(
            "azurerm_resource_group",
            label,
            [
                attr("name", expr::raw("var.azure_resource_group_name")),
                attr("location", expr::raw("var.azure_location")),
                attr("tags", tags(ctx, "resource-group")),
            ],
        )))
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            ("subscriptionId", expr::raw("var.azure_subscription_id")),
            (
                "resourceGroup",
                expr::traversal(["azurerm_resource_group", label, "name"]),
            ),
            (
                "location",
                expr::traversal(["azurerm_resource_group", label, "location"]),
            ),
        ]))
    }
}
