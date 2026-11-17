//! What an install has already accepted.
//!
//! Every enforcer of an install keeps the same small record: the trust root
//! it accepted state under, the highest signer list version and the digests of
//! the releases and configs it accepted. Setup creates the install's copy in the
//! customer's cloud non-empty, so a missing ledger is an error and never a
//! fresh start. Enforcers load it at start, take the maximum of it and their
//! own copy, and write the result back. Values only rise. An older signed
//! release or config may run again: what the ledger holds is trust, not a version floor.
//!
//! The accepted digests are what makes removing a signer safe: a statement
//! on the list keeps running after its signer leaves. Only enforcers write
//! them, after verifying the statement; management has no access to the
//! ledger (it is denied `/signed/*`). A digest still has to match a statement
//! with a valid signature at the list version it names, so a forged entry
//! can't make anything unsigned run.

use alien_error::AlienError;
use serde::{Deserialize, Serialize};

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

/// How many accepted digests a ledger keeps besides the ones running now; the
/// oldest go first.
pub const MAX_ACCEPTED: usize = 32;

/// What a ledger may hold at most: the history plus what runs now.
pub const MAX_LEDGER_ACCEPTED: usize = 1024;

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
    /// Digests of the releases and configs the install accepted, oldest
    /// first. One in this list keeps running after its signer leaves the
    /// list, unless the deny-list names it. It is still checked against the
    /// signer list version it was signed under.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accepted: Vec<String>,
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
            accepted: Vec::new(),
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
        for entry in [&ledger.signer_list].into_iter().flatten() {
            if !is_sha256_digest(&entry.digest) {
                return Err(refused(format!("'{}' is not a digest", entry.digest)));
            }
        }
        if ledger.accepted.len() > MAX_LEDGER_ACCEPTED {
            return Err(refused("it lists too many accepted statements"));
        }
        for digest in &ledger.accepted {
            if !is_sha256_digest(digest) {
                return Err(refused(format!("'{digest}' is not a digest")));
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
        merge_entry(&mut self.signer_list, &other.signer_list)?;
        // Digests only the other side holds count as older than ours, so
        // merging never pushes out what this enforcer runs.
        let mut merged: Vec<String> = other
            .accepted
            .iter()
            .filter(|digest| !self.accepted(digest))
            .cloned()
            .collect();
        merged.extend(std::mem::take(&mut self.accepted));
        let excess = merged.len().saturating_sub(MAX_LEDGER_ACCEPTED);
        merged.drain(..excess);
        self.accepted = merged;
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

    /// Whether the install accepted the statement with this digest.
    pub fn accepted(&self, digest: &str) -> bool {
        self.accepted.iter().any(|held| held == digest)
    }

    /// Record an accepted statement's digest. Accepting one again moves it to
    /// the end, so the newest are the last to go.
    pub fn accept(&mut self, digest: &str) {
        self.accept_with(digest, &[]);
    }

    /// Record the digests of the statements running now. They are never
    /// evicted, whatever else the install accepted since: a key rotation must
    /// not stop what runs.
    pub fn accept_running(&mut self, running: &[&str]) {
        let running: Vec<String> = running.iter().map(|digest| digest.to_string()).collect();
        for digest in &running {
            self.accept_with(digest, &running);
        }
    }

    fn accept_with(&mut self, digest: &str, running: &[String]) {
        self.accepted.retain(|held| held != digest);
        self.accepted.push(digest.to_string());
        while self.accepted.len() > MAX_ACCEPTED + running.len() {
            match self
                .accepted
                .iter()
                .position(|held| held != digest && !running.contains(held))
            {
                Some(oldest) => {
                    self.accepted.remove(oldest);
                }
                None => break,
            }
        }
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

fn merge_entry(held: &mut Option<Entry>, other: &Option<Entry>) -> Result<()> {
    let Some(theirs) = other else {
        return Ok(());
    };
    match held {
        Some(mine) if mine.sequence == theirs.sequence && mine.digest != theirs.digest => {
            Err(refused(format!(
                "two different signer lists were accepted with sequence {}",
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

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(n: usize) -> String {
        format!("sha256:{n:064x}")
    }

    #[test]
    fn the_running_release_and_config_survive_forty_config_changes() {
        let mut ledger = Ledger::new("shop-prod");
        let release = digest(1);
        for n in 0..40 {
            let config = digest(100 + n);
            ledger.accept_running(&[release.as_str(), config.as_str()]);
        }
        assert!(ledger.accepted(&release));
        assert!(ledger.accepted(&digest(139)));
        assert!(ledger.accepted.len() <= MAX_ACCEPTED + 2);
        // The oldest configs are gone, the history is bounded.
        assert!(!ledger.accepted(&digest(100)));
    }

    #[test]
    fn accepting_again_moves_a_digest_to_the_end() {
        let mut ledger = Ledger::new("shop-prod");
        ledger.accept(&digest(1));
        for n in 0..MAX_ACCEPTED - 1 {
            ledger.accept(&digest(10 + n));
        }
        ledger.accept(&digest(1));
        ledger.accept(&digest(500));
        assert!(ledger.accepted(&digest(1)));
        assert!(!ledger.accepted(&digest(10)));
    }

    #[test]
    fn merging_never_pushes_out_what_this_side_holds() {
        let mut ours = Ledger::new("shop-prod");
        ours.accept_running(&[digest(1).as_str(), digest(2).as_str()]);
        let mut theirs = Ledger::new("shop-prod");
        for n in 0..MAX_ACCEPTED {
            theirs.accept(&digest(100 + n));
        }
        ours.merge(&theirs).unwrap();
        assert!(ours.accepted(&digest(1)) && ours.accepted(&digest(2)));
    }

    #[test]
    fn a_batch_larger_than_the_history_is_kept_whole() {
        let mut ledger = Ledger::new("shop-prod");
        let running: Vec<String> = (0..40).map(digest).collect();
        let refs: Vec<&str> = running.iter().map(String::as_str).collect();
        ledger.accept_running(&refs);
        assert!(running.iter().all(|d| ledger.accepted(d)));
    }
}
