//! GCP ServiceAccount — IAM service account + per-permission-set custom
//! roles + project-level bindings.
//!
//! Mirrors what `GcpServiceAccountController` does at runtime:
//!
//! 1. `google_service_account` for the identity.
//! 2. One `google_project_iam_custom_role` per `PermissionSet` attached
//!    via `stack_permission_sets`, with the exact `included_permissions`
//!    list that `GcpRuntimePermissionsGenerator::generate_custom_role`
//!    would produce — so push and pull paths converge on the same role
//!    schema.
//! 3. One `google_project_iam_member` per binding, derived from
//!    `GcpRuntimePermissionsGenerator::generate_bindings`. Conditional
//!    bindings emit a `condition { }` block.
//!
//! Workload Identity wiring (binding the K8s ServiceAccount to this GCP
//! SA) happens in the GKE overlay — see `crate::k8s_identity`.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{
        downcast, emit_custom_role_and_bindings, permission_context, required_label,
        service_account_id_template, service_account_member_for_label,
    },
    expr,
};
use alien_core::{import::EmitContext, Result, ServiceAccount};
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
                    expr::template(format!("Alien {} service account", service_account.id)),
                ),
                attr(
                    "description",
                    expr::template(format!(
                        "${{var.stack_name}} stack service account for permission profile '{label}'"
                    )),
                ),
            ],
        ));

        // Member expression for re-use by all bindings.
        let member = service_account_member_for_label(label);

        // Per-permission-set custom roles + project-level bindings,
        // produced through the runtime permissions generator so the
        // emitted Terraform mirrors what the controller would do at
        // apply time.
        for permission_set in &service_account.stack_permission_sets {
            let context = permission_context(label);
            emit_custom_role_and_bindings(&mut fragment, label, &member, permission_set, &context)?;
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
}
