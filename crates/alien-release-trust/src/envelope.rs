//! The one envelope every statement uses.
//!
//! A signed statement is the payload bytes, base64, beside the signatures over
//! them. Signatures sit outside the payload so nothing is ever re-serialized
//! before it is verified, and no canonical JSON form is needed: whoever stores
//! or forwards a statement carries the two strings unchanged.

use std::fmt;

use alien_error::AlienError;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use chrono::{DateTime, Utc};
use serde::de::{DeserializeOwned, IgnoredAny};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{ErrorData, Result};

/// Largest payload an enforcer decodes. A release with a full stack is well
/// under this.
pub const MAX_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;

/// Most signatures one statement may carry.
pub const MAX_SIGNATURES: usize = 16;

/// What a statement may be about. The `type` strings are neutral, because
/// they are stored in the customer's environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// A release: every workload of one platform, exactly.
    Release,
    /// Developer config values for one deployment.
    Config,
    /// One exec or debug session.
    Session,
    /// An approved operations plan.
    Approval,
    /// One dynamic container spec.
    Dynamic,
    /// One signed command payload.
    Command,
    /// The next version of the signer list.
    SignerList,
    /// A build of an enforcer or other component, signed by the components key.
    Component,
}

/// A key's authority on the signer list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    /// Releases and developer config.
    Release,
    /// Exec and debug sessions, access approvals and plugin runs.
    Operate,
    /// Dynamic container specs and signed command payloads.
    Runtime,
    /// The next version of the signer list.
    Admin,
}

impl Role {
    /// The role's name in JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Release => "release",
            Self::Operate => "operate",
            Self::Runtime => "runtime",
            Self::Admin => "admin",
        }
    }
}

impl Kind {
    /// Every kind.
    pub const ALL: [Kind; 8] = [
        Kind::Release,
        Kind::Config,
        Kind::Session,
        Kind::Approval,
        Kind::Dynamic,
        Kind::Command,
        Kind::SignerList,
        Kind::Component,
    ];

    /// The `type` string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Release => "release/v2",
            Self::Config => "config/v1",
            Self::Session => "session/v1",
            Self::Approval => "approval/v1",
            Self::Dynamic => "dynamic/v1",
            Self::Command => "command/v1",
            Self::SignerList => "signer-list/v1",
            Self::Component => "component/v1",
        }
    }

    /// The kind a `type` string names.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }

    /// Role a signer needs to sign this kind. Components are signed by the
    /// components key in the anchor, not by the signer list.
    pub fn role(self) -> Option<Role> {
        match self {
            Self::Release | Self::Config => Some(Role::Release),
            Self::Session | Self::Approval => Some(Role::Operate),
            Self::Dynamic | Self::Command => Some(Role::Runtime),
            Self::SignerList => Some(Role::Admin),
            Self::Component => None,
        }
    }

    /// Whether statements of this kind name exactly one deployment.
    pub fn is_deployment_bound(self) -> bool {
        matches!(
            self,
            Self::Config | Self::Session | Self::Approval | Self::Dynamic | Self::Command
        )
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The signed payload: a header and a kind-specific body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Envelope<B> {
    /// Statement kind, one of [`Kind`]'s `type` strings.
    #[serde(rename = "type")]
    pub kind: String,
    /// Project the statement belongs to. Absent only on component statements,
    /// which are the same for every project.
    pub project: Option<String>,
    /// Deployment the statement is for, on deployment-bound kinds.
    pub deployment: Option<String>,
    /// Grows with every statement of a kind; for signer lists, the version.
    pub sequence: u64,
    /// When the statement stops being valid, if ever.
    pub not_after: Option<DateTime<Utc>>,
    /// Signer list version the signer took its keys from. For a signer list,
    /// the version that authorizes it: its own for version 1, otherwise the
    /// one before.
    pub signer_list_version: u64,
    /// Kind-specific content.
    pub body: B,
}

impl<B: Serialize> Envelope<B> {
    /// The payload bytes to sign.
    pub fn to_payload(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(|error| {
            AlienError::new(ErrorData::StatementMalformed {
                reason: format!("the statement could not be serialized: {error}"),
            })
        })
    }
}

/// One signature over a statement's payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Signature {
    /// Id of the signer on the signer list, or `components` for the
    /// components key.
    pub signer: String,
    /// Base64 DER ECDSA P-256 signature over the payload bytes.
    pub sig: String,
}

/// A statement as it is stored and carried.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignedStatement {
    /// Base64 of the payload bytes.
    pub payload: String,
    /// Signatures over the payload bytes.
    pub signatures: Vec<Signature>,
}

impl SignedStatement {
    /// An unsigned statement around `payload`.
    pub fn new(payload: &[u8]) -> Self {
        Self {
            payload: STANDARD.encode(payload),
            signatures: Vec::new(),
        }
    }

    /// Parse the JSON form.
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).map_err(|error| malformed(format!("{error}")))
    }

    /// The JSON form.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a signed statement always serializes")
    }

    /// Decode the payload and read its header, without checking signatures.
    pub fn open(&self) -> Result<Opened> {
        if self.payload.len() > MAX_PAYLOAD_BYTES / 3 * 4 + 4 {
            return Err(malformed("the payload is too large"));
        }
        let bytes = STANDARD
            .decode(&self.payload)
            .map_err(|_| malformed("the payload is not valid base64"))?;
        if self.signatures.len() > MAX_SIGNATURES {
            return Err(malformed(format!(
                "the statement carries more than {MAX_SIGNATURES} signatures"
            )));
        }
        let header: Envelope<IgnoredAny> = serde_json::from_slice(&bytes)
            .map_err(|error| malformed(format!("the payload is not a statement: {error}")))?;
        let kind = Kind::parse(&header.kind)
            .ok_or_else(|| malformed(format!("unknown statement type '{}'", header.kind)))?;
        let digest = format!("sha256:{}", hex::encode(Sha256::digest(&bytes)));
        Ok(Opened {
            kind,
            project: header.project,
            deployment: header.deployment,
            sequence: header.sequence,
            not_after: header.not_after,
            signer_list_version: header.signer_list_version,
            digest,
            bytes,
            signatures: self.signatures.clone(),
        })
    }
}

/// A decoded statement whose signatures are not yet checked.
#[derive(Debug, Clone)]
pub struct Opened {
    /// Statement kind.
    pub kind: Kind,
    /// Project the statement names.
    pub project: Option<String>,
    /// Deployment the statement names.
    pub deployment: Option<String>,
    /// Sequence.
    pub sequence: u64,
    /// Expiry.
    pub not_after: Option<DateTime<Utc>>,
    /// Signer list version the signer took its keys from.
    pub signer_list_version: u64,
    /// `sha256:<hex>` of the payload bytes. It identifies the statement exactly.
    pub digest: String,
    /// The payload bytes that were signed.
    pub bytes: Vec<u8>,
    /// Signatures over `bytes`.
    pub signatures: Vec<Signature>,
}

impl Opened {
    /// Read the body as `B`, refusing unknown fields. Parsing the bytes again,
    /// rather than a parsed value, keeps duplicate fields an error.
    pub fn body<B: DeserializeOwned>(&self) -> Result<B> {
        let envelope: Envelope<B> = serde_json::from_slice(&self.bytes).map_err(|error| {
            AlienError::new(ErrorData::BodyRefused {
                kind: self.kind.as_str().to_string(),
                reason: format!("the body does not match the kind: {error}"),
            })
        })?;
        Ok(envelope.body)
    }
}

pub(crate) fn malformed(reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::StatementMalformed {
        reason: reason.into(),
    })
}
