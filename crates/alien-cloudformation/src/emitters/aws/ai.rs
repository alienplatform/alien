//! AWS AI — Bedrock inference gateway.
//!
//! AWS Bedrock is a regional, account-scoped service with no per-stack
//! cloud resource to provision. The emitter returns zero CFN resources for
//! the AI resource itself and emits `AWS::IAM::Policy` resources for every
//! permission profile that references `ai/invoke` on this resource, attaching
//! them to the corresponding service-account role.
//!
//! The import ref carries the region so the controller can reconstruct the
//! Bedrock endpoint without a cloud round-trip.

use crate::{
    emitter::CfEmitter,
    emitters::aws::{
        helpers::{
            cf_from_json, required_logical_id, resource_config, service_account_role_id,
            uniquify_iam_statement_sids,
        },
        service_account::permission_context,
    },
    template::{CfExpression, CfResource},
};
use alien_core::{import::EmitContext, Ai, ErrorData, PermissionProfile, PermissionSetReference, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_permissions::{generators::AwsCloudFormationPermissionsGenerator, BindingTarget};

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsAiEmitter;

impl CfEmitter for AwsAiEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        resource_config::<Ai>(ctx, Ai::RESOURCE_TYPE)?;
        ai_iam_policies(ctx)
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

