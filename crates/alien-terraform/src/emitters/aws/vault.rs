//! AWS Vault — SSM Parameter Store namespace.
//!
//! AWS Systems Manager Parameter Store is account-and-region-scoped, not
//! a discrete resource. The vault is realized as a name prefix
//! (`${local.resource_prefix}-{vault.id}`) the controller uses for
//! `ssm:PutParameter`. ImportData carries the prefix so importers can
//! reconstruct the namespace without a cloud lookup.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{
        downcast, emit_iam_role_policy_for_target_with_label, emit_iam_role_policy_stub,
        iam_policy_name_sanitize, required_label, stack_permission_context,
        without_stack_policy_copies,
    },
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
        let context = stack_permission_context(ctx)
            .with_resource_name(format!("${{local.resource_prefix}}-{}", vault.id()));

        for (owner_label, (profile_name, profile), permission_set_refs) in
            vault_permission_owners(ctx)
        {
            // The workload's stack-level policy already grants its `"*"` sets on this vault,
            // which is named under the prefix. A set it holds keeps its address as a stub, and
            // every set keeps its index, so no policy changes address.
            let kept = without_stack_policy_copies(
                ctx,
                profile_name,
                profile,
                permission_set_refs.clone(),
                &context,
            )?;
            for (idx, permission_set_ref) in permission_set_refs.iter().enumerate() {
                if let Some(permission_set) = permission_set_ref
                    .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                {
                    if permission_set.id.ends_with("/provision") {
                        continue;
                    }
                    let vault_label_segment = sanitize_label_segment(vault.id());
                    let policy_label =
                        format!("{owner_label}_vault_{vault_label_segment}_set_{idx}");
                    let policy_name = format!(
                        "access-{}-{}",
                        vault.id(),
                        iam_policy_name_sanitize(&permission_set.id)
                    );
                    if !kept.contains(permission_set_ref) {
                        if permission_set.platforms.aws.is_some() {
                            emit_iam_role_policy_stub(
                                &mut fragment,
                                &owner_label,
                                &policy_label,
                                &policy_name,
                            );
                        }
                        continue;
                    }
                    emit_iam_role_policy_for_target_with_label(
                        &mut fragment,
                        &owner_label,
                        &permission_set,
                        &policy_label,
                        &policy_name,
                        &context,
                        BindingTarget::Resource,
                    )?;
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
                }
            }
        }

        if vault.id() == alien_core::SECRETS_VAULT_ID
            && alien_core::sandbox_egress::has_sandbox_gateway(ctx.stack)
        {
            emit_sandbox_gateway_replica_secret(&mut fragment, required_label(ctx)?);
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

/// Setup generates the sandbox gateway's replica secret in the customer account, so no vendor
/// identity ever writes or reads its value: only the gateway's role may read it, and the
/// management role is denied. The generated value is also kept in the Terraform state of the
/// account that runs this setup.
fn emit_sandbox_gateway_replica_secret(fragment: &mut TfFragment, label: &str) {
    let name = format!("{label}_sandbox_egress_replica_secret");
    fragment.resource_blocks.push(resource_block(
        "random_password",
        &name,
        [
            attr("length", Expression::Number(hcl::Number::from(64i64))),
            attr("special", Expression::Bool(false)),
        ],
    ));
    fragment.resource_blocks.push(resource_block(
        "aws_secretsmanager_secret",
        &name,
        [
            attr(
                "name",
                expr::template(format!(
                    "${{local.resource_prefix}}-{}",
                    alien_core::sandbox_egress::SANDBOX_EGRESS_REPLICA_SECRET
                )),
            ),
            attr(
                "description",
                Expression::String(
                    "Sandbox egress gateway replica secret, readable only by the gateway".into(),
                ),
            ),
            attr(
                "recovery_window_in_days",
                Expression::Number(hcl::Number::from(0i64)),
            ),
        ],
    ));
    fragment.resource_blocks.push(resource_block(
        "aws_secretsmanager_secret_version",
        &name,
        [
            attr(
                "secret_id",
                expr::raw(format!("aws_secretsmanager_secret.{name}.id")),
            ),
            attr(
                "secret_string",
                expr::raw(format!("random_password.{name}.result")),
            ),
        ],
    ));
}

/// The workload roles granted sets on this vault, each with its profile and the sets.
fn vault_permission_owners<'a>(
    ctx: &'a EmitContext<'_>,
) -> Vec<(
    String,
    (&'a str, &'a PermissionProfile),
    Vec<PermissionSetReference>,
)> {
    if ctx.stack_settings.setup_management == alien_core::SetupManagement::Automatic {
        return Vec::new();
    }
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = vault_permission_refs(profile, ctx.resource_id);
        if refs.is_empty() {
            continue;
        }

        let service_account_id = format!("{profile_name}-sa");
        if let Some((label, _service_account)) = service_account_for_id(ctx, &service_account_id) {
            owners.push((label.to_string(), (profile_name.as_str(), profile), refs));
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
