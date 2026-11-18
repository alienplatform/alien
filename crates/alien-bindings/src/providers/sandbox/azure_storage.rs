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

/// The longest a grant lives: as long as a user delegation key may (seven days from now, less a
/// margin). A grant also ends with the sandbox it is for, whichever comes first; the SAS is signed
/// with a delegation key that expires at the same moment, so it cannot outlive it.
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

    /// A grant to read and write `key`, until `deadline` or for [`GRANT_LIFETIME`] from now,
    /// whichever is earlier.
    pub(crate) async fn grant(
        &self,
        key: &str,
        deadline: Option<DateTime<Utc>>,
    ) -> Result<StorageGrant> {
        mint_key_grant(
            self.delegation.as_ref(),
            &self.account,
            &self.container,
            key,
            Utc::now(),
            deadline,
        )
        .await
    }
}

/// A grant to `key` in `container` of `account`, valid from `now` until `deadline` or for
/// [`GRANT_LIFETIME`], whichever is earlier.
///
/// The account and container come from setup; they are checked anyway, because they are put
/// into the URL the agent sends its SAS to.
pub(crate) async fn mint_key_grant(
    delegation: &dyn BlobDelegationApi,
    account: &str,
    container: &str,
    key: &str,
    now: DateTime<Utc>,
    deadline: Option<DateTime<Utc>>,
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
    let expires_at = deadline.map_or(now + GRANT_LIFETIME, |deadline| {
        deadline.min(now + GRANT_LIFETIME)
    });
    if expires_at <= now {
        return Err(invalid(
            "deadline",
            format!("the sandbox's life ended at {expires_at}"),
        ));
    }
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
            None,
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

    /// A sandbox that ends in two hours gets a grant, and a delegation key, that end with it: the
    /// SAS can never outlive the sandbox or the key that signed it.
    #[tokio::test]
    async fn a_grant_ends_with_the_sandbox() {
        let deadline = now() + Duration::hours(2);
        let mut api = MockBlobDelegationApi::new();
        api.expect_get_user_delegation_key()
            .times(1)
            .returning(move |_, start, expiry| {
                assert_eq!(start, now() - CLOCK_SKEW);
                assert_eq!(expiry, deadline, "the delegation key ends with the sandbox");
                Ok(UserDelegationKey {
                    signed_oid: "00000000-0000-0000-0000-000000000001".to_string(),
                    signed_tid: "00000000-0000-0000-0000-000000000002".to_string(),
                    signed_start: "2026-11-18T11:55:00Z".to_string(),
                    signed_expiry: "2026-11-18T14:00:00Z".to_string(),
                    signed_service: "b".to_string(),
                    signed_version: "2026-10-06".to_string(),
                    value: "c2VjcmV0LWtleS1ieXRlcy0zMi1sb25nLi4uLi4uLi4=".to_string(),
                })
            });
        let grant = mint_key_grant(
            &api,
            "acmeagents1a2b3c4d",
            "keys",
            "engine-42",
            now(),
            Some(deadline),
        )
        .await
        .expect("the grant mints");
        let StorageGrant::AzureSas {
            sas, expires_at, ..
        } = grant
        else {
            panic!("an Azure grant")
        };
        assert_eq!(expires_at, deadline);
        let query: std::collections::BTreeMap<String, String> =
            url::form_urlencoded::parse(sas.0.as_bytes())
                .into_owned()
                .collect();
        assert_eq!(query["se"], "2026-11-18T14:00:00Z");

        // A sandbox that may live longer than a delegation key gets the longest grant there is.
        let far = now() + Duration::days(30);
        let error = mint_key_grant(
            &MockBlobDelegationApi::new(),
            "acmeagents1a2b3c4d",
            "keys",
            "engine-42",
            now(),
            Some(now() - Duration::seconds(1)),
        )
        .await
        .expect_err("a sandbox already past its life gets no grant");
        assert_eq!(error.code, "INVALID_INPUT", "{error}");
        let grant = mint_key_grant(
            &delegation(),
            "acmeagents1a2b3c4d",
            "keys",
            "engine-42",
            now(),
            Some(far),
        )
        .await
        .expect("the grant mints");
        let StorageGrant::AzureSas { expires_at, .. } = grant else {
            panic!("an Azure grant")
        };
        assert_eq!(expires_at, now() + GRANT_LIFETIME);
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
                None,
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

/// Against a real hierarchical-namespace account (`AZURE_PERSISTENT_STORAGE_ACCOUNT`, container
/// `store`), with the binding's identity holding Blob Data Contributor on that container only:
///
/// ```text
/// AZURE_TARGET_SUBSCRIPTION_ID=… AZURE_TARGET_TENANT_ID=… AZURE_TARGET_CLIENT_ID=… \
/// AZURE_TARGET_CLIENT_SECRET=… AZURE_PERSISTENT_STORAGE_ACCOUNT=… \
/// cargo test -p alien-bindings --lib azure_storage::live -- --ignored
/// ```
#[cfg(test)]
mod live {
    use super::*;
    use alien_sandbox_storage::{AzureBlobStore, ObjectStore, Precondition, PutOutcome};

    fn env(name: &str) -> String {
        std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
    }

    fn grants() -> AzureGrants {
        let config = alien_core::AzureClientConfig {
            subscription_id: env("AZURE_TARGET_SUBSCRIPTION_ID"),
            tenant_id: env("AZURE_TARGET_TENANT_ID"),
            region: Some("eastus".to_string()),
            credentials: alien_core::AzureCredentials::ServicePrincipal {
                client_id: env("AZURE_TARGET_CLIENT_ID"),
                client_secret: env("AZURE_TARGET_CLIENT_SECRET"),
            },
            service_overrides: None,
        };
        AzureGrants::new(
            env("AZURE_PERSISTENT_STORAGE_ACCOUNT"),
            "store".to_string(),
            config,
        )
        .expect("the store builds")
    }

    /// A grant for a sandbox that ends in 60 s writes until then and is refused after: the
    /// effective expiry is the sandbox's deadline, measured against Azure, not a later one.
    #[tokio::test]
    #[ignore = "needs a live Azure hierarchical-namespace account"]
    async fn a_grant_stops_working_when_its_sandbox_ends() {
        let grants = grants();
        let key = format!("expiry-{}", Utc::now().timestamp_millis());
        let deadline = Utc::now() + Duration::seconds(60);
        let StorageGrant::AzureSas {
            key_url,
            sas,
            expires_at,
        } = grants
            .grant(&key, Some(deadline))
            .await
            .expect("the grant mints")
        else {
            panic!("an Azure grant")
        };
        assert_eq!(expires_at, deadline);
        let agent = AzureBlobStore::new(&key_url, &sas.0).expect("the agent's store");
        let mut last_ok = None;
        let mut first_refused = None;
        for attempt in 0..60 {
            let object = format!("packs/{attempt}");
            match agent
                .put_if(&object, b"pack".to_vec(), Precondition::Absent)
                .await
            {
                Ok(PutOutcome::Written { .. }) => last_ok = Some(Utc::now()),
                Ok(other) => panic!("unexpected outcome {other:?}"),
                Err(error) => {
                    let text = error.to_string();
                    assert!(text.contains("403"), "refused for expiry: {text}");
                    assert!(!text.contains("sig="), "the SAS is never printed: {text}");
                    first_refused = Some(Utc::now());
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
        let last_ok = last_ok.expect("the grant wrote before its deadline");
        let first_refused = first_refused.expect("the grant was refused after its deadline");
        eprintln!(
            "deadline {deadline}, last write {last_ok}, first refusal {first_refused}, \
             refused {}s after the deadline",
            (first_refused - deadline).num_seconds()
        );
        assert!(last_ok <= deadline + Duration::seconds(1));
        assert!(first_refused >= deadline - Duration::seconds(1));
        assert!(first_refused <= deadline + Duration::seconds(10));
        let store = grants.key_store(&key).expect("a key store");
        for object in store.list("").await.expect("the binding lists the key") {
            store.delete(&object).await.expect("the binding erases");
        }
    }

    /// The binding claims a key through `object_store` with its own identity (conditional puts
    /// on an HNS account), mints the agent's grant, and the grant writes and reads inside the
    /// key; the binding then reads what the grant wrote, lists the key and erases it.
    #[tokio::test]
    #[ignore = "needs a live Azure hierarchical-namespace account"]
    async fn the_binding_claims_grants_reads_and_erases_a_key() {
        let config = alien_core::AzureClientConfig {
            subscription_id: env("AZURE_TARGET_SUBSCRIPTION_ID"),
            tenant_id: env("AZURE_TARGET_TENANT_ID"),
            region: Some("eastus".to_string()),
            credentials: alien_core::AzureCredentials::ServicePrincipal {
                client_id: env("AZURE_TARGET_CLIENT_ID"),
                client_secret: env("AZURE_TARGET_CLIENT_SECRET"),
            },
            service_overrides: None,
        };
        let grants = AzureGrants::new(
            env("AZURE_PERSISTENT_STORAGE_ACCOUNT"),
            "store".to_string(),
            config,
        )
        .expect("the store builds");
        let key = format!("binding-{}", Utc::now().timestamp_millis());
        let store = grants.key_store(&key).expect("a key store");

        let claim = alien_sandbox_storage::takeover(&store, "sandbox-1")
            .await
            .expect("the binding claims the key");
        assert!(claim.manifest.is_none());
        let second = alien_sandbox_storage::takeover(&store, "sandbox-2")
            .await
            .expect("a second claim takes over");
        assert_ne!(claim.head_version, second.head_version);

        let StorageGrant::AzureSas { key_url, sas, .. } =
            grants.grant(&key, None).await.expect("the grant mints")
        else {
            panic!("an Azure grant")
        };
        let agent = AzureBlobStore::new(&key_url, &sas.0).expect("the agent's store");
        assert!(matches!(
            agent
                .put_if("packs/1-test", b"pack".to_vec(), Precondition::Absent)
                .await
                .expect("the grant writes in the key"),
            PutOutcome::Written { .. }
        ));
        assert!(matches!(
            agent
                .put_if(
                    "HEAD",
                    b"stale".to_vec(),
                    Precondition::Matches(claim.head_version)
                )
                .await
                .expect("answered"),
            PutOutcome::Conflict
        ));
        assert_eq!(
            store.get("packs/1-test").await.unwrap().unwrap().bytes,
            b"pack"
        );
        let mut listed = store.list("").await.expect("the binding lists the key");
        listed.sort();
        assert_eq!(listed, ["HEAD", "packs/1-test"]);
        for object in listed {
            store.delete(&object).await.expect("the binding erases");
        }
        assert!(store.list("").await.unwrap().is_empty());
    }
}
