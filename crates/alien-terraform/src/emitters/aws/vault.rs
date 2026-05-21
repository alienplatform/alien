//! AWS Vault — SSM Parameter Store namespace.
//!
//! AWS Systems Manager Parameter Store is account-and-region-scoped, not
//! a discrete resource. The vault is realized as a name prefix
//! (`${local.resource_prefix}-{vault.id}`) the controller uses for
//! `ssm:PutParameter`. ImportData carries the prefix so importers can
//! reconstruct the namespace without a cloud lookup.

use crate::{
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{
        aws_terraform_permission_context, downcast, emit_iam_role_policy_for_target_with_label,
        iam_policy_name_sanitize, required_label,
    },
    expr,
};
use alien_core::{
    import::EmitContext, PermissionProfile, PermissionSetReference, RemoteStackManagement, Result,
    Vault,
};
use alien_permissions::BindingTarget;
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsVaultEmitter;

impl TfEmitter for AwsVaultEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let vault = downcast::<Vault>(ctx, Vault::RESOURCE_TYPE)?;
        let Some(management_label) = remote_stack_management_label(ctx) else {
            return Ok(TfFragment::empty());
        };

        let mut fragment = TfFragment::default();
        let context = aws_terraform_permission_context()
            .with_resource_name(format!("${{local.resource_prefix}}-{}", vault.id()));

        for (idx, permission_set_ref) in management_permission_refs(ctx).into_iter().enumerate() {
            if let Some(permission_set) = permission_set_ref
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
            {
                if permission_set.id.ends_with("/provision") {
                    continue;
                }
                let vault_label_segment = sanitize_label_segment(vault.id());
                emit_iam_role_policy_for_target_with_label(
                    &mut fragment,
                    management_label,
                    &permission_set,
                    &format!("{management_label}_vault_{vault_label_segment}_set_{idx}"),
                    &format!(
                        "alien-mgmt-{}-{}",
                        vault.id(),
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
        let vault = downcast::<Vault>(ctx, Vault::RESOURCE_TYPE)?;
        let _ = required_label(ctx)?;
        Ok(expr::object([
            (
                "accountId",
                expr::raw("data.aws_caller_identity.current.account_id"),
            ),
            ("region", expr::raw("data.aws_region.current.region")),
            (
                "parameterPrefix",
                expr::template(format!("${{local.resource_prefix}}-{}", vault.id())),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let vault = downcast::<Vault>(ctx, Vault::RESOURCE_TYPE)?;
        Ok(Some(expr::object([
            ("service", Expression::String("parameter-store".to_string())),
            (
                "vaultPrefix",
                expr::template(format!("${{local.resource_prefix}}-{}", vault.id())),
            ),
        ])))
    }
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

fn remote_stack_management_label<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a str> {
    ctx.stack.resources().find_map(|(id, entry)| {
        if entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE {
            ctx.name_for(id)
        } else {
            None
        }
    })
}

fn management_permission_refs<'a>(ctx: &'a EmitContext<'_>) -> Vec<&'a PermissionSetReference> {
    let Some(profile) = ctx.stack.management().profile() else {
        return Vec::new();
    };
    let mut refs = Vec::new();
    refs.extend(resource_permission_refs(profile, ctx.resource_id));
    refs.extend(
        profile
            .0
            .get("*")
            .into_iter()
            .flat_map(|items| items.iter())
            .filter(|reference| reference.id().starts_with("vault/")),
    );
    refs
}

fn resource_permission_refs<'a>(
    profile: &'a PermissionProfile,
    resource_id: &str,
) -> Vec<&'a PermissionSetReference> {
    profile
        .0
        .get(resource_id)
        .map(|refs| refs.iter().collect())
        .unwrap_or_default()
}
