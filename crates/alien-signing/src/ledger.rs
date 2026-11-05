//! What an install has already accepted.
//!
//! Every enforcer of an install keeps the same small record: the trust root
//! it accepted state under, the highest signer list version, and the
//! release and config floors. Setup creates the install's copy in the
//! customer's cloud non-empty, so a missing ledger is an error and never a
//! fresh start. Enforcers load it at start, take the maximum of it and their
//! own copy, and write the result back. Values only rise.
//!
//! Each entry is the `(sequence, digest)` of a statement the enforcer can
//! verify again, so a ledger written by someone without a signing key can
//! cause refusals, but can't make anything unsigned run.

use alien_error::AlienError;
use serde::{Deserialize, Serialize};

use crate::envelope::Kind;
use crate::error::{ErrorData, Result};
use crate::release::is_sha256_digest;
use crate::settings::InstallId;
use crate::signer_list::{is_fingerprint, SignerChain};

/// One accepted statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Entry {
    /// Its sequence, or the version of a signer list.
    pub sequence: u64,
    /// `sha256:<hex>` of its payload.
    pub digest: String,
}

/// The ledger of one install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ledger {
    /// The install's prefix; it must equal where the ledger was found.
    pub prefix: String,
    /// Setup writes `true`, so an existing ledger is never read as no ledger.
    pub created: bool,
    /// The fingerprint the install first accepted state under.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    /// The install id the install first accepted state under.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install: Option<String>,
    /// Highest signer list version accepted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer_list: Option<Entry>,
    /// Release floor: no older release is accepted on any machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release: Option<Entry>,
    /// Config floor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<Entry>,
}

impl Ledger {
    /// The ledger setup creates for `prefix`.
    pub fn new(prefix: &str) -> Self {
        Self {
            prefix: prefix.to_string(),
            created: true,
            fingerprint: None,
            install: None,
            signer_list: None,
            release: None,
            config: None,
        }
    }

    /// Parse a ledger found for `prefix`.
    pub fn parse(bytes: &[u8], prefix: &str) -> Result<Self> {
        let ledger: Ledger =
            serde_json::from_slice(bytes).map_err(|error| refused(format!("{error}")))?;
        if ledger.prefix != prefix {
            return Err(refused(format!(
                "the ledger found for '{prefix}' names prefix '{}'",
                ledger.prefix
            )));
        }
        if !ledger.created {
            return Err(refused("setup didn't create this ledger"));
        }
        if let Some(fingerprint) = &ledger.fingerprint {
            if !is_fingerprint(fingerprint) {
                return Err(refused(format!("'{fingerprint}' is not a fingerprint")));
            }
        }
        if let Some(install) = &ledger.install {
            InstallId::parse(install)?;
        }
        for entry in [&ledger.signer_list, &ledger.release, &ledger.config]
            .into_iter()
            .flatten()
        {
            if !is_sha256_digest(&entry.digest) {
                return Err(refused(format!("'{}' is not a digest", entry.digest)));
            }
        }
        Ok(ledger)
    }

    /// The JSON form enforcers write back.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a ledger always serializes")
    }

    /// Take the higher value of each entry from `other`. Two different
    /// statements with one sequence, or two trust roots, are a fork and an
    /// error.
    pub fn merge(&mut self, other: &Ledger) -> Result<()> {
        if other.prefix != self.prefix {
            return Err(refused(format!(
                "can't merge the ledger of '{}' into '{}'",
                other.prefix, self.prefix
            )));
        }
        merge_root(&mut self.fingerprint, &other.fingerprint)?;
        merge_root(&mut self.install, &other.install)?;
        merge_entry(&mut self.signer_list, &other.signer_list, Kind::SignerList)?;
        merge_entry(&mut self.release, &other.release, Kind::Release)?;
        merge_entry(&mut self.config, &other.config, Kind::Config)?;
        Ok(())
    }

    /// Refuse a trust root other than the one the install accepted state
    /// under, or none where it had one. Protection is sticky: removing or
    /// changing the settings, or moving the install, never makes an enforcer
    /// run something new.
    pub fn check_root(&self, fingerprint: Option<&str>, install: &InstallId) -> Result<()> {
        if let Some(accepted) = &self.fingerprint {
            if fingerprint != Some(accepted.as_str()) {
                return Err(AlienError::new(ErrorData::TrustRootChanged {
                    accepted: format!("fingerprint {accepted}"),
                    found: fingerprint
                        .map(|found| format!("fingerprint {found}"))
                        .unwrap_or_else(|| "no signing settings".to_string()),
                }));
            }
        }
        let install = install.to_string();
        if let Some(accepted) = &self.install {
            if *accepted != install {
                return Err(AlienError::new(ErrorData::TrustRootChanged {
                    accepted: format!("install {accepted}"),
                    found: format!("install {install}"),
                }));
            }
        }
        Ok(())
    }

    /// Whether the install ever accepted signed state.
    pub fn is_sticky(&self) -> bool {
        self.fingerprint.is_some()
    }

    /// Record the trust root, after [`Ledger::check_root`] passed.
    pub fn adopt_root(&mut self, fingerprint: &str, install: &InstallId) {
        self.fingerprint
            .get_or_insert_with(|| fingerprint.to_string());
        self.install.get_or_insert_with(|| install.to_string());
    }

    /// Refuse a chain that stops below the accepted version or forks from it.
    pub fn check_chain(&self, chain: &SignerChain) -> Result<()> {
        let Some(entry) = &self.signer_list else {
            return Ok(());
        };
        let current = chain.current().version;
        if current < entry.sequence {
            return Err(AlienError::new(ErrorData::SignerListMissing {
                needed: entry.sequence,
                current,
            }));
        }
        if chain.digest_of(entry.sequence) != Some(entry.digest.as_str()) {
            return Err(refused(format!(
                "signer list version {} differs from the one this install accepted",
                entry.sequence
            )));
        }
        Ok(())
    }

    /// Record the chain's current version.
    pub fn record_chain(&mut self, chain: &SignerChain) {
        let current = chain.current();
        let entry = Entry {
            sequence: current.version,
            digest: current.digest.clone(),
        };
        if self
            .signer_list
            .as_ref()
            .is_none_or(|held| held.sequence < entry.sequence)
        {
            self.signer_list = Some(entry);
        }
    }

    /// Refuse a release or config older than the floor, or a different one
    /// at the floor. The statement at the floor passes again, because
    /// enforcers re-read their current statement.
    pub fn check(&self, kind: Kind, sequence: u64, digest: &str) -> Result<()> {
        if let Some(floor) = self.floor(kind)? {
            if sequence < floor.sequence || (sequence == floor.sequence && digest != floor.digest) {
                return Err(AlienError::new(ErrorData::SequenceReplayed {
                    kind: kind.as_str().to_string(),
                    sequence,
                    floor: floor.sequence,
                }));
            }
        }
        Ok(())
    }

    /// Raise the floor of `kind` to `sequence`. Never lowers it.
    pub fn raise(&mut self, kind: Kind, sequence: u64, digest: &str) -> Result<()> {
        self.check(kind, sequence, digest)?;
        let entry = Some(Entry {
            sequence,
            digest: digest.to_string(),
        });
        match kind {
            Kind::Release => self.release = entry,
            Kind::Config => self.config = entry,
            other => {
                return Err(refused(format!(
                    "'{other}' statements have no floor in the ledger"
                )))
            }
        }
        Ok(())
    }

    /// Refuse to go on without a statement of `kind` once the install has
    /// accepted one, so a control plane that withholds the config can't get
    /// workloads started without it.
    pub fn require(&self, kind: Kind, present: bool) -> Result<()> {
        match self.floor(kind)? {
            Some(floor) if !present => Err(refused(format!(
                "this install accepted '{kind}' sequence {} and none was delivered",
                floor.sequence
            ))),
            _ => Ok(()),
        }
    }

    /// The floor of `kind`.
    pub fn floor(&self, kind: Kind) -> Result<Option<&Entry>> {
        match kind {
            Kind::Release => Ok(self.release.as_ref()),
            Kind::Config => Ok(self.config.as_ref()),
            Kind::SignerList => Ok(self.signer_list.as_ref()),
            Kind::Session => Err(refused(
                "sessions are bound to a nonce, not a floor".to_string(),
            )),
        }
    }
}

fn merge_root(held: &mut Option<String>, other: &Option<String>) -> Result<()> {
    match (held.as_ref(), other) {
        (Some(mine), Some(theirs)) if mine != theirs => {
            Err(AlienError::new(ErrorData::TrustRootChanged {
                accepted: mine.clone(),
                found: theirs.clone(),
            }))
        }
        (None, Some(theirs)) => {
            *held = Some(theirs.clone());
            Ok(())
        }
        _ => Ok(()),
    }
}

fn merge_entry(held: &mut Option<Entry>, other: &Option<Entry>, kind: Kind) -> Result<()> {
    let Some(theirs) = other else {
        return Ok(());
    };
    match held {
        Some(mine) if mine.sequence == theirs.sequence && mine.digest != theirs.digest => {
            Err(refused(format!(
                "two different '{kind}' statements were accepted with sequence {}",
                mine.sequence
            )))
        }
        Some(mine) if mine.sequence >= theirs.sequence => Ok(()),
        _ => {
            *held = Some(theirs.clone());
            Ok(())
        }
    }
}

fn refused(reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::LedgerRefused {
        reason: reason.into(),
    })
}
