//! AWS Queue — SQS standard queue with managed SSE.
//!
//! Resource-scoped `aws_iam_role_policy` blocks for permission profiles
//! that reference this queue are emitted alongside it; wildcard-scope
//! grants stay on the service-account role via `stack_permission_sets`.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{
        aws_terraform_permission_context, downcast, emit_iam_role_policy_for_target_with_label,
        iam_policy_name_sanitize, required_label, resource_prefix_template, tags,
    },
    emitters::gates::permission_gate_count,
    expr,
};
use alien_core::{
    import::EmitContext, PermissionProfile, PermissionSetReference, Queue, RemoteStackManagement,
    Result, ServiceAccount, Worker, WorkerTrigger,
};
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
        emit_queue_iam(ctx, &mut fragment, label, queue)?;
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

fn emit_queue_iam(
    ctx: &EmitContext<'_>,
    fragment: &mut TfFragment,
    label: &str,
    queue: &Queue,
) -> Result<()> {
    let context = aws_terraform_permission_context()
        .with_resource_name(format!("${{aws_sqs_queue.{label}.name}}"));

    for (owner_label, profile_name, permission_set_refs) in queue_permission_owners(ctx) {
        for (idx, permission_set_ref) in permission_set_refs.iter().enumerate() {
            let Some(permission_set) = permission_set_ref
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            if !permission_set.id.starts_with("queue/")
                || permission_set.id.ends_with("/provision")
            {
                continue;
            }

            let gate_count = match &profile_name {
                Some(profile) => {
                    permission_gate_count(ctx, profile, &permission_set.id, &[ctx.resource_id])?
                }
                None => None,
            };
            let name_prefix = if profile_name.is_some() {
                "access"
            } else {
                "management"
            };
            let queue_label_segment = sanitize_label_segment(queue.id());
            emit_iam_role_policy_for_target_with_label(
                fragment,
                &owner_label,
                &permission_set,
                &format!("{owner_label}_queue_{queue_label_segment}_set_{idx}"),
                &format!(
                    "{name_prefix}-{}-{}",
                    queue.id(),
                    iam_policy_name_sanitize(&permission_set.id)
                ),
                &context,
                BindingTarget::Resource,
                gate_count,
            )?;
        }
    }

    Ok(())
}

fn queue_permission_owners(
    ctx: &EmitContext<'_>,
) -> Vec<(String, Option<String>, Vec<PermissionSetReference>)> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = resource_permission_refs(profile, ctx.resource_id);
        if refs.is_empty() {
            continue;
        }

        let service_account_id = format!("{profile_name}-sa");
        if let Some(label) = service_account_label(ctx, &service_account_id) {
            owners.push((label.to_string(), Some(profile_name.clone()), refs));
        }
    }

    if let Some(profile) = ctx.stack.management().profile() {
        let refs = resource_permission_refs(profile, ctx.resource_id);
        if !refs.is_empty() {
            if let Some(label) = remote_stack_management_label(ctx) {
                owners.push((label.to_string(), None, refs));
            }
        }
    }

    owners
}

fn resource_permission_refs(
    profile: &PermissionProfile,
    resource_id: &str,
) -> Vec<PermissionSetReference> {
    let mut refs = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
    if let Some(resource_refs) = profile.0.get(resource_id) {
        for permission_ref in resource_refs {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref.clone());
            }
        }
    }
    refs
}

fn service_account_label<'a>(
    ctx: &'a EmitContext<'_>,
    service_account_id: &str,
) -> Option<&'a str> {
    let (_id, entry) = ctx
        .stack
        .resources()
        .find(|(id, _entry)| id.as_str() == service_account_id)?;
    entry.config.downcast_ref::<ServiceAccount>()?;
    ctx.name_for(service_account_id)
}

fn remote_stack_management_label<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a str> {
    ctx.stack.resources().find_map(|(id, entry)| {
        if entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE {
            ctx.name_for(id)
        } else {
            None
        }
    })
}

fn sanitize_label_segment(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
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
