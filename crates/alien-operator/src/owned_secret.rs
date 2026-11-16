//! The Secret the Operator owns.
//!
//! A chart that generates a value while it renders cannot be rendered twice (Argo CD
//! renders offline on every sync, so each render would make new values). Instead the
//! chart names one Secret, `OPERATOR_OWNED_SECRET`, and the keys it needs in it
//! (`OPERATOR_OWNED_SECRET_KEYS`, `key=kind` pairs). The Operator creates what is
//! missing when it starts and never changes what is there, so a restart, or two
//! Operators starting at once, never rotates a value.
//!
//! A value that is missing is taken from the migration Secret (`OPERATOR_MIGRATION_SECRET`)
//! when an earlier chart installed it: that is how an upgrade keeps the key that encrypts
//! the Operator's state. Only a fresh install generates an encryption key. An Operator that
//! already has state and finds no key refuses to start rather than invent one.
//!
//! Remove the migration Secret handling one release after migration is reported.

use std::{collections::BTreeMap, path::Path};

use alien_error::AlienError;
use alien_k8s_clients::{ErrorData as KubernetesErrorData, SecretsApi};
use k8s_openapi::{api::core::v1::Secret, apimachinery::pkg::apis::meta::v1::ObjectMeta, ByteString};
use tracing::info;

use crate::error::{ErrorData, Result};

/// How many times a lost race is retried. Each retry re-reads the Secret, so a value
/// another Operator wrote in the meantime is kept.
const ATTEMPTS: usize = 8;

/// What a value looks like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ValueKind {
    /// 64 hex characters: the key that encrypts the Operator's state.
    Hex32,
    /// 48 letters and digits.
    Token48,
}

/// The Secret and the keys the chart asked for.
#[derive(Clone, Debug)]
pub(crate) struct OwnedSecret {
    pub name: String,
    pub keys: Vec<(String, ValueKind)>,
    pub migration: Option<String>,
}

impl OwnedSecret {
    /// Parse `key=kind,key=kind`.
    pub fn parse(name: &str, keys: &str, migration: Option<&str>) -> Result<Self> {
        let mut parsed = Vec::new();
        for entry in keys.split(',').filter(|entry| !entry.is_empty()) {
            let (key, kind) = entry.split_once('=').ok_or_else(|| invalid(entry))?;
            let kind = match kind {
                "hex32" => ValueKind::Hex32,
                "token48" => ValueKind::Token48,
                _ => return Err(invalid(entry)),
            };
            if key.is_empty() {
                return Err(invalid(entry));
            }
            parsed.push((key.to_string(), kind));
        }
        Ok(Self {
            name: name.to_string(),
            keys: parsed,
            migration: migration.filter(|name| !name.is_empty()).map(str::to_string),
        })
    }
}

fn invalid(entry: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ConfigurationError {
        message: format!("OPERATOR_OWNED_SECRET_KEYS has an invalid entry '{entry}'"),
    })
}

fn failed(message: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ConfigurationError {
        message: message.into(),
    })
}

/// What `ensure` did.
#[derive(Debug)]
pub(crate) struct Ensured {
    /// Every requested value, decoded.
    pub values: BTreeMap<String, String>,
    /// Keys copied from the migration Secret.
    pub migrated: Vec<String>,
    /// Keys the Operator generated.
    pub generated: Vec<String>,
}

enum Outcome {
    Done(Ensured),
    /// Another writer got there first; read again.
    Raced,
}

/// Make sure the owned Secret holds every requested key, creating only what is missing.
///
/// `has_state` is whether the Operator's state directory already holds a database. Such an
/// Operator never gets a new encryption key.
pub(crate) async fn ensure(
    secrets: &dyn SecretsApi,
    namespace: &str,
    owned: &OwnedSecret,
    has_state: bool,
) -> Result<Ensured> {
    for _ in 0..ATTEMPTS {
        match attempt(secrets, namespace, owned, has_state).await? {
            Outcome::Done(ensured) => return Ok(ensured),
            Outcome::Raced => continue,
        }
    }
    Err(failed(format!(
        "Secret '{}' kept changing while the Operator filled it in",
        owned.name
    )))
}

async fn attempt(
    secrets: &dyn SecretsApi,
    namespace: &str,
    owned: &OwnedSecret,
    has_state: bool,
) -> Result<Outcome> {
    let existing = read(secrets, namespace, &owned.name).await?;
    let mut data: BTreeMap<String, ByteString> = existing
        .as_ref()
        .and_then(|secret| secret.data.clone())
        .unwrap_or_default();
    let missing: Vec<&(String, ValueKind)> = owned
        .keys
        .iter()
        .filter(|(key, _)| data.get(key).is_none_or(|value| value.0.is_empty()))
        .collect();
    if missing.is_empty() {
        return Ok(Outcome::Done(Ensured {
            values: decode(&data, owned)?,
            migrated: Vec::new(),
            generated: Vec::new(),
        }));
    }

    let carried = match &owned.migration {
        Some(name) => read(secrets, namespace, name)
            .await?
            .and_then(|secret| secret.data)
            .unwrap_or_default(),
        None => BTreeMap::new(),
    };
    let mut migrated = Vec::new();
    let mut generated = Vec::new();
    for (key, kind) in missing {
        if let Some(value) = carried.get(key).filter(|value| !value.0.is_empty()) {
            check(key, *kind, &value.0)?;
            data.insert(key.clone(), value.clone());
            migrated.push(key.clone());
            continue;
        }
        if *kind == ValueKind::Hex32 && has_state {
            return Err(failed(format!(
                "The Operator has saved state but no encryption key: '{key}' is not in Secret '{}'{}. \
                 Restore the original key into that Secret. A new key would make the saved state unreadable.",
                owned.name,
                owned
                    .migration
                    .as_deref()
                    .map(|name| format!(" or in Secret '{name}'"))
                    .unwrap_or_default(),
            )));
        }
        data.insert(key.clone(), ByteString(generate(*kind)?.into_bytes()));
        generated.push(key.clone());
    }

    let written = match existing {
        // Create-only: a Secret made in the meantime answers 409 and is read again.
        None => {
            let secret = Secret {
                metadata: ObjectMeta {
                    name: Some(owned.name.clone()),
                    namespace: Some(namespace.to_string()),
                    labels: Some(BTreeMap::from([(
                        "app.kubernetes.io/managed-by".to_string(),
                        "alien-operator".to_string(),
                    )])),
                    ..Default::default()
                },
                type_: Some("Opaque".to_string()),
                data: Some(data.clone()),
                ..Default::default()
            };
            secrets.create_secret(namespace, &secret).await
        }
        // Adding a key to a Secret that exists: the update carries the resource version
        // we read, so a concurrent change is refused instead of overwritten.
        Some(mut secret) => {
            secret.data = Some(data.clone());
            secrets
                .update_secret(namespace, &owned.name, &secret)
                .await
        }
    };
    match written {
        Ok(_) => Ok(Outcome::Done(Ensured {
            values: decode(&data, owned)?,
            migrated,
            generated,
        })),
        Err(error)
            if matches!(
                error.error.as_ref(),
                Some(KubernetesErrorData::RemoteResourceConflict { .. })
            ) =>
        {
            Ok(Outcome::Raced)
        }
        Err(error) => Err(failed(format!(
            "writing Secret '{}': {error}",
            owned.name
        ))),
    }
}

async fn read(secrets: &dyn SecretsApi, namespace: &str, name: &str) -> Result<Option<Secret>> {
    match secrets.get_secret(namespace, name).await {
        Ok(secret) => Ok(Some(secret)),
        Err(error)
            if matches!(
                error.error.as_ref(),
                Some(KubernetesErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(failed(format!("reading Secret '{name}': {error}"))),
    }
}

fn decode(data: &BTreeMap<String, ByteString>, owned: &OwnedSecret) -> Result<BTreeMap<String, String>> {
    owned
        .keys
        .iter()
        .map(|(key, kind)| {
            let bytes = &data[key].0;
            check(key, *kind, bytes)?;
            let value = String::from_utf8(bytes.clone())
                .map_err(|_| failed(format!("'{key}' in Secret '{}' is not text", owned.name)))?;
            Ok((key.clone(), value))
        })
        .collect()
}

/// A stored value of the wrong shape is never used: an encryption key that is not 64 hex
/// characters would not open the state, and replacing it would lose the state.
fn check(key: &str, kind: ValueKind, value: &[u8]) -> Result<()> {
    let valid = match kind {
        ValueKind::Hex32 => value.len() == 64 && value.iter().all(u8::is_ascii_hexdigit),
        ValueKind::Token48 => !value.is_empty(),
    };
    if valid {
        Ok(())
    } else {
        Err(failed(format!(
            "'{key}' holds a value of the wrong form. Restore the original value; it is not replaced."
        )))
    }
}

fn generate(kind: ValueKind) -> Result<String> {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let random_error = |_| failed("Cannot generate a random value");
    match kind {
        ValueKind::Hex32 => {
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes).map_err(random_error)?;
            Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
        }
        ValueKind::Token48 => {
            // Reject bytes past the last full multiple of the alphabet so every letter is as
            // likely as any other.
            let limit = 256 - 256 % ALPHABET.len();
            let mut token = String::with_capacity(48);
            while token.len() < 48 {
                let mut bytes = [0u8; 64];
                getrandom::fill(&mut bytes).map_err(random_error)?;
                for byte in bytes.into_iter().filter(|byte| (*byte as usize) < limit) {
                    if token.len() < 48 {
                        token.push(ALPHABET[byte as usize % ALPHABET.len()] as char);
                    }
                }
            }
            Ok(token)
        }
    }
}

/// Whether the data directory already holds the Operator's database.
pub(crate) fn has_state(data_dir: &Path) -> bool {
    data_dir.join("operator.db").exists()
}

/// Log what `ensure` did. Values are never logged.
pub(crate) fn report(owned: &OwnedSecret, ensured: &Ensured) {
    if !ensured.migrated.is_empty() {
        info!(
            secret = %owned.name,
            migration = owned.migration.as_deref().unwrap_or_default(),
            keys = ?ensured.migrated,
            "Migrated values from an earlier chart into the Operator's Secret"
        );
    }
    if !ensured.generated.is_empty() {
        info!(secret = %owned.name, keys = ?ensured.generated, "Created values in the Operator's Secret");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use k8s_openapi::List;
    use async_trait::async_trait;

    use super::*;

    type ClientResult<T> = std::result::Result<T, AlienError<KubernetesErrorData>>;

    const NS: &str = "app";
    const OLD_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// An API server's Secret store: create answers 409 for a name that exists, and an
    /// update carrying a stale resource version is refused with 409.
    type Interference = Box<dyn FnOnce(&mut BTreeMap<String, Secret>) + Send>;

    #[derive(Default)]
    struct Cluster {
        secrets: Mutex<BTreeMap<String, Secret>>,
        /// Run once, between a read and the write that follows it: another Operator starting.
        interfere: Mutex<Option<Interference>>,
        writes: Mutex<Vec<String>>,
    }

    impl std::fmt::Debug for Cluster {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Cluster")
        }
    }

    impl Cluster {
        fn put(&self, name: &str, data: &[(&str, &str)]) {
            let mut secrets = self.secrets.lock().unwrap();
            put(&mut secrets, name, data);
        }
        fn read(&self, name: &str, key: &str) -> Option<String> {
            let secrets = self.secrets.lock().unwrap();
            let value = secrets.get(name)?.data.as_ref()?.get(key)?;
            Some(String::from_utf8(value.0.clone()).unwrap())
        }
    }

    fn put(secrets: &mut BTreeMap<String, Secret>, name: &str, data: &[(&str, &str)]) {
        let version = secrets
            .get(name)
            .and_then(|secret| secret.metadata.resource_version.as_deref())
            .and_then(|version| version.parse::<u32>().ok())
            .unwrap_or(0)
            + 1;
        secrets.insert(
            name.to_string(),
            Secret {
                metadata: ObjectMeta {
                    name: Some(name.to_string()),
                    resource_version: Some(version.to_string()),
                    ..Default::default()
                },
                data: Some(
                    data.iter()
                        .map(|(key, value)| (key.to_string(), ByteString(value.as_bytes().to_vec())))
                        .collect(),
                ),
                ..Default::default()
            },
        );
    }

    fn not_found(name: &str) -> AlienError<KubernetesErrorData> {
        AlienError::new(KubernetesErrorData::RemoteResourceNotFound {
            resource_type: "Secret".into(),
            resource_name: name.into(),
        })
    }

    fn conflict(name: &str) -> AlienError<KubernetesErrorData> {
        AlienError::new(KubernetesErrorData::RemoteResourceConflict {
            resource_type: "Secret".into(),
            resource_name: name.into(),
            message: "the object has been modified".into(),
        })
    }

    #[async_trait]
    impl SecretsApi for Cluster {
        async fn create_secret(&self, _: &str, secret: &Secret) -> ClientResult<Secret> {
            let name = secret.metadata.name.clone().unwrap();
            if let Some(interfere) = self.interfere.lock().unwrap().take() {
                interfere(&mut self.secrets.lock().unwrap());
            }
            let mut secrets = self.secrets.lock().unwrap();
            if secrets.contains_key(&name) {
                return Err(conflict(&name));
            }
            self.writes.lock().unwrap().push(format!("create {name}"));
            let mut stored = secret.clone();
            stored.metadata.resource_version = Some("1".into());
            secrets.insert(name, stored.clone());
            Ok(stored)
        }
        async fn get_secret(&self, _: &str, name: &str) -> ClientResult<Secret> {
            self.secrets
                .lock()
                .unwrap()
                .get(name)
                .cloned()
                .ok_or_else(|| not_found(name))
        }
        async fn get_secret_metadata(&self, _: &str, name: &str) -> ClientResult<ObjectMeta> {
            Err(not_found(name))
        }
        async fn list_secrets(
            &self,
            _: &str,
            _: Option<String>,
            _: Option<String>,
        ) -> ClientResult<List<Secret>> {
            unimplemented!()
        }
        async fn update_secret(&self, _: &str, name: &str, secret: &Secret) -> ClientResult<Secret> {
            if let Some(interfere) = self.interfere.lock().unwrap().take() {
                interfere(&mut self.secrets.lock().unwrap());
            }
            let mut secrets = self.secrets.lock().unwrap();
            let current = secrets.get(name).ok_or_else(|| not_found(name))?;
            if current.metadata.resource_version != secret.metadata.resource_version {
                return Err(conflict(name));
            }
            self.writes.lock().unwrap().push(format!("update {name}"));
            let mut stored = secret.clone();
            let version: u32 = current.metadata.resource_version.as_ref().unwrap().parse().unwrap();
            stored.metadata.resource_version = Some((version + 1).to_string());
            secrets.insert(name.to_string(), stored.clone());
            Ok(stored)
        }
        async fn delete_secret(&self, _: &str, _: &str) -> ClientResult<()> {
            unimplemented!()
        }
        async fn delete_secret_if_uid(&self, _: &str, _: &str, _: &str) -> ClientResult<()> {
            unimplemented!()
        }
    }

    fn owned(migration: bool) -> OwnedSecret {
        OwnedSecret::parse(
            "app-operator",
            "encryption-key=hex32,collector-token=token48,postgres-main-password=token48",
            migration.then_some("app-migration"),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn a_fresh_install_creates_every_value_once() {
        let cluster = Cluster::default();
        let first = ensure(&cluster, NS, &owned(true), false).await.unwrap();
        assert_eq!(first.generated.len(), 3);
        assert!(first.migrated.is_empty());
        assert_eq!(first.values["encryption-key"].len(), 64);
        assert!(first.values["encryption-key"].chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(first.values["collector-token"].len(), 48);
        assert!(first.values["collector-token"].chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(first.values["collector-token"], first.values["postgres-main-password"]);

        // A restart reads the same values and writes nothing.
        let second = ensure(&cluster, NS, &owned(true), true).await.unwrap();
        assert_eq!(first.values, second.values);
        assert!(second.generated.is_empty());
        assert_eq!(*cluster.writes.lock().unwrap(), ["create app-operator"]);
    }

    #[tokio::test]
    async fn create_only_never_overwrites_a_value_that_is_there() {
        let cluster = Cluster::default();
        cluster.put("app-operator", &[("encryption-key", OLD_KEY), ("collector-token", "kept-token")]);
        // The migration Secret holds other values for keys the Operator already has.
        cluster.put(
            "app-migration",
            &[
                ("encryption-key", &"f".repeat(64)),
                ("collector-token", "other-token"),
                ("postgres-main-password", "migrated-password"),
            ],
        );
        let ensured = ensure(&cluster, NS, &owned(true), true).await.unwrap();
        // Only the missing key is added: from the migration Secret, not generated.
        assert_eq!(ensured.migrated, ["postgres-main-password"]);
        assert!(ensured.generated.is_empty());
        assert_eq!(cluster.read("app-operator", "encryption-key").as_deref(), Some(OLD_KEY));
        assert_eq!(cluster.read("app-operator", "collector-token").as_deref(), Some("kept-token"));
        assert_eq!(
            cluster.read("app-operator", "postgres-main-password").as_deref(),
            Some("migrated-password")
        );
    }

    #[tokio::test]
    async fn two_operators_starting_at_once_end_with_one_set_of_values() {
        let cluster = Cluster::default();
        // Another Operator creates the Secret between our read and our create.
        *cluster.interfere.lock().unwrap() = Some(Box::new(|secrets| {
            put(
                secrets,
                "app-operator",
                &[
                    ("encryption-key", OLD_KEY),
                    ("collector-token", "winner-token"),
                    ("postgres-main-password", "winner-password"),
                ],
            );
        }));
        let ensured = ensure(&cluster, NS, &owned(false), false).await.unwrap();
        assert_eq!(ensured.values["encryption-key"], OLD_KEY);
        assert_eq!(ensured.values["collector-token"], "winner-token");
        assert!(ensured.generated.is_empty(), "the loser keeps the winner's values");
        assert!(cluster.writes.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_update_that_races_is_read_again_and_keeps_the_other_writer_s_values() {
        let cluster = Cluster::default();
        cluster.put("app-operator", &[("encryption-key", OLD_KEY)]);
        *cluster.interfere.lock().unwrap() = Some(Box::new(|secrets| {
            put(
                secrets,
                "app-operator",
                &[("encryption-key", OLD_KEY), ("collector-token", "winner-token")],
            );
        }));
        let ensured = ensure(&cluster, NS, &owned(false), true).await.unwrap();
        assert_eq!(ensured.values["collector-token"], "winner-token");
        assert_eq!(ensured.generated, ["postgres-main-password"]);
    }

    #[tokio::test]
    async fn an_upgrade_keeps_the_key_an_earlier_chart_installed() {
        let cluster = Cluster::default();
        cluster.put(
            "app-migration",
            &[("encryption-key", OLD_KEY), ("collector-token", "old-token")],
        );
        let ensured = ensure(&cluster, NS, &owned(true), true).await.unwrap();
        assert_eq!(ensured.values["encryption-key"], OLD_KEY);
        assert_eq!(ensured.values["collector-token"], "old-token");
        assert_eq!(ensured.migrated, ["encryption-key", "collector-token"]);
        // Only what the earlier chart never had is generated.
        assert_eq!(ensured.generated, ["postgres-main-password"]);
        assert_eq!(cluster.read("app-operator", "encryption-key").as_deref(), Some(OLD_KEY));
    }

    #[tokio::test]
    async fn saved_state_without_its_key_is_refused_and_nothing_is_generated() {
        let cluster = Cluster::default();
        let error = ensure(&cluster, NS, &owned(true), true).await.unwrap_err();
        assert!(error.to_string().contains("saved state but no encryption key"), "{error}");
        assert!(cluster.secrets.lock().unwrap().is_empty(), "nothing was written");
    }

    #[tokio::test]
    async fn a_stored_key_of_the_wrong_form_is_never_replaced() {
        let cluster = Cluster::default();
        cluster.put("app-migration", &[("encryption-key", "short")]);
        let error = ensure(&cluster, NS, &owned(true), false).await.unwrap_err();
        assert!(error.to_string().contains("wrong form"), "{error}");
        assert!(cluster.secrets.lock().unwrap().get("app-operator").is_none());
    }

    #[test]
    fn keys_are_parsed_and_malformed_entries_are_refused() {
        assert!(OwnedSecret::parse("s", "encryption-key=hex32,x=token48", None).is_ok());
        assert!(OwnedSecret::parse("s", "encryption-key", None).is_err());
        assert!(OwnedSecret::parse("s", "k=other", None).is_err());
    }
}
