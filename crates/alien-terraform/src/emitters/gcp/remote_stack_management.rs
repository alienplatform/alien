//! Setup-owned management account and customer-owned federation trust.

use crate::{
    block::{attr, data_block, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{
        binding_label_for_role, downcast, emit_custom_roles_for_bindings,
        emit_selected_custom_roles, gate_bindings, permission_context, push_iam_member,
        required_label, role_expression_for_binding, service_account_id_template,
        service_account_member_for_label, CUSTOM_ROLE_NAMESPACE_PLACEHOLDER,
    },
    expr,
};
use alien_core::{
    import::EmitContext, ErrorData, KubernetesCluster, PermissionProfile, PermissionSet,
    PermissionSetReference, RemoteStackManagement, Result,
};
use alien_error::{AlienError, Context};
use alien_permissions::{
    generators::{
        gcp_project_iam_write_role, gcp_project_role_grant_conditions, gcp_runtime_roles,
        strip_gcp_management_iam_writes, GcpBindingTargetScope, GcpIamBinding,
        GcpRuntimePermissionsGenerator,
    },
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
        let mut project_iam_write = false;
        if let Some(profile) = ctx.stack.management().profile() {
            for permission_set_ref in global_permission_refs(ctx, profile) {
                if let Some(permission_set) = permission_set_ref
                    .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                {
                    project_iam_write |= emit_management_identity_bindings(
                        &mut fragment,
                        label,
                        &member,
                        &permission_set,
                        &context,
                        BindingTarget::Stack,
                        true,
                    )?;
                }
            }
            for (resource_id, permission_set_ref) in resource_scoped_permission_refs(profile) {
                let Some(resource_entry) = ctx.stack.resources.get(resource_id) else {
                    continue;
                };
                let include_project_bindings = resource_entry
                    .config
                    .downcast_ref::<KubernetesCluster>()
                    .is_some();
                if let Some(permission_set) = permission_set_ref
                    .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                {
                    let binding_label = format!("{label}_{}", terraform_label_segment(resource_id));
                    let appended_from = fragment.resource_blocks.len();
                    let resource_context = context
                        .clone()
                        .with_resource_name(format!("${{local.resource_prefix}}-{resource_id}"));
                    project_iam_write |= emit_management_identity_bindings(
                        &mut fragment,
                        &binding_label,
                        &member,
                        &permission_set,
                        &resource_context,
                        BindingTarget::Resource,
                        include_project_bindings,
                    )?;
                    if !include_project_bindings {
                        gate_bindings(
                            &mut fragment,
                            appended_from,
                            resource_entry.enabled_when.as_deref(),
                        );
                    }
                }
            }
        }

        if project_iam_write {
            emit_runtime_project_role_grants(&mut fragment, ctx, label, &member, &context)?;
        }

        super::federation::emit(&mut fragment, label, &RemoteStackManagement::RESOURCE_TYPE);

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        let fields = vec![
            (
                "federation",
                super::federation::import_ref(label, &RemoteStackManagement::RESOURCE_TYPE),
            ),
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
        ];
        Ok(expr::object(fields))
    }
}

fn emit_management_identity_bindings(
    fragment: &mut TfFragment,
    label: &str,
    member: &Expression,
    permission_set: &PermissionSet,
    context: &PermissionContext,
    binding_target: BindingTarget,
    include_project_bindings: bool,
) -> Result<bool> {
    if permission_set.platforms.gcp.is_none() {
        return Ok(false);
    }

    let generator = GcpRuntimePermissionsGenerator::new();
    let mut grant_plan = generator
        .generate_grant_plan(permission_set, binding_target, context)
        .context(ErrorData::GenericError {
            message: format!(
                "failed to generate GCP remote management IAM grant plan for '{}'",
                permission_set.id
            ),
        })?;
    let project_iam_write = strip_gcp_management_iam_writes(&mut grant_plan);
    let mut bindings = if include_project_bindings {
        grant_plan.bindings_for_target(GcpBindingTargetScope::Project)
    } else {
        Vec::new()
    };
    bindings.extend(grant_plan.bindings_for_target(GcpBindingTargetScope::ServiceAccount));
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
        )?;
    }

    Ok(project_iam_write)
}

/// Create every custom role the runtime binds, and bind management's project IAM write only under
/// conditions that admit those roles and the predefined roles the runtime binds.
fn emit_runtime_project_role_grants(
    fragment: &mut TfFragment,
    ctx: &EmitContext<'_>,
    label: &str,
    member: &Expression,
    context: &PermissionContext,
) -> Result<()> {
    let runtime = gcp_runtime_roles(ctx.stack, context).context(ErrorData::GenericError {
        message: "failed to resolve the GCP roles the runtime binds".to_string(),
    })?;
    emit_selected_custom_roles(fragment, &runtime.custom_roles);

    let write_role = gcp_project_iam_write_role(context);
    emit_selected_custom_roles(fragment, std::slice::from_ref(&write_role));
    let role_label = binding_label_for_role(&write_role.name, std::slice::from_ref(&write_role))?;
    // Role names carry the namespace placeholder; the condition needs them as Terraform renders
    // the role ids.
    let placeholder = format!("role_{CUSTOM_ROLE_NAMESPACE_PLACEHOLDER}_");
    for (index, mut condition) in gcp_project_role_grant_conditions(&runtime.project_roles)
        .into_iter()
        .enumerate()
    {
        condition.expression = condition
            .expression
            .replace(&placeholder, "role_${local.gcp_custom_role_prefix}_");
        push_iam_member(
            fragment,
            &format!("{role_label}_{label}_binding_{index}"),
            role_expression_for_binding(&write_role.name, std::slice::from_ref(&write_role))?,
            member,
            &GcpIamBinding {
                role: write_role.name.clone(),
                members: Vec::new(),
                target: GcpBindingTargetScope::Project,
                target_resource_name: None,
                resource_kind: None,
                condition: Some(condition),
            },
        )?;
    }
    Ok(())
}

fn global_permission_refs<'a>(
    ctx: &EmitContext<'_>,
    profile: &'a PermissionProfile,
) -> Vec<&'a PermissionSetReference> {
    alien_permissions::management_identity_global_refs(ctx.stack.resources.values(), profile)
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
