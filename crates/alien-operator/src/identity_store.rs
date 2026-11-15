//! Where the operator keeps its enrolled identity.
//!
//! By default the identity lives in the encrypted local database. When
//! `OPERATOR_IDENTITY_SECRET` names a Kubernetes Secret, the identity and the
//! local database encryption key live in that Secret instead, and the data
//! directory is only a cache that may be an `emptyDir`.
//!
//! Every write to the Secret carries the `resourceVersion` of the last read, so
//! two operators (or a restart racing a rotation) cannot overwrite each other:
//! the loser gets [`ErrorData::IdentityConflict`], re-reads, and follows the
//! saved-pending-key path of the daily identity.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use alien_error::{AlienError, Context};
use alien_k8s_clients::{
    ErrorData as KubernetesErrorData, KubernetesClient, KubernetesClientConfig,
    KubernetesClientConfigExt,
};
use async_trait::async_trait;
use k8s_openapi::{api::core::v1::Secret, ByteString};
use tokio::sync::Mutex;
use tracing::error;

use crate::{
    daily_identity::DailyIdentityState,
    error::{ErrorData, Result},
};

const IDENTITY_KEY: &str = "identity";
const ENCRYPTION_KEY: &str = "encryptionKey";
/// Attempts to settle the local encryption key against other starting operators.
const KEY_ATTEMPTS: usize = 5;

/// The durable home of the enrolled identity, when it is not the database.
#[async_trait]
pub(crate) trait IdentityStore: Send + Sync {
    /// The saved identity, enrolled or not.
    async fn read(&self) -> Result<Option<DailyIdentityState>>;
    /// Save the identity. Fails with [`ErrorData::IdentityConflict`] when
    /// another writer saved since the last read.
    async fn write(&self, state: &DailyIdentityState) -> Result<()>;
    /// Save a confirmed enrollment, refusing to replace another deployment.
    async fn persist_enrolled(&self, state: &DailyIdentityState) -> Result<()>;
    /// Drop a saved key that never finished enrolling; an enrolled one stays.
    async fn clear_unenrolled(&self) -> Result<()>;
    /// Check that an enrolled identity still matches `state`.
    async fn check_enrolled(&self, state: &DailyIdentityState) -> Result<()>;
    /// Why the identity cannot be saved right now, if it cannot. While it is
    /// set the operator keeps its current key and does not rotate.
    fn rotation_blocked(&self) -> Option<String>;
}

/// What a write to the Secret came to.
pub(crate) enum PutOutcome {
    Stored(Secret),
    /// The `resourceVersion` is stale.
    Conflict,
    /// The Secret no longer exists.
    Missing,
}

/// The Kubernetes calls the store needs, so tests run without a cluster.
#[async_trait]
pub(crate) trait SecretBackend: Send + Sync {
    async fn get(&self) -> Result<Option<Secret>>;
    async fn put(&self, secret: &Secret) -> Result<PutOutcome>;
}

pub(crate) struct KubernetesSecret {
    client: KubernetesClient,
    namespace: String,
    name: String,
}

impl KubernetesSecret {
    pub(crate) async fn in_cluster(namespace: &str, name: &str) -> Result<Self> {
        let config = KubernetesClientConfig::try_incluster().await.context(
            ErrorData::ConfigurationError {
                message: "Cannot load in-cluster credentials for the identity Secret".into(),
            },
        )?;
        let client =
            KubernetesClient::new(config)
                .await
                .context(ErrorData::ConfigurationError {
                    message: "Cannot create the Kubernetes client for the identity Secret".into(),
                })?;
        Ok(Self {
            client,
            namespace: namespace.into(),
            name: name.into(),
        })
    }
}

#[async_trait]
impl SecretBackend for KubernetesSecret {
    async fn get(&self) -> Result<Option<Secret>> {
        match self.client.get_secret(&self.namespace, &self.name).await {
            Ok(secret) => Ok(Some(secret)),
            Err(error)
                if matches!(
                    error.error.as_ref(),
                    Some(KubernetesErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(error).context(ErrorData::ConfigurationError {
                message: format!("Cannot read the identity Secret '{}'", self.name),
            }),
        }
    }

    async fn put(&self, secret: &Secret) -> Result<PutOutcome> {
        match self
            .client
            .update_secret(&self.namespace, &self.name, secret)
            .await
        {
            Ok(stored) => Ok(PutOutcome::Stored(stored)),
            Err(error) => match error.error.as_ref() {
                Some(KubernetesErrorData::RemoteResourceConflict { .. }) => {
                    Ok(PutOutcome::Conflict)
                }
                Some(KubernetesErrorData::RemoteResourceNotFound { .. }) => Ok(PutOutcome::Missing),
                _ => Err(error).context(ErrorData::ConfigurationError {
                    message: format!("Cannot write the identity Secret '{}'", self.name),
                }),
            },
        }
    }
}

#[derive(Default)]
struct Inner {
    /// The Secret as last read or written: its `resourceVersion` is what the
    /// next write compares against.
    secret: Option<Secret>,
    /// The last identity seen, served while the Secret is missing or
    /// unreachable so a running operator keeps its key.
    memory: Option<DailyIdentityState>,
    /// The local database key last seen in, or generated for, the Secret.
    key: Option<String>,
}

/// What reading the Secret came to.
enum Loaded {
    Found(Secret),
    /// The Secret does not exist.
    Missing,
    /// The API server could not be asked; the Secret may well exist.
    Unreadable,
}

pub(crate) struct SecretIdentityStore {
    backend: Arc<dyn SecretBackend>,
    name: String,
    inner: Mutex<Inner>,
    blocked: std::sync::Mutex<Option<String>>,
    reported: AtomicBool,
}

impl SecretIdentityStore {
    pub(crate) fn new(backend: Arc<dyn SecretBackend>, name: &str) -> Self {
        Self {
            backend,
            name: name.into(),
            inner: Mutex::new(Inner::default()),
            blocked: std::sync::Mutex::new(None),
            reported: AtomicBool::new(false),
        }
    }

    fn fix(&self, what: &str) -> String {
        format!(
            "{what}. The operator keeps running on its current key and will not rotate it. \
             Restore the Secret '{}' (an empty Secret of that name in the release namespace; `helm upgrade` recreates it) \
             and allow the operator `get`, `update` and `patch` on it.",
            self.name
        )
    }

    /// Record why the Secret cannot be used; the first time, log it once.
    fn block(&self, what: &str) -> AlienError<ErrorData> {
        let message = self.fix(what);
        *self.blocked.lock().expect("blocked flag") = Some(message.clone());
        if !self.reported.swap(true, Ordering::Relaxed) {
            error!("{message}");
        }
        AlienError::new(ErrorData::ConfigurationError { message })
    }

    fn unblock(&self) {
        *self.blocked.lock().expect("blocked flag") = None;
        self.reported.store(false, Ordering::Relaxed);
    }

    /// Read the Secret and remember it. When the Secret exists but holds no
    /// identity (it was recreated empty by `helm upgrade --force`, `kubectl
    /// replace` or a GitOps replace) and an enrolled identity is known in
    /// memory, that identity is written back against the version just read.
    async fn load(&self, inner: &mut Inner) -> Loaded {
        match self.backend.get().await {
            Ok(Some(mut secret)) => {
                self.unblock();
                inner.secret = Some(secret.clone());
                let saved = parse_identity(&secret).ok().flatten();
                if saved.is_none() {
                    if let Some(restored) = self.restore(inner, &secret).await {
                        secret = restored;
                    }
                }
                inner.memory = parse_identity(&secret)
                    .ok()
                    .flatten()
                    .or(inner.memory.take());
                if let Some(key) = secret_text(&secret, ENCRYPTION_KEY).filter(|k| valid_key(k)) {
                    inner.key = Some(key);
                }
                Loaded::Found(secret)
            }
            Ok(None) => {
                inner.secret = None;
                self.block(&format!(
                    "The identity Secret '{}' does not exist",
                    self.name
                ));
                Loaded::Missing
            }
            Err(source) => {
                inner.secret = None;
                self.block(&format!(
                    "The identity Secret '{}' cannot be read ({})",
                    self.name, source
                ));
                Loaded::Unreadable
            }
        }
    }

    /// Write the enrolled identity (and the local key) known in memory back into
    /// a Secret that lost them. A conflict or failure leaves the Secret as read:
    /// the next read tries again.
    async fn restore(&self, inner: &mut Inner, secret: &Secret) -> Option<Secret> {
        let identity = inner
            .memory
            .as_ref()
            .filter(|state| !state.deployment_id.is_empty())?;
        let value = serialize(identity).ok()?;
        let mut restored = secret.clone();
        let mut data = restored.data.take().unwrap_or_default();
        data.insert(IDENTITY_KEY.into(), value);
        if let Some(key) = inner.key.clone() {
            data.entry(ENCRYPTION_KEY.into())
                .or_insert_with(|| ByteString(key.into_bytes()));
        }
        restored.data = Some(data);
        match self.backend.put(&restored).await {
            Ok(PutOutcome::Stored(stored)) => {
                inner.secret = Some(stored.clone());
                Some(stored)
            }
            _ => None,
        }
    }

    /// Replace the Secret's data with `change` applied, against the version
    /// last read.
    async fn update(
        &self,
        inner: &mut Inner,
        change: impl FnOnce(&mut BTreeMap<String, ByteString>),
    ) -> Result<()> {
        let mut secret = match inner.secret.clone() {
            Some(secret) => secret,
            None => match self.load(inner).await {
                Loaded::Found(_) => inner.secret.clone().expect("loaded secret is remembered"),
                Loaded::Missing | Loaded::Unreadable => {
                    return Err(AlienError::new(ErrorData::ConfigurationError {
                        message: self
                            .blocked
                            .lock()
                            .expect("blocked flag")
                            .clone()
                            .unwrap_or_default(),
                    }))
                }
            },
        };
        let mut data = secret.data.take().unwrap_or_default();
        change(&mut data);
        secret.data = Some(data);
        match self.backend.put(&secret).await {
            Ok(PutOutcome::Stored(stored)) => {
                self.unblock();
                inner.secret = Some(stored);
                Ok(())
            }
            Ok(PutOutcome::Conflict) => {
                // Forget the stale version: the next read sees the winner.
                inner.secret = None;
                Err(AlienError::new(ErrorData::IdentityConflict))
            }
            Ok(PutOutcome::Missing) => {
                inner.secret = None;
                Err(self.block(&format!("The identity Secret '{}' was deleted", self.name)))
            }
            Err(source) => {
                inner.secret = None;
                Err(self.block(&format!(
                    "The identity Secret '{}' cannot be written ({})",
                    self.name, source
                )))
            }
        }
    }

    /// The 64-hex key of the local database, generated on first use. Operators
    /// that start together settle on whichever key reached the Secret first.
    /// With no usable Secret, a key kept in memory only: the cache then starts
    /// empty again after a restart.
    pub(crate) async fn encryption_key(&self) -> Result<String> {
        let mut inner = self.inner.lock().await;
        for _ in 0..KEY_ATTEMPTS {
            let secret = match self.load(&mut inner).await {
                Loaded::Found(secret) => secret,
                // The Secret is gone: keep the key in use, or use one that lives in
                // memory only.
                Loaded::Missing => return self.memory_key(&mut inner),
                // The API server could not be asked. A key already in use stays; with
                // none, a throwaway key would orphan the cache and the caller must
                // retry instead.
                Loaded::Unreadable => {
                    return match inner.key.clone() {
                        Some(key) => Ok(key),
                        None => Err(AlienError::new(ErrorData::ConfigurationError {
                            message: format!(
                                "The identity Secret '{}' cannot be read yet; retrying",
                                self.name
                            ),
                        })),
                    }
                }
            };
            if let Some(key) = secret_text(&secret, ENCRYPTION_KEY).filter(|k| valid_key(k)) {
                return Ok(key);
            }
            let key = random_hex_key()?;
            let stored = key.clone();
            match self
                .update(&mut inner, |data| {
                    data.insert(ENCRYPTION_KEY.into(), ByteString(stored.into_bytes()));
                })
                .await
            {
                Ok(()) => {
                    inner.key = Some(key.clone());
                    return Ok(key);
                }
                Err(error) if matches!(error.error, Some(ErrorData::IdentityConflict)) => continue,
                Err(_) => return self.memory_key(&mut inner),
            }
        }
        Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "The identity Secret '{}' kept changing while the local key was set",
                self.name
            ),
        }))
    }

    fn memory_key(&self, inner: &mut Inner) -> Result<String> {
        if let Some(key) = inner.key.clone() {
            return Ok(key);
        }
        let key = random_hex_key()?;
        inner.key = Some(key.clone());
        Ok(key)
    }
}

fn valid_key(key: &str) -> bool {
    key.len() == 64 && key.chars().all(|c| c.is_ascii_hexdigit())
}

fn random_hex_key() -> Result<String> {
    let mut random = [0u8; 32];
    getrandom::fill(&mut random).map_err(|_| {
        AlienError::new(ErrorData::EncryptionError {
            message: "Cannot generate the local database key".into(),
        })
    })?;
    Ok(random.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn secret_text(secret: &Secret, key: &str) -> Option<String> {
    let bytes = secret.data.as_ref()?.get(key)?;
    String::from_utf8(bytes.0.clone())
        .ok()
        .filter(|text| !text.is_empty())
}

fn parse_identity(secret: &Secret) -> Result<Option<DailyIdentityState>> {
    match secret_text(secret, IDENTITY_KEY) {
        None => Ok(None),
        Some(text) => serde_json::from_str(&text).map(Some).map_err(|_| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "The saved operator identity in the Secret is not readable".into(),
            })
        }),
    }
}

fn serialize(state: &DailyIdentityState) -> Result<ByteString> {
    serde_json::to_vec(state).map(ByteString).map_err(|_| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "Cannot serialize the operator identity".into(),
        })
    })
}

#[async_trait]
impl IdentityStore for SecretIdentityStore {
    async fn read(&self) -> Result<Option<DailyIdentityState>> {
        let mut inner = self.inner.lock().await;
        match self.load(&mut inner).await {
            Loaded::Found(secret) => parse_identity(&secret),
            // Deleted: keep serving the key already in use.
            Loaded::Missing => Ok(inner.memory.clone()),
            // Unreachable: with no identity in memory, "no saved identity" would be a
            // guess that lets enrollment build a new one over an enrolled identity.
            Loaded::Unreadable => match inner.memory.clone() {
                Some(state) => Ok(Some(state)),
                None => Err(AlienError::new(ErrorData::ConfigurationError {
                    message: format!(
                        "The identity Secret '{}' cannot be read yet; retrying",
                        self.name
                    ),
                })),
            },
        }
    }

    async fn write(&self, state: &DailyIdentityState) -> Result<()> {
        let value = serialize(state)?;
        let mut inner = self.inner.lock().await;
        if inner.secret.is_none() {
            self.load(&mut inner).await;
        }
        // An enrolled identity is never replaced by one that is not enrolled.
        let enrolled_saved = inner
            .secret
            .as_ref()
            .and_then(|secret| parse_identity(secret).ok().flatten())
            .or_else(|| inner.memory.clone())
            .is_some_and(|saved| !saved.deployment_id.is_empty());
        if state.deployment_id.is_empty() && enrolled_saved {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: "Refusing to replace the enrolled operator identity with one that is not enrolled".into(),
            }));
        }
        self.update(&mut inner, |data| {
            data.insert(IDENTITY_KEY.into(), value);
        })
        .await?;
        inner.memory = Some(state.clone());
        Ok(())
    }

    async fn persist_enrolled(&self, state: &DailyIdentityState) -> Result<()> {
        if state.deployment_id.is_empty() {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: "Enrolled identity requires a deployment ID".into(),
            }));
        }
        let mut inner = self.inner.lock().await;
        let secret = match self.load(&mut inner).await {
            Loaded::Found(secret) => Some(secret),
            Loaded::Missing => None,
            Loaded::Unreadable => {
                return Err(AlienError::new(ErrorData::ConfigurationError {
                    message: format!("The identity Secret '{}' cannot be read", self.name),
                }))
            }
        };
        if let Some(saved) = secret.as_ref().map(parse_identity).transpose()?.flatten() {
            if !saved.deployment_id.is_empty() && saved.deployment_id != state.deployment_id {
                return Err(AlienError::new(ErrorData::ConfigurationError {
                    message: "Saved operator belongs to another deployment".into(),
                }));
            }
        }
        let value = serialize(state)?;
        self.update(&mut inner, |data| {
            data.insert(IDENTITY_KEY.into(), value);
        })
        .await?;
        inner.memory = Some(state.clone());
        Ok(())
    }

    async fn clear_unenrolled(&self) -> Result<()> {
        let mut inner = self.inner.lock().await;
        let secret = match self.load(&mut inner).await {
            Loaded::Found(secret) => secret,
            Loaded::Missing | Loaded::Unreadable => {
                inner.memory = inner
                    .memory
                    .take()
                    .filter(|state| !state.deployment_id.is_empty());
                return Ok(());
            }
        };
        if parse_identity(&secret)?.is_some_and(|saved| saved.deployment_id.is_empty()) {
            self.update(&mut inner, |data| {
                data.remove(IDENTITY_KEY);
            })
            .await?;
            inner.memory = None;
        }
        Ok(())
    }

    async fn check_enrolled(&self, state: &DailyIdentityState) -> Result<()> {
        let saved = self.read().await?;
        match saved {
            Some(saved)
                if saved.deployment_id == state.deployment_id
                    && saved.manager_url == state.manager_url
                    && saved.audience == state.audience =>
            {
                Ok(())
            }
            _ => Err(AlienError::new(ErrorData::ConfigurationError {
                message: "Saved enrollment no longer matches the operator identity".into(),
            })),
        }
    }

    fn rotation_blocked(&self) -> Option<String> {
        self.blocked.lock().expect("blocked flag").clone()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU32;

    use chrono::Utc;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    use super::*;

    /// An API server stand-in with `resourceVersion` compare-and-swap.
    struct Fake {
        secret: std::sync::Mutex<Option<Secret>>,
        version: AtomicU32,
        refuse_writes: AtomicBool,
        refuse_reads: AtomicBool,
    }

    impl Fake {
        fn with_empty_secret() -> Arc<Self> {
            Arc::new(Self {
                secret: std::sync::Mutex::new(Some(Secret {
                    metadata: ObjectMeta {
                        name: Some("identity".into()),
                        resource_version: Some("1".into()),
                        labels: Some([("app".to_string(), "operator".to_string())].into()),
                        ..Default::default()
                    },
                    ..Default::default()
                })),
                version: AtomicU32::new(1),
                refuse_writes: AtomicBool::new(false),
                refuse_reads: AtomicBool::new(false),
            })
        }
        /// What `helm upgrade --force`, `kubectl replace` and GitOps replace do.
        fn recreate_empty(&self) {
            let version = self.version.fetch_add(1, Ordering::Relaxed) + 1;
            *self.secret.lock().unwrap() = Some(Secret {
                metadata: ObjectMeta {
                    name: Some("identity".into()),
                    resource_version: Some(version.to_string()),
                    ..Default::default()
                },
                ..Default::default()
            });
        }
        fn delete(&self) {
            *self.secret.lock().unwrap() = None;
        }
        fn stored(&self) -> Secret {
            self.secret.lock().unwrap().clone().unwrap()
        }
    }

    #[async_trait]
    impl SecretBackend for Fake {
        async fn get(&self) -> Result<Option<Secret>> {
            if self.refuse_reads.load(Ordering::Relaxed) {
                return Err(AlienError::new(ErrorData::ConfigurationError {
                    message: "connection refused".into(),
                }));
            }
            Ok(self.secret.lock().unwrap().clone())
        }
        async fn put(&self, secret: &Secret) -> Result<PutOutcome> {
            if self.refuse_writes.load(Ordering::Relaxed) {
                return Err(AlienError::new(ErrorData::ConfigurationError {
                    message: "forbidden".into(),
                }));
            }
            let mut current = self.secret.lock().unwrap();
            let Some(existing) = current.as_ref() else {
                return Ok(PutOutcome::Missing);
            };
            if existing.metadata.resource_version != secret.metadata.resource_version {
                return Ok(PutOutcome::Conflict);
            }
            let mut stored = secret.clone();
            stored.metadata.resource_version =
                Some((self.version.fetch_add(1, Ordering::Relaxed) + 1).to_string());
            *current = Some(stored.clone());
            Ok(PutOutcome::Stored(stored))
        }
    }

    fn state(deployment: &str, current: &str) -> DailyIdentityState {
        DailyIdentityState {
            manager_url: "https://manager.example".into(),
            audience: "aud".into(),
            deployment_id: deployment.into(),
            current: current.into(),
            pending: None,
            pending_started_at: None,
            expires_at: Utc::now() + chrono::Duration::hours(20),
            enrolled_code_hash: None,
            pending_enrollment: None,
            previous: None,
        }
    }

    fn store(fake: &Arc<Fake>) -> SecretIdentityStore {
        SecretIdentityStore::new(fake.clone(), "identity")
    }

    #[tokio::test]
    async fn identity_round_trips_and_keeps_the_secret_labels() {
        let fake = Fake::with_empty_secret();
        let store = store(&fake);
        assert!(store.read().await.unwrap().is_none());
        store
            .persist_enrolled(&state("dep_1", "key-a"))
            .await
            .unwrap();
        let read = store.read().await.unwrap().unwrap();
        assert_eq!(
            (read.deployment_id.as_str(), read.current.as_str()),
            ("dep_1", "key-a")
        );
        assert_eq!(
            fake.stored()
                .metadata
                .labels
                .unwrap()
                .get("app")
                .map(String::as_str),
            Some("operator")
        );
    }

    #[tokio::test]
    async fn a_write_against_a_stale_version_is_a_conflict_and_changes_nothing() {
        let fake = Fake::with_empty_secret();
        let (a, b) = (store(&fake), store(&fake));
        a.persist_enrolled(&state("dep_1", "key-a")).await.unwrap();
        b.read().await.unwrap();
        a.write(&state("dep_1", "key-from-a")).await.unwrap();
        let error = b.write(&state("dep_1", "key-from-b")).await.unwrap_err();
        assert!(matches!(error.error, Some(ErrorData::IdentityConflict)));
        let saved = b.read().await.unwrap().unwrap();
        assert_eq!(saved.current, "key-from-a");
        // After re-reading, the same writer succeeds.
        b.write(&state("dep_1", "key-from-b")).await.unwrap();
    }

    #[tokio::test]
    async fn two_operators_racing_to_set_the_local_key_settle_on_one() {
        let fake = Fake::with_empty_secret();
        let (a, b) = (store(&fake), store(&fake));
        let (key_a, key_b) = tokio::join!(a.encryption_key(), b.encryption_key());
        let stored = secret_text(&fake.stored(), ENCRYPTION_KEY).unwrap();
        assert_eq!(key_a.unwrap(), stored);
        assert_eq!(key_b.unwrap(), stored);
    }

    #[tokio::test]
    async fn a_pending_key_saved_before_a_crash_is_found_after_restart() {
        let fake = Fake::with_empty_secret();
        let first = store(&fake);
        first
            .persist_enrolled(&state("dep_1", "key-a"))
            .await
            .unwrap();
        let mut rotating = state("dep_1", "key-a");
        rotating.pending = Some("key-b".into());
        rotating.pending_started_at = Some(Utc::now());
        first.write(&rotating).await.unwrap();
        drop(first);
        let restarted = store(&fake).read().await.unwrap().unwrap();
        assert_eq!(restarted.pending.as_deref(), Some("key-b"));
        assert_eq!(restarted.current, "key-a");
    }

    #[tokio::test]
    async fn a_deleted_secret_keeps_the_running_key_and_blocks_rotation() {
        let fake = Fake::with_empty_secret();
        let store = store(&fake);
        store
            .persist_enrolled(&state("dep_1", "key-a"))
            .await
            .unwrap();
        assert!(store.rotation_blocked().is_none());
        fake.delete();
        let kept = store.read().await.unwrap().unwrap();
        assert_eq!(kept.current, "key-a");
        let reason = store.rotation_blocked().unwrap();
        assert!(reason.contains("does not exist") && reason.contains("helm upgrade"));
        assert!(store.write(&state("dep_1", "key-b")).await.is_err());
        // The key for the local database still works, from memory.
        assert_eq!(store.encryption_key().await.unwrap().len(), 64);
    }

    #[tokio::test]
    async fn a_secret_that_cannot_be_written_blocks_rotation_until_it_can() {
        let fake = Fake::with_empty_secret();
        let store = store(&fake);
        store
            .persist_enrolled(&state("dep_1", "key-a"))
            .await
            .unwrap();
        fake.refuse_writes.store(true, Ordering::Relaxed);
        assert!(store.write(&state("dep_1", "key-b")).await.is_err());
        assert!(store.rotation_blocked().is_some());
        fake.refuse_writes.store(false, Ordering::Relaxed);
        store.read().await.unwrap();
        assert!(store.rotation_blocked().is_none());
        store.write(&state("dep_1", "key-b")).await.unwrap();
    }

    #[tokio::test]
    async fn enrolling_over_another_deployment_is_refused() {
        let fake = Fake::with_empty_secret();
        let store = store(&fake);
        store
            .persist_enrolled(&state("dep_1", "key-a"))
            .await
            .unwrap();
        let error = store
            .persist_enrolled(&state("dep_2", "key-x"))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("another deployment"));
    }

    #[tokio::test]
    async fn clearing_removes_only_an_unenrolled_key() {
        let fake = Fake::with_empty_secret();
        let store = store(&fake);
        store.write(&state("", "candidate")).await.unwrap();
        store.clear_unenrolled().await.unwrap();
        assert!(store.read().await.unwrap().is_none());
        store
            .persist_enrolled(&state("dep_1", "key-a"))
            .await
            .unwrap();
        store.clear_unenrolled().await.unwrap();
        assert!(store.read().await.unwrap().is_some());
    }

    #[tokio::test]
    async fn an_unreachable_secret_is_not_read_as_no_identity() {
        let fake = Fake::with_empty_secret();
        store(&fake)
            .persist_enrolled(&state("dep_1", "key-a"))
            .await
            .unwrap();
        let before = fake.stored();
        let restarted = store(&fake);
        fake.refuse_reads.store(true, Ordering::Relaxed);
        assert!(restarted.read().await.is_err());
        assert!(restarted.encryption_key().await.is_err());
        // Nothing may be written while the Secret cannot be read.
        assert!(restarted.write(&state("", "candidate")).await.is_err());
        fake.refuse_reads.store(false, Ordering::Relaxed);
        assert_eq!(fake.stored().data, before.data);
        assert_eq!(restarted.read().await.unwrap().unwrap().current, "key-a");
    }

    #[tokio::test]
    async fn an_enrolled_identity_is_never_replaced_by_an_unenrolled_one() {
        let fake = Fake::with_empty_secret();
        store(&fake)
            .persist_enrolled(&state("dep_1", "key-a"))
            .await
            .unwrap();
        // A restarted operator has nothing in memory and has not read yet.
        let restarted = store(&fake);
        let error = restarted.write(&state("", "candidate")).await.unwrap_err();
        assert!(error.to_string().contains("enrolled"));
        assert_eq!(
            store(&fake).read().await.unwrap().unwrap().deployment_id,
            "dep_1"
        );
    }

    #[tokio::test]
    async fn a_running_operator_restores_its_identity_into_a_recreated_secret() {
        let fake = Fake::with_empty_secret();
        let store = store(&fake);
        let key = store.encryption_key().await.unwrap();
        store
            .persist_enrolled(&state("dep_1", "key-a"))
            .await
            .unwrap();
        fake.delete();
        assert_eq!(store.read().await.unwrap().unwrap().current, "key-a");
        assert!(store.rotation_blocked().is_some());
        // The fix the error message names, or a forced upgrade, recreates it empty.
        fake.recreate_empty();
        assert_eq!(store.read().await.unwrap().unwrap().current, "key-a");
        assert!(store.rotation_blocked().is_none());
        // The Secret holds the identity and the local key again, so a restart finds both.
        let restarted = self::store(&fake);
        assert_eq!(restarted.read().await.unwrap().unwrap().current, "key-a");
        assert_eq!(restarted.encryption_key().await.unwrap(), key);
        store.write(&state("dep_1", "key-b")).await.unwrap();
    }
}
