//! The checks every enforcer runs on every statement.

use std::collections::BTreeSet;

use alien_error::AlienError;
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;

use crate::config::ConfigBody;
use crate::envelope::{Kind, Opened, SignedStatement};
use crate::error::{ErrorData, Result};
use crate::ledger::Ledger;
use crate::release::ReleaseBody;
use crate::settings::{InstallId, SigningSettings};
use crate::signer_list::SignerChain;

/// A statement that passed every check, with its body parsed.
#[derive(Debug, Clone)]
pub struct Verified<B> {
    /// Kind.
    pub kind: Kind,
    /// Sequence.
    pub sequence: u64,
    /// `sha256:<hex>` of the payload.
    pub digest: String,
    /// Expiry.
    pub not_after: Option<DateTime<Utc>>,
    /// Whether the install had accepted this statement before, so it was
    /// checked against the signer list version it names, not the current one.
    pub kept: bool,
    /// Signer ids whose signatures counted.
    pub signers: BTreeSet<String>,
    /// The body.
    pub body: B,
}

/// What an enforcer trusts: the settings setup wrote, the install id it
/// derived itself, and the signer chain that grows from the fingerprint.
#[derive(Debug, Clone)]
pub struct Trust {
    settings: SigningSettings,
    install: InstallId,
    chain: SignerChain,
    accepted: BTreeSet<String>,
}

impl Trust {
    /// Trust `chain`, which must start at the settings' fingerprint and reach
    /// at least the signer list version `ledger` holds.
    pub fn new(
        settings: SigningSettings,
        install: InstallId,
        chain: &[SignedStatement],
        ledger: &Ledger,
    ) -> Result<Self> {
        settings.validate()?;
        install.validate()?;
        ledger.check_root(Some(&settings.fingerprint), &install)?;
        let chain = SignerChain::from_statements(&settings.fingerprint, chain)?;
        ledger.check_chain(&chain)?;
        Ok(Self {
            settings,
            install,
            chain,
            accepted: ledger.accepted.iter().cloned().collect(),
        })
    }

    /// The settings.
    pub fn settings(&self) -> &SigningSettings {
        &self.settings
    }

    /// This install.
    pub fn install(&self) -> &InstallId {
        &self.install
    }

    /// The project, as signer list version 1 names it.
    pub fn project(&self) -> &str {
        self.chain.project()
    }

    /// The signer chain.
    pub fn chain(&self) -> &SignerChain {
        &self.chain
    }

    /// Accept newer signer list versions. Returns whether the chain grew.
    pub fn extend_chain(&mut self, statements: &[SignedStatement]) -> Result<bool> {
        self.chain.extend(statements)
    }

    /// Record this trust in `ledger`: the trust root, the chain's version and
    /// the digests of the statements the install now accepts.
    pub fn record(&self, ledger: &mut Ledger, accepted: &[&str]) -> Result<()> {
        ledger.check_root(Some(&self.settings.fingerprint), &self.install)?;
        ledger.check_chain(&self.chain)?;
        ledger.adopt_root(&self.settings.fingerprint, &self.install);
        ledger.record_chain(&self.chain);
        ledger.accept_running(accepted);
        Ok(())
    }

    /// Run the checks for a statement of `kind` and parse its body: project
    /// and install, the statement's own expiry, the deny-list and signatures
    /// by keys that hold the kind's role.
    ///
    /// A new statement is checked against the current signer list. One the
    /// install already accepted (the ledger holds its digest) is checked
    /// against the list version it names, so removing its signer later
    /// doesn't stop it; only the deny-list does. Every enforcer applies this
    /// one rule.
    pub fn verify<B: DeserializeOwned>(
        &self,
        statement: &SignedStatement,
        kind: Kind,
        now: DateTime<Utc>,
    ) -> Result<Verified<B>> {
        if kind == Kind::SignerList {
            return Err(AlienError::new(ErrorData::WrongKind {
                expected: "a statement the signer list signs".to_string(),
                actual: kind.as_str().to_string(),
            }));
        }
        let opened = statement.open()?;
        self.check_header(&opened, kind, now)?;
        let current = self.chain.current();
        let kept = self.accepted.contains(&opened.digest);
        let list = if kept {
            self.chain.version(opened.signer_list_version)
        } else {
            None
        };
        if opened.signer_list_version > current.version || (kept && list.is_none()) {
            return Err(AlienError::new(ErrorData::SignerListMissing {
                needed: opened.signer_list_version,
                current: current.version,
            }));
        }
        if current.refuses(&opened) {
            return Err(AlienError::new(ErrorData::Denied {
                digest: opened.digest,
            }));
        }
        let signers = list
            .unwrap_or(current)
            .check_signatures(&opened, kind.role())?;
        finish(opened, signers, kept)
    }

    /// Verify a release for this install's channel and check its rules.
    pub fn verify_release(
        &self,
        statement: &SignedStatement,
        now: DateTime<Utc>,
    ) -> Result<Verified<ReleaseBody>> {
        let verified: Verified<ReleaseBody> = self.verify(statement, Kind::Release, now)?;
        if verified.body.channel != self.settings.channel {
            return Err(AlienError::new(ErrorData::WrongChannel {
                expected: self.settings.channel.clone(),
                actual: verified.body.channel.clone(),
            }));
        }
        verified.body.validate()?;
        Ok(verified)
    }

    /// Verify a config statement for this install against the release it
    /// configures.
    pub fn verify_config(
        &self,
        statement: &SignedStatement,
        release: &ReleaseBody,
        now: DateTime<Utc>,
    ) -> Result<Verified<ConfigBody>> {
        let verified: Verified<ConfigBody> = self.verify(statement, Kind::Config, now)?;
        verified.body.check(release)?;
        Ok(verified)
    }

    fn check_header(&self, opened: &Opened, kind: Kind, now: DateTime<Utc>) -> Result<()> {
        if opened.kind != kind {
            return Err(AlienError::new(ErrorData::WrongKind {
                expected: kind.as_str().to_string(),
                actual: opened.kind.as_str().to_string(),
            }));
        }
        if opened.project != self.chain.project() {
            return Err(AlienError::new(ErrorData::WrongProject {
                expected: self.chain.project().to_string(),
                actual: opened.project.clone(),
            }));
        }
        let install = self.install.to_string();
        let expected_install = kind.is_install_bound().then_some(install.as_str());
        if opened.install.as_deref() != expected_install {
            return Err(AlienError::new(ErrorData::WrongInstall {
                expected: expected_install.unwrap_or("none").to_string(),
                actual: opened.install.clone().unwrap_or_else(|| "none".to_string()),
            }));
        }
        match opened.not_after {
            Some(not_after) if now > not_after => Err(AlienError::new(ErrorData::Expired {
                not_after: not_after.to_rfc3339(),
            })),
            _ => Ok(()),
        }
    }
}

fn finish<B: DeserializeOwned>(
    opened: Opened,
    signers: BTreeSet<String>,
    kept: bool,
) -> Result<Verified<B>> {
    let body = opened.body()?;
    Ok(Verified {
        kind: opened.kind,
        sequence: opened.sequence,
        digest: opened.digest,
        not_after: opened.not_after,
        kept,
        signers,
        body,
    })
}
