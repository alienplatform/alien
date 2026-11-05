//! The checks every enforcer runs on every statement.

use std::collections::BTreeSet;

use alien_error::AlienError;
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;

use crate::anchor::Anchor;
use crate::envelope::{Kind, Opened, SignedStatement};
use crate::error::{ErrorData, Result};
use crate::signer_list::SignerChain;

/// Signer id a component statement's signature names.
pub const COMPONENTS_SIGNER: &str = "components";

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
    /// Signer ids whose signatures counted.
    pub signers: BTreeSet<String>,
    /// The body.
    pub body: B,
}

/// What an enforcer trusts: the anchor its setup wrote and the signer chain
/// that grows from the anchor's fingerprint.
#[derive(Debug, Clone)]
pub struct Trust {
    anchor: Anchor,
    chain: SignerChain,
}

impl Trust {
    /// Trust `chain`, which must start at the anchor's fingerprint.
    pub fn new(anchor: Anchor, chain: &[SignedStatement]) -> Result<Self> {
        anchor.validate()?;
        let chain = SignerChain::from_statements(&anchor.project, &anchor.signing, chain)?;
        Ok(Self { anchor, chain })
    }

    /// The anchor.
    pub fn anchor(&self) -> &Anchor {
        &self.anchor
    }

    /// The signer chain.
    pub fn chain(&self) -> &SignerChain {
        &self.chain
    }

    /// Accept newer signer list versions. Returns whether the chain grew.
    pub fn extend_chain(&mut self, statements: &[SignedStatement]) -> Result<bool> {
        self.chain.extend(statements)
    }

    /// Run the checks for a statement of `kind` and parse its body:
    /// project and deployment, expiry, deny-list, and signatures by keys on
    /// the current signer list that hold the kind's role. The caller checks
    /// the sequence floor, because only it knows which floor applies.
    pub fn verify<B: DeserializeOwned>(
        &self,
        statement: &SignedStatement,
        kind: Kind,
        now: DateTime<Utc>,
    ) -> Result<Verified<B>> {
        let opened = statement.open()?;
        self.check_header(&opened, kind, now)?;
        let list = self.chain.current();
        if let Some(not_after) = list.not_after {
            if now > not_after {
                return Err(AlienError::new(ErrorData::SignerListRefused {
                    version: list.version,
                    reason: format!("it expired at {}", not_after.to_rfc3339()),
                }));
            }
        }
        if opened.signer_list_version > list.version {
            return Err(AlienError::new(ErrorData::SignerListMissing {
                needed: opened.signer_list_version,
                current: list.version,
            }));
        }
        if list.denies(&opened.digest) {
            return Err(AlienError::new(ErrorData::Denied {
                digest: opened.digest,
            }));
        }
        let role = kind
            .role()
            .expect("kinds signed by the signer list have a role");
        let signers = list.check_signatures(&opened, role)?;
        finish(opened, signers)
    }

    fn check_header(&self, opened: &Opened, kind: Kind, now: DateTime<Utc>) -> Result<()> {
        if kind == Kind::Component || kind == Kind::SignerList {
            return Err(AlienError::new(ErrorData::WrongKind {
                expected: "a statement the signer list signs".to_string(),
                actual: kind.as_str().to_string(),
            }));
        }
        check_kind(opened, kind)?;
        if opened.project.as_deref() != Some(self.anchor.project.as_str()) {
            return Err(AlienError::new(ErrorData::WrongProject {
                expected: self.anchor.project.clone(),
                actual: opened.project.clone().unwrap_or_else(|| "none".to_string()),
            }));
        }
        let expected_deployment = kind
            .is_deployment_bound()
            .then_some(self.anchor.deployment.as_str());
        if opened.deployment.as_deref() != expected_deployment {
            return Err(AlienError::new(ErrorData::WrongDeployment {
                expected: expected_deployment.unwrap_or("none").to_string(),
                actual: opened
                    .deployment
                    .clone()
                    .unwrap_or_else(|| "none".to_string()),
            }));
        }
        check_expiry(opened, now)
    }
}

/// Verify a component statement against `components_key` alone, for an
/// enforcer that checks its own update before it has an anchor.
pub fn verify_component<B: DeserializeOwned>(
    components_key: &str,
    statement: &SignedStatement,
    now: DateTime<Utc>,
) -> Result<Verified<B>> {
    let key = crate::key::PublicKey::parse(components_key)?;
    let opened = statement.open()?;
    check_kind(&opened, Kind::Component)?;
    if opened.project.is_some() || opened.deployment.is_some() {
        return Err(AlienError::new(ErrorData::BodyRefused {
            kind: Kind::Component.as_str().to_string(),
            reason: "a component statement names no project or deployment".to_string(),
        }));
    }
    check_expiry(&opened, now)?;
    let mut signers = BTreeSet::new();
    for signature in &opened.signatures {
        if signature.signer != COMPONENTS_SIGNER {
            return Err(AlienError::new(ErrorData::SignatureRefused {
                signer: signature.signer.clone(),
                reason: "only the components key signs components".to_string(),
            }));
        }
        key.verify(&opened.bytes, &signature.sig).map_err(|error| {
            AlienError::new(ErrorData::SignatureRefused {
                signer: signature.signer.clone(),
                reason: error.message,
            })
        })?;
        signers.insert(signature.signer.clone());
    }
    if signers.is_empty() {
        return Err(AlienError::new(ErrorData::Unsigned {
            role: COMPONENTS_SIGNER.to_string(),
        }));
    }
    finish(opened, signers)
}

fn check_kind(opened: &Opened, kind: Kind) -> Result<()> {
    if opened.kind != kind {
        return Err(AlienError::new(ErrorData::WrongKind {
            expected: kind.as_str().to_string(),
            actual: opened.kind.as_str().to_string(),
        }));
    }
    Ok(())
}

fn check_expiry(opened: &Opened, now: DateTime<Utc>) -> Result<()> {
    match opened.not_after {
        Some(not_after) if now > not_after => Err(AlienError::new(ErrorData::Expired {
            not_after: not_after.to_rfc3339(),
        })),
        _ => Ok(()),
    }
}

fn finish<B: DeserializeOwned>(opened: Opened, signers: BTreeSet<String>) -> Result<Verified<B>> {
    let body = opened.body()?;
    Ok(Verified {
        kind: opened.kind,
        sequence: opened.sequence,
        digest: opened.digest,
        not_after: opened.not_after,
        signers,
        body,
    })
}
