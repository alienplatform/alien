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
    block::{attr, block, nested, resource_block},
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

/// The store container's Terraform label.
fn container_label(label: &str) -> String {
    format!("{label}_store_keys")
}

/// ARM types of the store, created through ARM alone.
const STORAGE_ACCOUNT_TYPE: &str = "Microsoft.Storage/storageAccounts@2023-05-01";
const CONTAINER_TYPE: &str = "Microsoft.Storage/storageAccounts/blobServices/containers@2023-05-01";

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

/// The account, its container, and the holders' two grants; with a declared `encryptionKey`,
/// the account encrypts with that Key Vault key through an identity of its own.
pub(super) fn emit_store(
    ctx: &EmitContext<'_>,
    label: &str,
    storage: &SandboxPersistentStorage,
    holders: &[StoreHolder],
    fragment: &mut TfFragment,
) -> Result<()> {
    let store = store_label(label);
    let container = container_label(label);
    let mut properties = vec![
        ("accessTier", Expression::String("Hot".to_string())),
        // A directory SAS is honoured only with a hierarchical namespace.
        ("isHnsEnabled", Expression::Bool(true)),
        // Every request is Entra or a user-delegation SAS: no account key signs anything.
        ("allowSharedKeyAccess", Expression::Bool(false)),
        ("defaultToOAuthAuthentication", Expression::Bool(true)),
        ("allowBlobPublicAccess", Expression::Bool(false)),
        ("minimumTlsVersion", Expression::String("TLS1_2".to_string())),
        ("supportsHttpsTrafficOnly", Expression::Bool(true)),
    ];
    let mut account = vec![
        attr("type", Expression::String(STORAGE_ACCOUNT_TYPE.to_string())),
        attr("name", account_name(ctx.resource_id)),
        attr(
            "parent_id",
            expr::template(
                "/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}",
            ),
        ),
        attr("location", expr::raw("var.azure_location")),
        attr("tags", tags(ctx, "sandbox-storage")),
    ];
    let mut depends_on = Vec::new();
    if let Some(key) = &storage.encryption_key {
        let key_label = ctx.name_for(&key.id).ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: format!(
                    "persistentStorage.encryptionKey '{}' of sandbox '{}' is not in the stack",
                    key.id, ctx.resource_id
                ),
            })
        })?;
        let (identity, encryption, grant) =
            emit_customer_managed_key(ctx, &store, key_label, fragment);
        account.push(identity);
        properties.push(("encryption", encryption));
        // Azure checks the key when the account is created, so the grant must stand first.
        depends_on.push(grant);
    }
    // Through ARM alone: the azurerm account resource reads the queue and file services with an
    // account key after creating it, which an account with shared keys off refuses.
    account.push(attr(
        "body",
        expr::object([
            ("kind", Expression::String("StorageV2".to_string())),
            (
                "sku",
                expr::object([("name", Expression::String("Standard_LRS".to_string()))]),
            ),
            ("properties", expr::object(properties)),
        ]),
    ));
    if !depends_on.is_empty() {
        account.push(attr("depends_on", Expression::Array(depends_on)));
    }
    fragment
        .resource_blocks
        .push(resource_block("azapi_resource", &store, account));
    fragment.resource_blocks.push(resource_block(
        "azapi_resource",
        &container,
        [
            attr("type", Expression::String(CONTAINER_TYPE.to_string())),
            attr("name", Expression::String(STORE_CONTAINER.to_string())),
            attr(
                "parent_id",
                expr::template(format!("${{azapi_resource.{store}.id}}/blobServices/default")),
            ),
            attr(
                "body",
                expr::object([(
                    "properties",
                    expr::object([("publicAccess", Expression::String("None".to_string()))]),
                )]),
            ),
        ],
    ));

    for holder in holders {
        for (role, kind, scope) in [
            (
                BLOB_DELEGATOR_ROLE,
                "delegator",
                expr::traversal(["azapi_resource", store.as_str(), "id"]),
            ),
            (
                BLOB_DATA_CONTRIBUTOR_ROLE,
                "data",
                // The container's ARM id, so the data right ends at this container.
                expr::traversal(["azapi_resource", container.as_str(), "id"]),
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
    Ok(())
}

/// The store account encrypts with the declared key: a user-assigned identity of its own holds
/// Key Vault Crypto Service Encryption User on that key alone, and the account names both.
/// Returns the account's `identity` block, its `encryption` properties and the grant it waits on.
fn emit_customer_managed_key(
    ctx: &EmitContext<'_>,
    store: &str,
    key_label: &str,
    fragment: &mut TfFragment,
) -> (hcl::structure::Structure, Expression, Expression) {
    let identity_label = format!("{store}_encryption");
    let role_label = format!("{store}_encryption_key");
    fragment.resource_blocks.push(resource_block(
        "azurerm_user_assigned_identity",
        &identity_label,
        [
            attr(
                "name",
                expr::raw(format!(
                    "\"${{local.resource_prefix}}-{}-storage-encryption\"",
                    ctx.resource_id
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
    let identity = nested(block(
        "identity",
        [
            attr("type", Expression::String("UserAssigned".to_string())),
            attr("identity_ids", Expression::Array(vec![identity_id.clone()])),
        ],
    ));
    // No key version: the account follows the key's rotation.
    let encryption = expr::object([
        ("keySource", Expression::String("Microsoft.Keyvault".to_string())),
        (
            "identity",
            expr::object([("userAssignedIdentity", identity_id)]),
        ),
        (
            "keyvaultproperties",
            expr::object([
                (
                    "keyname",
                    expr::traversal(["azurerm_key_vault_key", key_label, "name"]),
                ),
                (
                    "keyvaulturi",
                    expr::raw(format!(
                        "regex(\"^https://[^/]+/\", azurerm_key_vault_key.{key_label}.versionless_id)"
                    )),
                ),
            ]),
        ),
        (
            "services",
            expr::object([(
                "blob",
                expr::object([
                    ("enabled", Expression::Bool(true)),
                    ("keyType", Expression::String("Account".to_string())),
                ]),
            )]),
        ),
    ]);
    let grant = expr::traversal(["azurerm_role_assignment", role_label.as_str()]);
    (identity, encryption, grant)
}

/// The binding's `persistentStorage`: the directory, and the store as the platform-neutral
/// locator. The store is the sandbox's own, so the prefix is empty and keys live at `<key>/`.
pub(super) fn binding(label: &str, storage: &SandboxPersistentStorage) -> Result<Expression> {
    let mut fields = vec![
        ("path", Expression::String(storage.path.clone())),
        (
            "store",
            expr::object([
                ("type", Expression::String("azure".to_string())),
                (
                    "account",
                    expr::traversal(["azapi_resource", &store_label(label), "name"]),
                ),
                ("container", Expression::String(STORE_CONTAINER.to_string())),
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
    use alien_core::bindings::{
        BindingValue, PersistentStoreLocator, SandboxPersistentStorageBinding,
    };
    use hcl::eval::{Context, Evaluate};

    /// The binding carries the store as the platform-neutral locator, plus the declared directory,
    /// exclusions and ceiling, with the account name read from the created account.
    #[test]
    fn the_binding_carries_the_store_locator() {
        let storage = SandboxPersistentStorage {
            path: "/workspace".to_string(),
            encryption_key: None,
            exclude: vec!["tmp/".to_string()],
            max_size: Some("8Gi".to_string()),
        };
        let expression = binding("agents", &storage).expect("the binding renders");

        let mut ctx = Context::new();
        ctx.declare_var(
            "azapi_resource",
            hcl::value!({ agents_store = { name = "acmeagents1a2b3c4d" } }),
        );
        let value = expression.evaluate(&ctx).expect("the binding evaluates");
        let json = serde_json::to_value(value).expect("the binding is JSON");
        let parsed: SandboxPersistentStorageBinding =
            serde_json::from_value(json.clone()).expect("the binding reads it");
        assert_eq!(
            parsed.store,
            PersistentStoreLocator::Azure {
                account: BindingValue::Value("acmeagents1a2b3c4d".to_string()),
                container: BindingValue::Value("keys".to_string()),
                prefix: String::new(),
            }
        );
        assert_eq!(
            json,
            serde_json::json!({
                "path": "/workspace",
                "store": {
                    "type": "azure",
                    "account": "acmeagents1a2b3c4d",
                    "container": "keys",
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
            encryption_key: None,
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
