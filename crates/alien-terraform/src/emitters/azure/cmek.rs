//! Customer-managed encryption keys for Azure data stores (Flexible Server, Managed Redis).
//!
//! The stack's `Key` is a Key Vault key created by setup in the deployment's subscription. A
//! store reads it through a user-assigned identity that holds the Key Vault Crypto Service
//! Encryption User role on the key. Disabling the key then stops the store, which is the kill
//! switch.

use alien_core::{import::EmitContext, ErrorData, ResourceRef, Result};
use alien_error::AlienError;
use crate::block::{attr, block, resource_block};
use crate::expr;
use crate::TfFragment;
use hcl::expr::Expression;

/// What a store needs to name its key.
pub(crate) struct StoreKey {
    /// `azurerm_user_assigned_identity.<owner>_encryption.id`
    pub identity_id: Expression,
    /// The key's versionless id, so the store follows key rotation.
    pub key_id: Expression,
    /// The key's current version id, for stores whose provider rejects a versionless id (Managed
    /// Redis). Such a store keeps that version until the next setup update.
    pub versioned_key_id: Expression,
    /// The role assignment the store must wait for.
    pub role_dependency: String,
    /// The store's `identity` block.
    pub identity_block: hcl::Block,
}

/// Creates the store's identity, lets it use `key`, and returns the pieces the store names.
pub(crate) fn grant_store_key(
    ctx: &EmitContext<'_>,
    fragment: &mut TfFragment,
    key: &ResourceRef,
    owner_label: &str,
    identity_name: &str,
) -> Result<StoreKey> {
    let key_label = ctx.name_for(key.id()).ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: format!("missing encryption Key '{}' for '{owner_label}'", key.id()),
        })
    })?;
    let identity_label = format!("{owner_label}_encryption");
    let role_label = format!("{owner_label}_encryption_key");
    fragment.resource_blocks.push(resource_block(
        "azurerm_user_assigned_identity",
        &identity_label,
        [
            attr(
                "name",
                // One identity per store: Azure names must be unique in the resource group.
                expr::raw(format!(
                    "\"${{local.resource_prefix}}-{identity_name}-{}\"",
                    owner_label.replace('_', "-")
                )),
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
    let identity_id = expr::traversal([
        "azurerm_user_assigned_identity",
        identity_label.as_str(),
        "id",
    ]);
    let identity_block = block(
        "identity",
        [
            attr("type", Expression::String("UserAssigned".to_string())),
            attr("identity_ids", Expression::Array(vec![identity_id.clone()])),
        ],
    );
    Ok(StoreKey {
        identity_id,
        key_id: expr::traversal(["azurerm_key_vault_key", key_label, "versionless_id"]),
        versioned_key_id: expr::traversal(["azurerm_key_vault_key", key_label, "id"]),
        role_dependency: format!("azurerm_role_assignment.{role_label}"),
        identity_block,
    })
}
