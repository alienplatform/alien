use crate::{
    block::{attr, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{
        aws_terraform_permission_context, downcast, emit_iam_role_policy_for_target_with_label,
        iam_policy_name_sanitize, required_label, tags,
    },
    expr,
};
use alien_core::{import::EmitContext, Key, PermissionSetReference, RemoteBindings, Result};
use alien_permissions::BindingTarget;
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsKeyEmitter;

impl TfEmitter for AwsKeyEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let _ = downcast::<Key>(ctx, Key::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let mut fragment = TfFragment::default().with_resource(resource_block(
            "aws_kms_key",
            label,
            [
                attr(
                    "description",
                    Expression::String(format!("Alien encryption key '{}'", ctx.resource_id)),
                ),
                attr("enable_key_rotation", Expression::Bool(true)),
                attr(
                    "deletion_window_in_days",
                    Expression::Number(hcl::Number::from(30i64)),
                ),
                attr("tags", tags(ctx, "key")),
                nested(crate::block::block(
                    "lifecycle",
                    [attr("prevent_destroy", Expression::Bool(true))],
                )),
            ],
        ));

        if let (Some(definition), Some(access_label)) = (
            alien_core::remote_bindings::remote_binding_for_entry(ctx.resource),
            remote_bindings_label(ctx),
        ) {
            let permission = PermissionSetReference::from_name(definition.permission_set);
            if let Some(permission_set) =
                permission.resolve(|name| alien_permissions::get_permission_set(name).cloned())
            {
                let context = aws_terraform_permission_context()
                    .with_resource_name(format!("${{aws_kms_key.{label}.arn}}"));
                emit_iam_role_policy_for_target_with_label(
                    &mut fragment,
                    access_label,
                    &permission_set,
                    &format!("{access_label}_{label}_remote_cryptography"),
                    &format!(
                        "access-{}-{}",
                        ctx.resource_id,
                        iam_policy_name_sanitize(&permission_set.id)
                    ),
                    &context,
                    BindingTarget::Resource,
                )?;
            }
        }

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([(
            "keyArn",
            expr::traversal(["aws_kms_key", label, "arn"]),
        )]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        Ok(Some(expr::object([
            ("service", Expression::String("kms".to_string())),
            ("keyArn", expr::traversal(["aws_kms_key", label, "arn"])),
            ("region", expr::raw("data.aws_region.current.region")),
        ])))
    }
}

fn remote_bindings_label<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a str> {
    ctx.stack.resources().find_map(|(id, entry)| {
        (entry.config.resource_type() == RemoteBindings::RESOURCE_TYPE)
            .then(|| ctx.name_for(id))
            .flatten()
    })
}
