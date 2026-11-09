use crate::{
    block::{attr, block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{
        downcast, emit_iam_role_policy_for_target_with_label, iam_policy_name_sanitize,
        json_value_to_expression, jsonencode, required_label, stack_permission_context, tags,
    },
    expr,
};
use alien_core::{
    id_error_message, id_regex_pattern, import::EmitContext, ErrorData, IdType, Key,
    PermissionSetReference, RemoteBindings, RemoteStackManagement, Result,
};
use alien_error::Context;
use alien_permissions::{
    aws_key_policy::{aws_key_policy, key_has_native_service_use, key_runtime_service_accounts},
    key_access::{cryptography_service_accounts, REMOTE_CRYPTOGRAPHY},
    BindingTarget,
};
use hcl::{
    expr::{Conditional, Expression},
    structure::Block,
};

/// The AWS partitions, as the AWS SDKs' `partitions.json` lists them.
pub(crate) const AWS_PARTITIONS: [&str; 8] = [
    "aws",
    "aws-cn",
    "aws-us-gov",
    "aws-iso",
    "aws-iso-b",
    "aws-iso-e",
    "aws-iso-f",
    "aws-eusc",
];

/// A KMS key ARN in this deployment's account and region, in a known AWS partition, naming the
/// key by its ID: a lowercase UUID, or `mrk-` and 32 hex digits for a multi-Region key. Aliases
/// are not immutable, so they are refused. The IAM size check bounds a resource attribute that
/// a `can(regex(KMS_KEY_ARN_PATTERN, self.<attribute>))` postcondition admits as a KMS key ARN,
/// at the longest partition and key ID this admits.
pub const KMS_KEY_ARN_PATTERN: &str = "^arn:(aws|aws-cn|aws-us-gov|aws-iso|aws-iso-b|aws-iso-e|aws-iso-f|aws-eusc):kms:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:key/(mrk-[0-9a-f]{32}|[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$";

/// Suffix of a variable naming a customer key connection, an object whose `deployment_id` is
/// the deployment holding the customer's key.
pub const CUSTOMER_KEY_CONNECTION_SUFFIX: &str = "_customer_key_connection";

/// The `validation` block a `<label>_customer_key_connection` variable needs before the IAM
/// size check bounds its `deployment_id`: null, or an ID in alien's deployment ID format.
pub fn customer_key_connection_validation(variable: &str) -> Result<Block> {
    Ok(block(
        "validation",
        [
            attr(
                "condition",
                expr::parse(&customer_key_connection_condition(variable))?,
            ),
            attr(
                "error_message",
                Expression::String(id_error_message(IdType::Deployment)),
            ),
        ],
    ))
}

/// The condition of [`customer_key_connection_validation`].
pub(crate) fn customer_key_connection_condition(variable: &str) -> String {
    format!(
        r#"var.{variable} == null || can(regex("{}", var.{variable}.deployment_id))"#,
        id_regex_pattern(IdType::Deployment)
    )
}

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
                attr("policy", key_policy(ctx)?),
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
                    attr("condition", expr::parse(&format!(r#"can(regex("{KMS_KEY_ARN_PATTERN}", self.output))"#))?),
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
                let context = stack_permission_context(ctx)
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
                let context = stack_permission_context(ctx)
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

        if let Some(permission_set) = alien_permissions::get_permission_set(REMOTE_CRYPTOGRAPHY) {
            for account_id in cryptography_service_accounts(ctx.stack, ctx.resource_id) {
                let Some(role_label) = ctx.name_for(&account_id) else {
                    continue;
                };
                let context = stack_permission_context(ctx)
                    .with_resource_name(format!("${{local.{label}_selected_key.arn}}"));
                emit_iam_role_policy_for_target_with_label(
                    &mut fragment,
                    role_label,
                    permission_set,
                    &format!("{role_label}_{label}_cryptography"),
                    &format!("runtime-{}-cryptography", ctx.resource_id),
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

/// Policy for a key that setup creates. A customer-supplied key keeps its own policy.
fn key_policy(ctx: &EmitContext<'_>) -> Result<Expression> {
    let management_role_arn = management_role_label(ctx)
        .map(|label| serde_json::Value::String(format!("${{aws_iam_role.{label}.arn}}")));
    Ok(jsonencode(json_value_to_expression(sorted_keys(aws_key_policy(
        serde_json::Value::String(
            "arn:${data.aws_partition.current.partition}:iam::${data.aws_caller_identity.current.account_id}:root"
                .to_string(),
        ),
        management_role_arn,
        serde_json::Value::String(
            "arn:${data.aws_partition.current.partition}:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-*"
                .to_string(),
        ),
        key_has_native_service_use(ctx.stack, ctx.resource_id),
        runtime_role_arns(ctx)?,
    )))))
}

/// Roles that may use a key no AWS service uses: the service accounts granted the
/// key, and the remote-bindings role when the key has remote access.
fn runtime_role_arns(ctx: &EmitContext<'_>) -> Result<Vec<serde_json::Value>> {
    let mut labels = key_runtime_service_accounts(ctx.stack, ctx.resource_id)
        .context(ErrorData::GenericError {
            message: "Failed to work out which roles may use the Key".to_string(),
        })?
        .into_iter()
        .filter_map(|account_id| ctx.name_for(&account_id).map(str::to_string))
        .collect::<Vec<_>>();
    if alien_core::remote_bindings::remote_binding_for_entry(ctx.resource).is_some() {
        labels.extend(remote_bindings_label(ctx).map(str::to_string));
    }
    Ok(labels
        .into_iter()
        .map(|label| serde_json::Value::String(format!("${{aws_iam_role.{label}.arn}}")))
        .collect())
}

/// Orders object keys so the rendered policy doesn't depend on whether
/// `serde_json` keeps insertion order in this build.
fn sorted_keys(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries = map.into_iter().collect::<Vec<_>>();
            entries.sort_by(|(a, _), (b, _)| a.cmp(b));
            serde_json::Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, sorted_keys(value)))
                    .collect(),
            )
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(sorted_keys).collect())
        }
        other => other,
    }
}

fn management_role_label<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a str> {
    ctx.stack.resources().find_map(|(id, entry)| {
        (entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE)
            .then(|| ctx.name_for(id))
            .flatten()
    })
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
    has_permission.then(|| management_role_label(ctx))?
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
