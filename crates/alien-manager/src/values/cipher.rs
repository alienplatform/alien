//! Encryption of values before they reach a shared key-value store.
//!
//! Each scope has its own AES-256-GCM data key. The manager's Key binding
//! wraps the data key, and only the wrapped copy is stored. A sealed value is
//! bound to its scope and slot, so moving a record to another slot makes it
//! unreadable. A key that is disabled or denied makes every read and write
//! fail; there is no plaintext path.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use alien_bindings::traits::{Key, Kv, PutCondition, PutOptions};
use alien_error::AlienError;
use async_trait::async_trait;

use super::{ValueScope, ValueSlot};
use crate::error::{ErrorData, Result};

/// How long an unwrapped data key stays in memory.
pub const DATA_KEY_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

const SEALED_VERSION: u8 = 1;
const NONCE_LEN: usize = 12;
const DATA_KEY_LEN: usize = 32;

/// Encrypts and decrypts one record for a scope and slot.
#[async_trait]
pub trait ValueCipher: Send + Sync {
    async fn seal(&self, scope: &ValueScope, slot: &ValueSlot, plaintext: &[u8])
        -> Result<Vec<u8>>;
    async fn open(&self, scope: &ValueScope, slot: &ValueSlot, sealed: &[u8]) -> Result<Vec<u8>>;
}

/// Data keys wrapped by a [`Key`] binding and stored in a key-value store.
pub struct EnvelopeCipher {
    key: Arc<dyn Key>,
    kv: Arc<dyn Kv>,
    cache: Mutex<HashMap<ValueScope, CachedKey>>,
    cache_ttl: Duration,
}

struct CachedKey {
    key: [u8; DATA_KEY_LEN],
    loaded_at: Instant,
}

impl EnvelopeCipher {
    pub fn new(key: Arc<dyn Key>, kv: Arc<dyn Kv>) -> Self {
        Self::with_cache_ttl(key, kv, DATA_KEY_CACHE_TTL)
    }

    pub fn with_cache_ttl(key: Arc<dyn Key>, kv: Arc<dyn Kv>, cache_ttl: Duration) -> Self {
        Self {
            key,
            kv,
            cache: Mutex::new(HashMap::new()),
            cache_ttl,
        }
    }

    fn wrapped_key_name(scope: &ValueScope) -> String {
        // Not under `val:`, so a scan of a scope's values never lists it.
        format!("valk:{scope}")
    }

    fn wrap_context(scope: &ValueScope) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("purpose".to_string(), "alien-values-data-key".to_string()),
            ("scope".to_string(), scope.to_string()),
        ])
    }

    fn key_unavailable(action: &str) -> ErrorData {
        ErrorData::ValueStoreKeyUnavailable {
            reason: format!("the key could not {action} the data key for these values"),
        }
    }

    fn kv_error(action: &str) -> ErrorData {
        ErrorData::InternalError {
            message: format!("Could not {action} in the key-value store"),
        }
    }

    fn cached(&self, scope: &ValueScope) -> Option<[u8; DATA_KEY_LEN]> {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        match cache.get(scope) {
            Some(entry) if entry.loaded_at.elapsed() < self.cache_ttl => Some(entry.key),
            Some(_) => {
                cache.remove(scope);
                None
            }
            None => None,
        }
    }

    fn remember(&self, scope: &ValueScope, key: [u8; DATA_KEY_LEN]) {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).insert(
            scope.clone(),
            CachedKey {
                key,
                loaded_at: Instant::now(),
            },
        );
    }

    async fn unwrap_stored(
        &self,
        scope: &ValueScope,
        wrapped: &[u8],
    ) -> Result<[u8; DATA_KEY_LEN]> {
        let plaintext = self
            .key
            .decrypt(wrapped, Some(&Self::wrap_context(scope)))
            .await
            .map_err(|_| AlienError::new(Self::key_unavailable("unwrap")))?;
        let key: [u8; DATA_KEY_LEN] = plaintext
            .try_into()
            .map_err(|_| ErrorData::internal("The stored data key for this scope is damaged"))?;
        self.remember(scope, key);
        Ok(key)
    }

    /// The scope's data key. Only a write creates one.
    async fn data_key(
        &self,
        scope: &ValueScope,
        create: bool,
    ) -> Result<Option<[u8; DATA_KEY_LEN]>> {
        if let Some(key) = self.cached(scope) {
            return Ok(Some(key));
        }
        let name = Self::wrapped_key_name(scope);
        let stored = self
            .kv
            .get(&name)
            .await
            .map_err(|_| AlienError::new(Self::kv_error("read a data key")))?;
        if let Some(entry) = stored {
            return self.unwrap_stored(scope, &entry.value).await.map(Some);
        }
        if !create {
            return Ok(None);
        }

        let mut fresh = [0u8; DATA_KEY_LEN];
        rand::RngCore::fill_bytes(&mut rand::rng(), &mut fresh);
        let wrapped = self
            .key
            .encrypt(&fresh, Some(&Self::wrap_context(scope)))
            .await
            .map_err(|_| AlienError::new(Self::key_unavailable("wrap")))?;
        let created = self
            .kv
            .put(
                &name,
                wrapped,
                Some(PutOptions {
                    ttl: None,
                    condition: PutCondition::Absent,
                }),
            )
            .await
            .map_err(|_| AlienError::new(Self::kv_error("write a data key")))?;
        if created {
            self.remember(scope, fresh);
            return Ok(Some(fresh));
        }
        // Another writer created the key first: use theirs.
        let winner = self
            .kv
            .get(&name)
            .await
            .map_err(|_| AlienError::new(Self::kv_error("read a data key")))?
            .ok_or_else(|| {
                ErrorData::internal(
                    "The data key for this scope disappeared while it was being created",
                )
            })?;
        self.unwrap_stored(scope, &winner.value).await.map(Some)
    }

    fn aad(scope: &ValueScope, slot: &ValueSlot) -> Vec<u8> {
        format!("alien-values-v{SEALED_VERSION}\0{scope}\0{slot}").into_bytes()
    }
}

#[async_trait]
impl ValueCipher for EnvelopeCipher {
    async fn seal(
        &self,
        scope: &ValueScope,
        slot: &ValueSlot,
        plaintext: &[u8],
    ) -> Result<Vec<u8>> {
        let key = self
            .data_key(scope, true)
            .await?
            .expect("a key is created when it is missing");
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|_| ErrorData::internal("The data key has a wrong length"))?;
        let mut nonce = [0u8; NONCE_LEN];
        rand::RngCore::fill_bytes(&mut rand::rng(), &mut nonce);
        let encrypted = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: &Self::aad(scope, slot),
                },
            )
            .map_err(|_| ErrorData::internal("Could not encrypt a value"))?;
        let mut sealed = Vec::with_capacity(1 + NONCE_LEN + encrypted.len());
        sealed.push(SEALED_VERSION);
        sealed.extend_from_slice(&nonce);
        sealed.extend_from_slice(&encrypted);
        Ok(sealed)
    }

    async fn open(&self, scope: &ValueScope, slot: &ValueSlot, sealed: &[u8]) -> Result<Vec<u8>> {
        let unreadable = |reason: &str| {
            AlienError::new(ErrorData::StoredValueUnreadable {
                slot: slot.to_string(),
                reason: reason.to_string(),
            })
        };
        let [version, rest @ ..] = sealed else {
            return Err(unreadable("the record is empty"));
        };
        if *version != SEALED_VERSION {
            return Err(unreadable("the record is not encrypted by this store"));
        }
        let (nonce, encrypted) = rest
            .split_first_chunk::<NONCE_LEN>()
            .ok_or_else(|| unreadable("the record is too short"))?;
        let key = self
            .data_key(scope, false)
            .await?
            .ok_or_else(|| unreadable("the key for its scope is missing"))?;
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|_| ErrorData::internal("The data key has a wrong length"))?;
        cipher
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: encrypted,
                    aad: &Self::aad(scope, slot),
                },
            )
            .map_err(|_| unreadable("it was changed or belongs to another slot"))
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use alien_bindings::traits::Binding;

    use super::*;

    /// A key whose wrap format binds the context, and which can be disabled.
    #[derive(Debug, Default)]
    pub struct TestKey {
        pub disabled: AtomicBool,
        pub unwraps: AtomicUsize,
    }

    impl TestKey {
        fn cipher() -> Aes256Gcm {
            Aes256Gcm::new_from_slice(&[0x42; 32]).unwrap()
        }

        fn context_bytes(context: Option<&BTreeMap<String, String>>) -> Vec<u8> {
            serde_json::to_vec(&context).unwrap()
        }

        fn check_enabled(&self) -> alien_bindings::error::Result<()> {
            if self.disabled.load(Ordering::SeqCst) {
                return Err(AlienError::new(
                    alien_bindings::error::ErrorData::KeyInputInvalid {
                        reason: "key disabled".to_string(),
                    },
                ));
            }
            Ok(())
        }
    }

    impl Binding for TestKey {}

    #[async_trait]
    impl Key for TestKey {
        async fn encrypt(
            &self,
            plaintext: &[u8],
            context: Option<&BTreeMap<String, String>>,
        ) -> alien_bindings::error::Result<Vec<u8>> {
            self.check_enabled()?;
            let mut nonce = [0u8; NONCE_LEN];
            rand::RngCore::fill_bytes(&mut rand::rng(), &mut nonce);
            let mut out = nonce.to_vec();
            out.extend(
                Self::cipher()
                    .encrypt(
                        Nonce::from_slice(&nonce),
                        Payload {
                            msg: plaintext,
                            aad: &Self::context_bytes(context),
                        },
                    )
                    .unwrap(),
            );
            Ok(out)
        }

        async fn decrypt(
            &self,
            ciphertext: &[u8],
            context: Option<&BTreeMap<String, String>>,
        ) -> alien_bindings::error::Result<Vec<u8>> {
            self.check_enabled()?;
            self.unwraps.fetch_add(1, Ordering::SeqCst);
            let (nonce, rest) = ciphertext.split_at(NONCE_LEN);
            Self::cipher()
                .decrypt(
                    Nonce::from_slice(nonce),
                    Payload {
                        msg: rest,
                        aad: &Self::context_bytes(context),
                    },
                )
                .map_err(|_| {
                    AlienError::new(alien_bindings::error::ErrorData::KeyCiphertextInvalid {
                        reason: "context mismatch".to_string(),
                    })
                })
        }
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::testing::TestKey;
    use super::*;
    use crate::values::KvValueBackend;

    /// A backend over `kv` that seals with a [`TestKey`], and that key.
    pub fn sealed_backend(kv: Arc<dyn Kv>) -> (Arc<TestKey>, KvValueBackend) {
        let key = Arc::new(TestKey::default());
        let cipher = Arc::new(EnvelopeCipher::new(key.clone(), kv.clone()));
        (key, KvValueBackend::new(kv, cipher))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use alien_bindings::providers::kv::local::LocalKv;
    use alien_bindings::traits::Kv;

    use super::fixtures::sealed_backend;
    use super::testing::TestKey;
    use super::*;
    use crate::values::{KvValueBackend, ValueKind, ValueStore};

    const CANARY: &str = "canary-3b7e91d4-value";

    async fn local_kv() -> (tempfile::TempDir, Arc<dyn Kv>) {
        let dir = tempfile::tempdir().unwrap();
        let kv = LocalKv::single_process(dir.path().join("kv"))
            .await
            .unwrap();
        (dir, Arc::new(kv))
    }

    fn scope(s: &str) -> ValueScope {
        s.parse().unwrap()
    }

    fn slot(s: &str) -> ValueSlot {
        s.parse().unwrap()
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    async fn every_entry(kv: &Arc<dyn Kv>) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        for prefix in ["val", "valk"] {
            let page = kv
                .scan_prefix(&format!("{prefix}:"), Some(1000), None)
                .await
                .unwrap();
            out.extend(page.items.into_iter().map(|e| (e.key, e.value)));
        }
        out
    }

    #[tokio::test]
    async fn a_raw_read_of_the_key_value_store_finds_no_value() {
        alien_deployment::set_ephemeral_secrets_digest_key();
        let (_dir, kv) = local_kv().await;
        let (_key, backend) = sealed_backend(kv.clone());
        let store = ValueStore::new(Arc::new(backend));
        let dep = scope("dep:dep_1");
        store
            .put(&dep, &slot("env:TOKEN"), ValueKind::Secret, CANARY.into())
            .await
            .unwrap();
        store
            .put(&dep, &slot("env:REGION"), ValueKind::Plain, CANARY.into())
            .await
            .unwrap();

        let entries = every_entry(&kv).await;
        assert_eq!(entries.len(), 3, "two values and one wrapped data key");
        for (key, value) in &entries {
            assert!(!contains(value, CANARY.as_bytes()), "{key} holds the value");
            assert!(!key.contains(CANARY), "{key}");
            assert!(!contains(value, b"TOKEN"), "{key} holds the name");
        }
        let got = store.get(&dep, &slot("env:TOKEN")).await.unwrap().unwrap();
        assert_eq!(got.value, CANARY);
    }

    /// Passes every call to `inner` and records the TTL of each write.
    #[derive(Debug)]
    struct RecordingKv {
        inner: Arc<dyn Kv>,
        writes: std::sync::Mutex<Vec<(String, Option<Duration>)>>,
    }

    impl alien_bindings::traits::Binding for RecordingKv {}

    #[async_trait::async_trait]
    impl Kv for RecordingKv {
        async fn get(
            &self,
            key: &str,
        ) -> alien_bindings::error::Result<Option<alien_bindings::traits::KvEntry>> {
            self.inner.get(key).await
        }

        async fn put(
            &self,
            key: &str,
            value: Vec<u8>,
            options: Option<PutOptions>,
        ) -> alien_bindings::error::Result<bool> {
            let ttl = options.as_ref().and_then(|options| options.ttl);
            self.writes.lock().unwrap().push((key.to_string(), ttl));
            self.inner.put(key, value, options).await
        }

        async fn delete(
            &self,
            key: &str,
            if_version: Option<&str>,
        ) -> alien_bindings::error::Result<bool> {
            self.inner.delete(key, if_version).await
        }

        async fn exists(&self, key: &str) -> alien_bindings::error::Result<bool> {
            self.inner.exists(key).await
        }

        async fn scan_prefix(
            &self,
            prefix: &str,
            limit: Option<usize>,
            cursor: Option<String>,
        ) -> alien_bindings::error::Result<alien_bindings::traits::ScanResult> {
            self.inner.scan_prefix(prefix, limit, cursor).await
        }
    }

    /// DynamoDB deletes an item whose TTL attribute has passed. A value or its
    /// wrapped data key written with a TTL would disappear without anyone
    /// deleting it, so no write may carry one.
    #[tokio::test]
    async fn values_and_their_data_keys_are_written_without_a_ttl() {
        alien_deployment::set_ephemeral_secrets_digest_key();
        let (_dir, inner) = local_kv().await;
        let recording = Arc::new(RecordingKv {
            inner,
            writes: Default::default(),
        });
        let kv: Arc<dyn Kv> = recording.clone();
        let (_key, backend) = sealed_backend(kv);
        let store = ValueStore::new(Arc::new(backend));
        let dep = scope("dep:dep_1");

        store
            .put(&dep, &slot("env:TOKEN"), ValueKind::Secret, CANARY.into())
            .await
            .unwrap();
        store
            .put(&dep, &slot("env:REGION"), ValueKind::Plain, "eu".into())
            .await
            .unwrap();

        let writes = recording.writes.lock().unwrap().clone();
        let keys: Vec<&str> = writes.iter().map(|(key, _)| key.as_str()).collect();
        assert!(keys.iter().any(|key| key.starts_with("val:")), "{keys:?}");
        assert!(keys.iter().any(|key| key.starts_with("valk:")), "{keys:?}");
        for (key, ttl) in &writes {
            assert_eq!(*ttl, None, "{key} was written with a TTL");
        }
    }

    #[tokio::test]
    async fn the_same_value_seals_differently_each_time() {
        let (_dir, kv) = local_kv().await;
        let key = Arc::new(TestKey::default());
        let cipher = EnvelopeCipher::new(key, kv);
        let dep = scope("dep:dep_1");
        let a = cipher.seal(&dep, &slot("env:A"), b"same").await.unwrap();
        let b = cipher.seal(&dep, &slot("env:A"), b"same").await.unwrap();
        assert_ne!(a, b);
    }

    #[tokio::test]
    async fn a_denied_key_fails_the_write_the_read_and_the_list() {
        alien_deployment::set_ephemeral_secrets_digest_key();
        let (_dir, kv) = local_kv().await;
        let key = Arc::new(TestKey::default());
        let cipher = Arc::new(EnvelopeCipher::with_cache_ttl(
            key.clone(),
            kv.clone(),
            Duration::ZERO,
        ));
        let store = ValueStore::new(Arc::new(KvValueBackend::new(kv.clone(), cipher)));
        let dep = scope("dep:dep_1");
        store
            .put(&dep, &slot("env:TOKEN"), ValueKind::Secret, CANARY.into())
            .await
            .unwrap();

        key.disabled.store(true, Ordering::SeqCst);

        for error in [
            store
                .put(&dep, &slot("env:NEW"), ValueKind::Secret, "x".into())
                .await
                .map(|_| ())
                .expect_err("a write needs the key"),
            store
                .get(&dep, &slot("env:TOKEN"))
                .await
                .map(|_| ())
                .expect_err("a read needs the key"),
            store
                .list(&dep)
                .await
                .map(|_| ())
                .expect_err("a list needs the key"),
        ] {
            assert_eq!(error.code, "VALUE_STORE_KEY_UNAVAILABLE");
            assert_eq!(error.http_status_code, Some(503));
        }

        key.disabled.store(false, Ordering::SeqCst);
        assert_eq!(
            store
                .get(&dep, &slot("env:TOKEN"))
                .await
                .unwrap()
                .unwrap()
                .value,
            CANARY,
            "re-enabling the key recovers"
        );
    }

    #[tokio::test]
    async fn a_record_moved_to_another_slot_or_scope_cannot_be_read() {
        alien_deployment::set_ephemeral_secrets_digest_key();
        let (_dir, kv) = local_kv().await;
        let (_key, backend) = sealed_backend(kv.clone());
        let store = ValueStore::new(Arc::new(backend));
        let dep = scope("dep:dep_1");
        store
            .put(&dep, &slot("env:A"), ValueKind::Secret, "a-value".into())
            .await
            .unwrap();
        store
            .put(&dep, &slot("env:B"), ValueKind::Secret, "b-value".into())
            .await
            .unwrap();
        store
            .put(
                &scope("dep:dep_2"),
                &slot("env:A"),
                ValueKind::Secret,
                "other".into(),
            )
            .await
            .unwrap();

        // Copy A's sealed record over B's key, and over the other scope's A.
        let a = kv.get("val:dep:dep_1:env:A").await.unwrap().unwrap().value;
        kv.put("val:dep:dep_1:env:B", a.clone(), None)
            .await
            .unwrap();
        kv.put("val:dep:dep_2:env:A", a, None).await.unwrap();

        let moved_slot = store
            .get(&dep, &slot("env:B"))
            .await
            .expect_err("a record under another slot must not open");
        assert_eq!(moved_slot.code, "STORED_VALUE_UNREADABLE");
        assert!(!moved_slot.message.contains("a-value"));
        let moved_scope = store
            .get(&scope("dep:dep_2"), &slot("env:A"))
            .await
            .expect_err("a record under another scope must not open");
        assert_eq!(moved_scope.code, "STORED_VALUE_UNREADABLE");
    }

    #[tokio::test]
    async fn a_changed_record_cannot_be_read() {
        alien_deployment::set_ephemeral_secrets_digest_key();
        let (_dir, kv) = local_kv().await;
        let (_key, backend) = sealed_backend(kv.clone());
        let store = ValueStore::new(Arc::new(backend));
        let dep = scope("dep:dep_1");
        store
            .put(&dep, &slot("env:A"), ValueKind::Secret, "a-value".into())
            .await
            .unwrap();
        let mut sealed = kv.get("val:dep:dep_1:env:A").await.unwrap().unwrap().value;
        let last = sealed.len() - 1;
        sealed[last] ^= 1;
        kv.put("val:dep:dep_1:env:A", sealed, None).await.unwrap();

        let error = store.get(&dep, &slot("env:A")).await.unwrap_err();
        assert_eq!(error.code, "STORED_VALUE_UNREADABLE");
    }

    #[tokio::test]
    async fn a_second_process_reads_what_the_first_wrote_and_caches_the_data_key() {
        alien_deployment::set_ephemeral_secrets_digest_key();
        let (_dir, kv) = local_kv().await;
        let key = Arc::new(TestKey::default());
        let writer = EnvelopeCipher::new(key.clone(), kv.clone());
        let dep = scope("dep:dep_1");
        let sealed = writer.seal(&dep, &slot("env:A"), b"value").await.unwrap();
        assert_eq!(
            key.unwraps.load(Ordering::SeqCst),
            0,
            "the writer that created the key never unwrapped it"
        );

        let reader = EnvelopeCipher::new(key.clone(), kv.clone());
        for _ in 0..3 {
            assert_eq!(
                reader.open(&dep, &slot("env:A"), &sealed).await.unwrap(),
                b"value"
            );
        }
        assert_eq!(
            key.unwraps.load(Ordering::SeqCst),
            1,
            "cached after one unwrap"
        );
    }

    #[tokio::test]
    async fn the_data_key_is_unwrapped_again_after_the_cache_expires() {
        let (_dir, kv) = local_kv().await;
        let key = Arc::new(TestKey::default());
        let cipher = EnvelopeCipher::with_cache_ttl(key.clone(), kv, Duration::ZERO);
        let dep = scope("dep:dep_1");
        let sealed = cipher.seal(&dep, &slot("env:A"), b"value").await.unwrap();
        cipher.open(&dep, &slot("env:A"), &sealed).await.unwrap();
        cipher.open(&dep, &slot("env:A"), &sealed).await.unwrap();
        assert_eq!(key.unwraps.load(Ordering::SeqCst), 2);
        assert!(DATA_KEY_CACHE_TTL <= Duration::from_secs(300));
    }

    #[tokio::test]
    async fn concurrent_first_writes_in_two_processes_end_with_one_data_key() {
        let (_dir, kv) = local_kv().await;
        let key = Arc::new(TestKey::default());
        let first = Arc::new(EnvelopeCipher::new(key.clone(), kv.clone()));
        let second = Arc::new(EnvelopeCipher::new(key.clone(), kv.clone()));
        let dep = scope("dep:dep_1");

        let (slot_a, slot_b) = (slot("env:A"), slot("env:B"));
        let (a, b) = tokio::join!(
            first.seal(&dep, &slot_a, b"one"),
            second.seal(&dep, &slot_b, b"two"),
        );
        let (a, b) = (a.unwrap(), b.unwrap());

        let third = EnvelopeCipher::new(key, kv.clone());
        assert_eq!(third.open(&dep, &slot("env:A"), &a).await.unwrap(), b"one");
        assert_eq!(third.open(&dep, &slot("env:B"), &b).await.unwrap(), b"two");
        let wrapped = kv.scan_prefix("valk:", None, None).await.unwrap();
        assert_eq!(wrapped.items.len(), 1);
    }

    #[tokio::test]
    async fn a_read_never_creates_a_data_key() {
        let (_dir, kv) = local_kv().await;
        let cipher = EnvelopeCipher::new(Arc::new(TestKey::default()), kv.clone());
        let sealed = {
            let other = EnvelopeCipher::new(Arc::new(TestKey::default()), local_kv().await.1);
            other
                .seal(&scope("dep:dep_1"), &slot("env:A"), b"x")
                .await
                .unwrap()
        };

        let error = cipher
            .open(&scope("dep:dep_1"), &slot("env:A"), &sealed)
            .await
            .unwrap_err();

        assert_eq!(error.code, "STORED_VALUE_UNREADABLE");
        assert!(kv
            .scan_prefix("valk:", None, None)
            .await
            .unwrap()
            .items
            .is_empty());
    }

    #[tokio::test]
    async fn a_scan_of_one_scope_never_lists_data_keys_or_other_scopes() {
        alien_deployment::set_ephemeral_secrets_digest_key();
        let (_dir, kv) = local_kv().await;
        let (_key, backend) = sealed_backend(kv.clone());
        let store = ValueStore::new(Arc::new(backend));
        for dep in ["dep:x", "dep:x.y", "dep:x-1", "dep:xx"] {
            store
                .put(&scope(dep), &slot("env:A"), ValueKind::Plain, dep.into())
                .await
                .unwrap();
        }
        let listed = store.list(&scope("dep:x")).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].value, "dep:x");
    }
}
