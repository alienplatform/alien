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
    emitters::enabled,
    expr,
};
use alien_core::{
    import::EmitContext, PermissionProfile, PermissionSetReference, RemoteStackManagement, Result,
    ServiceAccount, Vault,
};
use alien_permissions::BindingTarget;
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsVaultEmitter;

impl TfEmitter for AwsVaultEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let vault = downcast::<Vault>(ctx, Vault::RESOURCE_TYPE)?;
        let mut fragment = TfFragment::default();
        let enabled_when = ctx.resource.enabled_when.as_deref();
        let context = aws_terraform_permission_context()
            .with_resource_name(format!("${{local.resource_prefix}}-{}", vault.id()));

        for (owner_label, permission_set_refs) in vault_permission_owners(ctx) {
            for (idx, permission_set_ref) in permission_set_refs.iter().enumerate() {
                if let Some(permission_set) = permission_set_ref
                    .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                {
                    if permission_set.id.ends_with("/provision") {
                        continue;
                    }
                    let vault_label_segment = sanitize_label_segment(vault.id());
                    let appended_from = fragment.resource_blocks.len();
                    emit_iam_role_policy_for_target_with_label(
                        &mut fragment,
                        &owner_label,
                        &permission_set,
                        &format!("{owner_label}_vault_{vault_label_segment}_set_{idx}"),
                        &format!(
                            "access-{}-{}",
                            vault.id(),
                            iam_policy_name_sanitize(&permission_set.id)
                        ),
                        &context,
                        BindingTarget::Resource,
                    )?;
                    for block in &mut fragment.resource_blocks[appended_from..] {
                        enabled::gate(block, enabled_when)?;
                    }
                }
            }
        }

        if let Some(management_label) = remote_stack_management_label(ctx) {
            for (idx, permission_set_ref) in management_permission_refs(ctx).into_iter().enumerate()
            {
                if let Some(permission_set) = permission_set_ref
                    .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                {
                    if permission_set.id.ends_with("/provision") {
                        continue;
                    }
                    let vault_label_segment = sanitize_label_segment(vault.id());
                    let appended_from = fragment.resource_blocks.len();
                    emit_iam_role_policy_for_target_with_label(
                        &mut fragment,
                        management_label,
                        &permission_set,
                        &format!("{management_label}_vault_{vault_label_segment}_set_{idx}"),
                        &format!(
                            "management-{}-{}",
                            vault.id(),
                            iam_policy_name_sanitize(&permission_set.id)
                        ),
                        &context,
                        BindingTarget::Resource,
                    )?;
                    for block in &mut fragment.resource_blocks[appended_from..] {
                        enabled::gate(block, enabled_when)?;
                    }
                }
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

    /// The vault has no Terraform block of its own — it is a Parameter Store
    /// name prefix — and its import data is built entirely from `local` and
    /// `data` values. So nothing here needs an index: the IAM policies grant
    /// against a static `${local.resource_prefix}-<id>-*` pattern, never against
    /// a resource address.
    ///
    /// They still take the gate. `ssm:GetParameter` is a data-plane read, and
    /// the pattern is a prefix wildcard: resource ids may contain hyphens, so a
    /// declined vault `app` leaves a grant over `<prefix>-app-*`, which matches
    /// every parameter in a live sibling vault named `app-config`. Declining a
    /// vault has to withdraw the permission, not just the registration entry.
    fn supports_enabled_when(&self) -> bool {
        true
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

fn vault_permission_owners(ctx: &EmitContext<'_>) -> Vec<(String, Vec<PermissionSetReference>)> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = vault_permission_refs(profile, ctx.resource_id);
        if refs.is_empty() {
            continue;
        }

        let service_account_id = format!("{profile_name}-sa");
        if let Some((label, _service_account)) = service_account_for_id(ctx, &service_account_id) {
            owners.push((label.to_string(), refs));
        }
    }

    owners
}

fn vault_permission_refs(
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

    if let Some(wildcard_refs) = profile.0.get("*") {
        for permission_ref in wildcard_refs
            .iter()
            .filter(|permission_ref| permission_ref.id().starts_with("vault/"))
        {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref.clone());
            }
        }
    }

    refs
}

fn service_account_for_id<'a>(
    ctx: &'a EmitContext<'_>,
    service_account_id: &str,
) -> Option<(&'a str, &'a ServiceAccount)> {
    let (_id, entry) = ctx
        .stack
        .resources()
        .find(|(id, _entry)| id.as_str() == service_account_id)?;
    let service_account = entry.config.downcast_ref::<ServiceAccount>()?;
    let label = ctx.name_for(service_account_id)?;
    Some((label, service_account))
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
