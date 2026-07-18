//! Azure Storage — `azurerm_storage_container` inside the stack's
//! `azurerm_storage_account`.
//!
//! Mirrors `AzureStorageController`:
//!
//! * Container name = `lower(replace("${local.resource_prefix}-{id}", "_", "-"))`
//!   — the runtime's `get_azure_container_name` helper, reproduced in HCL
//!   so push/pull converge byte-identical.
//! * Public access maps `storage.public_read` to the provider's
//!   `container_access_type` of `blob` (anonymous read on objects only)
//!   versus `private`.
//! * Storage-account-level `versioning_enabled` is set by the auxiliary
//!   [`super::storage_account::AzureStorageAccountEmitter`] when any
//!   container opted in (Azure scopes the toggle to the account, not
//!   the container — see that file's doc comment).
//! * `lifecycle_rules` translate to a sibling `azurerm_storage_management_policy`
//!   referencing the storage account; the policy lives next to the
//!   container so the customer reads "what's the retention policy on
//!   `data`?" by opening `data.tf`.

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{
        downcast, permission_context, required_label, service_account_principal_id,
        setup_execution_role_label, setup_management_role_label,
    },
    emitters::gates::{permission_gate_count, TrackedPermissionRef},
    expr,
};
use alien_core::{
    import::EmitContext, AzureStorageAccount, ErrorData, LifecycleRule, PermissionProfile,
    PermissionSet, PermissionSetReference, RemoteStackManagement, Result, Storage,
};
use alien_error::{AlienError, Context};
use alien_permissions::{
    generators::{AzureCustomRole, AzureRoleDefinitionRef, AzureRuntimePermissionsGenerator},
    BindingTarget,
};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureStorageEmitter;

impl TfEmitter for AzureStorageEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let storage = downcast::<Storage>(ctx, Storage::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let parent_label = parent_storage_account_label(ctx)?.to_string();

        let mut fragment = TfFragment::default();

        let access_type = if storage.public_read {
            "blob"
        } else {
            "private"
        };

        fragment.resource_blocks.push(resource_block(
            "azurerm_storage_container",
            label,
            [
                attr("name", container_name_expr(storage.id())),
                attr(
                    "storage_account_name",
                    expr::traversal(["azurerm_storage_account", &parent_label, "name"]),
                ),
                attr(
                    "container_access_type",
                    Expression::String(access_type.to_string()),
                ),
            ],
        ));

        if !storage.lifecycle_rules.is_empty() {
            fragment.resource_blocks.push(lifecycle_policy(
                label,
                &parent_label,
                &storage.lifecycle_rules,
            ));
        }

        emit_storage_permissions(ctx, label, &parent_label, &mut fragment)?;

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let _ = downcast::<Storage>(ctx, Storage::RESOURCE_TYPE)?;
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
                "containerName",
                expr::traversal(["azurerm_storage_container", label, "name"]),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let _ = downcast::<Storage>(ctx, Storage::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let parent_label = parent_storage_account_label(ctx)?.to_string();
        Ok(Some(expr::object([
            ("service", Expression::String("blob".to_string())),
            (
                "accountName",
                expr::traversal(["azurerm_storage_account", &parent_label, "name"]),
            ),
            (
                "containerName",
                expr::traversal(["azurerm_storage_container", label, "name"]),
            ),
        ])))
    }
}

/// Find the auxiliary `azure_storage_account` resource in the stack and
/// return its Terraform label. The preflight pipeline always injects
/// exactly one of these per stack as `default-storage-account`; we
/// surface a typed error rather than panicking if it's missing.
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
            "Azure Storage resource requires a sibling `azure_storage_account` resource in the \
             stack (preflight-injected as `default-storage-account`)"
                .to_string(),
    }))
}

fn container_name_expr(storage_id: &str) -> Expression {
    // `replace(lower("${local.resource_prefix}-{id}"), "_", "-")` — match
    // runtime's `get_azure_container_name` so push and pull resolve to
    // the same physical container.
    expr::raw(format!(
        "replace(lower(\"${{local.resource_prefix}}-{}\"), \"_\", \"-\")",
        storage_id
    ))
}

fn lifecycle_policy(
    storage_label: &str,
    parent_label: &str,
    rules: &[LifecycleRule],
) -> hcl::structure::Block {
    let rule_blocks: Vec<hcl::structure::Structure> = rules
        .iter()
        .enumerate()
        .map(|(index, rule)| nested(rule_block(storage_label, index, rule)))
        .collect();

    let mut body: Vec<hcl::structure::Structure> = vec![attr(
        "storage_account_id",
        expr::traversal(["azurerm_storage_account", parent_label, "id"]),
    )];
    body.extend(rule_blocks);

    resource_block("azurerm_storage_management_policy", storage_label, body)
}

fn emit_storage_permissions(
    ctx: &EmitContext<'_>,
    storage_label: &str,
    parent_label: &str,
    fragment: &mut TfFragment,
) -> Result<()> {
    for (profile_name, permission_set_refs) in storage_permission_owners(ctx) {
        let Some(principal_id_expr) = service_account_principal_id(ctx, &profile_name) else {
            continue;
        };

        for tracked_ref in permission_set_refs {
            let permission_set = resolve_permission_set(&tracked_ref.reference, ctx.resource_id)?;
            if permission_set.id.ends_with("/provision") || permission_set.platforms.azure.is_none()
            {
                continue;
            }

            let gate_count = permission_gate_count(
                ctx,
                &profile_name,
                &permission_set.id,
                &tracked_ref.origin_keys(ctx.resource_id),
            )?;
            let generator = AzureRuntimePermissionsGenerator::new();
            let permission_context = permission_context(storage_label)
                .with_resource_name(container_name_expr_string(ctx.resource_id))
                .with_storage_account_name(storage_account_name_expr_string(parent_label));
            let grant_plan = generator
                .generate_grant_plan(
                    &permission_set,
                    BindingTarget::Resource,
                    &permission_context,
                )
                .context(ErrorData::GenericError {
                    message: format!(
                        "failed to generate Azure storage grants for '{}'",
                        permission_set.id
                    ),
                })?;

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
                    ctx.resource_id,
                    storage_label,
                    &profile_name,
                    binding_index,
                    &binding.role_name,
                    expr::template(binding.scope.clone()),
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
        let permission_set = resolve_permission_set(&permission_set_ref, ctx.resource_id)?;
        if permission_set.id.ends_with("/provision") || permission_set.platforms.azure.is_none() {
            continue;
        }

        let generator = AzureRuntimePermissionsGenerator::new();
        let permission_context = permission_context(storage_label)
            .with_resource_name(container_name_expr_string(ctx.resource_id))
            .with_storage_account_name(storage_account_name_expr_string(parent_label));
        let grant_plan = generator
            .generate_grant_plan(
                &permission_set,
                BindingTarget::Resource,
                &permission_context,
            )
            .context(ErrorData::GenericError {
                message: format!(
                    "failed to generate Azure storage management grants for '{}'",
                    permission_set.id
                ),
            })?;

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
                ctx.resource_id,
                storage_label,
                "management",
                binding_index,
                &binding.role_name,
                expr::template(binding.scope.clone()),
                role_definition_id,
                principal_id_expr.clone(),
                None,
            );
        }
    }

    Ok(())
}

fn resolve_permission_set(
    permission_set_ref: &PermissionSetReference,
    resource_id: &str,
) -> Result<PermissionSet> {
    permission_set_ref
        .resolve(|name| alien_permissions::get_permission_set(name).cloned())
        .ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: format!(
                    "permission set '{}' referenced by Azure storage resource '{}' was not found",
                    permission_set_ref.id(),
                    resource_id
                ),
            })
        })
}

fn custom_role_index(
    custom_roles: &[AzureCustomRole],
    key: &str,
    permission_set_id: &str,
) -> Result<usize> {
    custom_roles
        .iter()
        .position(|role| role.key == key)
        .ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: format!(
                    "Azure storage permission set '{}' generated a binding for missing custom role '{}'",
                    permission_set_id, key
                ),
            })
        })
}

fn emit_role_assignment(
    fragment: &mut TfFragment,
    storage_id: &str,
    storage_label: &str,
    principal_label: &str,
    binding_index: usize,
    role_name: &str,
    scope: Expression,
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
                "uuidv5(\"oid\", \"deployment:azure:storage-role-assign:${{local.resource_prefix}}:{storage_id}:{role_label}:{principal_label}:{binding_index}\")"
            )),
        ),
        attr("scope", scope),
        attr("role_definition_id", role_definition_id),
        attr("principal_id", principal_id_expr),
    ]);
    fragment.resource_blocks.push(resource_block(
        "azurerm_role_assignment",
        &format!("{storage_label}_{role_label}_{principal_label}_assignment_{binding_index}"),
        body,
    ));
}

fn storage_permission_owners(
    ctx: &EmitContext<'_>,
) -> Vec<(String, Vec<TrackedPermissionRef>)> {
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
        .into_iter()
        .map(|tracked| tracked.reference)
        .collect()
}

fn resource_permission_refs(
    profile: &PermissionProfile,
    resource_id: &str,
) -> Vec<TrackedPermissionRef> {
    let mut refs: Vec<TrackedPermissionRef> = Vec::new();

    if let Some(resource_refs) = profile.0.get(resource_id) {
        for permission_ref in resource_refs {
            if !refs
                .iter()
                .any(|tracked| tracked.reference.id() == permission_ref.id())
            {
                refs.push(TrackedPermissionRef {
                    reference: permission_ref.clone(),
                    in_resource: true,
                    in_wildcard: false,
                });
            }
        }
    }

    if let Some(wildcard_refs) = profile.0.get("*") {
        for permission_ref in wildcard_refs
            .iter()
            .filter(|permission_ref| permission_ref.id().starts_with("storage/"))
        {
            if let Some(tracked) = refs
                .iter_mut()
                .find(|tracked| tracked.reference.id() == permission_ref.id())
            {
                tracked.in_wildcard = true;
            } else {
                refs.push(TrackedPermissionRef {
                    reference: permission_ref.clone(),
                    in_resource: false,
                    in_wildcard: true,
                });
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

fn storage_account_name_expr_string(parent_label: &str) -> String {
    format!("${{azurerm_storage_account.{parent_label}.name}}")
}

fn container_name_expr_string(storage_id: &str) -> String {
    format!("${{replace(lower(\"${{local.resource_prefix}}-{storage_id}\"), \"_\", \"-\")}}")
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

fn rule_block(storage_label: &str, index: usize, rule: &LifecycleRule) -> hcl::structure::Block {
    let prefix_match = rule
        .prefix
        .clone()
        .map(|p| Expression::Array(vec![Expression::String(p)]))
        .unwrap_or_else(|| Expression::Array(vec![]));

    block(
        "rule",
        [
            attr(
                "name",
                Expression::String(format!("{storage_label}-rule-{}", index + 1)),
            ),
            attr("enabled", Expression::Bool(true)),
            nested(block(
                "filters",
                [
                    attr(
                        "blob_types",
                        Expression::Array(vec![Expression::String("blockBlob".to_string())]),
                    ),
                    attr("prefix_match", prefix_match),
                ],
            )),
            nested(block(
                "actions",
                [nested(block(
                    "base_blob",
                    [attr(
                        "delete_after_days_since_modification_greater_than",
                        Expression::Number(hcl::Number::from(i64::from(rule.days))),
                    )],
                ))],
            )),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{generate_terraform_module, TerraformOptions, TerraformTarget, TfRegistry};
    use alien_core::{
        AzureResourceGroup, ManagementPermissions, ResourceLifecycle, ServiceAccount, Stack,
        StackSettings,
    };

    const STORAGE_BLOB_DATA_CONTRIBUTOR_ROLE_ID: &str = "ba92f5b4-2d11-453d-a403-e96b0029c9fe";
    const RESOURCE_GROUP_SCOPE: &str =
        "\"/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}\"";
    const STORAGE_CONTAINER_SCOPE: &str =
        "\"/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}/providers/Microsoft.Storage/storageAccounts/${azurerm_storage_account.default_storage_account.name}/blobServices/default/containers/${replace(lower(\"${local.resource_prefix}-files\"), \"_\", \"-\")}\"";

    #[test]
    fn generated_storage_assignments_preserve_each_permission_binding_scope() {
        let permissions = PermissionProfile::new().resource(
            "files",
            ["storage/data-write", "storage/trigger-management"],
        );
        let stack = Stack::new("azure-storage-scopes".to_string())
            .permission("app", permissions.clone())
            .management(ManagementPermissions::override_(permissions))
            .add(
                AzureResourceGroup::new("default-resource-group".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                AzureStorageAccount::new("default-storage-account".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Storage::new("files".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                ServiceAccount::new("app-sa".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                RemoteStackManagement::new("management".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let registry = TfRegistry::built_in();
        let module = generate_terraform_module(
            &stack,
            TerraformTarget::Azure,
            TerraformOptions {
                display_name: None,
                registry: &registry,
                stack_settings: StackSettings::default(),
                registration: None,
                helm_install: None,
                supported_aws_regions: Vec::new(),
            },
        )
        .expect("Azure Terraform module should render");
        let storage_module = module.get("files.tf").expect("storage resource module");

        assert_principal_assignment_scopes(
            storage_module,
            "azurerm_user_assigned_identity.app_sa.principal_id",
        );
        assert_principal_assignment_scopes(
            storage_module,
            "azurerm_user_assigned_identity.management.principal_id",
        );
    }

    fn assert_principal_assignment_scopes(rendered: &str, principal_id: &str) {
        let assignments = rendered
            .split("resource \"azurerm_role_assignment\"")
            .skip(1)
            .filter_map(|chunk| chunk.split_once("\n}\n").map(|(block, _)| block))
            .filter(|block| block.contains(principal_id))
            .collect::<Vec<_>>();
        assert_eq!(assignments.len(), 2, "expected both storage grants");

        let data_write = assignments
            .iter()
            .find(|block| block.contains(STORAGE_BLOB_DATA_CONTRIBUTOR_ROLE_ID))
            .expect("storage/data-write assignment");
        assert!(
            data_write.contains(STORAGE_CONTAINER_SCOPE),
            "storage/data-write must retain its generated container scope:\n{data_write}"
        );

        let trigger_management = assignments
            .iter()
            .find(|block| !block.contains(STORAGE_BLOB_DATA_CONTRIBUTOR_ROLE_ID))
            .expect("storage/trigger-management assignment");
        assert!(
            trigger_management.contains(RESOURCE_GROUP_SCOPE),
            "storage/trigger-management must retain its generated resource-group scope:\n{trigger_management}"
        );
        assert!(
            !trigger_management.contains("blobServices/default/containers"),
            "storage/trigger-management must not be narrowed to the blob container"
        );
    }
}
