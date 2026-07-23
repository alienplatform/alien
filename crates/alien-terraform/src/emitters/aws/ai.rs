//! AWS AI — Bedrock inference gateway.
//!
//! AWS Bedrock is a regional, account-scoped service with no per-stack
//! cloud resource to provision. The emitter returns an empty fragment for
//! the resource itself and emits any resource-scoped `aws_iam_role_policy`
//! blocks for permission profiles that reference an `ai/*` set (e.g.
//! `ai/invoke`, or `ai/finetune` when the resource declares a fine-tuning
//! job) on this resource. The region is carried in the import ref so the
//! controller can reconstruct the Bedrock endpoint without a cloud round-trip.
//!
//! Stack-level permissions flow through `AwsServiceAccountEmitter` via
//! `stack_permission_sets`; resource-scoped grants are emitted here.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{
        aws_terraform_permission_context, downcast, emit_iam_role_policy_for_target_with_label,
        iam_policy_name_sanitize, iam_role_name_template, jsonencode, required_label,
        service_assume_role_policy, tags,
    },
    expr,
};
use alien_core::{
    import::EmitContext, Ai, PermissionProfile, PermissionSetReference, Result, ServiceAccount,
    Storage,
};
use alien_permissions::BindingTarget;
use hcl::expr::Expression;

/// The `ai/finetune` permission set id. When a permission profile references it on
/// this AI resource, the emitter provisions a dedicated Bedrock-trusted IAM role so
/// Bedrock can assume it to read training data and write output.
const AI_FINETUNE_PERMISSION_ID: &str = "ai/finetune";

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsAiEmitter;

impl TfEmitter for AwsAiEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let ai = downcast::<Ai>(ctx, Ai::RESOURCE_TYPE)?;
        let mut fragment = TfFragment::empty();

        let context = aws_terraform_permission_context()
            .with_resource_name(format!("${{local.resource_prefix}}-{}", ai.id()));

        for (owner_label, permission_set_refs) in ai_permission_owners(ctx) {
            for (idx, permission_set_ref) in permission_set_refs.iter().enumerate() {
                if let Some(permission_set) = permission_set_ref
                    .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                {
                    let ai_label_segment = sanitize_label_segment(ai.id());
                    emit_iam_role_policy_for_target_with_label(
                        &mut fragment,
                        &owner_label,
                        &permission_set,
                        &format!("{owner_label}_ai_{ai_label_segment}_set_{idx}"),
                        &format!(
                            "access-{}-{}",
                            ai.id(),
                            iam_policy_name_sanitize(&permission_set.id)
                        ),
                        &context,
                        BindingTarget::Resource,
                    )?;
                }
            }
        }

        // When a permission profile references `ai/finetune` on this resource, emit a
        // dedicated IAM role Bedrock can assume for model-customization jobs. The
        // stack's service-account roles only trust compute principals
        // (`lambda`/`codebuild`/`ec2`), so Bedrock cannot assume them — that is the
        // real AccessDenied this role fixes. Its name matches the controller's
        // `role_arn` (`{prefix}-{id}-finetune`) so the runtime gateway can pass it.
        if resource_references_finetune(ctx) {
            emit_finetune_role(&mut fragment, ctx, ai.id());
        }

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let _ = downcast::<Ai>(ctx, Ai::RESOURCE_TYPE)?;
        let _ = required_label(ctx)?;
        Ok(expr::object([("region", expr::raw("data.aws_region.current.region"))]))
    }
}

fn ai_permission_owners(ctx: &EmitContext<'_>) -> Vec<(String, Vec<PermissionSetReference>)> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = ai_permission_refs(profile, ctx.resource_id);
        if refs.is_empty() {
            continue;
        }
        let service_account_id = format!("{profile_name}-sa");
        if let Some(label) = service_account_label(ctx, &service_account_id) {
            owners.push((label.to_string(), refs));
        }
    }
    owners
}

fn ai_permission_refs(profile: &PermissionProfile, resource_id: &str) -> Vec<PermissionSetReference> {
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

fn service_account_label<'a>(ctx: &'a EmitContext<'_>, service_account_id: &str) -> Option<&'a str> {
    let (_id, entry) = ctx
        .stack
        .resources()
        .find(|(id, _entry)| id.as_str() == service_account_id)?;
    entry.config.downcast_ref::<ServiceAccount>()?;
    ctx.name_for(service_account_id)
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

/// True when any permission profile references the `ai/finetune` set on this AI
/// resource. Only then is the Bedrock-trusted finetune role needed.
fn resource_references_finetune(ctx: &EmitContext<'_>) -> bool {
    ctx.stack.permission_profiles().values().any(|profile| {
        ai_permission_refs(profile, ctx.resource_id)
            .iter()
            .any(|reference| reference.id() == AI_FINETUNE_PERMISSION_ID)
    })
}

/// Emit the dedicated Bedrock-trusted finetune IAM role plus its inline S3 policy.
///
/// The role is named `{prefix}-{id}-finetune` (matching the controller's
/// `role_arn`), trusts `bedrock.amazonaws.com`, and can read training data
/// (`s3:GetObject`/`s3:ListBucket`) and write output (`s3:PutObject`) on every
/// storage bucket in the stack.
fn emit_finetune_role(fragment: &mut TfFragment, ctx: &EmitContext<'_>, ai_id: &str) {
    let role_label = format!("{}_finetune", sanitize_label_segment(ai_id));

    fragment.resource_blocks.push(resource_block(
        "aws_iam_role",
        &role_label,
        [
            attr(
                "name",
                iam_role_name_template(&format!("{ai_id}-finetune")),
            ),
            attr(
                "assume_role_policy",
                service_assume_role_policy(&["bedrock.amazonaws.com"]),
            ),
            attr("tags", tags(ctx, "ai")),
        ],
    ));

    let statements = finetune_s3_statements(ctx);
    fragment.resource_blocks.push(resource_block(
        "aws_iam_role_policy",
        &format!("{role_label}_s3"),
        [
            attr(
                "name",
                Expression::String(format!("{ai_id}-finetune-s3")),
            ),
            attr(
                "role",
                expr::traversal(["aws_iam_role", role_label.as_str(), "id"]),
            ),
            attr(
                "policy",
                jsonencode(expr::object([
                    ("Version", Expression::String("2012-10-17".to_string())),
                    ("Statement", Expression::Array(statements)),
                ])),
            ),
        ],
    ));
}

/// S3 statements scoping the finetune role to the stack's storage buckets:
/// list/read on the bucket + objects, put on objects (training in, output out).
fn finetune_s3_statements(ctx: &EmitContext<'_>) -> Vec<Expression> {
    let mut bucket_arns = Vec::new();
    let mut object_arns = Vec::new();
    for (id, entry) in ctx.stack.resources() {
        if entry.config.downcast_ref::<Storage>().is_none() {
            continue;
        }
        let Some(label) = ctx.name_for(id) else {
            continue;
        };
        bucket_arns.push(expr::traversal(["aws_s3_bucket", label, "arn"]));
        object_arns.push(expr::template(format!("${{aws_s3_bucket.{label}.arn}}/*")));
    }

    vec![
        // Read the training dataset: list the bucket and get objects.
        expr::object([
            ("Effect", Expression::String("Allow".to_string())),
            (
                "Action",
                Expression::Array(vec![
                    Expression::String("s3:GetObject".to_string()),
                    Expression::String("s3:ListBucket".to_string()),
                ]),
            ),
            (
                "Resource",
                Expression::Array(bucket_arns.iter().cloned().chain(object_arns.iter().cloned()).collect()),
            ),
        ]),
        // Write the tuning-job output back to storage.
        expr::object([
            ("Effect", Expression::String("Allow".to_string())),
            (
                "Action",
                Expression::String("s3:PutObject".to_string()),
            ),
            ("Resource", Expression::Array(object_arns)),
        ]),
    ]
}
