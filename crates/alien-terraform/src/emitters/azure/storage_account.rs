//! Azure StorageAccount (auxiliary) — `azurerm_storage_account`.
//!
//! `AzureStorageAccount` is preflight-injected once per stack as
//! `default-storage-account`. Every Alien `Storage` resource then realises
//! a Blob container inside this single account; `Kv` (Azure Tables) and
//! `Vault` consumers reach the same account through the runtime's
//! `azure_utils::get_storage_account_name` helper.
//!
//! The emitter mirrors that controller surface:
//!
//! * Name derived deterministically from `${local.resource_prefix}` — see
//!   [`super::helpers::storage_account_name_local`]. Push and pull paths
//!   converge on the same name without an extra round-trip.
//! * Standard / LRS / Hot tier — production-safe defaults, customers
//!   override at the stack-settings level (out of scope for this rebuild).
//! * `blob_properties.versioning_enabled = true` whenever any Alien
//!   `Storage` resource in the stack opted into versioning. The spec
//!   stamps versioning on the parent because Azure scopes the toggle to
//!   the account, not the container.

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{downcast, required_label, storage_account_name_local, tags},
    expr,
};
use alien_core::{
    import::EmitContext, AzureStorageAccount, ErrorData, ResourceLifecycle, Result, Storage,
};
use alien_error::AlienError;
use hcl::expr::Expression;
use hcl::{expr::FuncCall, structure::BlockLabel, Identifier};

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureStorageAccountEmitter;

impl TfEmitter for AzureStorageAccountEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let _ = downcast::<AzureStorageAccount>(ctx, AzureStorageAccount::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;

        let versioning_enabled = storage_capability(ctx, |storage| storage.versioning)?;
        let public_enabled = storage_capability(ctx, |storage| storage.public_read)?;
        let encryption_key_label = stack_encryption_key_label(ctx);

        let mut body: Vec<hcl::structure::Structure> = vec![
            attr("name", storage_account_name_local()),
            attr(
                "resource_group_name",
                expr::raw("var.azure_resource_group_name"),
            ),
            attr("location", expr::raw("var.azure_location")),
            attr("account_tier", Expression::String("Standard".to_string())),
            attr(
                "account_replication_type",
                Expression::String("LRS".to_string()),
            ),
            attr("account_kind", Expression::String("StorageV2".to_string())),
            attr("access_tier", Expression::String("Hot".to_string())),
            attr("min_tls_version", Expression::String("TLS1_2".to_string())),
            attr("allow_nested_items_to_be_public", public_enabled),
            attr("https_traffic_only_enabled", Expression::Bool(true)),
            attr("tags", tags(ctx, "storage-account")),
        ];
        let mut fragment = TfFragment::default();
        let mut blob_properties = vec![
            attr("versioning_enabled", versioning_enabled),
            attr("change_feed_enabled", Expression::Bool(false)),
        ];
        if let Some(origins) = storage_cors_origins(ctx) {
            let local_name = format!("{label}_cors_origins");
            fragment.locals.insert(local_name.clone(), origins);
            blob_properties.push(nested(cors_rule(&local_name)?));
            body.push(nested(cors_limits(&local_name)?));
        }
        body.push(nested(block("blob_properties", blob_properties)));
        if let Some(key_label) = encryption_key_label {
            let identity_label = format!("{label}_encryption");
            let role_label = format!("{label}_encryption_key");
            fragment.resource_blocks.push(resource_block(
                "azurerm_user_assigned_identity",
                &identity_label,
                [
                    attr(
                        "name",
                        expr::raw("\"${local.resource_prefix}-storage-encryption\""),
                    ),
                    attr(
                        "resource_group_name",
                        expr::raw("var.azure_resource_group_name"),
                    ),
                    attr("location", expr::raw("var.azure_location")),
                ],
            ));
            fragment.resource_blocks.push(resource_block(
                "azurerm_role_assignment",
                &role_label,
                [
                    attr(
                        "scope",
                        expr::traversal([
                            "azurerm_key_vault_key",
                            key_label,
                            "resource_versionless_id",
                        ]),
                    ),
                    attr(
                        "role_definition_name",
                        Expression::String("Key Vault Crypto Service Encryption User".to_string()),
                    ),
                    attr(
                        "principal_id",
                        expr::traversal([
                            "azurerm_user_assigned_identity",
                            identity_label.as_str(),
                            "principal_id",
                        ]),
                    ),
                ],
            ));
            body.push(nested(block(
                "identity",
                [
                    attr("type", Expression::String("UserAssigned".to_string())),
                    attr(
                        "identity_ids",
                        Expression::Array(vec![expr::traversal([
                            "azurerm_user_assigned_identity",
                            identity_label.as_str(),
                            "id",
                        ])]),
                    ),
                ],
            )));
            body.push(nested(block(
                "customer_managed_key",
                [
                    attr(
                        "key_vault_key_id",
                        expr::traversal(["azurerm_key_vault_key", key_label, "versionless_id"]),
                    ),
                    attr(
                        "user_assigned_identity_id",
                        expr::traversal([
                            "azurerm_user_assigned_identity",
                            identity_label.as_str(),
                            "id",
                        ]),
                    ),
                ],
            )));
            body.push(attr(
                "depends_on",
                Expression::Array(vec![expr::traversal([
                    "azurerm_role_assignment",
                    role_label.as_str(),
                ])]),
            ));
        }

        fragment
            .resource_blocks
            .push(resource_block("azurerm_storage_account", label, body));
        fragment.retain_resources(|kind, name| {
            kind == "azurerm_storage_account"
                || (kind == "azurerm_user_assigned_identity"
                    && name == format!("{label}_encryption"))
                || (kind == "azurerm_role_assignment" && name == format!("{label}_encryption_key"))
        });
        emit_lifecycle_policy(ctx, label, &mut fragment)?;
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            ("subscriptionId", expr::raw("var.azure_subscription_id")),
            ("resourceGroup", expr::raw("var.azure_resource_group_name")),
            (
                "storageAccountName",
                expr::traversal(["azurerm_storage_account", label, "name"]),
            ),
            (
                "resourceId",
                expr::traversal(["azurerm_storage_account", label, "id"]),
            ),
            (
                "blobEndpoint",
                expr::traversal(["azurerm_storage_account", label, "primary_blob_endpoint"]),
            ),
            (
                "fileEndpoint",
                expr::traversal(["azurerm_storage_account", label, "primary_file_endpoint"]),
            ),
            (
                "queueEndpoint",
                expr::traversal(["azurerm_storage_account", label, "primary_queue_endpoint"]),
            ),
            (
                "tableEndpoint",
                expr::traversal(["azurerm_storage_account", label, "primary_table_endpoint"]),
            ),
        ]))
    }
}

/// Azure CORS is shared by every container. Union enabled contributors into a
/// single read-only rule, rather than competing account-wide configurations.
fn storage_cors_origins(ctx: &EmitContext<'_>) -> Option<Expression> {
    let mut origins = Vec::new();
    for (_, entry) in ctx.stack.resources() {
        let Some(storage) = entry.config.downcast_ref::<Storage>() else {
            continue;
        };
        origins.extend(storage.cors_allowed_origins.iter().map(|origin| {
            (
                entry.enabled_when.clone(),
                Expression::String(origin.clone()),
            )
        }));
    }
    if origins.is_empty() {
        return None;
    }
    let mut expression = crate::emitters::enabled::registration_list(origins);
    for function in ["distinct", "sort"] {
        expression = Expression::FuncCall(Box::new(
            FuncCall::builder(Identifier::sanitized(function))
                .arg(expression)
                .build(),
        ));
    }
    Some(expression)
}

fn cors_rule(local_name: &str) -> Result<hcl::structure::Block> {
    let strings = |values: &[&str]| {
        Expression::Array(
            values
                .iter()
                .map(|value| Expression::from(*value))
                .collect(),
        )
    };
    let mut rule = block(
        "dynamic",
        [
            attr(
                "for_each",
                expr::parse(&format!(
                    "length(local.{local_name}) == 0 ? [] : [local.{local_name}]"
                ))?,
            ),
            nested(block(
                "content",
                [
                    attr("allowed_origins", expr::traversal(["cors_rule", "value"])),
                    attr("allowed_methods", strings(&["GET", "HEAD"])),
                    attr("allowed_headers", strings(&["*"])),
                    attr(
                        "exposed_headers",
                        strings(&["ETag", "Content-Range", "Accept-Ranges"]),
                    ),
                    attr("max_age_in_seconds", Expression::from(3600)),
                ],
            )),
        ],
    );
    rule.labels
        .push(BlockLabel::String("cors_rule".to_string()));
    Ok(rule)
}

fn cors_limits(local_name: &str) -> Result<hcl::structure::Block> {
    // Azure limits all CORS settings, excluding XML tags, to 2 KiB. The
    // base64 length computes UTF-8 bytes instead of Terraform's character count.
    let payload = format!(
        "base64encode(join(\"\", [join(\",\", local.{local_name}), \"GET,HEAD*ETag,Content-Range,Accept-Ranges3600\"]))"
    );
    Ok(block(
        "lifecycle",
        [nested(block(
            "precondition",
            [
                attr(
                    "condition",
                    expr::parse(&format!(
                        "alltrue([for origin in local.{local_name} : length(origin) > 0 && length(origin) <= 256 && length(regexall(\"[,\\r\\n]\", origin)) == 0]) && length({payload}) * 3 / 4 - length(regexall(\"=\", {payload})) <= 2048"
                    ))?,
                ),
                attr(
                    "error_message",
                    Expression::from(
                        "Azure account-wide CORS origins must be nonempty, at most 256 characters each, contain no commas or line breaks, and fit with the read-rule settings within 2 KiB. Reduce the enabled containers' combined origins.",
                    ),
                ),
            ],
        ))],
    ))
}

/// Account settings include Live containers, whose parent is still setup-owned.
/// A declined container must not enable a capability on its siblings.
fn storage_capability(
    ctx: &EmitContext<'_>,
    requested: impl Fn(&Storage) -> bool,
) -> Result<Expression> {
    let mut gates = std::collections::BTreeSet::new();
    for (_, entry) in ctx.stack.resources() {
        let Some(storage) = entry.config.downcast_ref::<Storage>() else {
            continue;
        };
        if !requested(storage) {
            continue;
        }
        let Some(gate) = entry.enabled_when.as_deref() else {
            return Ok(Expression::Bool(true));
        };
        gates.insert(format!(
            "var.{}",
            crate::generator::stack_input_variable_name(gate)
        ));
    }
    if gates.is_empty() {
        Ok(Expression::Bool(false))
    } else {
        expr::parse(&gates.into_iter().collect::<Vec<_>>().join(" || "))
    }
}

fn stack_encryption_key_label<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a str> {
    ctx.stack.resources().find_map(|(_, entry)| {
        entry
            .config
            .downcast_ref::<Storage>()
            .and_then(|storage| storage.encryption_key.as_ref())
            .and_then(|key| ctx.name_for(&key.id))
    })
}

/// One setup owner writes the account policy. Disabled contributors keep disabled
/// rules, preserving a valid policy even when every container is declined.
fn emit_lifecycle_policy(
    ctx: &EmitContext<'_>,
    label: &str,
    fragment: &mut TfFragment,
) -> Result<()> {
    let mut rules = Vec::new();
    let mut legacy_aliases = Vec::new();
    for (id, entry) in ctx.stack.resources() {
        let Some(storage) = entry.config.downcast_ref::<Storage>() else {
            continue;
        };
        let Some(storage_label) = ctx.name_for(id) else {
            return Err(AlienError::new(ErrorData::GenericError {
                message: format!("Missing Azure storage Terraform name for '{id}'"),
            }));
        };
        // Older packages gave each Frozen container a Terraform address for the
        // same remote policy. Forget those aliases without deleting that policy.
        if storage_label != label {
            legacy_aliases.push(block(
                "removed",
                [
                    attr(
                        "from",
                        expr::traversal(["azurerm_storage_management_policy", storage_label]),
                    ),
                    nested(block(
                        "lifecycle",
                        [attr("destroy", Expression::Bool(false))],
                    )),
                ],
            ));
        }
        let mut enabled = "var.azure_storage_lifecycle_enabled".to_string();
        if entry.lifecycle == ResourceLifecycle::Live {
            // Previously ignored Live declarations must never start expiring old
            // objects merely because the installer downloads a newer package.
            enabled.push_str(" && var.azure_storage_live_lifecycle_enabled");
        }
        if let Some(gate) = entry.enabled_when.as_deref() {
            enabled.push_str(&format!(
                " && var.{}",
                crate::generator::stack_input_variable_name(gate)
            ));
        }
        for (index, rule) in storage.lifecycle_rules.iter().enumerate() {
            if rule.days > 99_999 {
                return Err(AlienError::new(ErrorData::GenericError {
                    message: format!(
                        "Azure storage '{id}' lifecycle rule {} exceeds 99999 days",
                        index + 1
                    ),
                }));
            }
            let prefix = Expression::FuncCall(Box::new(
                FuncCall::builder(Identifier::sanitized("format"))
                    .arg(Expression::from("%s/%s"))
                    .arg(super::storage::container_name_expr(id))
                    .arg(Expression::from(rule.prefix.clone().unwrap_or_default()))
                    .build(),
            ));
            rules.push(nested(block(
                "rule",
                [
                    attr(
                        "name",
                        Expression::from(format!("{storage_label}-rule-{}", index + 1)),
                    ),
                    attr("enabled", expr::parse(&enabled)?),
                    nested(block(
                        "filters",
                        [
                            attr(
                                "blob_types",
                                Expression::Array(vec![Expression::from("blockBlob")]),
                            ),
                            attr("prefix_match", Expression::Array(vec![prefix])),
                        ],
                    )),
                    nested(block(
                        "actions",
                        [nested(block(
                            "base_blob",
                            [attr(
                                "delete_after_days_since_modification_greater_than",
                                Expression::from(i64::from(rule.days)),
                            )],
                        ))],
                    )),
                ],
            )));
        }
    }
    if rules.is_empty() {
        return Ok(());
    }
    if rules.len() > 100 {
        return Err(AlienError::new(ErrorData::GenericError {
            message: format!(
                "Azure account lifecycle policy has {} rules; at most 100 are supported across all containers",
                rules.len()
            ),
        }));
    }
    fragment.moved_blocks.extend(legacy_aliases);
    fragment.moved_blocks.push(block(
        "import",
        [
            attr(
                "for_each",
                expr::parse(
                    "var.azure_storage_import_existing_policy ? toset([\"existing\"]) : toset([])",
                )?,
            ),
            attr(
                "to",
                expr::traversal(["azurerm_storage_management_policy", label]),
            ),
            attr(
                "id",
                expr::raw(format!(
                    "\"${{azurerm_storage_account.{label}.id}}/managementPolicies/default\""
                )),
            ),
        ],
    ));
    let mut body = vec![attr(
        "storage_account_id",
        expr::traversal(["azurerm_storage_account", label, "id"]),
    )];
    body.extend(rules);
    fragment.resource_blocks.push(resource_block(
        "azurerm_storage_management_policy",
        label,
        body,
    ));
    Ok(())
}
