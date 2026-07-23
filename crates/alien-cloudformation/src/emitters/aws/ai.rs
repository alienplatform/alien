//! AWS AI — Bedrock inference gateway.
//!
//! AWS Bedrock is a regional, account-scoped service with no per-stack
//! cloud resource to provision. The emitter returns zero CFN resources for
//! the AI resource itself and emits `AWS::IAM::Policy` resources for every
//! permission profile that references an `ai/*` set (e.g. `ai/invoke`, or
//! `ai/finetune` when the resource declares a fine-tuning job) on this
//! resource, attaching them to the corresponding service-account role.
//!
//! The import ref carries the region so the controller can reconstruct the
//! Bedrock endpoint without a cloud round-trip.

use crate::{
    emitter::CfEmitter,
    emitters::aws::{
        helpers::{
            cf_from_json, required_logical_id, resource_config, service_account_role_id,
            service_trust_policy, stack_name, tags, uniquify_iam_statement_sids,
        },
        service_account::permission_context,
    },
    template::{CfExpression, CfResource},
};
use alien_core::{
    import::EmitContext, Ai, ErrorData, PermissionProfile, PermissionSetReference, Result, Storage,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_permissions::{generators::AwsCloudFormationPermissionsGenerator, BindingTarget};

/// The `ai/finetune` permission set id. When a permission profile references it on
/// this AI resource, the emitter provisions a dedicated Bedrock-trusted IAM role so
/// Bedrock can assume it to read training data and write output.
const AI_FINETUNE_PERMISSION_ID: &str = "ai/finetune";

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsAiEmitter;

impl CfEmitter for AwsAiEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let ai = resource_config::<Ai>(ctx, Ai::RESOURCE_TYPE)?;
        let mut resources = ai_iam_policies(ctx)?;

        // When a permission profile references `ai/finetune` on this resource, emit a
        // dedicated IAM role Bedrock can assume for model-customization jobs. The
        // stack's service-account roles only trust compute principals
        // (`lambda`/`codebuild`/`ec2`), so Bedrock cannot assume them — that is the
        // real AccessDenied this role fixes. Its name matches the controller's
        // `role_arn` (`{prefix}-{id}-finetune`) so the runtime gateway can pass it.
        if resource_references_finetune(ctx) {
            resources.push(finetune_role(ctx, ai.id()));
        }

        Ok(resources)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        resource_config::<Ai>(ctx, Ai::RESOURCE_TYPE)?;
        Ok(CfExpression::object([("region", CfExpression::ref_("AWS::Region"))]))
    }
}

fn ai_iam_policies(ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
    let mut resources = Vec::new();
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let logical_id = required_logical_id(ctx)?;
    let context = permission_context();

    for (owner_index, (role_id, permission_refs)) in ai_permission_owners(ctx).into_iter().enumerate() {
        for (permission_index, permission_ref) in permission_refs.iter().enumerate() {
            let Some(permission_set) =
                permission_ref.resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };

            let policy = generator
                .generate_policy(&permission_set, BindingTarget::Resource, &context)
                .context(ErrorData::GenericError {
                    message: format!(
                        "failed to generate AWS CloudFormation AI IAM policy for '{}'",
                        ctx.resource_id
                    ),
                })?;
            let policy_value = serde_json::to_value(policy).into_alien_error().context(
                ErrorData::TemplateSerializationFailed {
                    format: "CloudFormation IAM policy".to_string(),
                    reason: "Failed to serialize AI IAM policy".to_string(),
                },
            )?;
            let CfExpression::Object(mut policy_object) = cf_from_json(policy_value)? else {
                return Err(AlienError::new(ErrorData::TemplateSerializationFailed {
                    format: "CloudFormation IAM policy".to_string(),
                    reason: "AI policy did not serialize to a JSON object".to_string(),
                }));
            };
            let Some(CfExpression::List(policy_statements)) =
                policy_object.shift_remove("Statement")
            else {
                continue;
            };

            let policy_id = format!(
                "{logical_id}{role_id}AiPermission{owner_index}{permission_index}"
            );
            let mut policy_resource =
                CfResource::new(policy_id, "AWS::IAM::Policy".to_string());
            policy_resource.properties.insert(
                "PolicyName".to_string(),
                CfExpression::sub(format!(
                    "${{AWS::StackName}}-{}-ai-{owner_index}-{permission_index}",
                    ctx.resource_id
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
            policy_resource.depends_on.push(role_id.clone());
            resources.push(policy_resource);
        }
    }

    Ok(resources)
}

fn ai_permission_owners(ctx: &EmitContext<'_>) -> Vec<(String, Vec<PermissionSetReference>)> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = ai_permission_refs(profile, ctx.resource_id);
        if refs.is_empty() {
            continue;
        }
        if let Some(role_id) = service_account_role_id(ctx, profile_name) {
            owners.push((role_id, refs));
        }
    }
    owners
}

fn ai_permission_refs(
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

/// True when any permission profile references the `ai/finetune` set on this AI
/// resource. Only then is the Bedrock-trusted finetune role needed.
fn resource_references_finetune(ctx: &EmitContext<'_>) -> bool {
    ctx.stack.permission_profiles().values().any(|profile| {
        ai_permission_refs(profile, ctx.resource_id)
            .iter()
            .any(|reference| reference.id() == AI_FINETUNE_PERMISSION_ID)
    })
}

/// The dedicated Bedrock-trusted finetune IAM role plus its inline S3 policy.
///
/// The role is named `${AWS::StackName}-{id}-finetune` (matching the controller's
/// `role_arn`), trusts `bedrock.amazonaws.com`, and can read training data
/// (`s3:GetObject`/`s3:ListBucket`) and write output (`s3:PutObject`) on every
/// storage bucket in the stack.
fn finetune_role(ctx: &EmitContext<'_>, ai_id: &str) -> CfResource {
    let logical_id = ctx.name_for(ctx.resource_id).unwrap_or(ctx.resource_id);
    let role_id = format!("{logical_id}FinetuneRole");

    let mut role = CfResource::new(role_id, "AWS::IAM::Role".to_string());
    role.properties
        .insert("RoleName".to_string(), stack_name(&format!("{ai_id}-finetune")));
    role.properties.insert(
        "AssumeRolePolicyDocument".to_string(),
        service_trust_policy(["bedrock.amazonaws.com"]),
    );
    role.properties.insert(
        "Policies".to_string(),
        CfExpression::list([CfExpression::object([
            ("PolicyName", stack_name(&format!("{ai_id}-finetune-s3"))),
            (
                "PolicyDocument",
                CfExpression::object([
                    ("Version", CfExpression::from("2012-10-17")),
                    ("Statement", CfExpression::list(finetune_s3_statements(ctx))),
                ]),
            ),
        ])]),
    );
    role.properties.insert("Tags".to_string(), tags(ctx));

    // The role reads/writes the storage buckets, so it must be created after them.
    for bucket_id in storage_bucket_logical_ids(ctx) {
        role.depends_on.push(bucket_id);
    }

    role
}

/// S3 statements scoping the finetune role to the stack's storage buckets:
/// list/read on the bucket + objects, put on objects (training in, output out).
fn finetune_s3_statements(ctx: &EmitContext<'_>) -> Vec<CfExpression> {
    let mut bucket_arns = Vec::new();
    let mut object_arns = Vec::new();
    for bucket_id in storage_bucket_logical_ids(ctx) {
        bucket_arns.push(CfExpression::get_att(&bucket_id, "Arn"));
        object_arns.push(CfExpression::sub(format!("${{{bucket_id}.Arn}}/*")));
    }

    vec![
        // Read the training dataset: list the bucket and get objects.
        CfExpression::object([
            ("Effect", CfExpression::from("Allow")),
            (
                "Action",
                CfExpression::list([
                    CfExpression::from("s3:GetObject"),
                    CfExpression::from("s3:ListBucket"),
                ]),
            ),
            (
                "Resource",
                CfExpression::list(
                    bucket_arns
                        .iter()
                        .cloned()
                        .chain(object_arns.iter().cloned()),
                ),
            ),
        ]),
        // Write the tuning-job output back to storage.
        CfExpression::object([
            ("Effect", CfExpression::from("Allow")),
            ("Action", CfExpression::from("s3:PutObject")),
            ("Resource", CfExpression::list(object_arns)),
        ]),
    ]
}

/// Logical ids of every `Storage` (S3 bucket) resource in the stack.
fn storage_bucket_logical_ids(ctx: &EmitContext<'_>) -> Vec<String> {
    ctx.stack
        .resources()
        .filter_map(|(id, entry)| {
            entry.config.downcast_ref::<Storage>()?;
            ctx.name_for(id).map(|label| label.to_string())
        })
        .collect()
}

