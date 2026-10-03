//! Azure KV — Azure Table Storage table in the stack storage account.
//!
//! This mirrors `AzureKvController`: the shared `azure_storage_account`
//! auxiliary resource owns the Storage account, and each `Kv` resource is
//! realised as an Azure Table. The imported state can then produce the
//! same `tablestorage` binding the runtime uses in push mode.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{
        downcast, permission_context, remote_bindings_role_label, required_label,
    },
    expr,
};
use alien_core::{import::EmitContext, AzureStorageAccount, ErrorData, Kv, RemoteBindings, Result};
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

        let table = resource_block(
            "azurerm_storage_table",
            label,
            [
                attr("name", table_name_expr(kv.id())),
                attr(
                    "storage_account_name",
                    expr::traversal(["azurerm_storage_account", &parent_label, "name"]),
                ),
            ],
        );
        fragment.resource_blocks.push(table);

        emit_remote_access(ctx, &mut fragment, label, &parent_label)?;
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

fn emit_remote_access(
    ctx: &EmitContext<'_>,
    fragment: &mut TfFragment,
    label: &str,
    parent_label: &str,
) -> Result<()> {
    let Some(definition) = alien_core::remote_bindings::remote_binding_for_entry(ctx.resource)
    else {
        return Ok(());
    };
    let access_label = ctx
        .stack
        .resources()
        .find_map(|(id, entry)| {
            (entry.config.resource_type() == RemoteBindings::RESOURCE_TYPE)
                .then(|| ctx.name_for(id))
                .flatten()
        })
        .ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: "Remote KV requires its setup-owned access identity".to_string(),
            })
        })?;
    let permission_set = alien_permissions::get_permission_set(definition.permission_set)
        .ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: format!(
                    "Remote KV permission set {} is not registered",
                    definition.permission_set
                ),
            })
        })?;
    let context = permission_context(label)
        .with_resource_name(format!("${{azurerm_storage_table.{label}.name}}"))
        .with_storage_account_name(format!("${{azurerm_storage_account.{parent_label}.name}}"));
    let plan = AzureRuntimePermissionsGenerator::new()
        .generate_grant_plan(permission_set, BindingTarget::Resource, &context)
        .context(ErrorData::GenericError {
            message: "Generate remote KV table grants".to_string(),
        })?;
    for (index, binding) in plan.bindings.iter().enumerate() {
        let role_id = match &binding.role_definition {
            AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                expr::template(role_definition_id.clone())
            }
            AzureRoleDefinitionRef::Custom { key } => {
                let role_index = plan
                    .custom_roles
                    .iter()
                    .position(|role| &role.key == key)
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::GenericError {
                            message: "Missing remote KV custom role".to_string(),
                        })
                    })?;
                let role_label = remote_bindings_role_label(&binding.role_name, role_index);
                expr::traversal([
                    "azurerm_role_definition",
                    role_label.as_str(),
                    "role_definition_resource_id",
                ])
            }
        };
        fragment.resource_blocks.push(resource_block(
            "azurerm_role_assignment",
            &format!("{label}_access_{index}"),
            [
                attr("scope", expr::template(binding.scope.clone())),
                attr("role_definition_id", role_id),
                attr(
                    "principal_id",
                    expr::traversal([
                        "azurerm_user_assigned_identity",
                        access_label,
                        "principal_id",
                    ]),
                ),
            ],
        ));
    }
    Ok(())
}
