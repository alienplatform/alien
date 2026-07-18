//! AWS Queue — SQS standard queue with managed SSE, plus `AWS::IAM::Policy`
//! resources for every permission profile that references a `queue/`
//! permission set on this resource.

use crate::{
    emitter::CfEmitter,
    emitters::aws::{
        helpers::{
            iam_policy_resource, iam_policy_statements, permission_gate_condition,
            remote_stack_management_role_id, required_logical_id, resource_config,
            service_account_role_id, tags,
        },
        service_account::permission_context,
    },
    generator::sanitize_logical_id,
    template::{CfExpression, CfResource},
};
use alien_core::{
    import::EmitContext, PermissionProfile, PermissionSetReference, Queue, Result, Worker,
    WorkerTrigger,
};
use alien_permissions::BindingTarget;
use std::collections::HashSet;

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

        let mut resources = vec![queue];
        resources.extend(queue_iam_policies(ctx, queue_id)?);

        Ok(resources)
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

fn queue_iam_policies(ctx: &EmitContext<'_>, queue_id: &str) -> Result<Vec<CfResource>> {
    let mut resources = Vec::new();
    // `Ref` on an SQS queue is the URL; the ARN needs the name via the
    // GetAtt-in-Sub form.
    let context = permission_context().with_resource_name(format!("${{{queue_id}.QueueName}}"));

    for (profile_name, role_id, permission_refs) in queue_permission_owners(ctx) {
        for permission_ref in permission_refs {
            let Some(permission_set) =
                permission_ref.resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            let Some(statements) = iam_policy_statements(
                &permission_set,
                BindingTarget::Resource,
                &context,
                &format!("queue '{}'", ctx.resource_id),
            )?
            else {
                continue;
            };

            let condition = profile_name.as_deref().and_then(|profile| {
                permission_gate_condition(ctx, profile, &permission_set.id, &[ctx.resource_id])
            });
            let policy_id = format!(
                "{queue_id}{role_id}{}",
                sanitize_logical_id(&permission_set.id)
            );
            let policy_name = CfExpression::sub(format!(
                "${{AWS::StackName}}-{}-{}",
                ctx.resource_id,
                permission_set.id.replace('/', "-")
            ));
            let mut policy =
                iam_policy_resource(policy_id, policy_name, statements, &role_id, condition);
            policy.depends_on.push(queue_id.to_string());
            resources.push(policy);
        }
    }

    Ok(resources)
}

fn queue_permission_owners(
    ctx: &EmitContext<'_>,
) -> Vec<(Option<String>, String, Vec<PermissionSetReference>)> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = queue_permission_refs(profile, ctx.resource_id);
        if refs.is_empty() {
            continue;
        }
        if let Some(role_id) = service_account_role_id(ctx, profile_name) {
            owners.push((Some(profile_name.clone()), role_id, refs));
        }
    }

    if let Some(profile) = ctx.stack.management().profile() {
        let refs = queue_permission_refs(profile, ctx.resource_id);
        if !refs.is_empty() {
            if let Some(role_id) = remote_stack_management_role_id(ctx) {
                owners.push((None, role_id, refs));
            }
        }
    }

    owners
}

fn queue_permission_refs(
    profile: &PermissionProfile,
    resource_id: &str,
) -> Vec<PermissionSetReference> {
    let mut refs = Vec::new();
    let mut seen_ids = HashSet::new();
    let Some(resource_refs) = profile.0.get(resource_id) else {
        return refs;
    };

    for permission_ref in resource_refs {
        let id = permission_ref.id();
        if !id.starts_with("queue/") || id.ends_with("/provision") {
            continue;
        }
        if seen_ids.insert(id.to_string()) {
            refs.push(permission_ref.clone());
        }
    }

    refs
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
