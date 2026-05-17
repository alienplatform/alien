//! GCP Queue — Pub/Sub topic plus default subscription.
//!
//! Each Alien Queue maps to one topic + one default pull subscription.
//! Functions triggered by the queue land additional push subscriptions
//! at function-emit time; this file owns only the topic + the
//! controller-readable subscription.

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{downcast, labels, required_label, stack_name_template},
    expr,
};
use alien_core::{import::EmitContext, Worker, WorkerTrigger, Queue, Result};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpQueueEmitter;

impl TfEmitter for GcpQueueEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let queue = downcast::<Queue>(ctx, Queue::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;

        let topic = resource_block(
            "google_pubsub_topic",
            label,
            [
                attr("name", stack_name_template(queue.id())),
                attr("project", expr::raw("var.gcp_project")),
                attr("labels", labels(ctx, "queue")),
            ],
        );

        let ack_deadline = ack_deadline_for(ctx);
        let subscription = resource_block(
            "google_pubsub_subscription",
            label,
            [
                attr(
                    "name",
                    expr::template(format!("${{var.stack_name}}-{}-default", queue.id())),
                ),
                attr("project", expr::raw("var.gcp_project")),
                attr(
                    "topic",
                    expr::traversal(["google_pubsub_topic", label, "id"]),
                ),
                attr(
                    "ack_deadline_seconds",
                    Expression::Number(hcl::Number::from(i64::from(ack_deadline))),
                ),
                attr(
                    "message_retention_duration",
                    Expression::String("604800s".to_string()),
                ),
                attr("enable_message_ordering", Expression::Bool(false)),
                nested(block(
                    "expiration_policy",
                    [attr("ttl", Expression::String("".to_string()))],
                )),
                attr("labels", labels(ctx, "queue")),
            ],
        );

        let mut fragment = TfFragment::default();
        fragment.resource_blocks.push(topic);
        fragment.resource_blocks.push(subscription);
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            ("projectId", expr::raw("var.gcp_project")),
            (
                "topicId",
                expr::traversal(["google_pubsub_topic", label, "name"]),
            ),
            (
                "topicName",
                expr::traversal(["google_pubsub_topic", label, "id"]),
            ),
            (
                "subscriptionId",
                expr::traversal(["google_pubsub_subscription", label, "name"]),
            ),
            (
                "subscriptionName",
                expr::traversal(["google_pubsub_subscription", label, "id"]),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        Ok(Some(expr::object([
            ("service", Expression::String("pubsub".to_string())),
            (
                "topic",
                expr::traversal(["google_pubsub_topic", label, "id"]),
            ),
            (
                "subscription",
                expr::traversal(["google_pubsub_subscription", label, "id"]),
            ),
        ])))
    }
}

fn ack_deadline_for(ctx: &EmitContext<'_>) -> u32 {
    let mut max_function_timeout = 0u32;
    for (_id, entry) in ctx.stack.resources() {
        let Some(function) = entry.config.downcast_ref::<Worker>() else {
            continue;
        };
        if function.triggers.iter().any(|trigger| {
            matches!(
                trigger,
                WorkerTrigger::Queue { queue }
                    if queue.resource_type == Queue::RESOURCE_TYPE && queue.id == ctx.resource_id
            )
        }) {
            max_function_timeout = max_function_timeout.max(function.timeout_seconds);
        }
    }
    if max_function_timeout == 0 {
        return 60;
    }
    // Pub/Sub max ack-deadline is 600 seconds.
    max_function_timeout.saturating_mul(2).clamp(10, 600)
}
