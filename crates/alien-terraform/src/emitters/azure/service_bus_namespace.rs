//! Azure ServiceBusNamespace (auxiliary) — `azurerm_servicebus_namespace`.
//!
//! Preflight-injected as `default-service-bus-namespace`. Every Alien
//! `Queue` resource lands a `azurerm_servicebus_queue` inside this
//! namespace.
//!
//! Standard SKU is the cheapest tier that supports queues (Basic
//! cannot host topics; Premium is overkill for most stacks). Customers
//! who need Premium override at the controller layer; the rebuild's
//! priority is push/pull symmetry, not SKU sweep.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{downcast, required_label, tags},
    expr,
};
use alien_core::{import::EmitContext, AzureServiceBusNamespace, Result};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureServiceBusNamespaceEmitter;

impl TfEmitter for AzureServiceBusNamespaceEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let _ = downcast::<AzureServiceBusNamespace>(ctx, AzureServiceBusNamespace::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;

        Ok(TfFragment::default().with_resource(resource_block(
            "azurerm_servicebus_namespace",
            label,
            [
                attr("name", namespace_name_expr()),
                attr(
                    "resource_group_name",
                    expr::raw("var.azure_resource_group_name"),
                ),
                attr("location", expr::raw("var.azure_location")),
                attr("sku", Expression::String("Standard".to_string())),
                attr("tags", tags(ctx, "service-bus-namespace")),
            ],
        )))
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            ("subscriptionId", expr::raw("var.azure_subscription_id")),
            ("resourceGroup", expr::raw("var.azure_resource_group_name")),
            (
                "namespaceName",
                expr::traversal(["azurerm_servicebus_namespace", label, "name"]),
            ),
            (
                "endpoint",
                expr::template(format!(
                    "${{azurerm_servicebus_namespace.{label}.name}}.servicebus.windows.net"
                )),
            ),
        ]))
    }
}

/// Service Bus namespace names must be globally unique, 6-50 alphanumeric
/// or hyphen characters, and Azure rejects names ending in `-sb`/`-mgmt`.
/// Keep a readable prefix and append a short hash so truncation cannot leave
/// a trailing hyphen or a reserved suffix.
fn namespace_name_expr() -> Expression {
    expr::raw(
        "format(\"%s-bus-%s\", trim(substr(lower(\"a-${local.resource_prefix}\"), 0, 37), \"-\"), substr(sha1(local.resource_prefix), 0, 8))",
    )
}
