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
    emitters::azure::helpers::{downcast, required_label},
    expr,
};
use alien_core::{
    import::EmitContext, AzureStorageAccount, ErrorData, LifecycleRule, Result, Storage,
};
use alien_error::AlienError;
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
