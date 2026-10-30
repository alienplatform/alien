//! AWS Queue — SQS standard queue with managed SSE.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{
        downcast, emit_iam_role_policy_for_target_with_label, emit_workload_resource_grants,
        required_label, resource_prefix_template, stack_permission_context, tags,
    },
    expr,
};
use alien_core::{
    import::EmitContext, ErrorData, PermissionSetReference, Queue, RemoteBindings,
    RemoteStackManagement, Result, Worker, WorkerTrigger,
};
use alien_error::AlienError;
use alien_permissions::BindingTarget;
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsQueueEmitter;

impl TfEmitter for AwsQueueEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let queue = downcast::<Queue>(ctx, Queue::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;

        let q = resource_block(
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

        let mut fragment = TfFragment::default().with_resource(q);
        if alien_core::storage_notifications::queue_receives_storage_notifications(
            ctx.stack,
            ctx.resource_id,
        ) {
            fragment
                .resource_blocks
                .push(super::storage_notifications::queue_policy(label));
        }
        if ctx.stack.management().profile().is_some_and(|profile| {
            profile.0.get(ctx.resource_id).is_some_and(|grants| {
                grants.iter().any(|grant| {
                    matches!(grant, PermissionSetReference::Name(name) if name == "queue/batch-metadata")
                })
            })
        }) {
            let role_label = ctx
                .stack
                .resources()
                .find_map(|(id, entry)| {
                    (entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE)
                        .then(|| ctx.name_for(id))
                        .flatten()
                })
                .ok_or_else(|| AlienError::new(ErrorData::GenericError {
                    message: "Queue metadata access requires a setup management role".into(),
                }))?;
            let permission = alien_permissions::get_permission_set("queue/batch-metadata")
                .ok_or_else(|| AlienError::new(ErrorData::GenericError {
                    message: "Queue batch metadata permission is not registered".into(),
                }))?;
            let context = stack_permission_context(ctx)
                .with_resource_name(format!("${{aws_sqs_queue.{label}.name}}"));
            // Attach after the queue exists, outside the bootstrap policy graph.
            emit_iam_role_policy_for_target_with_label(
                &mut fragment,
                role_label,
                permission,
                &format!("{label}_batch_metadata"),
                &format!("{label}-batch-metadata"),
                &context,
                BindingTarget::Resource,
            )?;
        }
        if let Some(definition) =
            alien_core::remote_bindings::remote_binding_for_entry(ctx.resource)
        {
            if let Some(access_label) = ctx.stack.resources().find_map(|(id, entry)| {
                (entry.config.resource_type() == RemoteBindings::RESOURCE_TYPE)
                    .then(|| ctx.name_for(id))
                    .flatten()
            }) {
                let permission_set = alien_permissions::get_permission_set(
                    definition.permission_set,
                )
                .ok_or_else(|| {
                    AlienError::new(ErrorData::GenericError {
                        message: format!(
                            "Remote Queue permission set {} is not registered",
                            definition.permission_set
                        ),
                    })
                })?;
                let context = stack_permission_context(ctx)
                    .with_resource_name(format!("${{aws_sqs_queue.{label}.name}}"));
                emit_iam_role_policy_for_target_with_label(
                    &mut fragment,
                    access_label,
                    permission_set,
                    &format!("{label}_remote_access"),
                    &format!("access-{}", queue.id()),
                    &context,
                    BindingTarget::Resource,
                )?;
            }
        }
        emit_workload_resource_grants(ctx, &mut fragment, "queue/")?;
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            (
                "queueName",
                expr::traversal(["aws_sqs_queue", label, "name"]),
            ),
            ("queueUrl", expr::traversal(["aws_sqs_queue", label, "url"])),
            ("queueArn", expr::traversal(["aws_sqs_queue", label, "arn"])),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        Ok(Some(expr::object([
            ("service", Expression::String("sqs".to_string())),
            ("queueUrl", expr::traversal(["aws_sqs_queue", label, "url"])),
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
                WorkerTrigger::Queue { queue, .. }
                    if queue.resource_type == Queue::RESOURCE_TYPE && queue.id == ctx.resource_id
            )
        }) {
            max_function_timeout = max_function_timeout.max(
                function.timeout_seconds.saturating_mul(6).saturating_add(
                    function
                        .queue_batch_config(ctx.resource_id)
                        .map_or(0, |batch| batch.max_window_seconds),
                ),
            );
        }
    }
    if max_function_timeout == 0 {
        return 30;
    }
    max_function_timeout.clamp(30, 43_200)
}
