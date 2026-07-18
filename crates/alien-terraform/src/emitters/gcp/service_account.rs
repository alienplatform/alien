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
    emitters::gates::permission_gate_count,
    emitters::gcp::helpers::{
        binding_label_for_role, downcast, emit_custom_roles_for_bindings, permission_context,
        push_iam_member, required_label, role_expression_for_binding, service_account_id_template,
        service_account_member_for_label,
    },
    expr,
};
use alien_core::{
    import::EmitContext, ErrorData, PermissionSet, PermissionSetReference, Result, ServiceAccount,
};
use alien_error::AlienError;
use alien_permissions::{
    generators::{GcpBindingTargetScope, GcpRuntimePermissionsGenerator},
    BindingTarget, PermissionContext,
};
use hcl::expr::Expression;
use hcl::Block;

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
                    expr::template(format!(
                        "${{local.deployment_name}}: Runtime service account ({})",
                        service_account.id
                    )),
                ),
                attr(
                    "description",
                    expr::template(format!(
                        "Runtime cloud identity for ${{local.deployment_name}}. Resource prefix: ${{local.resource_prefix}}. Resource: {label}."
                    )),
                ),
            ],
        ));

        // Member expression for re-use by all bindings.
        let member = service_account_member_for_label(label);

        let profile_name = service_account.id.strip_suffix("-sa");
        for permission_set in &service_account.stack_permission_sets {
            let gate_count = match profile_name {
                Some(profile) => permission_gate_count(ctx, profile, &permission_set.id, &["*"])?,
                None => None,
            };
            let context = permission_context(label, ctx.stack.id());
            emit_project_bindings(
                &mut fragment,
                label,
                &member,
                permission_set,
                &context,
                BindingTarget::Stack,
                "stack",
                gate_count,
            )?;
        }

        if let Some(profile_name) = profile_name {
            if let Some(profile) = ctx.stack.permission_profiles().get(profile_name) {
                for (resource_id, permission_set_refs) in &profile.0 {
                    if resource_id == "*" {
                        continue;
                    }
                    let context = permission_context(label, ctx.stack.id())
                        .with_resource_name(format!("${{local.resource_prefix}}-{resource_id}"));
                    for permission_set_ref in permission_set_refs {
                        if let Some(permission_set) = resolve_permission_set(permission_set_ref) {
                            let gate_count = permission_gate_count(
                                ctx,
                                profile_name,
                                &permission_set.id,
                                &[resource_id.as_str()],
                            )?;
                            emit_project_bindings(
                                &mut fragment,
                                label,
                                &member,
                                &permission_set,
                                &context,
                                BindingTarget::Resource,
                                resource_id,
                                gate_count,
                            )?;
                        }
                    }
                }
            }
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

fn emit_project_bindings(
    fragment: &mut TfFragment,
    label: &str,
    member: &Expression,
    permission_set: &PermissionSet,
    context: &PermissionContext,
    binding_target: BindingTarget,
    scope_label: &str,
    gate_count: Option<Expression>,
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
                    "failed to generate GCP service-account IAM grant plan for '{}': {}",
                    permission_set.id, err
                ),
            })
        })?;
    let bindings = grant_plan.bindings_for_target(GcpBindingTargetScope::Project);
    let custom_roles = emit_custom_roles_for_bindings(fragment, &grant_plan, &bindings)?;

    for (idx, binding) in bindings.into_iter().enumerate() {
        let role_label = binding_label_for_role(&binding.role, &custom_roles)?;
        let role = role_expression_for_binding(&binding.role, &custom_roles)?;
        let binding_label = unique_iam_member_label(
            fragment,
            &role_label,
            label,
            &permission_set.id,
            scope_label,
            idx,
        );
        push_iam_member(
            fragment,
            &binding_label,
            role,
            member,
            &binding,
            gate_count.as_ref(),
        )?;
    }

    Ok(())
}

fn resolve_permission_set(permission_set_ref: &PermissionSetReference) -> Option<PermissionSet> {
    permission_set_ref.resolve(|name| alien_permissions::get_permission_set(name).cloned())
}

fn unique_iam_member_label(
    fragment: &TfFragment,
    role_label: &str,
    service_account_label: &str,
    permission_set_id: &str,
    scope_label: &str,
    idx: usize,
) -> String {
    let base = format!("{role_label}_{service_account_label}_binding_{idx}");
    if !iam_member_label_exists(fragment, &base) {
        return base;
    }

    let scope = terraform_label_segment(scope_label);
    let permission = terraform_label_segment(permission_set_id);
    let scoped = format!("{role_label}_{scope}_{permission}_{service_account_label}_binding_{idx}");
    if !iam_member_label_exists(fragment, &scoped) {
        return scoped;
    }

    let mut suffix = 2;
    loop {
        let candidate = format!("{scoped}_{suffix}");
        if !iam_member_label_exists(fragment, &candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

fn iam_member_label_exists(fragment: &TfFragment, label: &str) -> bool {
    fragment.resource_blocks.iter().any(|block| {
        is_iam_member_block(block)
            && block
                .labels
                .get(1)
                .is_some_and(|existing| existing.as_str() == label)
    })
}

fn is_iam_member_block(block: &Block) -> bool {
    block.identifier.as_str() == "resource"
        && block
            .labels
            .first()
            .is_some_and(|provider_type| provider_type.as_str().ends_with("_iam_member"))
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
