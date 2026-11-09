//! The manager's secrets digest key, kept in the manager's key-value store.
//!
//! The key must be the same for every process of one manager and survive a
//! restart, so a digest computed yesterday still matches today's. A manager
//! with a cloud key-value store keeps it there. The first writer creates it
//! with a conditional put and every other process adopts that copy. When the
//! manager has a values key, the stored copy is wrapped by it, so a reader of
//! the key-value store cannot use it to test guesses at digests.

use std::collections::BTreeMap;

use alien_bindings::traits::{Key, Kv, PutCondition, PutOptions};
use alien_error::AlienError;

use crate::error::{ErrorData, Result};

/// Key-value name of the stored digest key.
pub const DIGEST_KEY_NAME: &str = "manager:secrets-digest-key";

const RAW: u8 = 1;
const WRAPPED: u8 = 2;
const DIGEST_KEY_LEN: usize = 32;

fn wrap_context() -> BTreeMap<String, String> {
    BTreeMap::from([(
        "purpose".to_string(),
        "alien-secrets-digest-key".to_string(),
    )])
}

fn kv_error(action: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::InternalError {
        message: format!("Could not {action} the secrets digest key in the key-value store"),
    })
}

async fn read_stored(
    kv: &dyn Kv,
    wrapper: Option<&dyn Key>,
) -> Result<Option<[u8; DIGEST_KEY_LEN]>> {
    let Some(entry) = kv
        .get(DIGEST_KEY_NAME)
        .await
        .map_err(|_| kv_error("read"))?
    else {
        return Ok(None);
    };
    let damaged = || ErrorData::internal("The stored secrets digest key is damaged");
    let (tag, body) = entry.value.split_first().ok_or_else(damaged)?;
    let bytes = match *tag {
        RAW => body.to_vec(),
        WRAPPED => {
            let wrapper = wrapper.ok_or_else(|| {
                ErrorData::internal(
                    "The secrets digest key is protected by the values key, which is not set",
                )
            })?;
            wrapper
                .decrypt(body, Some(&wrap_context()))
                .await
                .map_err(|_| {
                    AlienError::new(ErrorData::ValueStoreKeyUnavailable {
                        reason: "the key could not unwrap the secrets digest key".to_string(),
                    })
                })?
        }
        _ => return Err(damaged()),
    };
    bytes.try_into().map(Some).map_err(|_| damaged())
}

/// Reads the digest key from `kv`, creating it on the first start. When several
/// processes start together, the key that was stored first wins for all of them.
pub async fn load_or_create_digest_key(
    kv: &dyn Kv,
    wrapper: Option<&dyn Key>,
) -> Result<[u8; DIGEST_KEY_LEN]> {
    if let Some(key) = read_stored(kv, wrapper).await? {
        return Ok(key);
    }

    let mut fresh = [0u8; DIGEST_KEY_LEN];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut fresh);
    let mut stored = Vec::new();
    match wrapper {
        Some(wrapper) => {
            stored.push(WRAPPED);
            stored.extend(
                wrapper
                    .encrypt(&fresh, Some(&wrap_context()))
                    .await
                    .map_err(|_| {
                        AlienError::new(ErrorData::ValueStoreKeyUnavailable {
                            reason: "the key could not wrap the secrets digest key".to_string(),
                        })
                    })?,
            );
        }
        None => {
            stored.push(RAW);
            stored.extend(fresh);
        }
    }
    let created = kv
        .put(
            DIGEST_KEY_NAME,
            stored,
            Some(PutOptions {
                ttl: None,
                condition: PutCondition::Absent,
            }),
        )
        .await
        .map_err(|_| kv_error("write"))?;
    if created {
        return Ok(fresh);
    }
    read_stored(kv, wrapper).await?.ok_or_else(|| {
        ErrorData::internal("The secrets digest key disappeared while it was being created")
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use alien_bindings::providers::kv::local::LocalKv;

    use super::super::cipher::testing::TestKey;
    use super::*;

    async fn local_kv(path: &std::path::Path) -> Arc<dyn Kv> {
        Arc::new(LocalKv::single_process(path.to_path_buf()).await.unwrap())
    }

    #[tokio::test]
    async fn a_new_process_over_the_same_store_gets_the_same_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kv");
        let first = {
            let kv = local_kv(&path).await;
            load_or_create_digest_key(kv.as_ref(), None).await.unwrap()
        };
        let kv = local_kv(&path).await;
        let second = load_or_create_digest_key(kv.as_ref(), None).await.unwrap();
        assert_eq!(first, second);
        assert_ne!(first, [0u8; 32]);
    }

    #[tokio::test]
    async fn processes_starting_together_agree_on_one_key() {
        let dir = tempfile::tempdir().unwrap();
        let kv = local_kv(&dir.path().join("kv")).await;
        let keys = futures::future::join_all(
            (0..16).map(|_| load_or_create_digest_key(kv.as_ref(), None)),
        )
        .await
        .into_iter()
        .map(|key| key.unwrap())
        .collect::<Vec<_>>();
        assert!(keys.iter().all(|key| *key == keys[0]));
    }

    #[tokio::test]
    async fn a_wrapped_key_is_not_readable_in_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let kv = local_kv(&dir.path().join("kv")).await;
        let wrapper = Arc::new(TestKey::default());
        let key = load_or_create_digest_key(kv.as_ref(), Some(wrapper.as_ref()))
            .await
            .unwrap();

        let stored = kv.get(DIGEST_KEY_NAME).await.unwrap().unwrap().value;
        assert!(!stored.windows(key.len()).any(|window| window == key));
        let again = load_or_create_digest_key(kv.as_ref(), Some(wrapper.as_ref()))
            .await
            .unwrap();
        assert_eq!(key, again);

        let without = load_or_create_digest_key(kv.as_ref(), None)
            .await
            .expect_err("a wrapped key needs the values key");
        assert_eq!(without.code, "INTERNAL_ERROR");
    }

    #[tokio::test]
    async fn a_denied_values_key_fails_instead_of_making_a_new_key() {
        let dir = tempfile::tempdir().unwrap();
        let kv = local_kv(&dir.path().join("kv")).await;
        let wrapper = Arc::new(TestKey::default());
        let key = load_or_create_digest_key(kv.as_ref(), Some(wrapper.as_ref()))
            .await
            .unwrap();
        wrapper
            .disabled
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let error = load_or_create_digest_key(kv.as_ref(), Some(wrapper.as_ref()))
            .await
            .expect_err("a denied key cannot unwrap");
        assert_eq!(error.code, "VALUE_STORE_KEY_UNAVAILABLE");
        wrapper
            .disabled
            .store(false, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            load_or_create_digest_key(kv.as_ref(), Some(wrapper.as_ref()))
                .await
                .unwrap(),
            key
        );
    }

    #[tokio::test]
    async fn a_stored_key_of_the_wrong_length_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let kv = local_kv(&dir.path().join("kv")).await;
        kv.put(DIGEST_KEY_NAME, vec![RAW, 1, 2, 3], None)
            .await
            .unwrap();
        let error = load_or_create_digest_key(kv.as_ref(), None)
            .await
            .expect_err("a short key is damaged");
        assert_eq!(error.code, "INTERNAL_ERROR");
    }
}
