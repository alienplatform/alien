//! Azure Queue — `azurerm_servicebus_queue` inside the stack's
//! `azurerm_servicebus_namespace`.
//!
//! Mirrors `AzureQueueController`:
//!
//! * Queue name = `${var.stack_name}-{id}`, matching
//!   [`super::helpers::stack_name_template`].
//! * Default lock duration / TTL / partitioning — the controller leaves
//!   them at provider defaults too; rebuild stays consistent.
//! * Parent `azurerm_servicebus_namespace` is preflight-injected as
//!   `default-service-bus-namespace`. The auxiliary
//!   [`super::service_bus_namespace::AzureServiceBusNamespaceEmitter`]
//!   realises it.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{downcast, required_label, stack_name_template},
    expr,
};
use alien_core::{
    import::EmitContext, AzureServiceBusNamespace, ErrorData, Function, FunctionTrigger, Queue,
    Result,
};
use alien_error::AlienError;
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureQueueEmitter;

impl TfEmitter for AzureQueueEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let queue = downcast::<Queue>(ctx, Queue::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let parent_label = parent_namespace_label(ctx)?.to_string();

        let lock_duration = lock_duration_for(ctx);

        let q = resource_block(
            "azurerm_servicebus_queue",
            label,
            [
                attr("name", stack_name_template(queue.id())),
                attr(
                    "namespace_id",
                    expr::traversal(["azurerm_servicebus_namespace", &parent_label, "id"]),
                ),
                attr("partitioning_enabled", Expression::Bool(false)),
                attr(
                    "lock_duration",
                    Expression::String(format!("PT{}S", lock_duration)),
                ),
                attr(
                    "default_message_ttl",
                    Expression::String("P14D".to_string()),
                ),
                attr(
                    "max_delivery_count",
                    Expression::Number(hcl::Number::from(10i64)),
                ),
                attr(
                    "dead_lettering_on_message_expiration",
                    Expression::Bool(true),
                ),
            ],
        );

        Ok(TfFragment::default().with_resource(q))
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        let parent_label = parent_namespace_label(ctx)?.to_string();
        Ok(expr::object([
            ("subscriptionId", expr::raw("var.azure_subscription_id")),
            ("resourceGroup", expr::raw("var.azure_resource_group_name")),
            (
                "namespaceName",
                expr::traversal(["azurerm_servicebus_namespace", &parent_label, "name"]),
            ),
            (
                "queueName",
                expr::traversal(["azurerm_servicebus_queue", label, "name"]),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        let parent_label = parent_namespace_label(ctx)?.to_string();
        Ok(Some(expr::object([
            ("service", Expression::String("servicebus".to_string())),
            (
                "namespace",
                expr::traversal(["azurerm_servicebus_namespace", &parent_label, "name"]),
            ),
            (
                "queueName",
                expr::traversal(["azurerm_servicebus_queue", label, "name"]),
            ),
        ])))
    }
}

fn parent_namespace_label<'a>(ctx: &EmitContext<'a>) -> Result<&'a str> {
    for (id, entry) in ctx.stack.resources() {
        if entry
            .config
            .downcast_ref::<AzureServiceBusNamespace>()
            .is_some()
        {
            if let Some(label) = ctx.name_for(id) {
                return Ok(label);
            }
        }
    }
    Err(AlienError::new(ErrorData::GenericError {
        message:
            "Azure Queue resource requires a sibling `azure_service_bus_namespace` resource in \
             the stack (preflight-injected as `default-service-bus-namespace`)"
                .to_string(),
    }))
}

/// Service Bus queue lock duration must be in `[5s, 5m]`. Use the
/// max-consumer-function timeout × 2, clamped to the supported range.
fn lock_duration_for(ctx: &EmitContext<'_>) -> u32 {
    let mut max_function_timeout = 0u32;
    for (_id, entry) in ctx.stack.resources() {
        let Some(function) = entry.config.downcast_ref::<Function>() else {
            continue;
        };
        if function.triggers.iter().any(|trigger| {
            matches!(
                trigger,
                FunctionTrigger::Queue { queue }
                    if queue.resource_type == Queue::RESOURCE_TYPE && queue.id == ctx.resource_id
            )
        }) {
            max_function_timeout = max_function_timeout.max(function.timeout_seconds);
        }
    }
    if max_function_timeout == 0 {
        return 30;
    }
    max_function_timeout.saturating_mul(2).clamp(5, 300)
}
