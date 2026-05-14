//! AWS RemoteStackManagement — cross-account IAM role.
//!
//! The role's trust policy allows the manager's IAM identity (passed via
//! `ManagingRoleArn` parameter) to assume it. The inline policy is auto-derived
//! from the stack's management permission profile.

use crate::{
    emitter::CfEmitter,
    emitters::aws::{
        helpers::{
            cf_from_json, required_logical_id, resource_config, tags, PARAM_MANAGING_ROLE_ARN,
        },
        service_account::permission_context,
    },
    template::{CfExpression, CfResource},
};
use alien_core::{
    import::EmitContext, ErrorData, PermissionProfile, PermissionSetReference,
    RemoteStackManagement, Result,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_permissions::{generators::AwsCloudFormationPermissionsGenerator, BindingTarget};

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsRemoteStackManagementEmitter;

impl CfEmitter for AwsRemoteStackManagementEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        resource_config::<RemoteStackManagement>(ctx, RemoteStackManagement::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        let role_id = role_logical_id(logical_id);

        let mut role = CfResource::new(role_id.clone(), "AWS::IAM::Role".to_string());
        role.properties.insert(
            "RoleName".to_string(),
            CfExpression::sub("${AWS::StackName}-management"),
        );
        role.properties.insert(
            "AssumeRolePolicyDocument".to_string(),
            remote_management_trust_policy(),
        );
        role.properties.insert(
            "Policies".to_string(),
            CfExpression::list([CfExpression::object([
                ("PolicyName", CfExpression::from("alien-management-policy")),
                ("PolicyDocument", remote_management_policy_document(ctx)?),
            ])]),
        );
        role.properties.insert("Tags".to_string(), tags(ctx));

        Ok(vec![role])
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        resource_config::<RemoteStackManagement>(ctx, RemoteStackManagement::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        let role_id = role_logical_id(logical_id);
        Ok(CfExpression::object([
            ("roleName", CfExpression::ref_(&role_id)),
            ("roleArn", CfExpression::get_att(&role_id, "Arn")),
            ("managementPermissionsApplied", CfExpression::from(true)),
        ]))
    }
}

fn role_logical_id(resource_logical_id: &str) -> String {
    if resource_logical_id == "Management" {
        "ManagementRole".to_string()
    } else {
        format!("{resource_logical_id}Role")
    }
}

fn remote_management_trust_policy() -> CfExpression {
    CfExpression::object([
        ("Version", CfExpression::from("2012-10-17")),
        (
            "Statement",
            CfExpression::list([CfExpression::object([
                ("Sid", CfExpression::from("AllowManagingRole")),
                ("Effect", CfExpression::from("Allow")),
                (
                    "Principal",
                    CfExpression::object([("AWS", CfExpression::ref_(PARAM_MANAGING_ROLE_ARN))]),
                ),
                ("Action", CfExpression::from("sts:AssumeRole")),
                (
                    "Condition",
                    CfExpression::object([(
                        "StringEquals",
                        CfExpression::object([(
                            "aws:PrincipalArn",
                            CfExpression::ref_(PARAM_MANAGING_ROLE_ARN),
                        )]),
                    )]),
                ),
            ])]),
        ),
    ])
}

fn remote_management_policy_document(ctx: &EmitContext<'_>) -> Result<CfExpression> {
    let mut statements = Vec::new();
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let context = permission_context();

    if let Some(profile) = ctx.stack.management().profile() {
        for permission_set_ref in global_permission_refs(profile) {
            if let Some(permission_set) = permission_set_ref
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
            {
                if permission_set.platforms.aws.is_none() {
                    continue;
                }
                let policy = generator
                    .generate_policy(&permission_set, BindingTarget::Stack, &context)
                    .context(ErrorData::GenericError {
                        message: "failed to generate AWS management IAM policy".to_string(),
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
                if let Some(CfExpression::List(policy_statements)) =
                    policy_object.shift_remove("Statement")
                {
                    statements.extend(policy_statements);
                }
            }
        }
    }

    statements.push(CfExpression::object([
        ("Sid", CfExpression::from("ReadOwnManagementRole")),
        ("Effect", CfExpression::from("Allow")),
        ("Action", CfExpression::from("iam:GetRole")),
        (
            "Resource",
            CfExpression::sub(
                "arn:${AWS::Partition}:iam::${AWS::AccountId}:role/${AWS::StackName}-management",
            ),
        ),
    ]));

    Ok(CfExpression::object([
        ("Version", CfExpression::from("2012-10-17")),
        ("Statement", CfExpression::list(statements)),
    ]))
}

fn global_permission_refs(profile: &PermissionProfile) -> Vec<&PermissionSetReference> {
    profile
        .0
        .get("*")
        .map(|refs| refs.iter().collect())
        .unwrap_or_default()
}
