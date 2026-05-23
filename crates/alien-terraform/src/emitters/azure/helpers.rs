//! Shared helpers for Azure Terraform emitters.
//!
//! Mirrors the GCP shape ([`super::super::gcp::helpers`]) but stamps Azure
//! conventions:
//!
//! * `azurerm_*` resource references for IAM (the `principal_id` of a
//!   `azurerm_user_assigned_identity` rather than the `email` of a
//!   `google_service_account`).
//! * `tags` map literal in HCL — the Azure portal calls them tags, not
//!   labels, but the generated key set is identical.
//! * Resource names lowercase + sanitised to Azure's per-service rules.
//!   The two we have to handle in HCL today: storage account names are
//!   `[a-z0-9]{3,24}` (no hyphens, lowercase), and role assignment names
//!   are `uuidv5("dns", "${resource_prefix}-${role}-${principal}")` so they
//!   stay stable across applies.
//!
//! When the runtime controller surface in
//! [`alien_infra::service_account::azure`] calls
//! [`AzureRuntimePermissionsGenerator::generate_role_definition`], the
//! Terraform side calls the same method with the same
//! [`PermissionContext`] and converts the resulting
//! [`AzureRoleDefinition`] into an `azurerm_role_definition` block — that
//! way push-mode and pull-mode produce byte-identical IAM, which is the
//! whole point of the per-permission-set generator.

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::TfFragment,
    expr,
};
use alien_core::{
    import::EmitContext, permissions::PermissionSetReference, BindingValue,
    ContainerAppsEnvironmentBinding, ErrorData, PermissionSet, ResourceDefinition, ResourceRef,
    ResourceType, Result, ServiceAccount, Stack,
};
use alien_error::{AlienError, Context};
use alien_permissions::{
    generators::AzureRuntimePermissionsGenerator, BindingTarget, PermissionContext,
};
use hcl::expr::Expression;
use std::collections::HashSet;

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

/// Look up the precomputed Terraform label for the current emitter context.
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

/// `${local.resource_prefix}-{suffix}` template. Azure resources accept hyphenated
/// kebab-case names except where noted (see [`storage_account_name`]).
pub fn resource_prefix_template(suffix: &str) -> Expression {
    expr::template(format!("${{local.resource_prefix}}-{suffix}"))
}

/// Standard tags map for Azure. Same key set as GCP labels — the `tags` block
/// accepts arbitrary string values, no kebab-case constraint.
pub fn tags(ctx: &EmitContext<'_>, alien_resource_type: &'static str) -> Expression {
    expr::object([
        ("managed-by", Expression::String("deployment".to_string())),
        ("deployment", expr::raw("local.resource_prefix")),
        ("resource", Expression::String(ctx.resource_id.to_string())),
        (
            "resource-type",
            Expression::String(alien_resource_type.to_string()),
        ),
    ])
}

/// Look up the `principal_id` expression for an Alien `service-account`
/// resource by permissions profile name (the `<profile>-sa` convention used
/// across cloud emitters).
///
/// The GCP analogue surfaces an `email`; the Azure analogue surfaces a
/// `principal_id` (the GUID Azure AD assigns to the managed identity).
/// Callers that compose role assignments must treat this expression as a
/// raw GUID string — quoting and template interpolation happen at the
/// `azurerm_role_assignment` block, not here.
pub fn service_account_principal_id(
    ctx: &EmitContext<'_>,
    profile_name: &str,
) -> Option<Expression> {
    let service_account_id = format!("{profile_name}-sa");
    let (_id, entry) = ctx
        .stack
        .resources()
        .find(|(id, _entry)| id.as_str() == service_account_id)?;
    entry.config.downcast_ref::<ServiceAccount>()?;
    let label = ctx.name_for(&service_account_id)?;
    Some(expr::traversal([
        "azurerm_user_assigned_identity",
        label,
        "principal_id",
    ]))
}

/// Return an external Container Apps Environment binding for `resource_id`,
/// when the caller supplied one in stack settings.
pub fn container_apps_environment_binding<'a>(
    ctx: &'a EmitContext<'_>,
    resource_id: &str,
) -> Result<Option<&'a ContainerAppsEnvironmentBinding>> {
    let Some(bindings) = &ctx.stack_settings.external_bindings else {
        return Ok(None);
    };
    bindings.get_container_apps_environment(resource_id)
}

/// Convert a binding value into a Terraform string expression.
///
/// Terraform modules currently only support concrete external binding values.
/// Secret refs and template expressions are runtime-controller concepts; letting
/// them through here would render invalid or misleading HCL.
pub fn binding_string_expr(
    resource_id: &str,
    field_name: &str,
    value: &BindingValue<String>,
) -> Result<Expression> {
    match value {
        BindingValue::Value(value) => Ok(Expression::String(value.clone())),
        BindingValue::Expression(_) | BindingValue::SecretRef { .. } => Err(AlienError::new(
            ErrorData::GenericError {
                message: format!(
                    "Terraform external binding for resource '{resource_id}' field '{field_name}' must be a concrete string value"
                ),
            },
        )),
    }
}

/// Extract a concrete string from a binding value for HCL templates.
pub fn binding_string_value(
    resource_id: &str,
    field_name: &str,
    value: &BindingValue<String>,
) -> Result<String> {
    match value {
        BindingValue::Value(value) => Ok(value.clone()),
        BindingValue::Expression(_) | BindingValue::SecretRef { .. } => Err(AlienError::new(
            ErrorData::GenericError {
                message: format!(
                    "Terraform external binding for resource '{resource_id}' field '{field_name}' must be a concrete string value"
                ),
            },
        )),
    }
}

/// Build a `PermissionContext` shared by every generator call. `label` is
/// the SA's terraform label and is used as `service_account_name` so
/// generators that mention `${variable.service_account_name}` resolve
/// correctly.
///
/// The Azure permission context carries:
/// * `stack_prefix` → `${local.resource_prefix}` (matches AWS / GCP).
/// * `subscription_id` → `${var.azure_subscription_id}` — the
///   `subscriptions/<sub>/...` segment of every Azure resource ID.
/// * `resource_group` → `${var.azure_resource_group}`.
/// * `managing_subscription_id` / `managing_resource_group` default to the
///   target sub/RG (single-subscription mode). Cross-subscription
///   management is wired up by `remote_stack_management/azure.rs` and the
///   matching emitter, both of which pass an explicit
///   `managing_subscription_id` instead of relying on this default.
/// * `storage_account_name` → see [`storage_account_name_local`]. Computed
///   as a `locals` block so the same expression can be referenced from
///   `kv` / `vault` emitters without re-deriving it.
pub fn permission_context(label: &str) -> PermissionContext {
    PermissionContext::new()
        .with_stack_prefix("${local.resource_prefix}".to_string())
        .with_subscription_id("${var.azure_subscription_id}".to_string())
        .with_resource_group("${var.azure_resource_group_name}".to_string())
        .with_managing_subscription_id("${var.azure_subscription_id}".to_string())
        .with_managing_resource_group("${var.azure_resource_group_name}".to_string())
        .with_storage_account_name("${local.alien_storage_account_name}".to_string())
        .with_service_account_name(label.to_string())
}

/// HCL `locals` expression that derives the Azure storage account name
/// deterministically.
///
/// The runtime controller in
/// [`alien_infra::infra_requirements::generate_storage_account_name`] uses
/// `lower(replace("{prefix}{suffix}", "-", ""))` truncated to 24 chars.
/// Reproduce in HCL so push and pull converge on the same name without an
/// extra round-trip:
///
/// ```hcl
/// locals {
///   alien_storage_account_name = substr(
///     replace(lower("${local.resource_prefix}default"), "-", ""),
///     0,
///     24,
///   )
/// }
/// ```
///
/// Don't surface this as a `var.azure_storage_account_name` — the
/// generator emits a single `locals.tf` and any per-resource override
/// would silently drift between cloud-discovery and the executor.
pub fn storage_account_name_local() -> Expression {
    expr::raw("substr(replace(lower(\"${local.resource_prefix}default\"), \"-\", \"\"), 0, 24)")
}

/// Emit `azurerm_role_definition` + `azurerm_role_assignment` blocks for
/// `permission_set`. Mirror of the GCP custom-role binding
/// helper.
///
/// `principal_id_expr` is intentionally an explicit parameter — the GCP
/// shape derives it from the context's service-account name, but Azure
/// uses a different resource type for the principal (`principal_id` of a
/// `azurerm_user_assigned_identity`) and the call sites need to be able
/// to override it (e.g. AKS workload identity overlay).
///
/// Role assignment names use `uuidv5("dns", "{resource_prefix}-{role}-{principal}")`
/// so they stay stable across applies — Azure rejects role assignments
/// with non-GUID names.
pub fn emit_role_definition_and_assignments(
    fragment: &mut TfFragment,
    sa_label: &str,
    service_account_id: &str,
    role_index: usize,
    principal_id_expr: Expression,
    permission_set: &PermissionSet,
    context: &PermissionContext,
) -> Result<()> {
    if permission_set.platforms.azure.is_none() {
        return Ok(());
    }

    let generator = AzureRuntimePermissionsGenerator::new();
    let role_definition = generator
        .generate_role_definition(permission_set, BindingTarget::Stack, context)
        .context(ErrorData::GenericError {
            message: format!(
                "failed to generate Azure role definition for permission set '{}'",
                permission_set.id
            ),
        })?;

    let role_label = format!("{sa_label}_{}", sanitize_role_label(&permission_set.id));
    let role_name = format!("${{local.resource_prefix}}-{service_account_id}-{role_index}");

    fragment.resource_blocks.push(resource_block(
        "azurerm_role_definition",
        &role_label,
        [
            attr("name", expr::raw(&format!("\"{role_name}\""))),
            attr(
                "role_definition_id",
                expr::raw(&format!(
                    "uuidv5(\"oid\", \"deployment:azure:role-def:{role_name}\")"
                )),
            ),
            attr(
                "scope",
                expr::raw(
                    "\"/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}\"",
                ),
            ),
            attr(
                "description",
                Expression::String(role_definition.description.clone()),
            ),
            nested(block(
                "permissions",
                [
                    attr(
                        "actions",
                        Expression::Array(
                            role_definition
                                .actions
                                .iter()
                                .map(|a| Expression::String(a.clone()))
                                .collect(),
                        ),
                    ),
                    attr(
                        "data_actions",
                        Expression::Array(
                            role_definition
                                .data_actions
                                .iter()
                                .map(|a| Expression::String(a.clone()))
                                .collect(),
                        ),
                    ),
                    attr(
                        "not_actions",
                        Expression::Array(
                            role_definition
                                .not_actions
                                .iter()
                                .map(|a| Expression::String(a.clone()))
                                .collect(),
                        ),
                    ),
                    attr(
                        "not_data_actions",
                        Expression::Array(
                            role_definition
                                .not_data_actions
                                .iter()
                                .map(|a| Expression::String(a.clone()))
                                .collect(),
                        ),
                    ),
                ],
            )),
            attr(
                "assignable_scopes",
                Expression::Array(
                    role_definition
                        .assignable_scopes
                        .iter()
                        .map(|s| expr::template(s.clone()))
                        .collect(),
                ),
            ),
        ],
    ));

    let assignment_label = format!("{role_label}_assignment");
    fragment.resource_blocks.push(resource_block(
        "azurerm_role_assignment",
        &assignment_label,
        [
            attr(
                "name",
                expr::raw(&format!(
                    "uuidv5(\"oid\", \"deployment:azure:role-assign:${{azurerm_role_definition.{role_label}.role_definition_resource_id}}:${{{}}}\")",
                    render_expression_for_uuidv5(&principal_id_expr)
                )),
            ),
            attr(
                "scope",
                expr::raw(
                    "\"/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}\"",
                ),
            ),
            attr(
                "role_definition_id",
                expr::traversal([
                    "azurerm_role_definition",
                    role_label.as_str(),
                    "role_definition_resource_id",
                ]),
            ),
            attr("principal_id", principal_id_expr),
        ],
    ));

    Ok(())
}

/// Emit setup-owned Azure role definitions used later by live resource
/// controllers. Runtime reconciliation creates role assignments only; these
/// definitions must already exist and use the same deterministic UUID seeds as
/// `AzurePermissionsHelper`.
pub fn emit_setup_resource_role_definitions(
    stack: &Stack,
    fragment: &mut TfFragment,
) -> Result<()> {
    let mut seen_execution_roles = HashSet::new();
    let mut seen_management_roles = HashSet::new();

    for (resource_id, entry) in stack.resources() {
        let resource_type = entry.config.resource_type();
        let resource_type = resource_type.to_string();

        for (profile_name, profile) in &stack.permissions.profiles {
            for permission_set_ref in
                resource_scoped_permission_refs(profile, resource_id, &resource_type)
            {
                let Some(permission_set) = permission_set_ref
                    .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                else {
                    continue;
                };
                if permission_set.platforms.azure.is_none() {
                    continue;
                }
                if !seen_execution_roles.insert((profile_name.clone(), permission_set.id.clone())) {
                    continue;
                }

                emit_setup_execution_role_definition(fragment, profile_name, &permission_set)?;
            }
        }

        let Some(management_profile) = stack.management().profile() else {
            continue;
        };
        for permission_set_ref in
            resource_scoped_permission_refs(management_profile, resource_id, &resource_type)
                .into_iter()
                .filter(|reference| !reference.id().ends_with("/provision"))
        {
            let Some(permission_set) = permission_set_ref
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            if permission_set.platforms.azure.is_none() {
                continue;
            }
            if !seen_management_roles.insert(permission_set.id.clone()) {
                continue;
            }

            emit_setup_management_role_definition(fragment, &permission_set)?;
        }
    }

    Ok(())
}

fn resource_scoped_permission_refs<'a>(
    profile: &'a alien_core::permissions::PermissionProfile,
    resource_id: &str,
    resource_type: &str,
) -> Vec<&'a PermissionSetReference> {
    let type_prefix = format!("{resource_type}/");
    let mut refs = Vec::new();

    if let Some(resource_refs) = profile.0.get(resource_id) {
        refs.extend(resource_refs.iter().filter(|reference| {
            !is_worker_command_transport_permission(resource_type, reference.id())
        }));
    }
    if let Some(wildcard_refs) = profile.0.get("*") {
        refs.extend(
            wildcard_refs
                .iter()
                .filter(|reference| reference.id().starts_with(&type_prefix))
                .filter(|reference| {
                    !is_worker_command_transport_permission(resource_type, reference.id())
                }),
        );
    }

    refs
}

fn emit_setup_execution_role_definition(
    fragment: &mut TfFragment,
    profile_name: &str,
    permission_set: &PermissionSet,
) -> Result<()> {
    let role_label = setup_execution_role_label(profile_name, &permission_set.id);
    let mut role_definition = setup_resource_role_definition(permission_set)?;
    role_definition.name = format!("{} [{}]", role_definition.name, profile_name);

    fragment.resource_blocks.push(setup_role_definition_block(
        &role_label,
        expr::template(role_definition.name.clone()),
        expr::raw(&format!(
            "uuidv5(\"oid\", \"deployment:azure:res-role-def:${{local.resource_prefix}}:{profile_name}:{}\")",
            permission_set.id
        )),
        role_definition,
    ));

    Ok(())
}

fn emit_setup_management_role_definition(
    fragment: &mut TfFragment,
    permission_set: &PermissionSet,
) -> Result<()> {
    let role_label = setup_management_role_label(&permission_set.id);
    let mut role_definition = setup_resource_role_definition(permission_set)?;
    role_definition.name = format!("{} [mgmt]", role_definition.name);

    fragment.resource_blocks.push(setup_role_definition_block(
        &role_label,
        expr::template(role_definition.name.clone()),
        expr::raw(&format!(
            "uuidv5(\"oid\", \"deployment:azure:mgmt-res-role-def:${{local.resource_prefix}}:{}\")",
            permission_set.id
        )),
        role_definition,
    ));

    Ok(())
}

fn setup_resource_role_definition(
    permission_set: &PermissionSet,
) -> Result<alien_permissions::generators::AzureRoleDefinition> {
    let generator = AzureRuntimePermissionsGenerator::new();
    let context = permission_context("setup-resource-permissions")
        .with_resource_name("${local.resource_prefix}-setup-role-scope".to_string());

    let mut role_definition = generator
        .generate_role_definition(permission_set, BindingTarget::Resource, &context)
        .context(ErrorData::GenericError {
            message: format!(
                "failed to generate setup-owned Azure role definition for permission set '{}'",
                permission_set.id
            ),
        })?;
    role_definition.assignable_scopes = vec![
        "/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}"
            .to_string(),
    ];
    Ok(role_definition)
}

fn setup_role_definition_block(
    label: &str,
    name: Expression,
    role_definition_id: Expression,
    role_definition: alien_permissions::generators::AzureRoleDefinition,
) -> hcl::Block {
    resource_block(
        "azurerm_role_definition",
        label,
        [
            attr("name", name),
            attr("role_definition_id", role_definition_id),
            attr(
                "scope",
                expr::raw(
                    "\"/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}\"",
                ),
            ),
            attr("description", Expression::String(role_definition.description)),
            nested(block(
                "permissions",
                [
                    attr("actions", string_array(role_definition.actions)),
                    attr("data_actions", string_array(role_definition.data_actions)),
                    attr("not_actions", string_array(role_definition.not_actions)),
                    attr(
                        "not_data_actions",
                        string_array(role_definition.not_data_actions),
                    ),
                ],
            )),
            attr(
                "assignable_scopes",
                Expression::Array(
                    role_definition
                        .assignable_scopes
                        .into_iter()
                        .map(expr::template)
                        .collect(),
                ),
            ),
        ],
    )
}

fn setup_execution_role_label(profile_name: &str, permission_set_id: &str) -> String {
    format!(
        "setup_{}_{}",
        sanitize_role_label(profile_name),
        sanitize_role_label(permission_set_id)
    )
}

pub fn setup_management_role_label(permission_set_id: &str) -> String {
    format!(
        "setup_management_{}",
        sanitize_role_label(permission_set_id)
    )
}

fn string_array(items: Vec<String>) -> Expression {
    Expression::Array(items.into_iter().map(Expression::String).collect())
}

fn is_worker_command_transport_permission(resource_type: &str, permission_set_id: &str) -> bool {
    resource_type == "worker" && permission_set_id == "worker/dispatch-command"
}

/// Sanitise a permission-set id like `storage/object-admin` into a
/// Terraform label segment (`storage_object_admin`).
fn sanitize_role_label(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    out
}

/// Render an `Expression` as a string suitable for embedding in a
/// `uuidv5(...)` HCL call. We can't string-format an `hcl::Expression`
/// directly (it serialises to an HCL fragment, not a name), so we extract
/// the traversal path or fall back to a debug print.
fn render_expression_for_uuidv5(expr: &Expression) -> String {
    match expr {
        Expression::Traversal(t) => {
            let path = std::iter::once(traversal_root(&t.expr))
                .chain(t.operators.iter().filter_map(|op| match op {
                    hcl::expr::TraversalOperator::GetAttr(ident) => {
                        Some(ident.as_str().to_string())
                    }
                    _ => None,
                }))
                .collect::<Vec<_>>()
                .join(".");
            path
        }
        other => format!("{:?}", other),
    }
}

fn traversal_root(expr: &Expression) -> String {
    match expr {
        Expression::Variable(v) => v.as_str().to_string(),
        other => format!("{:?}", other),
    }
}
