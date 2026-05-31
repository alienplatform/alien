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
use alien_core::{import::EmitContext, AzureStorageAccount, Result, Storage};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureStorageAccountEmitter;

impl TfEmitter for AzureStorageAccountEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let _ = downcast::<AzureStorageAccount>(ctx, AzureStorageAccount::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;

        let versioning_enabled = stack_has_versioned_storage(ctx);

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
            attr("allow_nested_items_to_be_public", Expression::Bool(false)),
            attr("https_traffic_only_enabled", Expression::Bool(true)),
            attr("tags", tags(ctx, "storage-account")),
        ];
        body.push(nested(block(
            "blob_properties",
            [
                attr("versioning_enabled", Expression::Bool(versioning_enabled)),
                attr("change_feed_enabled", Expression::Bool(false)),
            ],
        )));

        Ok(TfFragment::default().with_resource(resource_block(
            "azurerm_storage_account",
            label,
            body,
        )))
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

/// True when any `Storage` resource in the stack has `versioning = true`.
/// Azure scopes Blob versioning to the storage account, so the parent
/// emitter folds the per-container intent up to the shared resource.
fn stack_has_versioned_storage(ctx: &EmitContext<'_>) -> bool {
    ctx.stack.resources().any(|(_id, entry)| {
        entry
            .config
            .downcast_ref::<Storage>()
            .map(|storage| storage.versioning)
            .unwrap_or(false)
    })
}
