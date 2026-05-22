//! GCP ServiceAccount — IAM service account + custom-role project-level bindings.
//!
//! Mirrors what `GcpServiceAccountController` does at runtime:
//!
//! 1. `google_service_account` for the identity.
//! 2. One `google_project_iam_member` per custom-role binding, derived from
//!    `GcpRuntimePermissionsGenerator::generate_bindings`. Conditional
//!    bindings emit a `condition { }` block.
//!
//! Workload Identity wiring (binding the K8s ServiceAccount to this GCP
//! SA) happens in the GKE overlay — see `crate::k8s_identity`.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{
        binding_label_for_role, downcast, emit_custom_roles, permission_context, push_iam_member,
        required_label, role_expression_for_binding, service_account_id_template,
        service_account_member_for_label,
    },
    expr,
};
use alien_core::{import::EmitContext, ErrorData, PermissionSet, Result, ServiceAccount};
use alien_error::AlienError;
use alien_permissions::{
    generators::{GcpBindingTargetScope, GcpRuntimePermissionsGenerator},
    BindingTarget, PermissionContext,
};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpServiceAccountEmitter;

impl TfEmitter for GcpServiceAccountEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let service_account = downcast::<ServiceAccount>(ctx, ServiceAccount::RESOURCE_TYPE)?;
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
                    expr::template(format!("Deployment {} service account", service_account.id)),
                ),
                attr(
                    "description",
                    expr::template(format!(
                        "${{local.resource_prefix}} deployment service account for permission profile '{label}'"
                    )),
                ),
            ],
        ));

        // Member expression for re-use by all bindings.
        let member = service_account_member_for_label(label);

        for permission_set in &service_account.stack_permission_sets {
            let context = permission_context(label, ctx.stack.id());
            emit_project_stack_bindings(&mut fragment, label, &member, permission_set, &context)?;
        }

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            ("projectId", expr::raw("var.gcp_project")),
            (
                "serviceAccountEmail",
                expr::traversal(["google_service_account", label, "email"]),
            ),
            (
                "serviceAccountUniqueId",
                expr::traversal(["google_service_account", label, "unique_id"]),
            ),
            ("stackPermissionsApplied", Expression::Bool(true)),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        Ok(Some(expr::object([
            (
                "service",
                Expression::String("gcpserviceaccount".to_string()),
            ),
            (
                "email",
                expr::traversal(["google_service_account", label, "email"]),
            ),
            (
                "uniqueId",
                expr::traversal(["google_service_account", label, "unique_id"]),
            ),
        ])))
    }
}

fn emit_project_stack_bindings(
    fragment: &mut TfFragment,
    label: &str,
    member: &Expression,
    permission_set: &PermissionSet,
    context: &PermissionContext,
) -> Result<()> {
    if permission_set.platforms.gcp.is_none() {
        return Ok(());
    }

    let custom_roles = emit_custom_roles(fragment, permission_set, context)?;
    let generator = GcpRuntimePermissionsGenerator::new();
    let bindings = generator
        .generate_bindings(permission_set, BindingTarget::Stack, context)
        .map_err(|err| {
            AlienError::new(ErrorData::GenericError {
                message: format!(
                    "failed to generate GCP service-account IAM bindings for '{}': {}",
                    permission_set.id, err
                ),
            })
        })?;

    for (idx, binding) in bindings.bindings.into_iter().enumerate() {
        if binding.target != GcpBindingTargetScope::Project {
            continue;
        }

        let role_label = binding_label_for_role(&binding.role, &custom_roles)?;
        let role = role_expression_for_binding(&binding.role, &custom_roles)?;
        push_iam_member(
            fragment,
            &format!("{role_label}_{label}_binding_{idx}"),
            role,
            member,
            &binding,
        )?;
    }

    Ok(())
}
