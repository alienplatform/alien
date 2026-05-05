//! GCP RemoteStackManagement — service account the manager impersonates.
//!
//! Mirrors `GcpRemoteStackManagementController`:
//!
//! 1. Create a `google_service_account` for the management identity.
//! 2. For every permission set in `ctx.stack.management().profile()`,
//!    emit a `google_project_iam_custom_role` + matching
//!    `google_project_iam_member` binding through
//!    [`emit_custom_role_and_bindings`]. The role contents come
//!    straight from `GcpRuntimePermissionsGenerator::generate_custom_role`,
//!    so apply-time and TF-time bind identical role schemas.
//! 3. Grant `roles/iam.serviceAccountTokenCreator` +
//!    `roles/iam.serviceAccountUser` on the management SA to the
//!    caller-supplied manager identity (`var.managing_service_account_email`).
//!    The variable defaults to `""` so the bindings are no-ops in
//!    pure-OSS scenarios where there's no central manager.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{
        downcast, emit_custom_role_and_bindings, permission_context, required_label,
        service_account_member_for_label,
    },
    expr,
};
use alien_core::{
    import::EmitContext, PermissionProfile, PermissionSetReference, RemoteStackManagement, Result,
};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpRemoteStackManagementEmitter;

impl TfEmitter for GcpRemoteStackManagementEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let _ = downcast::<RemoteStackManagement>(ctx, RemoteStackManagement::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let mut fragment = TfFragment::default();

        let account_id_template = expr::raw(format!(
            "substr(replace(\"${{var.stack_name}}-{label}\", \"_\", \"-\"), 0, 30)"
        ));
        fragment.resource_blocks.push(resource_block(
            "google_service_account",
            label,
            [
                attr("project", expr::raw("var.gcp_project")),
                attr("account_id", account_id_template),
                attr(
                    "display_name",
                    expr::template("Alien stack management identity".to_string()),
                ),
                attr(
                    "description",
                    expr::template(
                        "${var.stack_name} cross-account management service account".to_string(),
                    ),
                ),
            ],
        ));

        // Per-permission-set custom roles + bindings, derived from the
        // stack's management profile via alien-permissions. Matches
        // exactly what GcpRemoteStackManagementController emits at run
        // time.
        let member = service_account_member_for_label(label);
        let context = permission_context(label);
        if let Some(profile) = ctx.stack.management().profile() {
            for permission_set_ref in global_permission_refs(profile) {
                if let Some(permission_set) = permission_set_ref
                    .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                {
                    emit_custom_role_and_bindings(
                        &mut fragment,
                        label,
                        &member,
                        &permission_set,
                        &context,
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

fn global_permission_refs(profile: &PermissionProfile) -> Vec<&PermissionSetReference> {
    profile
        .0
        .get("*")
        .map(|refs| refs.iter().collect())
        .unwrap_or_default()
}
