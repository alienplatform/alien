//! Version-one signed policy records. The short-lived admin envelope is verified separately.
use crate::{
    error::{denied, ErrorData, Result},
    policy::{canonical, Policy},
};
use alien_error::AlienError;
use ed25519_compact::PublicKey;
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, collections::HashMap};

pub const MAX_RECORD_BYTES: usize = 384 * 1024;
pub const TOMBSTONE_RETENTION_MS: u64 = 25 * 60 * 60 * 1000;
pub type Version = (u64, String);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Claims {
    pub v: u8,
    pub aud: String,
    pub sub: String,
    pub ver: Version,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<Policy>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub deleted: bool,
}
fn is_false(value: &bool) -> bool {
    !value
}
#[derive(Clone)]
pub struct Record {
    pub signed: String,
    pub claims: Claims,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Mutation {
    pub record: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub aud: String,
    pub sub: String,
    pub ver: Version,
}

/// The same ordering is used by management writes, snapshots and anti-entropy.
pub fn compare_versions(left: &Version, right: &Version) -> Ordering {
    left.cmp(right)
}
/// Pure merge decision shared by the store and independent convergence tests.
pub fn should_replace(current: Option<&Version>, incoming: &Version) -> bool {
    current.is_none_or(|version| compare_versions(incoming, version).is_gt())
}
impl Record {
    pub fn verify(
        signed: String,
        keys: &HashMap<String, PublicKey>,
        audiences: &[String],
        now_ms: u64,
    ) -> Result<Self> {
        let claims: Claims = crate::token::verify_signed_limit(
            &signed,
            keys,
            Some("alien-egress-record"),
            MAX_RECORD_BYTES,
        )?;
        if claims.v != 1 {
            tracing::warn!(version = claims.v, "unsupported policy record format");
            return Err(AlienError::new(ErrorData::RecordFormatUnsupported));
        }
        if claims.aud.is_empty()
            || claims.sub.is_empty()
            || claims.aud.len() > 256
            || claims.sub.len() > 256
            || !audiences.contains(&claims.aud)
            || claims.ver.1.len() != 32
            || !claims.ver.1.bytes().all(|byte| byte.is_ascii_hexdigit())
            || claims.policy.is_some() == claims.deleted
        {
            return Err(denied("invalid policy record identity or payload"));
        }
        if claims.ver.0 > now_ms.saturating_add(60_000) {
            return Err(AlienError::new(ErrorData::VersionInFuture));
        }
        if let Some(policy) = &claims.policy {
            canonical(policy)?;
        }
        Ok(Self { signed, claims })
    }
}
