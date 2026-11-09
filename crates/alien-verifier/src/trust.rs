//! What the verifier trusts, found by itself.
//!
//! The signing settings and the install ledger come from the install's own
//! store (SSM on AWS, Parameter Manager on GCP, the resource group and Key
//! Vault on Azure), at names under a root no management grant reaches. The
//! install id comes from the verifier's own identity. Nothing here comes from
//! the request or the verifier's environment.

use std::collections::BTreeSet;

use alien_error::{AlienError, Context};
use alien_signing::settings::names;
use alien_signing::{
    candidate_prefixes, discover, ConfigBody, Entry, InstallId, Kind, Ledger, Lookup, ReleaseBody,
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
    /// Replace the ledger of `prefix`. A store that numbers its writes gets a ledger whose
    /// `based_on` is the version it was merged onto.
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
    /// For a store that numbers each write of the ledger: the ledger of `prefix` and its
    /// version. `None` when the store doesn't number writes.
    async fn numbered_ledger(&self, _prefix: &str) -> Result<Option<(Lookup, u64)>> {
        Ok(None)
    }
    /// Version `version` of the ledger of `prefix`. Called only on a store whose
    /// `numbered_ledger` answers.
    async fn ledger_version(&self, prefix: &str, _version: u64) -> Result<Lookup> {
        Err(refused(format!(
            "the ledger at {} has no numbered writes",
            self.ledger_name(prefix)
        )))
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
            if let Lookup::Found(bytes) = read_ledger(store, prefix).await? {
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
    let ledger = match read_ledger(store, &prefix).await? {
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

/// The statement at the install's floor for `kind`, kept: it passed when
/// it was accepted, and removing its signer from the list doesn't stop it
/// (only the deny-list does). `None` when `statement` isn't the one there.
fn kept<B: serde::de::DeserializeOwned>(
    trust: &Trust,
    ledger: &Ledger,
    kind: Kind,
    statement: &SignedStatement,
) -> alien_signing::Result<Option<Verified<B>>> {
    let opened = statement.open()?;
    let floor = match kind {
        Kind::Release => ledger.release.as_ref(),
        Kind::Config => ledger.config.as_ref(),
        _ => None,
    };
    if opened.kind != kind || floor.is_none_or(|floor| floor.digest != opened.digest) {
        return Ok(None);
    }
    trust.keeps(&opened.digest)?;
    Ok(Some(Verified {
        kind,
        sequence: opened.sequence,
        digest: opened.digest.clone(),
        not_after: opened.not_after,
        signers: BTreeSet::new(),
        body: opened.body()?,
    }))
}

/// `release` verified for this install, or kept at its floor.
pub(crate) fn verify_or_keep_release(
    trust: &Trust,
    ledger: &Ledger,
    release: &SignedStatement,
    now: DateTime<Utc>,
) -> alien_signing::Result<Verified<ReleaseBody>> {
    match kept::<ReleaseBody>(trust, ledger, Kind::Release, release)? {
        Some(kept) => {
            if kept.body.channel != trust.settings().channel {
                return Err(AlienError::new(alien_signing::ErrorData::WrongChannel {
                    expected: trust.settings().channel.clone(),
                    actual: kept.body.channel.clone(),
                }));
            }
            kept.body.validate()?;
            Ok(kept)
        }
        None => trust.verify_release(release, now),
    }
}

/// `config` verified for this install and `release`, or kept at its floor.
fn verify_or_keep_config(
    trust: &Trust,
    ledger: &Ledger,
    config: &SignedStatement,
    release: &ReleaseBody,
    now: DateTime<Utc>,
) -> alien_signing::Result<Verified<ConfigBody>> {
    match kept::<ConfigBody>(trust, ledger, Kind::Config, config)? {
        Some(kept) => {
            kept.body.check(release)?;
            Ok(kept)
        }
        None => trust.verify_config(config, release, now),
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
    let release = verify_or_keep_release(&trust, &install.ledger, release, now).context(
        ErrorData::Refused {
            reason: "the release was refused".to_string(),
        },
    )?;
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
            let config = verify_or_keep_config(&trust, &install.ledger, config, &release.body, now)
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

/// The ledger of `prefix` as stored. A ledger that is gone or unreadable refuses.
async fn stored(store: &dyn InstallStore, prefix: &str, lookup: Lookup) -> Result<Ledger> {
    match lookup {
        Lookup::Found(bytes) => Ledger::parse(&bytes, prefix).context(ErrorData::Refused {
            reason: format!("the ledger at {} is not valid", store.ledger_name(prefix)),
        }),
        Lookup::Absent | Lookup::Denied => Err(refused(format!(
            "the ledger at {} could not be read again",
            store.ledger_name(prefix)
        ))),
    }
}

/// The ledger of `prefix` as the store holds it.
///
/// A store that numbers its writes has no compare-and-swap, so a write can replace one made
/// meanwhile and show a lower floor than the install accepted. Each write records the version
/// it was merged onto (`based_on`), so the versions written since are the ones after it: every
/// one is read and merged in. The result holds everything written up to the version read, which
/// is why nothing that checks the floor reads the stored value alone.
async fn read_ledger(store: &dyn InstallStore, prefix: &str) -> Result<Lookup> {
    let Some((lookup, version)) = store.numbered_ledger(prefix).await? else {
        return store.ledger(prefix).await;
    };
    let Lookup::Found(bytes) = &lookup else {
        return Ok(lookup);
    };
    let mut ledger = stored(store, prefix, Lookup::Found(bytes.clone())).await?;
    let unmerged_versions = ledger
        .unmerged_versions(version)
        .context(ErrorData::Refused {
            reason: format!(
                "the ledger at {} is not a write of the version it was read at",
                store.ledger_name(prefix)
            ),
        })?;
    for unmerged in unmerged_versions {
        match store.ledger_version(prefix, unmerged).await? {
            Lookup::Found(bytes) => {
                let other = stored(store, prefix, Lookup::Found(bytes)).await?;
                merge_into(store, prefix, &mut ledger, &other)?;
            }
            Lookup::Absent | Lookup::Denied => {
                return Err(refused(format!(
                    "version {unmerged} of the ledger at {} could not be read, so the floor can't be known",
                    store.ledger_name(prefix)
                )))
            }
        }
    }
    ledger.resolved_at(version);
    Ok(Lookup::Found(ledger.to_json().into_bytes()))
}

/// Write `accepted` back to the install's ledger: read it again, take the higher value of
/// each entry, and store the result. Another enforcer (another verifier call, or a machine) may
/// have raised it in between; nothing here lowers it.
///
/// Where the store numbers writes, a write made between this read and this write is not lost:
/// this write records the version it was merged onto, and every reader merges in the versions
/// after that (see [`read_ledger`]). Elsewhere one verifier instance serves the install, and
/// machines merge before they write.
pub async fn store_ledger(store: &dyn InstallStore, prefix: &str, accepted: &Ledger) -> Result<()> {
    let lookup = read_ledger(store, prefix).await?;
    let mut current = stored(store, prefix, lookup).await?;
    let before = current.clone();
    merge_into(store, prefix, &mut current, accepted)?;
    if current != before {
        store.store_ledger(prefix, &current).await?;
    }
    Ok(())
}

fn merge_into(
    store: &dyn InstallStore,
    prefix: &str,
    current: &mut Ledger,
    other: &Ledger,
) -> Result<()> {
    current.merge(other).context(ErrorData::Refused {
        reason: format!(
            "the ledger at {} changed in a way that forks from this release",
            store.ledger_name(prefix)
        ),
    })
}

/// The release and config a request is applying, which must still be the install's newest when
/// each of its functions is written. A config is checked too: a function's env follows it, so
/// an older config applied after a newer one would roll the env back.
pub struct Floor<'a> {
    /// Where the ledger lives.
    pub store: &'a dyn InstallStore,
    /// The install's prefix.
    pub prefix: &'a str,
    /// The release, as the ledger records it.
    pub release: Entry,
    /// The config, as the ledger records it; none when the install has accepted none.
    pub config: Option<Entry>,
}

#[async_trait]
impl crate::function::StillNewest for Floor<'_> {
    async fn check(&self) -> Result<()> {
        let lookup = read_ledger(self.store, self.prefix).await?;
        let ledger = stored(self.store, self.prefix, lookup).await?;
        match &ledger.release {
            Some(newest) if *newest == self.release => {}
            Some(newest) => {
                return Err(refused(format!(
                    "release {} was accepted while release {} was being applied",
                    newest.sequence, self.release.sequence
                )))
            }
            None => {
                return Err(refused(format!(
                    "the ledger at {} no longer records release {}",
                    self.store.ledger_name(self.prefix),
                    self.release.sequence
                )))
            }
        }
        if ledger.config != self.config {
            return Err(refused(format!(
                "config {} was accepted while config {} was being applied",
                ledger.config.as_ref().map_or(0, |entry| entry.sequence),
                self.config.as_ref().map_or(0, |entry| entry.sequence)
            )));
        }
        Ok(())
    }
}

/// What a request applying `accepted` does with the install's ledger: records it before the
/// first write, then rechecks it before each.
pub struct Acceptance<'a> {
    floor: Floor<'a>,
    ledger: &'a Ledger,
}

impl<'a> Acceptance<'a> {
    /// The acceptance of `accepted` in the ledger of `prefix` in `store`.
    pub fn new(store: &'a dyn InstallStore, prefix: &'a str, accepted: &'a Accepted) -> Self {
        Self {
            floor: Floor {
                store,
                prefix,
                release: Entry {
                    sequence: accepted.release.sequence,
                    digest: accepted.release.digest.clone(),
                },
                config: accepted.ledger.config.clone(),
            },
            ledger: &accepted.ledger,
        }
    }
}

#[async_trait]
impl crate::function::StillNewest for Acceptance<'_> {
    async fn check(&self) -> Result<()> {
        self.floor.check().await
    }
}

#[async_trait]
impl crate::function::Accepting for Acceptance<'_> {
    async fn accept(&self) -> Result<()> {
        store_ledger(self.floor.store, self.floor.prefix, self.ledger).await
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::fixtures::*;
    use alien_signing::{Role, SecretKey, Signer, SignerKind, SignerListBody};
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

    /// A store that numbers each ledger write, like SSM, and replaces the value on every write:
    /// where another writer's write lands between the verifier's read and its write once.
    struct Numbered {
        versions: Mutex<Vec<Ledger>>,
        meanwhile: Mutex<Option<Ledger>>,
        version_reads: Mutex<Vec<u64>>,
        gone: Mutex<Vec<u64>>,
    }

    impl Numbered {
        fn new(versions: Vec<Ledger>) -> Self {
            Self {
                versions: Mutex::new(versions),
                meanwhile: Mutex::new(None),
                version_reads: Mutex::new(Vec::new()),
                gone: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl InstallStore for Numbered {
        async fn settings(&self, _prefix: &str) -> Result<Lookup> {
            unreachable!("only the ledger is used here")
        }
        async fn ledger(&self, _prefix: &str) -> Result<Lookup> {
            unreachable!("a numbered store is read through numbered_ledger")
        }
        async fn store_ledger(&self, _prefix: &str, ledger: &Ledger) -> Result<()> {
            let mut versions = self.versions.lock().unwrap();
            if let Some(other) = self.meanwhile.lock().unwrap().take() {
                versions.push(other);
            }
            versions.push(ledger.clone());
            Ok(())
        }
        async fn input(&self, _prefix: &str, _name: &str) -> Result<Option<String>> {
            unreachable!("only the ledger is used here")
        }
        async fn numbered_ledger(&self, _prefix: &str) -> Result<Option<(Lookup, u64)>> {
            let versions = self.versions.lock().unwrap();
            let latest = versions.last().unwrap();
            Ok(Some((
                Lookup::Found(latest.to_json().into_bytes()),
                versions.len() as u64,
            )))
        }
        async fn ledger_version(&self, _prefix: &str, version: u64) -> Result<Lookup> {
            self.version_reads.lock().unwrap().push(version);
            let versions = self.versions.lock().unwrap();
            if self.gone.lock().unwrap().contains(&version) {
                return Ok(Lookup::Absent);
            }
            Ok(match versions.get(version as usize - 1) {
                Some(ledger) => Lookup::Found(ledger.to_json().into_bytes()),
                None => Lookup::Absent,
            })
        }
    }

    fn at_release(sequence: u64, digit: char) -> Ledger {
        based_on(0, sequence, digit)
    }

    /// A ledger at release `sequence`, written after merging onto version `based_on`.
    fn based_on(based_on: u64, sequence: u64, digit: char) -> Ledger {
        let mut ledger = Ledger::new(PREFIX);
        ledger
            .raise(
                Kind::Release,
                sequence,
                &format!("sha256:{}", digit.to_string().repeat(64)),
            )
            .unwrap();
        ledger.based_on = based_on;
        ledger
    }

    async fn floor_of(store: &Numbered) -> u64 {
        let lookup = read_ledger(store, PREFIX).await.unwrap();
        stored(store, PREFIX, lookup)
            .await
            .unwrap()
            .release
            .unwrap()
            .sequence
    }

    #[tokio::test]
    async fn a_ledger_write_never_hides_a_raise_made_meanwhile() {
        // Another writer raises the ledger to release 11 between this read and this write of
        // release 10. The write of 10 is the latest value, but it records that it was merged
        // onto version 1 only, so every reader merges in version 2 and sees 11.
        let store = Numbered::new(vec![Ledger::new(PREFIX)]);
        *store.meanwhile.lock().unwrap() = Some(based_on(1, 11, 'b'));
        store_ledger(&store, PREFIX, &at_release(10, 'a'))
            .await
            .unwrap();
        {
            let versions = store.versions.lock().unwrap();
            assert_eq!(versions.len(), 3, "empty, theirs, ours");
            assert_eq!(versions[2].release.as_ref().unwrap().sequence, 10);
            assert_eq!(versions[2].based_on, 1, "ours was merged onto version 1");
        }
        assert_eq!(floor_of(&store).await, 11);
    }

    #[tokio::test]
    async fn a_reader_never_sees_the_floor_an_older_write_lowered() {
        // Release 11 landed at version 2, then a writer that had read only version 1 wrote
        // release 10 as version 3. A request for release 10 must still be refused, and must
        // not write the ledger.
        let store = Numbered::new(vec![
            Ledger::new(PREFIX),
            based_on(1, 11, 'b'),
            based_on(1, 10, 'a'),
        ]);
        let floor = Floor {
            store: &store,
            prefix: PREFIX,
            release: Entry {
                sequence: 10,
                digest: format!("sha256:{}", "a".repeat(64)),
            },
            config: None,
        };
        let error = crate::function::StillNewest::check(&floor)
            .await
            .expect_err("release 10 is older than the accepted 11");
        assert!(
            reason(error).contains("release 11 was accepted while release 10 was being applied"),
        );
        store_ledger(&store, PREFIX, &at_release(10, 'a'))
            .await
            .unwrap();
        assert_eq!(
            store.versions.lock().unwrap().len(),
            3,
            "nothing was raised"
        );
    }

    #[tokio::test]
    async fn only_the_versions_after_the_one_a_write_was_merged_onto_are_read() {
        let store = Numbered::new(vec![
            Ledger::new(PREFIX),
            based_on(1, 11, 'b'),
            based_on(2, 12, 'c'),
            based_on(3, 12, 'c'),
        ]);
        assert_eq!(floor_of(&store).await, 12);
        assert!(
            store.version_reads.lock().unwrap().is_empty(),
            "a write merged onto the version before it needs no other read"
        );

        let store = Numbered::new(vec![
            Ledger::new(PREFIX),
            based_on(1, 11, 'b'),
            based_on(1, 12, 'c'),
            based_on(1, 10, 'a'),
        ]);
        assert_eq!(floor_of(&store).await, 12);
        assert_eq!(*store.version_reads.lock().unwrap(), vec![2, 3]);
    }

    #[tokio::test]
    async fn a_version_that_can_no_longer_be_read_refuses() {
        // Version 2 is gone (SSM keeps only the latest 100): the floor can't be known.
        let store = Numbered::new(vec![
            Ledger::new(PREFIX),
            based_on(1, 11, 'b'),
            based_on(1, 10, 'a'),
        ]);
        store.gone.lock().unwrap().push(2);
        let error = read_ledger(&store, PREFIX)
            .await
            .expect_err("version 2 is missing");
        assert!(reason(error).contains("version 2 of the ledger"));
    }

    #[tokio::test]
    async fn a_write_claiming_to_be_merged_onto_a_later_version_is_refused() {
        // Version 3 says it was merged onto version 5, which would skip version 2's raise.
        let store = Numbered::new(vec![
            Ledger::new(PREFIX),
            based_on(1, 11, 'b'),
            based_on(5, 10, 'a'),
        ]);
        let error = read_ledger(&store, PREFIX)
            .await
            .expect_err("a write is merged onto a version before its own");
        assert!(reason(error).contains("not a write of the version it was read at"));
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

    /// A fresh install's store and two requests that both passed `accept` against its first
    /// ledger: release 10 with `older`, release 11 with `newer`. The second overtakes the first
    /// when they run one after the other.
    pub async fn overtaking(
        platform: &str,
        older: &ReleaseBody,
        newer: &ReleaseBody,
    ) -> (MemoryStore, Accepted, Accepted) {
        let store = MemoryStore::protected(&settings());
        let install = found(&store).await.unwrap();
        let accepted = |sequence, body: &ReleaseBody| {
            accept(
                &install,
                &[signer_list()],
                &signed_release(sequence, body),
                None,
                platform,
                now(),
            )
            .unwrap()
        };
        let (older, newer) = (accepted(10, older), accepted(11, newer));
        (store, older, newer)
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

    /// Signer list version 1 with a laptop (admin, release) and CI
    /// (release), and version 2 without CI.
    fn lists_removing_ci() -> (SignedStatement, SignedStatement, SigningSettings) {
        let laptop = key(2);
        let signer = |id: &str, key: &SecretKey, kind: SignerKind, roles: Vec<Role>| Signer {
            id: id.to_string(),
            key: key.public_key().as_str().to_string(),
            kind,
            roles,
            label: None,
        };
        let laptop_signer = signer(
            "laptop",
            &laptop,
            SignerKind::Device,
            vec![Role::Admin, Role::Release],
        );
        let body = |previous: Option<String>, signers: Vec<Signer>| SignerListBody {
            previous,
            admin_threshold: 1,
            signers,
            deny: Vec::new(),
            recovered: false,
        };
        let v1 = statement(
            PROJECT,
            Kind::SignerList,
            None,
            1,
            Some(Utc::now() + chrono::Duration::days(60)),
            body(
                None,
                vec![
                    laptop_signer.clone(),
                    signer("ci", &ci(), SignerKind::Ci, vec![Role::Release]),
                ],
            ),
        )
        .sign_with("laptop", &laptop)
        .unwrap();
        let v2 = statement_under(
            PROJECT,
            Kind::SignerList,
            None,
            2,
            Some(Utc::now() + chrono::Duration::days(60)),
            1,
            body(Some(v1.open().unwrap().digest), vec![laptop_signer]),
        )
        .sign_with("laptop", &laptop)
        .unwrap();
        let settings = settings_for(&v1, PREFIX, CHANNEL);
        (v1, v2, settings)
    }

    #[tokio::test]
    async fn removing_the_ci_key_keeps_what_it_signed_and_refuses_what_it_signs_next() {
        let (v1, v2, settings) = lists_removing_ci();
        let store = MemoryStore::protected(&settings);
        let install = found(&store).await.unwrap();
        let ten = release(10, function_workload());
        let configured = config(
            1,
            ConfigBody {
                env: BTreeMap::from([("LOG_LEVEL".to_string(), "info".to_string())]),
                ..ConfigBody::default()
            },
        );
        let accepted = accept(
            &install,
            &[v1.clone()],
            &ten,
            Some(&configured),
            "aws",
            Utc::now(),
        )
        .unwrap();
        store_ledger(&store, PREFIX, &accepted.ledger)
            .await
            .unwrap();

        // CI leaves the list. What it signed and the install accepted keeps
        // passing, so the verifier can still check and re-apply it.
        let install = found(&store).await.unwrap();
        let both = [v1, v2];
        let kept = accept(&install, &both, &ten, Some(&configured), "aws", Utc::now()).unwrap();
        assert_eq!(kept.release.sequence, 10);
        assert!(kept.release.signers.is_empty(), "kept, not verified again");
        store_ledger(&store, PREFIX, &kept.ledger).await.unwrap();

        // A release CI signs now is refused.
        let install = found(&store).await.unwrap();
        let by_ci = statement_under(
            PROJECT,
            Kind::Release,
            None,
            11,
            None,
            2,
            &release_body(vec![function_workload()]),
        )
        .sign_with("ci", &ci())
        .unwrap();
        let error = reason(
            accept(
                &install,
                &both,
                &by_ci,
                Some(&configured),
                "aws",
                Utc::now(),
            )
            .unwrap_err(),
        );
        assert!(error.contains("not on signer list version 2"), "{error}");

        // The laptop's next release lands with the config CI signed before.
        let by_laptop = statement_under(
            PROJECT,
            Kind::Release,
            None,
            11,
            None,
            2,
            &release_body(vec![function_workload()]),
        )
        .sign_with("laptop", &key(2))
        .unwrap();
        let next = accept(
            &install,
            &both,
            &by_laptop,
            Some(&configured),
            "aws",
            Utc::now(),
        )
        .unwrap();
        assert_eq!(next.release.sequence, 11);
        assert_eq!(next.config.unwrap().sequence, 1);
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
