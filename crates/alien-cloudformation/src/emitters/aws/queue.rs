//! AWS Queue — SQS standard queue with managed SSE.

use crate::{
    emitter::CfEmitter,
    emitters::aws::helpers::{required_logical_id, resource_config, tags},
    template::{CfExpression, CfResource},
};
use alien_core::{import::EmitContext, Worker, WorkerTrigger, Queue, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsQueueEmitter;

impl CfEmitter for AwsQueueEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        resource_config::<Queue>(ctx, Queue::RESOURCE_TYPE)?;
        let queue_id = required_logical_id(ctx)?;
        let mut queue = CfResource::new(queue_id.to_string(), "AWS::SQS::Queue".to_string());

        queue
            .properties
            .insert("SqsManagedSseEnabled".to_string(), CfExpression::from(true));
        queue.properties.insert(
            "VisibilityTimeout".to_string(),
            CfExpression::Integer(i64::from(visibility_timeout(ctx))),
        );
        queue.properties.insert(
            "MessageRetentionPeriod".to_string(),
            CfExpression::Integer(345_600),
        );
        queue.properties.insert("Tags".to_string(), tags(ctx));

        Ok(vec![queue])
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        let queue_id = required_logical_id(ctx)?;
        Ok(CfExpression::object([
            ("queueName", CfExpression::get_att(queue_id, "QueueName")),
            ("queueUrl", CfExpression::ref_(queue_id)),
            ("queueArn", CfExpression::get_att(queue_id, "Arn")),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<CfExpression>> {
        let queue_id = required_logical_id(ctx)?;
        Ok(Some(CfExpression::object([
            ("service", CfExpression::from("sqs")),
            ("queueUrl", CfExpression::ref_(queue_id)),
        ])))
    }
}

/// SQS visibility timeout = max function timeout × 6, clamped to
/// `[30, 43200]`. Falls back to 30s when no consumer is wired.
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
