//! The developer's signer list and its chain of versions.
//!
//! Version 1 is pinned by its fingerprint, the SHA-256 of its payload bytes,
//! which the customer's setup writes into the anchor. Every later version
//! names the digest of the one before and is signed by `adminThreshold` admin
//! keys of the one before. Whoever carries the chain can withhold a version,
//! but can't change one or add one.

use std::collections::{BTreeMap, BTreeSet};

use alien_error::AlienError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::envelope::{Kind, Opened, Role, SignedStatement};
use crate::error::{ErrorData, Result};
use crate::key::PublicKey;

/// Most signers one list may hold.
pub const MAX_SIGNERS: usize = 256;

/// Where a signer's key lives. It is shown to admins and gives no authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SignerKind {
    /// A CI key, in a CI secret or a cloud KMS.
    Ci,
    /// A key in a laptop's secure hardware.
    Device,
    /// A key on a laptop with no secure hardware, stored in a file.
    Software,
    /// The offline recovery key.
    Recovery,
    /// A key held by the developer's backend, for dynamic containers and
    /// signed commands.
    Service,
}

/// One key on the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Signer {
    /// Unique id, such as `ci` or `alice-macbook`.
    pub id: String,
    /// `ecdsa-p256:<base64 SPKI>`.
    pub key: String,
    /// Where the key lives.
    pub kind: SignerKind,
    /// What the key may sign.
    pub roles: Vec<Role>,
    /// Display label, such as the engineer's email.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// The body of a `signer-list/v1` statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignerListBody {
    /// Digest of the previous version's payload; absent on version 1.
    pub previous: Option<String>,
    /// How many admin keys of this version must sign the next one.
    pub admin_threshold: u32,
    /// The keys.
    pub signers: Vec<Signer>,
    /// Digests of statements that must be refused even though they verify.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<String>,
}

/// One trusted version, with its keys parsed.
#[derive(Debug, Clone)]
pub struct TrustedList {
    /// Version.
    pub version: u64,
    /// Digest of the version's payload.
    pub digest: String,
    /// When the list stops being valid, if ever.
    pub not_after: Option<DateTime<Utc>>,
    /// The body.
    pub body: SignerListBody,
    keys: BTreeMap<String, (PublicKey, BTreeSet<Role>)>,
}

impl TrustedList {
    /// The key and roles of signer `id`.
    pub fn signer(&self, id: &str) -> Option<(&PublicKey, &BTreeSet<Role>)> {
        self.keys.get(id).map(|(key, roles)| (key, roles))
    }

    /// Whether a statement digest is denied.
    pub fn denies(&self, digest: &str) -> bool {
        self.body.deny.iter().any(|denied| denied == digest)
    }

    /// Check every signature on `opened` against this list. Each must be by a
    /// signer on the list holding `role`; returns the distinct signer ids.
    /// Any signature that doesn't count refuses the whole statement, so a
    /// statement never passes on a signature nobody could check.
    pub fn check_signatures(&self, opened: &Opened, role: Role) -> Result<BTreeSet<String>> {
        let mut signers = BTreeSet::new();
        for signature in &opened.signatures {
            let (key, roles) = self.signer(&signature.signer).ok_or_else(|| {
                refused_signature(
                    &signature.signer,
                    format!("not on signer list version {}", self.version),
                )
            })?;
            if !roles.contains(&role) {
                return Err(refused_signature(
                    &signature.signer,
                    format!("does not hold the '{}' role", role.as_str()),
                ));
            }
            key.verify(&opened.bytes, &signature.sig)
                .map_err(|error| refused_signature(&signature.signer, error.message))?;
            if !signers.insert(signature.signer.clone()) {
                return Err(refused_signature(
                    &signature.signer,
                    "signed twice".to_string(),
                ));
            }
        }
        if signers.is_empty() {
            return Err(AlienError::new(ErrorData::Unsigned {
                role: role.as_str().to_string(),
            }));
        }
        Ok(signers)
    }

    fn admin_count(&self) -> usize {
        self.keys
            .values()
            .filter(|(_, roles)| roles.contains(&Role::Admin))
            .count()
    }
}

/// The chain of signer lists an enforcer trusts, from the pinned version 1
/// to the highest version it has accepted.
#[derive(Debug, Clone)]
pub struct SignerChain {
    project: String,
    statements: Vec<SignedStatement>,
    digests: Vec<String>,
    current: TrustedList,
}

impl SignerChain {
    /// Trust version 1, which must hash to `fingerprint` (`SHA256:<hex>`).
    pub fn genesis(project: &str, fingerprint: &str, statement: &SignedStatement) -> Result<Self> {
        let opened = statement.open()?;
        let version = opened.sequence;
        let expected = fingerprint_digest(fingerprint)?;
        if opened.digest != expected {
            return Err(list_refused(
                version,
                format!(
                    "its digest {} does not match the pinned fingerprint {fingerprint}",
                    opened.digest
                ),
            ));
        }
        let list = parse_list(project, &opened, 1)?;
        if list.body.previous.is_some() {
            return Err(list_refused(1, "version 1 names a previous version"));
        }
        let signers = list.check_signatures(&opened, Role::Admin)?;
        if signers.len() < list.body.admin_threshold as usize {
            return Err(list_refused(
                1,
                format!(
                    "signed by {} of its own admins, below its threshold of {}",
                    signers.len(),
                    list.body.admin_threshold
                ),
            ));
        }
        Ok(Self {
            project: project.to_string(),
            statements: vec![statement.clone()],
            digests: vec![opened.digest],
            current: list,
        })
    }

    /// Rebuild a chain an enforcer stored, verifying every link again.
    pub fn from_statements(
        project: &str,
        fingerprint: &str,
        statements: &[SignedStatement],
    ) -> Result<Self> {
        let (first, rest) = statements
            .split_first()
            .ok_or_else(|| list_refused(1, "the chain is empty"))?;
        let mut chain = Self::genesis(project, fingerprint, first)?;
        chain.extend(rest)?;
        Ok(chain)
    }

    /// The highest trusted version.
    pub fn current(&self) -> &TrustedList {
        &self.current
    }

    /// The project the chain belongs to.
    pub fn project(&self) -> &str {
        &self.project
    }

    /// Every trusted version's statement, oldest first, for storage.
    pub fn statements(&self) -> &[SignedStatement] {
        &self.statements
    }

    /// Accept whatever `statements` adds beyond the current version.
    ///
    /// Versions already trusted are skipped when their bytes match and refused
    /// when they don't, so a second, different version N is reported as a
    /// fork rather than ignored. The rest must follow on in order. Returns
    /// whether the chain grew.
    pub fn extend(&mut self, statements: &[SignedStatement]) -> Result<bool> {
        let mut opened = statements
            .iter()
            .map(|statement| statement.open().map(|opened| (opened, statement)))
            .collect::<Result<Vec<_>>>()?;
        opened.sort_by_key(|(opened, _)| opened.sequence);
        let before = self.current.version;
        for (opened, statement) in opened {
            if opened.kind != Kind::SignerList {
                return Err(list_refused(
                    opened.sequence,
                    format!("it is a '{}' statement", opened.kind),
                ));
            }
            let version = opened.sequence;
            if version <= self.current.version {
                let known = version
                    .checked_sub(1)
                    .and_then(|index| self.digests.get(index as usize));
                if known != Some(&opened.digest) {
                    return Err(list_refused(
                        version,
                        "a different list with this version is already trusted",
                    ));
                }
                continue;
            }
            self.advance(&opened, statement)?;
        }
        Ok(self.current.version > before)
    }

    fn advance(&mut self, opened: &Opened, statement: &SignedStatement) -> Result<()> {
        let version = opened.sequence;
        if version != self.current.version + 1 {
            return Err(list_refused(
                version,
                format!(
                    "version {} is missing; the chain is at version {}",
                    self.current.version + 1,
                    self.current.version
                ),
            ));
        }
        let next = parse_list(&self.project, opened, self.current.version)?;
        if next.body.previous.as_deref() != Some(self.current.digest.as_str()) {
            return Err(list_refused(
                version,
                format!("it does not name version {}'s digest", self.current.version),
            ));
        }
        let signers = self.current.check_signatures(opened, Role::Admin)?;
        let threshold = self.current.body.admin_threshold as usize;
        if signers.len() < threshold {
            return Err(list_refused(
                version,
                format!(
                    "signed by {} admins of version {}, below its threshold of {threshold}",
                    signers.len(),
                    self.current.version
                ),
            ));
        }
        self.statements.push(statement.clone());
        self.digests.push(opened.digest.clone());
        self.current = next;
        Ok(())
    }
}

/// The fingerprint that pins a version 1, from its digest.
pub fn fingerprint(digest: &str) -> String {
    format!(
        "SHA256:{}",
        digest.strip_prefix("sha256:").unwrap_or(digest)
    )
}

fn fingerprint_digest(fingerprint: &str) -> Result<String> {
    let hex_part = fingerprint
        .strip_prefix("SHA256:")
        .filter(|hex_part| {
            hex_part.len() == 64
                && hex_part
                    .bytes()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        })
        .ok_or_else(|| {
            AlienError::new(ErrorData::AnchorInvalid {
                reason: format!("'{fingerprint}' is not SHA256:<64 lowercase hex>"),
            })
        })?;
    Ok(format!("sha256:{hex_part}"))
}

fn parse_list(project: &str, opened: &Opened, authorizing_version: u64) -> Result<TrustedList> {
    let version = opened.sequence;
    if opened.kind != Kind::SignerList {
        return Err(list_refused(
            version,
            format!("it is a '{}' statement", opened.kind),
        ));
    }
    if opened.project.as_deref() != Some(project) {
        return Err(AlienError::new(ErrorData::WrongProject {
            expected: project.to_string(),
            actual: opened.project.clone().unwrap_or_else(|| "none".to_string()),
        }));
    }
    if opened.deployment.is_some() {
        return Err(list_refused(version, "a signer list names no deployment"));
    }
    if version == 0 {
        return Err(list_refused(version, "versions start at 1"));
    }
    if opened.signer_list_version != authorizing_version {
        return Err(list_refused(
            version,
            format!(
                "it says it is authorized by version {}, not {authorizing_version}",
                opened.signer_list_version
            ),
        ));
    }
    let body: SignerListBody = opened.body()?;
    if body.signers.len() > MAX_SIGNERS {
        return Err(list_refused(
            version,
            format!("more than {MAX_SIGNERS} signers"),
        ));
    }
    let mut keys = BTreeMap::new();
    let mut key_texts = BTreeSet::new();
    for signer in &body.signers {
        if !valid_signer_id(&signer.id) {
            return Err(list_refused(
                version,
                format!("signer id '{}' is not 1-64 of [a-z0-9-]", signer.id),
            ));
        }
        let key = PublicKey::parse(&signer.key).map_err(|error| {
            list_refused(
                version,
                format!("signer '{}': {}", signer.id, error.message),
            )
        })?;
        if !key_texts.insert(key.as_str().to_string()) {
            return Err(list_refused(
                version,
                format!("signer '{}' reuses a key already on the list", signer.id),
            ));
        }
        let roles: BTreeSet<Role> = signer.roles.iter().copied().collect();
        if roles.is_empty() || roles.len() != signer.roles.len() {
            return Err(list_refused(
                version,
                format!("signer '{}' has no roles or repeats one", signer.id),
            ));
        }
        if keys.insert(signer.id.clone(), (key, roles)).is_some() {
            return Err(list_refused(
                version,
                format!("signer id '{}' appears twice", signer.id),
            ));
        }
    }
    let list = TrustedList {
        version,
        digest: opened.digest.clone(),
        not_after: opened.not_after,
        body,
        keys,
    };
    let admins = list.admin_count();
    let threshold = list.body.admin_threshold as usize;
    if threshold == 0 || threshold > admins {
        return Err(list_refused(
            version,
            format!("admin threshold {threshold} must be between 1 and its {admins} admins"),
        ));
    }
    Ok(list)
}

fn valid_signer_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-'))
}

fn list_refused(version: u64, reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::SignerListRefused {
        version,
        reason: reason.into(),
    })
}

fn refused_signature(signer: &str, reason: String) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::SignatureRefused {
        signer: signer.to_string(),
        reason,
    })
}
