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
    generators::{GcpIamBinding, GcpRuntimePermissionsGenerator},
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

/// `${var.stack_name}-{suffix}` HCL template expression for GCP
/// resource names. GCP names are lowercase-hyphenated already so this
/// matches AWS/Azure conventions byte-for-byte.
pub fn stack_name_template(suffix: &str) -> Expression {
    expr::template(format!("${{var.stack_name}}-{suffix}"))
}

/// GCP service-account IDs are capped at 30 chars and must start with a
/// letter and end with an alphanumeric character. Keep a readable prefix and
/// append an 8-char hash of the full stack/resource identity so long resource
/// ids like `artifact-registry-pull` and `artifact-registry-push` do not
/// collide after truncation.
pub fn service_account_id_template(suffix: &str) -> Expression {
    expr::raw(format!(
        "format(\"%s-%s\", trim(substr(replace(lower(format(\"a-%s-{suffix}\", var.stack_name)), \"_\", \"-\"), 0, 21), \"-\"), substr(sha1(replace(lower(format(\"%s-{suffix}\", var.stack_name)), \"_\", \"-\")), 0, 8))"
    ))
}

/// GCP custom role ids are project-global. Scope every generated role id to
/// the stack name and owning resource label so parallel distribution tests,
/// repeated applies, and multiple identities in one stack do not try to create
/// the same `storageDataRead`/`functionExecute` role in the target project.
fn custom_role_id_template(owner_label: &str, role_id: &str) -> Expression {
    let owner_prefix = role_id_segment(owner_label, 12);
    let role_id_prefix = role_id_segment(role_id, 26);
    expr::raw(format!(
        "format(\"%s_{owner_prefix}_{role_id_prefix}_%s\", substr(replace(var.stack_name, \"-\", \"_\"), 0, 12), substr(sha1(format(\"%s-{owner_label}-{role_id}\", var.stack_name)), 0, 8))"
    ))
}

fn role_id_segment(value: &str, max_len: usize) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .take(max_len)
        .collect()
}

/// Standard Alien labels block for GCP. GCP labels must be lowercase
/// kebab-case for both keys and values, max 63 chars.
pub fn labels(ctx: &EmitContext<'_>, alien_resource_type: &'static str) -> Expression {
    let resource_id_label = sanitize_label_value(ctx.resource_id);
    expr::object([
        ("managed-by", Expression::String("alien-dev".to_string())),
        ("alien-stack-id", expr::raw("var.stack_name")),
        ("alien-resource-id", Expression::String(resource_id_label)),
        (
            "alien-resource-type",
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
    let service_account_id = format!("{profile_name}-sa");
    let (_id, entry) = ctx
        .stack
        .resources()
        .find(|(id, _entry)| id.as_str() == service_account_id)?;
    entry.config.downcast_ref::<ServiceAccount>()?;
    let label = ctx.name_for(&service_account_id)?;
    Some(expr::traversal(["google_service_account", label, "email"]))
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
pub fn permission_context(label: &str) -> PermissionContext {
    PermissionContext::new()
        .with_stack_prefix("${var.stack_name}".to_string())
        .with_project_name("${var.gcp_project}".to_string())
        .with_project_number("${data.google_project.current.number}".to_string())
        .with_region("${var.gcp_region}".to_string())
        .with_service_account_name(label.to_string())
}

/// Emit one `google_project_iam_custom_role` + matching
/// `google_project_iam_member` bindings for `permission_set`.
///
/// Mirrors the runtime path:
///
/// * `GcpRuntimePermissionsGenerator::generate_custom_role` produces
///   the role definition (`role_id`, `included_permissions`, …).
/// * `GcpRuntimePermissionsGenerator::generate_bindings` produces the
///   bindings the controller would attach via
///   `google_project_iam_policy`.
///
/// `member_override` lets callers swap the generator-derived member
/// (`serviceAccount:<email>@<project>...`) for the actual Terraform
/// resource reference (`serviceAccount:${google_service_account.x.email}`).
/// The Terraform-side member is always the right answer because the
/// controller doesn't know the email up front either; the role
/// definition is what we want to reuse.
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
    let custom_role = generator
        .generate_custom_role(permission_set, context)
        .context(ErrorData::GenericError {
            message: format!(
                "failed to generate GCP custom role for permission set '{}'",
                permission_set.id
            ),
        })?;

    let role_id = custom_role
        .name
        .rsplit('/')
        .next()
        .unwrap_or(&custom_role.name)
        .to_string();
    let role_label = format!("{sa_label}_{}", sanitize_label_value(&role_id));

    fragment.resource_blocks.push(resource_block(
        "google_project_iam_custom_role",
        &role_label,
        [
            attr(
                "count",
                expr::raw("var.gcp_use_existing_custom_roles ? 0 : 1"),
            ),
            attr("project", expr::raw("var.gcp_project")),
            attr("role_id", custom_role_id_template(sa_label, &role_id)),
            attr("title", Expression::String(custom_role.title.clone())),
            attr(
                "description",
                Expression::String(custom_role.description.clone()),
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

    let bindings = generator
        .generate_bindings(permission_set, target, context)
        .context(ErrorData::GenericError {
            message: format!(
                "failed to generate GCP IAM bindings for permission set '{}'",
                permission_set.id
            ),
        })?;

    for (idx, binding) in bindings.bindings.into_iter().enumerate() {
        let binding_label = if bindings_count(&binding) {
            format!("{role_label}_binding_{idx}")
        } else {
            format!("{role_label}_binding")
        };
        let role_expression = expr::raw(format!(
            "var.gcp_use_existing_custom_roles ? format(\"projects/%s/roles/{role_id}\", var.gcp_project) : google_project_iam_custom_role.{role_label}[0].name"
        ));
        push_iam_member(
            fragment,
            &binding_label,
            role_expression,
            member_override,
            &binding,
        )?;
    }

    Ok(())
}

fn bindings_count(_binding: &GcpIamBinding) -> bool {
    // Always suffix bindings with their index — the conditional binding
    // shape is symmetric with the unconditional one this way.
    true
}

fn push_iam_member(
    fragment: &mut TfFragment,
    binding_label: &str,
    role: Expression,
    member_override: &Expression,
    binding: &GcpIamBinding,
) -> Result<()> {
    let mut body: Vec<Structure> = vec![
        attr("project", expr::raw("var.gcp_project")),
        attr("role", role),
        attr("member", member_override.clone()),
    ];

    if let Some(condition) = &binding.condition {
        // The expression has already been interpolated by
        // `GcpRuntimePermissionsGenerator` against the Terraform-side
        // permission context (`var.stack_name` / `var.gcp_project` /
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

    fragment.resource_blocks.push(resource_block(
        "google_project_iam_member",
        binding_label,
        body,
    ));
    Ok(())
}

fn escape_template_string_body(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
