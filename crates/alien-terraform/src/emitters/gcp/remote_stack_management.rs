//! GCP management access — service account the manager impersonates.
//!
//! Mirrors `GcpRemoteStackManagementController`:
//!
//! 1. Create a `google_service_account` for the management identity.
//! 2. For every permission set in `ctx.stack.management().profile()`,
//!    emit matching custom-role `google_project_iam_member` bindings.
//! 3. Grant `roles/iam.serviceAccountTokenCreator` +
//!    `roles/iam.serviceAccountUser` on the management SA to the
//!    caller-supplied manager identity (`var.managing_service_account_email`).
//!    The variable defaults to `""` so the bindings are no-ops in
//!    pure-OSS scenarios where there's no central manager.

use crate::{
    block::{attr, data_block, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{
        binding_label_for_role, downcast, emit_custom_roles_for_bindings, permission_context,
        push_iam_member, required_label, role_expression_for_binding, service_account_id_template,
        service_account_member_for_label,
    },
    expr,
};
use alien_core::{
    import::EmitContext, ErrorData, KubernetesCluster, PermissionProfile, PermissionSet,
    PermissionSetReference, RemoteStackManagement, Result,
};
use alien_error::AlienError;
use alien_permissions::{
    generators::{GcpBindingTargetScope, GcpRuntimePermissionsGenerator},
    BindingTarget, PermissionContext,
};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpRemoteStackManagementEmitter;

impl TfEmitter for GcpRemoteStackManagementEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let _ = downcast::<RemoteStackManagement>(ctx, RemoteStackManagement::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let mut fragment = TfFragment::default();

        let account_id_template = service_account_id_template(label);
        fragment.resource_blocks.push(resource_block(
            "google_service_account",
            label,
            [
                attr("project", expr::raw("var.gcp_project")),
                attr("account_id", account_id_template),
                attr(
                    "display_name",
                    expr::template("${local.deployment_name}: Management service account".to_string()),
                ),
                attr(
                    "description",
                    expr::template(
                        "Management cloud identity for ${local.deployment_name}. Resource prefix: ${local.resource_prefix}.".to_string(),
                    ),
                ),
            ],
        ));
        fragment.data_blocks.push(data_block(
            "google_project",
            "current",
            [attr("project_id", expr::raw("var.gcp_project"))],
        ));

        let member = service_account_member_for_label(label);
        let context = permission_context(label, ctx.stack.id());
        if let Some(profile) = ctx.stack.management().profile() {
            for permission_set_ref in global_permission_refs(profile) {
                if let Some(permission_set) = permission_set_ref
                    .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                {
                    emit_project_management_bindings(
                        &mut fragment,
                        label,
                        &member,
                        &permission_set,
                        &context,
                        BindingTarget::Stack,
                    )?;
                }
            }
            for (resource_id, permission_set_ref) in resource_scoped_permission_refs(profile) {
                let Some(resource_entry) = ctx.stack.resources.get(resource_id) else {
                    continue;
                };
                if resource_entry
                    .config
                    .downcast_ref::<KubernetesCluster>()
                    .is_none()
                {
                    continue;
                }
                if let Some(permission_set) = permission_set_ref
                    .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                {
                    let binding_label = format!("{label}_{}", terraform_label_segment(resource_id));
                    emit_project_management_bindings(
                        &mut fragment,
                        &binding_label,
                        &member,
                        &permission_set,
                        &context,
                        BindingTarget::Resource,
                    )?;
                }
            }
        }

        // Allow the configured manager identity to mint tokens for the
        // management SA + attach it. `for_each(toset(compact([...])))`
        // makes the bindings no-ops when the variable is empty (default).
        for (suffix, role) in [
            ("token_creator", "roles/iam.serviceAccountTokenCreator"),
            ("user", "roles/iam.serviceAccountUser"),
        ] {
            fragment.resource_blocks.push(resource_block(
                "google_service_account_iam_member",
                &format!("{label}_manager_{suffix}"),
                [
                    attr(
                        "for_each",
                        expr::raw("toset(compact([var.managing_service_account_email]))"),
                    ),
                    attr(
                        "service_account_id",
                        expr::traversal(["google_service_account", label, "id"]),
                    ),
                    attr("role", Expression::String(role.to_string())),
                    attr(
                        "member",
                        expr::template("serviceAccount:${each.value}".to_string()),
                    ),
                ],
            ));
        }

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            ("projectId", expr::raw("var.gcp_project")),
            (
                "projectNumber",
                expr::traversal(["data", "google_project", "current", "number"]),
            ),
            (
                "serviceAccountEmail",
                expr::traversal(["google_service_account", label, "email"]),
            ),
            (
                "serviceAccountUniqueId",
                expr::traversal(["google_service_account", label, "unique_id"]),
            ),
            ("managementPermissionsApplied", Expression::Bool(true)),
        ]))
    }
}

fn emit_project_management_bindings(
    fragment: &mut TfFragment,
    label: &str,
    member: &Expression,
    permission_set: &PermissionSet,
    context: &PermissionContext,
    binding_target: BindingTarget,
) -> Result<()> {
    if permission_set.platforms.gcp.is_none() {
        return Ok(());
    }

    let generator = GcpRuntimePermissionsGenerator::new();
    let grant_plan = generator
        .generate_grant_plan(permission_set, binding_target, context)
        .map_err(|err| {
            AlienError::new(ErrorData::GenericError {
                message: format!(
                    "failed to generate GCP remote management IAM grant plan for '{}': {}",
                    permission_set.id, err
                ),
            })
        })?;
    let bindings = grant_plan.bindings_for_target(GcpBindingTargetScope::Project);
    let custom_roles = emit_custom_roles_for_bindings(fragment, &grant_plan, &bindings)?;

    for (idx, binding) in bindings.into_iter().enumerate() {
        let role_label = binding_label_for_role(&binding.role, &custom_roles)?;
        let role = role_expression_for_binding(&binding.role, &custom_roles)?;
        push_iam_member(
            fragment,
            &format!("{role_label}_{label}_binding_{idx}"),
            role,
            member,
            &binding,
            None,
        )?;
    }

    Ok(())
}

fn global_permission_refs(profile: &PermissionProfile) -> Vec<&PermissionSetReference> {
    profile
        .0
        .get("*")
        .map(|refs| refs.iter().collect())
        .unwrap_or_default()
}

fn resource_scoped_permission_refs(
    profile: &PermissionProfile,
) -> Vec<(&str, &PermissionSetReference)> {
    profile
        .0
        .iter()
        .filter(|(scope, _)| scope.as_str() != "*")
        .flat_map(|(resource_id, refs)| {
            refs.iter()
                .map(move |permission_set_ref| (resource_id.as_str(), permission_set_ref))
        })
        .collect()
}

fn terraform_label_segment(value: &str) -> String {
    value
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
