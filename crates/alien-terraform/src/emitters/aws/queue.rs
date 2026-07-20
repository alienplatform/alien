//! AWS Queue — SQS standard queue with managed SSE.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{downcast, required_label, resource_prefix_template, tags},
    emitters::enabled,
    expr,
};
use alien_core::{import::EmitContext, Queue, Result, Worker, WorkerTrigger};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsQueueEmitter;

impl TfEmitter for AwsQueueEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let queue = downcast::<Queue>(ctx, Queue::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let enabled_when = ctx.resource.enabled_when.as_deref();

        let mut q = resource_block(
            "aws_sqs_queue",
            label,
            [
                attr("name", resource_prefix_template(queue.id())),
                attr("sqs_managed_sse_enabled", Expression::Bool(true)),
                attr(
                    "visibility_timeout_seconds",
                    Expression::Number(hcl::Number::from(i64::from(visibility_timeout(ctx)))),
                ),
                attr(
                    "message_retention_seconds",
                    Expression::Number(hcl::Number::from(345_600i64)),
                ),
                attr("tags", tags(ctx, "queue")),
            ],
        );
        enabled::gate(&mut q, enabled_when)?;

        Ok(TfFragment::default().with_resource(q))
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        let enabled_when = ctx.resource.enabled_when.as_deref();
        Ok(expr::object([
            (
                "queueName",
                enabled::attribute(enabled_when, "aws_sqs_queue", label, "name"),
            ),
            (
                "queueUrl",
                enabled::attribute(enabled_when, "aws_sqs_queue", label, "url"),
            ),
            (
                "queueArn",
                enabled::attribute(enabled_when, "aws_sqs_queue", label, "arn"),
            ),
        ]))
    }

    fn supports_enabled_when(&self) -> bool {
        true
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        let enabled_when = ctx.resource.enabled_when.as_deref();
        Ok(Some(expr::object([
            ("service", Expression::String("sqs".to_string())),
            (
                "queueUrl",
                enabled::attribute(enabled_when, "aws_sqs_queue", label, "url"),
            ),
        ])))
    }
}

fn visibility_timeout(ctx: &EmitContext<'_>) -> u32 {
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
        return 30;
    }
    max_function_timeout.saturating_mul(6).clamp(30, 43_200)
}
