//! Rights to one key of an Azure sandbox's persistent storage store.
//!
//! The binding's identity holds Storage Blob Delegator on the store account and Storage Blob Data
//! Contributor on its container (setup grants both). It asks the account for a user delegation
//! key and signs with it a SAS for the key's directory alone: read, add, create and write, no
//! list, delete, move or ACL rights. The agent receives that SAS as its grant and holds no Azure
//! identity; Azure refuses it anywhere outside `<container>/<key>/`.

use alien_azure_clients::azure::blob_sas::{
    directory_sas, BlobDelegationApi, DirectorySasPermissions,
};
use alien_error::{AlienError, Context};
use alien_sandbox_storage::{SasToken, StorageGrant};
use chrono::{DateTime, Duration, Utc};

use crate::error::{ErrorData, Result};

/// How long a grant lives: as long as a user delegation key may (seven days from now, less a
/// margin). Nothing renews it yet, as on AWS, so a sandbox that runs longer than this stops
/// committing (its state turns `behind`) and says so.
pub(crate) const GRANT_LIFETIME: Duration = Duration::minutes(7 * 24 * 60 - 10);

/// Back-dating of the start, so a grant is valid on a store whose clock runs a little behind.
const CLOCK_SKEW: Duration = Duration::minutes(5);

/// The SAS rights: everything a writer and a restore need, nothing that lists, deletes or moves.
const KEY_RIGHTS: DirectorySasPermissions = DirectorySasPermissions {
    read: true,
    add: true,
    create: true,
    write: true,
};

/// The store of one Azure sandbox's persistent storage, as the binding reaches it: its own Entra
/// identity for claims, reads and erasure (Blob Data Contributor on the container), and user
/// delegation keys (Blob Delegator on the account) for the agent's grants.
pub(crate) struct AzureGrants {
    account: String,
    container: String,
    store: std::sync::Arc<dyn object_store::ObjectStore>,
    delegation: std::sync::Arc<dyn BlobDelegationApi>,
}

impl std::fmt::Debug for AzureGrants {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AzureGrants")
            .field("account", &self.account)
            .field("container", &self.container)
            .finish_non_exhaustive()
    }
}

impl AzureGrants {
    /// The store in `container` of `account`, reached with `config`'s identity.
    pub(crate) fn new(
        account: String,
        container: String,
        config: alien_core::AzureClientConfig,
    ) -> Result<Self> {
        let store = object_store::azure::MicrosoftAzureBuilder::new()
            .with_account(&account)
            .with_container_name(&container)
            .with_credentials(std::sync::Arc::new(
                crate::providers::storage::credential_bridge::AzureCredentialBridge::new(
                    config.clone(),
                ),
            ))
            .build()
            .map_err(|error| {
                AlienError::new(ErrorData::BindingSetupFailed {
                    binding_type: "Azure sandbox".to_string(),
                    reason: format!(
                        "cannot build the persistent store client for '{account}': {error}"
                    ),
                })
            })?;
        let delegation = alien_azure_clients::azure::blob_sas::AzureBlobDelegationClient::new(
            reqwest::Client::new(),
            alien_azure_clients::AzureTokenCache::new(config),
        );
        Ok(Self {
            account,
            container,
            store: std::sync::Arc::new(store),
            delegation: std::sync::Arc::new(delegation),
        })
    }

    /// One key with the binding's full rights.
    pub(crate) fn key_store(
        &self,
        key: &str,
    ) -> alien_sandbox_storage::Result<super::persistent_store::KeyStore> {
        super::persistent_store::KeyStore::new(self.store.clone(), "", key)
    }

    /// A grant to read and write `key`, for [`GRANT_LIFETIME`] from now.
    pub(crate) async fn grant(&self, key: &str) -> Result<StorageGrant> {
        mint_key_grant(
            self.delegation.as_ref(),
            &self.account,
            &self.container,
            key,
            Utc::now(),
        )
        .await
    }
}

/// A grant to `key` in `container` of `account`, valid from `now` for [`GRANT_LIFETIME`].
///
/// The account and container come from setup; they are checked anyway, because they are put
/// into the URL the agent sends its SAS to.
pub(crate) async fn mint_key_grant(
    delegation: &dyn BlobDelegationApi,
    account: &str,
    container: &str,
    key: &str,
    now: DateTime<Utc>,
) -> Result<StorageGrant> {
    let invalid = |field: &str, details: String| {
        AlienError::new(ErrorData::InvalidInput {
            operation_context: "mint persistent storage rights".to_string(),
            details,
            field_name: Some(field.to_string()),
        })
    };
    if !(3..=24).contains(&account.len())
        || !account
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        return Err(invalid(
            "account",
            format!("'{account}' is not an Azure storage account name"),
        ));
    }
    if !(3..=63).contains(&container.len())
        || !container
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        || container.starts_with('-')
        || container.ends_with('-')
        || container.contains("--")
    {
        return Err(invalid(
            "container",
            format!("'{container}' is not an Azure container name"),
        ));
    }
    alien_sandbox_storage::validate_key(key)
        .map_err(|error| invalid("storageKey", error.message))?;

    let start = now - CLOCK_SKEW;
    let expires_at = now + GRANT_LIFETIME;
    let delegation_key = delegation
        .get_user_delegation_key(account, start, expires_at)
        .await
        .context(ErrorData::StorageOperationFailed {
            binding_name: account.to_string(),
            operation: "get a user delegation key for persistent storage".to_string(),
        })?;
    let sas = directory_sas(
        &delegation_key,
        account,
        container,
        key,
        KEY_RIGHTS,
        start,
        expires_at,
    )
    .context(ErrorData::StorageOperationFailed {
        binding_name: account.to_string(),
        operation: format!("sign the SAS for persistent storage key '{key}'"),
    })?;
    Ok(StorageGrant::AzureSas {
        key_url: format!("https://{account}.blob.core.windows.net/{container}/{key}/"),
        sas: SasToken(sas),
        expires_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_azure_clients::azure::blob_sas::{MockBlobDelegationApi, UserDelegationKey};
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 18, 12, 0, 0).unwrap()
    }

    fn delegation() -> MockBlobDelegationApi {
        let mut api = MockBlobDelegationApi::new();
        api.expect_get_user_delegation_key()
            .returning(|account, start, expiry| {
                assert_eq!(account, "acmeagents1a2b3c4d");
                assert_eq!(start, now() - CLOCK_SKEW);
                assert_eq!(expiry, now() + GRANT_LIFETIME);
                Ok(UserDelegationKey {
                    signed_oid: "00000000-0000-0000-0000-000000000001".to_string(),
                    signed_tid: "00000000-0000-0000-0000-000000000002".to_string(),
                    signed_start: "2026-11-18T11:55:00Z".to_string(),
                    signed_expiry: "2026-11-25T11:50:00Z".to_string(),
                    signed_service: "b".to_string(),
                    signed_version: "2026-10-06".to_string(),
                    value: "c2VjcmV0LWtleS1ieXRlcy0zMi1sb25nLi4uLi4uLi4=".to_string(),
                })
            });
        api
    }

    /// The grant names the key's directory and nothing above it, with read, add, create and write
    /// rights only, and expires with the delegation key.
    #[tokio::test]
    async fn a_grant_reaches_one_key_directory_with_writer_rights_only() {
        let grant = mint_key_grant(
            &delegation(),
            "acmeagents1a2b3c4d",
            "keys",
            "engine-42",
            now(),
        )
        .await
        .expect("the grant mints");
        let StorageGrant::AzureSas {
            key_url,
            sas,
            expires_at,
        } = grant
        else {
            panic!("an Azure grant")
        };
        assert_eq!(
            key_url,
            "https://acmeagents1a2b3c4d.blob.core.windows.net/keys/engine-42/"
        );
        assert_eq!(expires_at, now() + GRANT_LIFETIME);
        let query: std::collections::BTreeMap<String, String> =
            url::form_urlencoded::parse(sas.0.as_bytes())
                .into_owned()
                .collect();
        assert_eq!(query["sr"], "d");
        assert_eq!(query["sdd"], "1", "the depth of `engine-42` alone");
        assert_eq!(query["sp"], "racw");
        assert_eq!(query["spr"], "https");
        assert_eq!(query["se"], "2026-11-25T11:50:00Z");
    }

    /// Account, container and key go into the URL the agent sends its SAS to, so anything that
    /// could redirect it or widen the directory is refused before a delegation key is asked for.
    #[tokio::test]
    async fn names_that_could_move_the_url_or_the_directory_are_refused() {
        for (account, container, key) in [
            ("evil.example.com#", "keys", "engine-42"),
            ("Acme", "keys", "engine-42"),
            ("acmeagents1a2b3c4d", "keys/..", "engine-42"),
            ("acmeagents1a2b3c4d", "-keys", "engine-42"),
            ("acmeagents1a2b3c4d", "keys", "../engine-43"),
            ("acmeagents1a2b3c4d", "keys", "engine-42/sub"),
            ("acmeagents1a2b3c4d", "keys", ""),
        ] {
            let error = mint_key_grant(
                &MockBlobDelegationApi::new(),
                account,
                container,
                key,
                now(),
            )
            .await
            .expect_err("refused");
            assert_eq!(
                error.code, "INVALID_INPUT",
                "{account} {container} {key}: {error}"
            );
        }
    }
}

#[cfg(test)]
mod create_tests {
    use crate::providers::sandbox::azure::AzureSandbox;
    use crate::traits::{CreateSandboxRequest, Sandbox as _};
    use alien_azure_clients::azure::sandbox_data_plane::MockSandboxDataPlaneApi;
    use alien_core::bindings::{
        BindingValue, PersistentStoreLocator, SandboxPersistentStorageBinding,
    };
    use std::sync::Arc;

    fn declared() -> AzureSandbox {
        // No expectation on the data plane: any call to it fails the test.
        AzureSandbox::new(
            Arc::new(MockSandboxDataPlaneApi::new()),
            "group".into(),
            "ubuntu".into(),
            alien_core::SandboxEgress::None,
            None,
            "1000m".into(),
            "2048Mi".into(),
            None,
        )
        .with_persistent_storage(
            Some(SandboxPersistentStorageBinding {
                path: "/workspace".to_string(),
                store: PersistentStoreLocator::Azure {
                    account: BindingValue::Value("acmeagents1a2b3c4d".to_string()),
                    container: BindingValue::Value("keys".to_string()),
                    prefix: String::new(),
                },
                exclude: Vec::new(),
                max_size_bytes: None,
            }),
            "agents",
        )
        .expect("an Azure store")
    }

    /// A declared sandbox needs a key on create, and until the attach lands a key is refused
    /// outright: no sandbox is created whose directory would silently not persist.
    #[tokio::test]
    async fn a_declared_sandbox_is_not_created_without_its_storage() {
        let sandbox = declared();
        let missing = sandbox
            .create(CreateSandboxRequest::default())
            .await
            .expect_err("a key is required");
        assert_eq!(missing.code, "INVALID_INPUT", "{missing}");

        let keyed = sandbox
            .create(CreateSandboxRequest {
                storage_key: Some("engine-42".to_string()),
                ..Default::default()
            })
            .await
            .expect_err("not attached yet");
        assert_eq!(keyed.code, "OPERATION_NOT_SUPPORTED", "{keyed}");
    }
}
