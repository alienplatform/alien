//! What the verifier trusts, found by itself.
//!
//! The signing settings and the install ledger come from the install's own
//! store (SSM on AWS, Parameter Manager on GCP, the resource group and Key
//! Vault on Azure), at names under a root no management grant reaches. The
//! install id comes from the verifier's own identity. Nothing here comes from
//! the request or the verifier's environment.

use alien_error::{AlienError, Context};
use alien_signing::settings::names;
use alien_signing::{
    candidate_prefixes, discover, ConfigBody, InstallId, Kind, Ledger, Lookup, ReleaseBody,
    SignedStatement, SigningSettings, Trust, Verified,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::error::{ErrorData, Result};

/// Where one install keeps its signing settings, ledger and deployer inputs.
#[async_trait]
pub trait InstallStore: Send + Sync {
    /// The signing settings of `prefix`, as stored.
    async fn settings(&self, prefix: &str) -> Result<Lookup>;
    /// The ledger of `prefix`, as stored.
    async fn ledger(&self, prefix: &str) -> Result<Lookup>;
    /// Replace the ledger of `prefix`.
    async fn store_ledger(&self, prefix: &str, ledger: &Ledger) -> Result<()>;
    /// Deployer input `name` of `prefix`, if the deployer set it.
    async fn input(&self, prefix: &str, name: &str) -> Result<Option<String>>;
    /// Where the settings of `prefix` live, for messages.
    fn settings_name(&self, prefix: &str) -> String {
        names::aws_settings(prefix)
    }
    /// Where the ledger of `prefix` lives, for messages.
    fn ledger_name(&self, prefix: &str) -> String {
        names::aws_ledger(prefix)
    }
}

/// A protected install, as the verifier found it.
#[derive(Debug, Clone)]
pub struct Install {
    /// What setup pinned.
    pub settings: SigningSettings,
    /// The install id, derived from the verifier's identity.
    pub id: InstallId,
    /// What the install accepted so far.
    pub ledger: Ledger,
}

fn refused(reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::Refused {
        reason: reason.into(),
    })
}

/// Find the install the verifier named `own_name` belongs to.
///
/// Every prefix `own_name` could belong to is read (D9): exactly one place
/// may hold settings, and they must name that place. When none does, the
/// verifier still refuses: it runs only on protected installs, and a ledger
/// that accepted signed state under some fingerprint makes the missing
/// settings a changed trust root (D8). `install_id` derives the id for a
/// prefix from the verifier's own identity.
pub async fn find_install(
    store: &dyn InstallStore,
    own_name: &str,
    install_id: &(dyn Fn(&str) -> InstallId + Sync),
) -> Result<Install> {
    let candidates = candidate_prefixes(own_name);
    let mut lookups = Vec::with_capacity(candidates.len());
    for prefix in &candidates {
        lookups.push((prefix.clone(), store.settings(prefix).await?));
    }
    let settings = discover(&lookups).context(ErrorData::Refused {
        reason: "the signing settings could not be decided".to_string(),
    })?;
    let Some(settings) = settings else {
        for prefix in &candidates {
            if let Lookup::Found(bytes) = store.ledger(prefix).await? {
                let ledger = Ledger::parse(&bytes, prefix).context(ErrorData::Refused {
                    reason: format!("the ledger at {} is not valid", store.ledger_name(prefix)),
                })?;
                ledger
                    .check_root(None, &install_id(prefix))
                    .context(ErrorData::Refused {
                        reason: format!(
                            "the signing settings at {} are gone, and this install accepted signed state",
                            store.settings_name(prefix)
                        ),
                    })?;
            }
        }
        return Err(refused(
            "no signing settings were found, so this install is not protected and the verifier applies nothing",
        ));
    };
    let prefix = settings.prefix.clone();
    let id = install_id(&prefix);
    let ledger = match store.ledger(&prefix).await? {
        Lookup::Found(bytes) => Ledger::parse(&bytes, &prefix).context(ErrorData::Refused {
            reason: format!("the ledger at {} is not valid", store.ledger_name(&prefix)),
        })?,
        Lookup::Absent => {
            return Err(refused(format!(
                "the ledger setup creates at {} is missing",
                store.ledger_name(&prefix)
            )))
        }
        Lookup::Denied => {
            return Err(refused(format!(
                "the verifier may not read its ledger at {}",
                store.ledger_name(&prefix)
            )))
        }
    };
    Ok(Install {
        settings,
        id,
        ledger,
    })
}

/// A release, and the config with it, that passed every check, with the
/// ledger raised to them.
#[derive(Debug, Clone)]
pub struct Accepted {
    /// The trust the statements verified under.
    pub trust: Trust,
    /// The release.
    pub release: Verified<ReleaseBody>,
    /// The config, if one was delivered.
    pub config: Option<Verified<ConfigBody>>,
    /// The ledger after accepting them. Values only rose.
    pub ledger: Ledger,
}

impl Accepted {
    /// The verified config body, or an empty one.
    pub fn config_body(&self) -> ConfigBody {
        self.config
            .as_ref()
            .map(|config| config.body.clone())
            .unwrap_or_default()
    }
}

/// Verify `release` (and `config`) for `install` on `platform`, against
/// the signer chain `signer_lists` (from version 1) and the install's
/// ledger.
pub fn accept(
    install: &Install,
    signer_lists: &[SignedStatement],
    release: &SignedStatement,
    config: Option<&SignedStatement>,
    platform: &str,
    now: DateTime<Utc>,
) -> Result<Accepted> {
    let trust = Trust::new(
        install.settings.clone(),
        install.id.clone(),
        signer_lists,
        &install.ledger,
        now,
    )
    .context(ErrorData::Refused {
        reason: "the signer lists were refused".to_string(),
    })?;
    let release = trust
        .verify_release(release, now)
        .context(ErrorData::Refused {
            reason: "the release was refused".to_string(),
        })?;
    if release.body.platform != platform {
        return Err(refused(format!(
            "the release is for platform '{}', not '{platform}'",
            release.body.platform
        )));
    }
    let mut ledger = install.ledger.clone();
    ledger
        .raise(Kind::Release, release.sequence, &release.digest)
        .context(ErrorData::Refused {
            reason: "the release is older than one this install accepted".to_string(),
        })?;
    let config = match config {
        Some(config) => {
            let config =
                trust
                    .verify_config(config, &release.body, now)
                    .context(ErrorData::Refused {
                        reason: "the config was refused".to_string(),
                    })?;
            ledger
                .raise(Kind::Config, config.sequence, &config.digest)
                .context(ErrorData::Refused {
                    reason: "the config is older than one this install accepted".to_string(),
                })?;
            Some(config)
        }
        None => {
            ledger
                .require(Kind::Config, false)
                .context(ErrorData::Refused {
                    reason: "the config was withheld".to_string(),
                })?;
            None
        }
    };
    trust.record(&mut ledger).context(ErrorData::Refused {
        reason: "the ledger refused this trust".to_string(),
    })?;
    Ok(Accepted {
        trust,
        release,
        config,
        ledger,
    })
}

/// Write `accepted` back to the install's ledger: read it again, take the
/// higher value of each entry, and store the result. Another enforcer may
/// have raised it in between; nothing here lowers it.
pub async fn store_ledger(store: &dyn InstallStore, prefix: &str, accepted: &Ledger) -> Result<()> {
    let mut current = match store.ledger(prefix).await? {
        Lookup::Found(bytes) => Ledger::parse(&bytes, prefix).context(ErrorData::Refused {
            reason: format!("the ledger at {} is not valid", store.ledger_name(prefix)),
        })?,
        Lookup::Absent | Lookup::Denied => {
            return Err(refused(format!(
                "the ledger at {} could not be read again",
                store.ledger_name(prefix)
            )))
        }
    };
    let before = current.clone();
    current.merge(accepted).context(ErrorData::Refused {
        reason: "the ledger changed in a way that forks from this release".to_string(),
    })?;
    if current != before {
        store.store_ledger(prefix, &current).await?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::fixtures::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// An install store held in memory, with places the verifier may not read.
    #[derive(Default)]
    pub struct MemoryStore {
        pub settings: Mutex<BTreeMap<String, Vec<u8>>>,
        pub ledgers: Mutex<BTreeMap<String, Vec<u8>>>,
        pub inputs: BTreeMap<String, String>,
        pub denied: Vec<String>,
    }

    impl MemoryStore {
        pub fn protected(settings: &SigningSettings) -> Self {
            let store = Self::default();
            store
                .settings
                .lock()
                .unwrap()
                .insert(settings.prefix.clone(), settings.to_json().into_bytes());
            store.ledgers.lock().unwrap().insert(
                settings.prefix.clone(),
                Ledger::new(&settings.prefix).to_json().into_bytes(),
            );
            store
        }

        pub fn ledger_of(&self, prefix: &str) -> Ledger {
            Ledger::parse(&self.ledgers.lock().unwrap()[prefix], prefix).unwrap()
        }
    }

    fn lookup(map: &Mutex<BTreeMap<String, Vec<u8>>>, denied: &[String], prefix: &str) -> Lookup {
        if denied.iter().any(|name| name == prefix) {
            return Lookup::Denied;
        }
        match map.lock().unwrap().get(prefix) {
            Some(bytes) => Lookup::Found(bytes.clone()),
            None => Lookup::Absent,
        }
    }

    #[async_trait]
    impl InstallStore for MemoryStore {
        async fn settings(&self, prefix: &str) -> Result<Lookup> {
            Ok(lookup(&self.settings, &self.denied, prefix))
        }
        async fn ledger(&self, prefix: &str) -> Result<Lookup> {
            Ok(lookup(&self.ledgers, &self.denied, prefix))
        }
        async fn store_ledger(&self, prefix: &str, ledger: &Ledger) -> Result<()> {
            self.ledgers
                .lock()
                .unwrap()
                .insert(prefix.to_string(), ledger.to_json().into_bytes());
            Ok(())
        }
        async fn input(&self, _prefix: &str, name: &str) -> Result<Option<String>> {
            Ok(self.inputs.get(name).cloned())
        }
    }

    pub fn aws_install(prefix: &str) -> InstallId {
        InstallId::Aws {
            account: ACCOUNT.to_string(),
            region: REGION.to_string(),
            prefix: prefix.to_string(),
        }
    }

    pub async fn found(store: &MemoryStore) -> Result<Install> {
        find_install(store, &format!("{PREFIX}-verifier"), &aws_install).await
    }

    fn reason(error: AlienError<ErrorData>) -> String {
        crate::describe(&error)
    }

    #[tokio::test]
    async fn the_settings_are_found_at_the_verifiers_own_prefix() {
        let store = MemoryStore::protected(&settings());
        let install = found(&store).await.unwrap();
        assert_eq!(install.settings, settings());
        assert_eq!(
            install.id.to_string(),
            format!("aws:{ACCOUNT}:{REGION}:{PREFIX}")
        );
    }

    #[tokio::test]
    async fn two_places_with_settings_are_refused() {
        // A neighbor install `acme` whose settings a role reading `*` can see.
        let store = MemoryStore::protected(&settings());
        let mut neighbor = settings();
        neighbor.prefix = "acme".to_string();
        store
            .settings
            .lock()
            .unwrap()
            .insert("acme".to_string(), neighbor.to_json().into_bytes());
        let error = reason(found(&store).await.unwrap_err());
        assert!(error.contains("2 places hold signing settings"), "{error}");
    }

    #[tokio::test]
    async fn settings_copied_from_another_prefix_are_refused() {
        let store = MemoryStore::default();
        let mut copied = settings();
        copied.prefix = "other".to_string();
        store
            .settings
            .lock()
            .unwrap()
            .insert(PREFIX.to_string(), copied.to_json().into_bytes());
        let error = reason(found(&store).await.unwrap_err());
        assert!(error.contains("name prefix 'other'"), "{error}");
    }

    #[tokio::test]
    async fn every_place_denied_is_not_unprotected() {
        let store = MemoryStore {
            denied: candidate_prefixes(&format!("{PREFIX}-verifier")),
            ..MemoryStore::default()
        };
        let error = reason(found(&store).await.unwrap_err());
        assert!(error.contains("refused the read"), "{error}");
    }

    #[tokio::test]
    async fn a_missing_ledger_is_an_error_not_a_fresh_start() {
        let store = MemoryStore::protected(&settings());
        store.ledgers.lock().unwrap().clear();
        let error = reason(found(&store).await.unwrap_err());
        assert!(error.contains("ledger setup creates"), "{error}");
    }

    #[tokio::test]
    async fn removed_settings_after_acceptance_are_a_changed_trust_root() {
        let store = MemoryStore::protected(&settings());
        let install = found(&store).await.unwrap();
        let accepted = accept(
            &install,
            &[signer_list()],
            &release(10, function_workload()),
            None,
            "aws",
            now(),
        )
        .unwrap();
        store_ledger(&store, PREFIX, &accepted.ledger)
            .await
            .unwrap();

        store.settings.lock().unwrap().clear();
        let error = reason(found(&store).await.unwrap_err());
        assert!(
            error.contains("SIGNING_TRUST_ROOT_CHANGED") || error.contains("no signing settings"),
            "{error}"
        );
        assert!(error.contains("are gone"), "{error}");

        // Settings replaced with another fingerprint are refused too.
        let mut other = settings();
        other.fingerprint = format!("SHA256:{}", "a".repeat(64));
        store
            .settings
            .lock()
            .unwrap()
            .insert(PREFIX.to_string(), other.to_json().into_bytes());
        let install = found(&store).await.unwrap();
        let error = reason(
            accept(
                &install,
                &[signer_list()],
                &release(11, function_workload()),
                None,
                "aws",
                now(),
            )
            .unwrap_err(),
        );
        assert!(error.contains("signer lists were refused"), "{error}");
    }

    #[tokio::test]
    async fn the_ledger_only_rises_and_refuses_older_releases() {
        let store = MemoryStore::protected(&settings());
        let install = found(&store).await.unwrap();
        let accepted = accept(
            &install,
            &[signer_list()],
            &release(20, function_workload()),
            None,
            "aws",
            now(),
        )
        .unwrap();
        store_ledger(&store, PREFIX, &accepted.ledger)
            .await
            .unwrap();
        assert_eq!(store.ledger_of(PREFIX).release.unwrap().sequence, 20);
        assert_eq!(
            store.ledger_of(PREFIX).fingerprint,
            Some(settings().fingerprint)
        );

        // A fresh verifier with the stored ledger refuses an older release.
        let install = found(&store).await.unwrap();
        let error = reason(
            accept(
                &install,
                &[signer_list()],
                &release(10, function_workload()),
                None,
                "aws",
                now(),
            )
            .unwrap_err(),
        );
        assert!(
            error.contains("older than one this install accepted"),
            "{error}"
        );
        // The release at the floor passes again.
        accept(
            &install,
            &[signer_list()],
            &release(20, function_workload()),
            None,
            "aws",
            now(),
        )
        .unwrap();
        // An older accepted ledger written back doesn't lower the floor.
        store_ledger(&store, PREFIX, &install.ledger).await.unwrap();
        assert_eq!(store.ledger_of(PREFIX).release.unwrap().sequence, 20);
    }

    #[tokio::test]
    async fn a_release_for_another_channel_or_platform_is_refused() {
        let store = MemoryStore::protected(&settings());
        let install = found(&store).await.unwrap();
        let mut dev = release_body(vec![function_workload()]);
        dev.channel = "dev".to_string();
        let error = reason(
            accept(
                &install,
                &[signer_list()],
                &signed_release(10, &dev),
                None,
                "aws",
                now(),
            )
            .unwrap_err(),
        );
        assert!(
            error.contains("SIGNING_WRONG_CHANNEL") || error.contains("channel"),
            "{error}"
        );

        let error = reason(
            accept(
                &install,
                &[signer_list()],
                &release(10, function_workload()),
                None,
                "gcp",
                now(),
            )
            .unwrap_err(),
        );
        assert!(error.contains("not 'gcp'"), "{error}");
    }

    #[tokio::test]
    async fn a_withheld_config_is_refused_once_one_was_accepted() {
        let store = MemoryStore::protected(&settings());
        let install = found(&store).await.unwrap();
        let config = config(3, ConfigBody::default());
        let accepted = accept(
            &install,
            &[signer_list()],
            &release(10, function_workload()),
            Some(&config),
            "aws",
            now(),
        )
        .unwrap();
        store_ledger(&store, PREFIX, &accepted.ledger)
            .await
            .unwrap();
        let install = found(&store).await.unwrap();
        let error = reason(
            accept(
                &install,
                &[signer_list()],
                &release(11, function_workload()),
                None,
                "aws",
                now(),
            )
            .unwrap_err(),
        );
        assert!(error.contains("config was withheld"), "{error}");
    }
}
