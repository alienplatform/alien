//! Shared helpers for GCP Terraform emitters.
//!
//! GCP-specific naming + tagging + identity. Resource names follow GCP's
//! lowercase-hyphenated convention; resource labels (the GCP-side
//! `labels` map) are kebab-case lowercase per the official style guide.
//! IAM bindings flow through service-account members (no domains, no
//! `allAuthenticatedUsers`); Workload Identity bindings flow through the
//! K8s identity overlay.

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::TfFragment,
    expr,
};
use alien_core::{
    import::EmitContext, ErrorData, PermissionSet, ResourceDefinition, ResourceRef, ResourceType,
    Result, ServiceAccount,
};
use alien_error::{AlienError, Context};
use alien_permissions::{
    generators::{
        GcpBindingTargetScope, GcpCustomRole, GcpIamBinding, GcpRuntimePermissionsGenerator,
    },
    BindingTarget, PermissionContext,
};
use hcl::{
    expr::Expression,
    structure::{Block, Structure},
};

/// Downcast `ctx.resource.config` to the typed resource definition or
/// return a typed `UnexpectedResourceType` error.
pub fn downcast<'a, T: ResourceDefinition>(
    ctx: &'a EmitContext<'_>,
    expected: ResourceType,
) -> Result<&'a T> {
    ctx.resource.config.downcast_ref::<T>().ok_or_else(|| {
        AlienError::new(ErrorData::UnexpectedResourceType {
            resource_id: ctx.resource_id.to_string(),
            expected,
            actual: ctx.resource.config.resource_type(),
        })
    })
}

/// Look up the precomputed Terraform label for the current emitter
/// context.
pub fn required_label<'a>(ctx: &'a EmitContext<'_>) -> Result<&'a str> {
    ctx.name_for(ctx.resource_id).ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: format!("missing terraform label for resource '{}'", ctx.resource_id),
        })
    })
}

/// Look up the precomputed label for a referenced resource.
pub fn label_for_ref<'a>(ctx: &'a EmitContext<'_>, reference: &ResourceRef) -> Result<&'a str> {
    ctx.name_for(reference.id()).ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: format!(
                "missing terraform label for referenced resource '{}'",
                reference.id()
            ),
        })
    })
}

/// `${local.resource_prefix}-{suffix}` HCL template expression for GCP
/// resource names. GCP names are lowercase-hyphenated already so this
/// matches AWS/Azure conventions byte-for-byte.
pub fn resource_prefix_template(suffix: &str) -> Expression {
    expr::template(format!("${{local.resource_prefix}}-{suffix}"))
}

/// GCP service-account IDs are capped at 30 chars and must start with a
/// letter and end with an alphanumeric character. Keep a readable prefix and
/// append an 8-char hash of the full resource identity so long resource
/// ids like `artifact-registry-pull` and `artifact-registry-push` do not
/// collide after truncation.
pub fn service_account_id_template(suffix: &str) -> Expression {
    expr::raw(format!(
        "format(\"%s-%s\", trim(substr(replace(lower(format(\"a-%s-{suffix}\", local.resource_prefix)), \"_\", \"-\"), 0, 21), \"-\"), substr(sha1(replace(lower(format(\"%s-{suffix}\", local.resource_prefix)), \"_\", \"-\")), 0, 8))"
    ))
}

/// Sanitized full Artifact Registry repository ID before length capping.
pub fn artifact_registry_repository_full_id_template(suffix: &str) -> Expression {
    expr::raw(format!(
        "replace(lower(format(\"%s-{suffix}\", local.resource_prefix)), \"_\", \"-\")"
    ))
}

/// GCP Artifact Registry repository IDs are capped at 63 chars. Keep
/// the readable deployment-prefixed name when it fits; otherwise keep a
/// deterministic prefix and append an 8-char hash of the full name.
pub fn artifact_registry_repository_id_from_local(local_name: &str) -> Expression {
    expr::raw(format!(
        "length(local.{local_name}) <= 63 ? local.{local_name} : format(\"%s-%s\", trim(substr(local.{local_name}, 0, 54), \"-\"), substr(sha1(local.{local_name}), 0, 8))"
    ))
}

/// Standard labels block for GCP. GCP labels must be lowercase kebab-case for
/// both keys and values, max 63 chars.
pub fn labels(ctx: &EmitContext<'_>, alien_resource_type: &'static str) -> Expression {
    let resource_id_label = sanitize_label_value(ctx.resource_id);
    expr::object([
        ("managed-by", Expression::String("deployment".to_string())),
        ("deployment", expr::raw("local.resource_prefix")),
        ("resource", Expression::String(resource_id_label)),
        (
            "resource-type",
            Expression::String(alien_resource_type.to_string()),
        ),
    ])
}

/// GCP labels disallow `_`, uppercase, and dots. Replace each invalid
/// char with `-` and lowercase the rest. Truncate to 63 characters per
/// GCP's hard limit.
pub fn sanitize_label_value(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('-');
        }
    }
    if out.len() > 63 {
        out.truncate(63);
    }
    out
}

/// Look up the IAM service-account email expression for an Alien
/// `service-account` resource by permissions profile name (the
/// `<profile>-sa` convention used across cloud emitters).
pub fn service_account_email(ctx: &EmitContext<'_>, profile_name: &str) -> Option<Expression> {
    service_account_attribute(ctx, profile_name, "email")
}

/// Look up the fully-qualified IAM service-account resource name for an Alien
/// `service-account` resource. Some GCP APIs, including Cloud Build triggers,
/// require `projects/{project}/serviceAccounts/{account}` instead of the bare
/// email address accepted by Cloud Run and Eventarc.
pub fn service_account_name(ctx: &EmitContext<'_>, profile_name: &str) -> Option<Expression> {
    service_account_attribute(ctx, profile_name, "name")
}

fn service_account_attribute(
    ctx: &EmitContext<'_>,
    profile_name: &str,
    attribute: &'static str,
) -> Option<Expression> {
    let service_account_id = format!("{profile_name}-sa");
    let (_id, entry) = ctx
        .stack
        .resources()
        .find(|(id, _entry)| id.as_str() == service_account_id)?;
    entry.config.downcast_ref::<ServiceAccount>()?;
    let label = ctx.name_for(&service_account_id)?;
    Some(expr::traversal([
        "google_service_account",
        label,
        attribute,
    ]))
}

/// Build a `google_project_iam_member` block binding `role` to
/// `member`. `label` is the Terraform resource label (e.g.
/// `<sa>_storage_<bucket>_object_admin`).
pub fn project_iam_member(label: &str, role: &str, member: Expression) -> Block {
    resource_block(
        "google_project_iam_member",
        label,
        [
            attr("project", expr::raw("var.gcp_project")),
            attr("role", Expression::String(role.to_string())),
            attr("member", member),
        ],
    )
}

/// Build a `<resource>_iam_member` block — same shape as
/// `google_project_iam_member` but scoped to one resource. The block's
/// resource type and any non-standard attributes (e.g. `bucket`,
/// `topic`, `secret_id`) are passed in via `body`.
pub fn resource_iam_member(
    resource_type: &str,
    label: &str,
    body: impl IntoIterator<Item = Structure>,
) -> Block {
    resource_block(resource_type, label, body)
}

/// `serviceAccount:${<resource>.<label>.email}` member expression
/// pointing at a Terraform-emitted `google_service_account`. Renders
/// as a quoted template without losing the embedded interpolation.
pub fn service_account_member_for_label(label: &str) -> Expression {
    expr::template(format!(
        "serviceAccount:${{google_service_account.{label}.email}}"
    ))
}

/// `serviceAccount:${var.<variable>}` member expression for a
/// caller-supplied service-account variable.
pub fn service_account_member_for_var(variable: &str) -> Expression {
    expr::template(format!("serviceAccount:${{var.{variable}}}"))
}

/// Build a `PermissionContext` shared by every generator call. `label`
/// is the SA's terraform label and is used as `service_account_name`
/// so generators that mention `${variable.service_account_name}`
/// resolve correctly. Concrete project / region values are surfaced
/// as `var.gcp_project` / `var.gcp_region` so the rendered HCL stays
/// parameterised — the runtime generator interpolates them at apply
/// time, exactly the same way the controller does.
pub fn permission_context(label: &str, _stack_name: &str) -> PermissionContext {
    PermissionContext::new()
        .with_stack_prefix("${local.resource_prefix}".to_string())
        .with_deployment_name("${local.deployment_name}".to_string())
        .with_project_name("${var.gcp_project}".to_string())
        .with_project_number("${data.google_project.current.number}".to_string())
        .with_region("${var.gcp_region}".to_string())
        .with_service_account_name(label.to_string())
}

/// Emit a GCP custom role plus IAM bindings for `permission_set`.
pub fn emit_custom_role_and_bindings(
    fragment: &mut TfFragment,
    sa_label: &str,
    member_override: &Expression,
    permission_set: &PermissionSet,
    context: &PermissionContext,
) -> Result<()> {
    emit_custom_role_and_bindings_for_target(
        fragment,
        sa_label,
        member_override,
        permission_set,
        context,
        BindingTarget::Stack,
    )
}

pub fn emit_custom_role_and_bindings_for_target(
    fragment: &mut TfFragment,
    sa_label: &str,
    member_override: &Expression,
    permission_set: &PermissionSet,
    context: &PermissionContext,
    target: BindingTarget,
) -> Result<()> {
    if permission_set.platforms.gcp.is_none() {
        return Ok(());
    }

    let generator = GcpRuntimePermissionsGenerator::new();

    let grant_plan = generator
        .generate_grant_plan(permission_set, target, context)
        .context(ErrorData::GenericError {
            message: format!(
                "failed to generate GCP IAM grant plan for permission set '{}'",
                permission_set.id
            ),
        })?;
    let bindings = grant_plan.bindings_for_target(GcpBindingTargetScope::Project);
    let custom_roles = emit_custom_roles_for_bindings(fragment, &grant_plan, &bindings)?;

    for (idx, binding) in bindings.into_iter().enumerate() {
        let role = role_expression_for_binding(&binding.role, &custom_roles)?;
        let role_label = binding_label_for_role(&binding.role, &custom_roles)?;
        let binding_label = format!("{role_label}_{sa_label}_binding_{idx}",);
        push_iam_member(fragment, &binding_label, role, member_override, &binding)?;
    }

    Ok(())
}

pub(crate) fn emit_custom_roles_for_bindings(
    fragment: &mut TfFragment,
    grant_plan: &alien_permissions::generators::GcpGrantPlan,
    bindings: &[GcpIamBinding],
) -> Result<Vec<GcpCustomRole>> {
    let custom_roles = grant_plan.custom_roles_for_bindings(bindings);
    emit_selected_custom_roles(fragment, &custom_roles);
    Ok(custom_roles)
}

fn emit_selected_custom_roles(fragment: &mut TfFragment, custom_roles: &[GcpCustomRole]) {
    for custom_role in custom_roles {
        let role_label = custom_role_label(custom_role);
        let role_id = custom_role_id_template(custom_role);

        fragment.resource_blocks.push(resource_block(
            "google_project_iam_custom_role",
            &role_label,
            [
                attr("count", expr::raw("var.gcp_manage_custom_roles ? 1 : 0")),
                attr("project", expr::raw("var.gcp_project")),
                attr("role_id", role_id),
                attr("title", expr::template(custom_role.title.clone())),
                attr(
                    "description",
                    expr::template(custom_role.description.clone()),
                ),
                attr("stage", Expression::String(custom_role.stage.clone())),
                attr(
                    "permissions",
                    Expression::Array(
                        custom_role
                            .included_permissions
                            .iter()
                            .map(|p| Expression::String(p.clone()))
                            .collect(),
                    ),
                ),
            ],
        ));
    }
}

pub(crate) fn custom_role_label(custom_role: &GcpCustomRole) -> String {
    let suffix = custom_role_suffix(custom_role);
    format!("gcp_role_{}", suffix.replace('-', "_"))
}

pub(crate) fn binding_label_role_segment(role: &str) -> String {
    if let Some(predefined) = role.strip_prefix("roles/") {
        return predefined
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() {
                    ch.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect();
    }
    role.rsplit('/').next().unwrap_or(role).replace('-', "_")
}

pub(crate) fn binding_label_for_role(role: &str, custom_roles: &[GcpCustomRole]) -> Result<String> {
    if role.starts_with("roles/") {
        return Ok(binding_label_role_segment(role));
    }

    let custom_role = custom_role_for_binding(role, custom_roles)?;
    Ok(custom_role_label(custom_role))
}

pub(crate) fn role_expression_for_binding(
    role: &str,
    custom_roles: &[GcpCustomRole],
) -> Result<Expression> {
    if role.starts_with("roles/") {
        return Ok(Expression::String(role.to_string()));
    }

    let custom_role = custom_role_for_binding(role, custom_roles)?;
    let role_label = custom_role_label(custom_role);
    let suffix = custom_role_suffix(custom_role);
    Ok(expr::raw(format!(
        "var.gcp_manage_custom_roles ? google_project_iam_custom_role.{role_label}[0].name : format(\"projects/%s/roles/role_%s_{suffix}\", var.gcp_project, local.gcp_custom_role_prefix)"
    )))
}

fn custom_role_for_binding<'a>(
    role: &str,
    custom_roles: &'a [GcpCustomRole],
) -> Result<&'a GcpCustomRole> {
    custom_roles
        .iter()
        .find(|custom_role| custom_role.name == role)
        .ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: format!("missing generated custom role for GCP binding '{role}'"),
            })
        })
}

fn custom_role_id_template(custom_role: &GcpCustomRole) -> Expression {
    let suffix = custom_role_suffix(custom_role);
    expr::raw(format!(
        "format(\"role_%s_{suffix}\", local.gcp_custom_role_prefix)"
    ))
}

fn custom_role_suffix(custom_role: &GcpCustomRole) -> String {
    custom_role
        .role_id
        .strip_prefix("role_local_resource_pre_")
        .unwrap_or(&custom_role.role_id)
        .to_string()
}

pub(crate) fn push_iam_member(
    fragment: &mut TfFragment,
    binding_label: &str,
    role: Expression,
    member_override: &Expression,
    binding: &GcpIamBinding,
) -> Result<()> {
    let mut body: Vec<Structure> =
        vec![attr("role", role), attr("member", member_override.clone())];

    let resource_type = match binding.target {
        GcpBindingTargetScope::Project => {
            body.insert(0, attr("project", expr::raw("var.gcp_project")));
            "google_project_iam_member"
        }
        GcpBindingTargetScope::CurrentResource => {
            return Err(AlienError::new(ErrorData::GenericError {
                message: format!(
                    "cannot emit generic resource-level GCP IAM binding for role '{}'",
                    binding.role
                ),
            }));
        }
    };

    if let Some(condition) = &binding.condition {
        // The expression has already been interpolated by
        // `GcpRuntimePermissionsGenerator` against the Terraform-side
        // permission context (`local.resource_prefix` / `var.gcp_project` /
        // `var.gcp_region`). Escape CEL string quotes but leave Terraform
        // `${...}` interpolation intact for apply time.
        body.push(nested(block(
            "condition",
            [
                attr("title", Expression::String(condition.title.clone())),
                attr(
                    "description",
                    Expression::String(condition.description.clone()),
                ),
                attr(
                    "expression",
                    expr::template(escape_template_string_body(&condition.expression)),
                ),
            ],
        )));
    }

    fragment
        .resource_blocks
        .push(resource_block(resource_type, binding_label, body));
    Ok(())
}

fn escape_template_string_body(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
