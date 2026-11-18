//! The store an Azure sandbox's persistent storage is replicated to.
//!
//! Rendered only when the sandbox declares persistent storage, so every other stack renders
//! exactly as before. The store is its own account rather than a container in the stack's shared
//! one: a key's rights are a user-delegation SAS for one directory, and only an account with a
//! hierarchical namespace honours a directory SAS. It also keeps the shared account's public-read
//! and lifecycle rules away from the keys.
//!
//! The identities that create sandboxes on this group are the ones that mint a key's SAS, so each
//! gets Storage Blob Delegator on the account (a user delegation key is an account-level action)
//! and Storage Blob Data Contributor on the container alone. Nothing else in the stack is granted
//! anything on the store, and the guests never hold an Azure identity.

use crate::{
    block::{attr, resource_block},
    emitter::TfFragment,
    emitters::azure::helpers::tags,
    expr,
};
use alien_core::{import::EmitContext, ErrorData, Result, SandboxPersistentStorage};
use alien_error::AlienError;
use hcl::expr::Expression;

/// The container every key of the store lives in.
pub(super) const STORE_CONTAINER: &str = "keys";

/// Storage Blob Delegator: `Microsoft.Storage/storageAccounts/blobServices/generateUserDelegationKey/action`.
const BLOB_DELEGATOR_ROLE: &str = "db58b8e5-c6ad-4a2a-8342-4190687cbf4a";

/// Storage Blob Data Contributor: read, write and delete blobs.
const BLOB_DATA_CONTRIBUTOR_ROLE: &str = "ba92f5b4-2d11-453d-a403-e96b0029c9fe";

/// An identity that creates this group's sandboxes, and so mints their keys' SAS.
pub(super) struct StoreHolder {
    /// Unique among the sandbox's holders and made of `[a-z0-9_-]`: part of the Terraform address
    /// and of the assignment's name, so it must not change while the grant stands.
    pub(super) key: String,
    pub(super) principal_id: Expression,
}

/// The store account's Terraform label.
pub(super) fn store_label(label: &str) -> String {
    format!("{label}_store")
}

/// The account name: 3 to 24 lowercase letters and digits, unique across Azure.
///
/// The resolved prefix with everything but letters and digits removed, cut to 16, then 8 hex
/// digits of a hash of the prefix and the sandbox id, so two sandboxes whose ids share a long
/// start never meet in one name.
fn account_name(resource_id: &str) -> Expression {
    expr::raw(format!(
        "\"${{substr(replace(lower(local.resource_prefix), \"/[^a-z0-9]/\", \"\"), 0, 16)}}${{substr(sha256(\"${{local.resource_prefix}}:{resource_id}\"), 0, 8)}}\""
    ))
}

/// The account, its container, and the holders' two grants.
pub(super) fn emit_store(
    ctx: &EmitContext<'_>,
    label: &str,
    holders: &[StoreHolder],
    fragment: &mut TfFragment,
) {
    let store = store_label(label);
    fragment.resource_blocks.push(resource_block(
        "azurerm_storage_account",
        &store,
        [
            attr("name", account_name(ctx.resource_id)),
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
            // A directory SAS is honoured only with a hierarchical namespace.
            attr("is_hns_enabled", Expression::Bool(true)),
            // Every request is Entra or a user-delegation SAS: no account key signs anything.
            attr("shared_access_key_enabled", Expression::Bool(false)),
            attr("default_to_oauth_authentication", Expression::Bool(true)),
            attr("allow_nested_items_to_be_public", Expression::Bool(false)),
            attr("min_tls_version", Expression::String("TLS1_2".to_string())),
            attr("https_traffic_only_enabled", Expression::Bool(true)),
            attr("tags", tags(ctx, "sandbox-storage")),
        ],
    ));
    fragment.resource_blocks.push(resource_block(
        "azurerm_storage_container",
        &store,
        [
            attr("name", Expression::String(STORE_CONTAINER.to_string())),
            attr(
                "storage_account_id",
                expr::traversal(["azurerm_storage_account", store.as_str(), "id"]),
            ),
            attr(
                "container_access_type",
                Expression::String("private".to_string()),
            ),
        ],
    ));

    for holder in holders {
        for (role, kind, scope) in [
            (
                BLOB_DELEGATOR_ROLE,
                "delegator",
                expr::traversal(["azurerm_storage_account", store.as_str(), "id"]),
            ),
            (
                BLOB_DATA_CONTRIBUTOR_ROLE,
                "data",
                // The container's ARM id, so the data right ends at this container.
                expr::traversal([
                    "azurerm_storage_container",
                    store.as_str(),
                    "resource_manager_id",
                ]),
            ),
        ] {
            fragment.resource_blocks.push(resource_block(
                "azurerm_role_assignment",
                &format!("{store}-{}-{kind}", holder.key),
                [
                    attr(
                        "name",
                        expr::raw(format!(
                            "uuidv5(\"oid\", \"deployment:azure:sandbox-store:${{local.resource_prefix}}:{label}:{}:{kind}\")",
                            holder.key
                        )),
                    ),
                    attr("scope", scope),
                    attr(
                        "role_definition_id",
                        expr::template(format!(
                            "/subscriptions/${{var.azure_subscription_id}}/providers/Microsoft.Authorization/roleDefinitions/{role}"
                        )),
                    ),
                    attr("principal_id", holder.principal_id.clone()),
                ],
            ));
        }
    }
}

/// The binding's `persistentStorage`: the directory, and the store as the platform-neutral
/// locator `{account, container, prefix}`. Keys live at `<prefix>/<key>/` in the container.
pub(super) fn binding(label: &str, storage: &SandboxPersistentStorage) -> Result<Expression> {
    let mut fields = vec![
        ("path", Expression::String(storage.path.clone())),
        (
            "store",
            expr::object([
                (
                    "account",
                    expr::traversal(["azurerm_storage_account", &store_label(label), "name"]),
                ),
                ("container", Expression::String(STORE_CONTAINER.to_string())),
                (
                    "prefix",
                    Expression::String(alien_core::PERSISTENT_STORAGE_PREFIX.to_string()),
                ),
            ]),
        ),
    ];
    if !storage.exclude.is_empty() {
        fields.push((
            "exclude",
            Expression::Array(
                storage
                    .exclude
                    .iter()
                    .map(|path| Expression::String(path.clone()))
                    .collect(),
            ),
        ));
    }
    if let Some(bytes) = storage.max_size_bytes() {
        let bytes = i64::try_from(bytes).map_err(|_| {
            AlienError::new(ErrorData::GenericError {
                message: format!(
                    "persistentStorage.maxSize of {} bytes is larger than a binding carries",
                    bytes
                ),
            })
        })?;
        fields.push(("maxSizeBytes", Expression::Number(bytes.into())));
    }
    Ok(expr::object(fields))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{ResourceRef, Storage};
    use hcl::eval::{Context, Evaluate};

    /// The binding carries the store as the platform-neutral locator, plus the declared directory,
    /// exclusions and ceiling, with the account name read from the created account.
    #[test]
    fn the_binding_carries_the_store_locator() {
        let storage = SandboxPersistentStorage {
            path: "/workspace".to_string(),
            storage: ResourceRef::new(Storage::RESOURCE_TYPE, "files"),
            exclude: vec!["tmp/".to_string()],
            max_size: Some("8Gi".to_string()),
        };
        let expression = binding("agents", &storage).expect("the binding renders");

        let mut ctx = Context::new();
        ctx.declare_var(
            "azurerm_storage_account",
            hcl::value!({ agents_store = { name = "acmeagents1a2b3c4d" } }),
        );
        let value = expression.evaluate(&ctx).expect("the binding evaluates");
        assert_eq!(
            serde_json::to_value(value).expect("the binding is JSON"),
            serde_json::json!({
                "path": "/workspace",
                "store": {
                    "account": "acmeagents1a2b3c4d",
                    "container": "keys",
                    "prefix": "sandbox-storage",
                },
                "exclude": ["tmp/"],
                "maxSizeBytes": 8_589_934_592_u64,
            })
        );
    }

    /// No ceiling and no exclusions declared: neither key is written.
    #[test]
    fn undeclared_options_are_absent_from_the_binding() {
        let storage = SandboxPersistentStorage {
            path: "/workspace".to_string(),
            storage: ResourceRef::new(Storage::RESOURCE_TYPE, "files"),
            exclude: Vec::new(),
            max_size: None,
        };
        let rendered = hcl::format::to_string(&binding("agents", &storage).expect("renders"))
            .expect("formats");
        assert!(
            !rendered.contains("exclude") && !rendered.contains("maxSizeBytes"),
            "{rendered}"
        );
    }
}
