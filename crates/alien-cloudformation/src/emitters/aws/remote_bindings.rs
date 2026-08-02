use crate::{
    emitter::CfEmitter,
    emitters::aws::helpers::{required_logical_id, resource_config, tags, PARAM_MANAGING_ROLE_ARN},
    template::{CfExpression, CfResource},
};
use alien_core::{import::EmitContext, RemoteBindings, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsRemoteBindingsEmitter;

impl CfEmitter for AwsRemoteBindingsEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        resource_config::<RemoteBindings>(ctx, RemoteBindings::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        let role_id = format!("{logical_id}Role");
        let mut role = CfResource::new(role_id, "AWS::IAM::Role".to_string());
        role.properties.insert(
            "RoleName".to_string(),
            CfExpression::sub("${AWS::StackName}-access"),
        );
        role.properties
            .insert("AssumeRolePolicyDocument".to_string(), trust_policy());
        role.properties.insert("Tags".to_string(), tags(ctx));
        Ok(vec![role])
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        resource_config::<RemoteBindings>(ctx, RemoteBindings::RESOURCE_TYPE)?;
        let role_id = format!("{}Role", required_logical_id(ctx)?);
        Ok(CfExpression::object([
            ("roleName", CfExpression::ref_(&role_id)),
            ("roleArn", CfExpression::get_att(&role_id, "Arn")),
        ]))
    }
}

fn trust_policy() -> CfExpression {
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
