use crate::{
    block::{attr, block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{
        aws_terraform_permission_context, downcast, emit_iam_role_policy_for_target_with_label,
        iam_policy_name_sanitize, required_label, tags,
    },
    expr,
};
use alien_core::{
    Key, PermissionSetReference, RemoteBindings, RemoteStackManagement, Result, import::EmitContext,
};
use alien_permissions::BindingTarget;
use hcl::{
    expr::{Conditional, Expression},
    structure::Block,
};

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
                    "count",
                    expr::parse(&format!("var.{label}_existing_key == null ? 1 : 0"))?,
                ),
                attr(
                    "description",
                    Expression::String(format!("Encryption key '{}'", ctx.resource_id)),
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

        // The object is known during planning even when its ARN comes from a key
        // created in the same apply. A bare optional string cannot drive count.
        fragment.variable_blocks.push(Block::builder("variable")
            .add_label(format!("{label}_existing_key"))
            .add_attribute(("type", expr::parse("object({ arn = string })")?))
            .add_attribute(("default", Expression::Null))
            .add_attribute(("description", Expression::String(
                "Existing same-account, same-region KMS key. Omit to create a key. The selection is immutable after setup; the supplying setup owns deletion.".to_string())))
            .build());
        fragment.moved_blocks.push(block(
            "moved",
            [
                attr("from", expr::traversal(["aws_kms_key", label])),
                attr("to", expr::parse(&format!("aws_kms_key.{label}[0]"))?),
            ],
        ));
        let selected = format!(
            "var.{label}_existing_key == null ? aws_kms_key.{label}[0].arn : var.{label}_existing_key.arn"
        );
        // Keep the first accepted ARN in Terraform state. The postcondition stops
        // a later key substitution before any dependent encryption setting changes.
        fragment.resource_blocks.push(resource_block("terraform_data", &format!("{label}_key_identity"), [
            attr("input", expr::parse(&selected)?),
            nested(block("lifecycle", [
                attr("ignore_changes", expr::parse("[input]")?),
                nested(block("postcondition", [
                    attr("condition", expr::parse(&format!("self.output == ({selected})"))?),
                    attr("error_message", Expression::String("The encryption key is immutable. Create a new deployment to use a different key.".to_string())),
                ])),
                nested(block("postcondition", [
                    attr("condition", expr::parse(r#"can(regex("^arn:aws[a-z-]*:kms:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:key/[a-zA-Z0-9-]+$", self.output))"#)?),
                    attr("error_message", Expression::String("The key must be an immutable KMS key ARN in this deployment's AWS account and region. Aliases are not supported.".to_string())),
                ])),
            ])),
        ]));

        fragment.locals.insert(
            format!("{label}_selected_key"),
            key_selection_local_value(
                ctx,
                expr::traversal(["terraform_data", &format!("{label}_key_identity"), "output"]),
            )?,
        );

        if let Some(management_label) = management_label(ctx) {
            let profile = ctx.stack.management().profile();
            for permission in profile
                .and_then(|profile| profile.0.get(ctx.resource_id))
                .into_iter()
                .flatten()
            {
                let permission_set = permission
                    .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                    .ok_or_else(|| {
                        alien_error::AlienError::new(alien_core::ErrorData::GenericError {
                            message: format!(
                                "Unknown Key management permission '{}'",
                                permission.id()
                            ),
                        })
                    })?;
                let context = aws_terraform_permission_context()
                    .with_resource_name(format!("${{local.{label}_selected_key.arn}}"));
                let suffix = iam_policy_name_sanitize(&permission_set.id);
                emit_iam_role_policy_for_target_with_label(
                    &mut fragment,
                    management_label,
                    &permission_set,
                    &format!("{management_label}_{label}_{suffix}"),
                    &format!("management-{}-{suffix}", ctx.resource_id),
                    &context,
                    BindingTarget::Resource,
                )?;
            }
        }

        if let (Some(definition), Some(access_label)) = (
            alien_core::remote_bindings::remote_binding_for_entry(ctx.resource),
            remote_bindings_label(ctx),
        ) {
            let permission = PermissionSetReference::from_name(definition.permission_set);
            if let Some(permission_set) =
                permission.resolve(|name| alien_permissions::get_permission_set(name).cloned())
            {
                let context = aws_terraform_permission_context()
                    .with_resource_name(format!("${{local.{label}_selected_key.arn}}"));
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
        Ok(expr::object([
            ("keyArn", key_arn(label)),
            (
                "availabilityProbe",
                Expression::Bool(
                    ctx.stack
                        .management()
                        .profile()
                        .and_then(|profile| profile.0.get(ctx.resource_id))
                        .is_some_and(|refs| {
                            refs.iter()
                                .any(|reference| reference.id() == "key/availability")
                        }),
                ),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        Ok(Some(expr::object([
            ("service", Expression::String("kms".to_string())),
            ("keyArn", key_arn(label)),
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

fn management_label<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a str> {
    let has_permission = ctx
        .stack
        .management()
        .profile()
        .and_then(|profile| profile.0.get(ctx.resource_id))
        .is_some_and(|refs| {
            refs.iter()
                .any(|reference| reference.id() == "key/management")
        });
    has_permission.then(|| {
        ctx.stack.resources().find_map(|(id, entry)| {
            (entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE)
                .then(|| ctx.name_for(id))
                .flatten()
        })
    })?
}

/// The setup-pinned AWS key identity used by imports and encrypted resources.
pub fn key_arn(label: &str) -> Expression {
    expr::traversal(["local", &format!("{label}_selected_key"), "arn"])
}

/// Keep disabled keys out of global local evaluation. Consumers still fail if
/// they require the ARN of a disabled key, rather than silently selecting a default key.
pub fn key_selection_local_value(ctx: &EmitContext<'_>, arn: Expression) -> Result<Expression> {
    let selection = expr::object([("arn", arn)]);
    match ctx.resource.enabled_when.as_deref() {
        Some(input_id) => Ok(Expression::Conditional(Box::new(Conditional::new(
            expr::parse(&format!(
                "var.{}",
                crate::generator::stack_input_variable_name(input_id)
            ))?,
            selection,
            Expression::Null,
        )))),
        None => Ok(selection),
    }
}
