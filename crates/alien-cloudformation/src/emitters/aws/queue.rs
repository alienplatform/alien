//! AWS Queue — SQS standard queue with managed SSE.
//!
//! When a service-account permission profile grants `queue/*` permission sets
//! for this queue (directly by resource id or through a `*` wildcard), the
//! matching IAM policies are emitted against the owning roles, with every
//! statement pinned to this queue's ARN (the physical queue name is
//! CloudFormation-generated, so name-pattern bindings cannot match it).

use crate::{
    emitter::CfEmitter,
    emitters::aws::{
        helpers::{
            cf_from_json, required_logical_id, resource_config, service_account_role_id, tags,
            uniquify_iam_statement_sids,
        },
        service_account::permission_context,
    },
    template::{CfExpression, CfResource},
};
use alien_core::{
    import::EmitContext, ErrorData, PermissionProfile, PermissionSetReference, Queue, Result,
    ServiceAccount, Worker, WorkerTrigger,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_permissions::{generators::AwsCloudFormationPermissionsGenerator, BindingTarget};

/// Permission-set id prefix for this resource type.
const PERMISSION_SET_PREFIX: &str = "queue/";

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsQueueEmitter;

impl CfEmitter for AwsQueueEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let queue = resource_config::<Queue>(ctx, Queue::RESOURCE_TYPE)?;
        let queue_id = required_logical_id(ctx)?;
        let mut queue_resource =
            CfResource::new(queue_id.to_string(), "AWS::SQS::Queue".to_string());

        queue_resource
            .properties
            .insert("SqsManagedSseEnabled".to_string(), CfExpression::from(true));
        queue_resource.properties.insert(
            "VisibilityTimeout".to_string(),
            CfExpression::Integer(i64::from(visibility_timeout(ctx))),
        );
        queue_resource.properties.insert(
            "MessageRetentionPeriod".to_string(),
            CfExpression::Integer(345_600),
        );
        queue_resource.properties.insert("Tags".to_string(), tags(ctx));

        let mut resources = vec![queue_resource];
        resources.extend(queue_iam_policies(ctx, queue, queue_id)?);
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

/// IAM policies attaching granted `queue/*` permission sets to the owning
/// service-account roles, scoped to this queue's ARN.
fn queue_iam_policies(
    ctx: &EmitContext<'_>,
    queue: &Queue,
    queue_id: &str,
) -> Result<Vec<CfResource>> {
    let mut resources = Vec::new();
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let context =
        permission_context().with_resource_name(format!("${{AWS::StackName}}-{}", queue.id()));

    for (owner_index, (role_id, permission_refs)) in permission_owners(ctx).into_iter().enumerate()
    {
        for (permission_index, permission_ref) in permission_refs.iter().enumerate() {
            let Some(permission_set) =
                permission_ref.resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            if !permission_set.id.starts_with(PERMISSION_SET_PREFIX) {
                continue;
            }

            let policy = generator
                .generate_policy(&permission_set, BindingTarget::Resource, &context)
                .context(ErrorData::GenericError {
                    message: format!(
                        "failed to generate AWS CloudFormation queue IAM policy for '{}'",
                        queue.id()
                    ),
                })?;
            let policy_value = serde_json::to_value(policy).into_alien_error().context(
                ErrorData::TemplateSerializationFailed {
                    format: "CloudFormation IAM policy".to_string(),
                    reason: "Failed to serialize IAM policy".to_string(),
                },
            )?;
            let CfExpression::Object(mut policy_object) = cf_from_json(policy_value)? else {
                return Err(AlienError::new(ErrorData::TemplateSerializationFailed {
                    format: "CloudFormation IAM policy".to_string(),
                    reason: "policy did not serialize to a JSON object".to_string(),
                }));
            };
            let Some(CfExpression::List(policy_statements)) =
                policy_object.shift_remove("Statement")
            else {
                continue;
            };
            // The physical queue name carries a CloudFormation-generated
            // suffix, so name-pattern resource bindings cannot match it; pin
            // every statement to this queue's ARN.
            let policy_statements = policy_statements
                .into_iter()
                .map(|statement| pin_statement_to_queue(statement, queue_id))
                .collect::<Vec<_>>();

            let policy_id =
                format!("{queue_id}{role_id}QueuePermission{owner_index}{permission_index}");
            let mut policy_resource = CfResource::new(policy_id, "AWS::IAM::Policy".to_string());
            policy_resource.properties.insert(
                "PolicyName".to_string(),
                CfExpression::sub(format!(
                    "${{AWS::StackName}}-{}-queue-{owner_index}-{permission_index}",
                    queue.id()
                )),
            );
            policy_resource.properties.insert(
                "PolicyDocument".to_string(),
                CfExpression::object([
                    ("Version", CfExpression::from("2012-10-17")),
                    (
                        "Statement",
                        CfExpression::list(uniquify_iam_statement_sids(policy_statements)),
                    ),
                ]),
            );
            policy_resource.properties.insert(
                "Roles".to_string(),
                CfExpression::list([CfExpression::ref_(&role_id)]),
            );
            policy_resource.depends_on.push(queue_id.to_string());
            policy_resource.depends_on.push(role_id.clone());
            resources.push(policy_resource);
        }
    }

    Ok(resources)
}

fn pin_statement_to_queue(statement: CfExpression, queue_id: &str) -> CfExpression {
    let CfExpression::Object(mut statement_object) = statement else {
        return statement;
    };
    statement_object.insert(
        "Resource".to_string(),
        CfExpression::get_att(queue_id, "Arn"),
    );
    CfExpression::Object(statement_object)
}

/// Service-account roles whose permission profile references a `queue/*`
/// permission set for this resource (either directly by resource id or
/// through a `*` wildcard grant).
fn permission_owners(ctx: &EmitContext<'_>) -> Vec<(String, Vec<PermissionSetReference>)> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = resource_permission_refs(profile, ctx.resource_id);
        if refs.is_empty() {
            continue;
        }

        let service_account_id = format!("{profile_name}-sa");
        if service_account_for_id(ctx, &service_account_id).is_some() {
            if let Some(role_id) = service_account_role_id(ctx, profile_name) {
                owners.push((role_id, refs));
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
        for permission_ref in resource_refs
            .iter()
            .filter(|permission_ref| permission_ref.id().starts_with(PERMISSION_SET_PREFIX))
        {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref.clone());
            }
        }
    }

    if let Some(wildcard_refs) = profile.0.get("*") {
        for permission_ref in wildcard_refs
            .iter()
            .filter(|permission_ref| permission_ref.id().starts_with(PERMISSION_SET_PREFIX))
        {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref.clone());
            }
        }
    }

    refs
}

fn service_account_for_id<'a>(
    ctx: &'a EmitContext<'_>,
    service_account_id: &str,
) -> Option<&'a ServiceAccount> {
    let (_id, entry) = ctx
        .stack
        .resources()
        .find(|(id, _entry)| id.as_str() == service_account_id)?;
    entry.config.downcast_ref::<ServiceAccount>()
}
