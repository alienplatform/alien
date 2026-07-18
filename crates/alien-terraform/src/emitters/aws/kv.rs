//! AWS KV — DynamoDB on-demand table with composite key, TTL, SSE, PITR.
//!
//! Resource-scoped `aws_iam_role_policy` blocks for permission profiles
//! that reference this table are emitted alongside it; wildcard-scope
//! grants stay on the service-account role via `stack_permission_sets`.

use crate::{
    block::{attr, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{
        aws_terraform_permission_context, downcast, emit_iam_role_policy_for_target_with_label,
        iam_policy_name_sanitize, nested_block, required_label, resource_prefix_template, tags,
    },
    emitters::gates::permission_gate_count,
    expr,
};
use alien_core::{
    import::EmitContext, Kv, PermissionProfile, PermissionSetReference, RemoteStackManagement,
    Result, ServiceAccount,
};
use alien_permissions::BindingTarget;
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsKvEmitter;

impl TfEmitter for AwsKvEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let kv = downcast::<Kv>(ctx, Kv::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;

        let table = resource_block(
            "aws_dynamodb_table",
            label,
            [
                attr("name", resource_prefix_template(kv.id())),
                attr(
                    "billing_mode",
                    Expression::String("PAY_PER_REQUEST".to_string()),
                ),
                attr("hash_key", Expression::String("pk".to_string())),
                attr("range_key", Expression::String("sk".to_string())),
                nested_block(
                    "attribute",
                    vec![
                        attr("name", Expression::String("pk".to_string())),
                        attr("type", Expression::String("S".to_string())),
                    ],
                ),
                nested_block(
                    "attribute",
                    vec![
                        attr("name", Expression::String("sk".to_string())),
                        attr("type", Expression::String("S".to_string())),
                    ],
                ),
                nested_block(
                    "ttl",
                    vec![
                        attr("attribute_name", Expression::String("ttl".to_string())),
                        attr("enabled", Expression::Bool(true)),
                    ],
                ),
                nested_block(
                    "server_side_encryption",
                    vec![attr("enabled", Expression::Bool(true))],
                ),
                nested_block(
                    "point_in_time_recovery",
                    vec![attr("enabled", Expression::Bool(true))],
                ),
                attr("tags", tags(ctx, "kv")),
                nested(crate::block::block(
                    "lifecycle",
                    [attr("prevent_destroy", Expression::Bool(false))],
                )),
            ],
        );

        let mut fragment = TfFragment::default().with_resource(table);
        emit_kv_iam(ctx, &mut fragment, label, kv)?;
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            (
                "tableName",
                expr::traversal(["aws_dynamodb_table", label, "name"]),
            ),
            (
                "tableArn",
                expr::traversal(["aws_dynamodb_table", label, "arn"]),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        Ok(Some(expr::object([
            ("service", Expression::String("dynamodb".to_string())),
            (
                "tableName",
                expr::traversal(["aws_dynamodb_table", label, "name"]),
            ),
            ("region", expr::raw("data.aws_region.current.region")),
        ])))
    }
}

fn emit_kv_iam(ctx: &EmitContext<'_>, fragment: &mut TfFragment, label: &str, kv: &Kv) -> Result<()> {
    let context = aws_terraform_permission_context()
        .with_resource_name(format!("${{aws_dynamodb_table.{label}.name}}"));

    for (owner_label, profile_name, permission_set_refs) in kv_permission_owners(ctx) {
        for (idx, permission_set_ref) in permission_set_refs.iter().enumerate() {
            let Some(permission_set) = permission_set_ref
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            if !permission_set.id.starts_with("kv/") || permission_set.id.ends_with("/provision") {
                continue;
            }

            let gate_count = match &profile_name {
                Some(profile) => {
                    permission_gate_count(ctx, profile, &permission_set.id, &[ctx.resource_id])?
                }
                None => None,
            };
            let name_prefix = if profile_name.is_some() {
                "access"
            } else {
                "management"
            };
            let kv_label_segment = sanitize_label_segment(kv.id());
            emit_iam_role_policy_for_target_with_label(
                fragment,
                &owner_label,
                &permission_set,
                &format!("{owner_label}_kv_{kv_label_segment}_set_{idx}"),
                &format!(
                    "{name_prefix}-{}-{}",
                    kv.id(),
                    iam_policy_name_sanitize(&permission_set.id)
                ),
                &context,
                BindingTarget::Resource,
                gate_count,
            )?;
        }
    }

    Ok(())
}

fn kv_permission_owners(
    ctx: &EmitContext<'_>,
) -> Vec<(String, Option<String>, Vec<PermissionSetReference>)> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = resource_permission_refs(profile, ctx.resource_id);
        if refs.is_empty() {
            continue;
        }

        let service_account_id = format!("{profile_name}-sa");
        if let Some(label) = service_account_label(ctx, &service_account_id) {
            owners.push((label.to_string(), Some(profile_name.clone()), refs));
        }
    }

    if let Some(profile) = ctx.stack.management().profile() {
        let refs = resource_permission_refs(profile, ctx.resource_id);
        if !refs.is_empty() {
            if let Some(label) = remote_stack_management_label(ctx) {
                owners.push((label.to_string(), None, refs));
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
        for permission_ref in resource_refs {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref.clone());
            }
        }
    }
    refs
}

fn service_account_label<'a>(
    ctx: &'a EmitContext<'_>,
    service_account_id: &str,
) -> Option<&'a str> {
    let (_id, entry) = ctx
        .stack
        .resources()
        .find(|(id, _entry)| id.as_str() == service_account_id)?;
    entry.config.downcast_ref::<ServiceAccount>()?;
    ctx.name_for(service_account_id)
}

fn remote_stack_management_label<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a str> {
    ctx.stack.resources().find_map(|(id, entry)| {
        if entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE {
            ctx.name_for(id)
        } else {
            None
        }
    })
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
