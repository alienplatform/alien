//! Azure KV — Azure Table Storage table in the stack storage account.
//!
//! This mirrors `AzureKvController`: the shared `azure_storage_account`
//! auxiliary resource owns the Storage account, and each `Kv` resource is
//! realised as an Azure Table. The imported state can then produce the
//! same `tablestorage` binding the runtime uses in push mode.
//!
//! Resource-scoped `azurerm_role_assignment` blocks for permission
//! profiles that reference this table are emitted alongside it; wildcard
//! grants stay on the service-account identity via `stack_permission_sets`.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{
        downcast, permission_context, required_label, service_account_principal_id,
        setup_execution_role_label, setup_management_role_label,
    },
    emitters::gates::permission_gate_count,
    expr,
};
use alien_core::{
    import::EmitContext, AzureStorageAccount, ErrorData, Kv, PermissionProfile,
    PermissionSetReference, RemoteStackManagement, Result,
};
use alien_error::{AlienError, Context};
use alien_permissions::{
    generators::{AzureRoleDefinitionRef, AzureRuntimePermissionsGenerator},
    BindingTarget,
};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureKvEmitter;

impl TfEmitter for AzureKvEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let kv = downcast::<Kv>(ctx, Kv::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let parent_label = parent_storage_account_label(ctx)?.to_string();

        let mut fragment = TfFragment::default();

        fragment.resource_blocks.push(resource_block(
            "azurerm_storage_table",
            label,
            [
                attr("name", table_name_expr(kv.id())),
                attr(
                    "storage_account_name",
                    expr::traversal(["azurerm_storage_account", &parent_label, "name"]),
                ),
            ],
        ));

        emit_kv_permissions(ctx, kv, label, &parent_label, &mut fragment)?;

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        let parent_label = parent_storage_account_label(ctx)?.to_string();
        Ok(expr::object([
            ("subscriptionId", expr::raw("var.azure_subscription_id")),
            ("resourceGroup", expr::raw("var.azure_resource_group_name")),
            (
                "storageAccountName",
                expr::traversal(["azurerm_storage_account", &parent_label, "name"]),
            ),
            (
                "tableName",
                expr::traversal(["azurerm_storage_table", label, "name"]),
            ),
            (
                "tableEndpoint",
                expr::traversal([
                    "azurerm_storage_account",
                    &parent_label,
                    "primary_table_endpoint",
                ]),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        let parent_label = parent_storage_account_label(ctx)?.to_string();
        Ok(Some(expr::object([
            ("service", Expression::String("tablestorage".to_string())),
            (
                "resourceGroupName",
                expr::raw("var.azure_resource_group_name"),
            ),
            (
                "accountName",
                expr::traversal(["azurerm_storage_account", &parent_label, "name"]),
            ),
            (
                "tableName",
                expr::traversal(["azurerm_storage_table", label, "name"]),
            ),
        ])))
    }
}

/// Find the auxiliary Azure Storage Account required by Azure KV.
fn parent_storage_account_label<'a>(ctx: &EmitContext<'a>) -> Result<&'a str> {
    for (id, entry) in ctx.stack.resources() {
        if entry.config.downcast_ref::<AzureStorageAccount>().is_some() {
            if let Some(label) = ctx.name_for(id) {
                return Ok(label);
            }
        }
    }
    // Missing sibling resource is a stack-authoring mistake the developer can
    // fix, so keep it external-safe rather than an internal serialization error.
    Err(AlienError::new(ErrorData::GenericError {
        message:
            "Azure KV resource requires a sibling `azure_storage_account` resource in the stack \
             (preflight-injected as `default-storage-account`)"
                .to_string(),
    }))
}

/// Azure Table names are 3-63 alphanumeric characters and must start with
/// a letter. Prefixing with `kv` keeps generated names valid even when the
/// stack name starts with a digit.
fn table_name_expr(kv_id: &str) -> Expression {
    expr::raw(format!(
        "substr(lower(replace(\"kv${{local.resource_prefix}}{}\", \"/[^A-Za-z0-9]/\", \"\")), 0, 63)",
        kv_id
    ))
}

fn emit_kv_permissions(
    ctx: &EmitContext<'_>,
    kv: &Kv,
    kv_label: &str,
    parent_label: &str,
    fragment: &mut TfFragment,
) -> Result<()> {
    for (profile_name, permission_set_refs) in kv_permission_owners(ctx) {
        let Some(principal_id_expr) = service_account_principal_id(ctx, &profile_name) else {
            continue;
        };

        for permission_set_ref in permission_set_refs {
            let Some(permission_set) = permission_set_ref
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            if !permission_set.id.starts_with("kv/")
                || permission_set.id.ends_with("/provision")
                || permission_set.platforms.azure.is_none()
            {
                continue;
            }

            let gate_count =
                permission_gate_count(ctx, &profile_name, &permission_set.id, &[ctx.resource_id])?;
            let grant_plan = kv_grant_plan(kv_label, parent_label, &permission_set)?;

            for (binding_index, binding) in grant_plan.bindings.iter().enumerate() {
                let role_definition_id = match &binding.role_definition {
                    AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                        expr::template(role_definition_id.clone())
                    }
                    AzureRoleDefinitionRef::Custom { key } => {
                        let index =
                            custom_role_index(&grant_plan.custom_roles, key, &permission_set.id)?;
                        let role_label =
                            setup_execution_role_label(&profile_name, &binding.role_name, index);
                        expr::traversal([
                            "azurerm_role_definition",
                            role_label.as_str(),
                            "role_definition_resource_id",
                        ])
                    }
                };
                emit_role_assignment(
                    fragment,
                    kv.id(),
                    kv_label,
                    parent_label,
                    &profile_name,
                    binding_index,
                    &binding.role_name,
                    role_definition_id,
                    principal_id_expr.clone(),
                    gate_count.clone(),
                );
            }
        }
    }

    let Some(management_label) = remote_stack_management_label(ctx) else {
        return Ok(());
    };

    let principal_id_expr = expr::traversal([
        "azurerm_user_assigned_identity",
        management_label,
        "principal_id",
    ]);

    for permission_set_ref in management_permission_refs(ctx) {
        let Some(permission_set) =
            permission_set_ref.resolve(|name| alien_permissions::get_permission_set(name).cloned())
        else {
            continue;
        };
        if !permission_set.id.starts_with("kv/")
            || permission_set.id.ends_with("/provision")
            || permission_set.platforms.azure.is_none()
        {
            continue;
        }

        let grant_plan = kv_grant_plan(kv_label, parent_label, &permission_set)?;

        for (binding_index, binding) in grant_plan.bindings.iter().enumerate() {
            let role_definition_id = match &binding.role_definition {
                AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                    expr::template(role_definition_id.clone())
                }
                AzureRoleDefinitionRef::Custom { key } => {
                    let index =
                        custom_role_index(&grant_plan.custom_roles, key, &permission_set.id)?;
                    let role_label = setup_management_role_label(&binding.role_name, index);
                    expr::traversal([
                        "azurerm_role_definition",
                        role_label.as_str(),
                        "role_definition_resource_id",
                    ])
                }
            };
            emit_role_assignment(
                fragment,
                kv.id(),
                kv_label,
                parent_label,
                "management",
                binding_index,
                &binding.role_name,
                role_definition_id,
                principal_id_expr.clone(),
                None,
            );
        }
    }

    Ok(())
}

fn kv_grant_plan(
    kv_label: &str,
    parent_label: &str,
    permission_set: &alien_core::PermissionSet,
) -> Result<alien_permissions::generators::AzureGrantPlan> {
    let generator = AzureRuntimePermissionsGenerator::new();
    let permission_context = permission_context(kv_label)
        .with_resource_name(format!("${{azurerm_storage_table.{kv_label}.name}}"))
        .with_storage_account_name(format!("${{azurerm_storage_account.{parent_label}.name}}"));
    generator
        .generate_grant_plan(permission_set, BindingTarget::Resource, &permission_context)
        .context(ErrorData::TemplateSerializationFailed {
            format: "Terraform".to_string(),
            reason: format!(
                "failed to generate Azure KV grants for '{}'",
                permission_set.id
            ),
        })
}

fn custom_role_index(
    custom_roles: &[alien_permissions::generators::AzureCustomRole],
    key: &str,
    permission_set_id: &str,
) -> Result<usize> {
    custom_roles
        .iter()
        .position(|role| role.key == key)
        .ok_or_else(|| {
            AlienError::new(ErrorData::TemplateSerializationFailed {
                format: "Terraform".to_string(),
                reason: format!(
                    "Azure KV permission set '{}' generated a binding for missing custom role '{}'",
                    permission_set_id, key
                ),
            })
        })
}

fn emit_role_assignment(
    fragment: &mut TfFragment,
    kv_id: &str,
    kv_label: &str,
    parent_label: &str,
    principal_label: &str,
    binding_index: usize,
    role_name: &str,
    role_definition_id: Expression,
    principal_id_expr: Expression,
    gate_count: Option<Expression>,
) {
    let role_label = sanitize_role_label(role_name);
    let mut body = Vec::new();
    if let Some(gate_count) = gate_count {
        body.push(attr("count", gate_count));
    }
    body.extend([
        attr(
            "name",
            expr::raw(&format!(
                "uuidv5(\"oid\", \"deployment:azure:kv-role-assign:${{local.resource_prefix}}:{kv_id}:{role_label}:{principal_label}:{binding_index}\")"
            )),
        ),
        attr("scope", kv_table_scope_expr(parent_label, kv_label)),
        attr("role_definition_id", role_definition_id),
        attr("principal_id", principal_id_expr),
    ]);
    fragment.resource_blocks.push(resource_block(
        "azurerm_role_assignment",
        &format!("{kv_label}_{role_label}_{principal_label}_assignment_{binding_index}"),
        body,
    ));
}

/// Scope mirrors the runtime controller's explicit ARM scope (the table
/// under the shared storage account), not the jsonc resource-scope template.
fn kv_table_scope_expr(parent_label: &str, kv_label: &str) -> Expression {
    expr::raw(format!(
        "\"${{azurerm_storage_account.{parent_label}.id}}/tableServices/default/tables/${{azurerm_storage_table.{kv_label}.name}}\""
    ))
}

fn kv_permission_owners(ctx: &EmitContext<'_>) -> Vec<(String, Vec<PermissionSetReference>)> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = resource_permission_refs(profile, ctx.resource_id);
        if refs.is_empty() {
            continue;
        }
        owners.push((profile_name.clone(), refs));
    }
    owners
}

fn management_permission_refs(ctx: &EmitContext<'_>) -> Vec<PermissionSetReference> {
    let Some(profile) = ctx.stack.management().profile() else {
        return Vec::new();
    };
    resource_permission_refs(profile, ctx.resource_id)
}

fn resource_permission_refs(
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
    refs
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

fn sanitize_role_label(input: &str) -> String {
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
