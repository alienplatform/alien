//! What a writer may do in a key's store without holding a cloud identity: rights minted by the
//! binding in the customer's environment and handed to the agent over its control channel.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A grant to write one key, for the push transport, one variant per cloud; the agent receives
/// it on attach and on every renewal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StorageGrant {
    /// S3: a POST policy limited to `<key>/` for packs and manifests, a presigned PUT for
    /// `<key>/HEAD` (sent with `If-Match`), and presigned GETs for what a restore reads.
    #[serde(rename = "s3", rename_all = "camelCase")]
    S3 {
        /// Uploads of packs and manifests.
        post: S3PostPolicy,
        /// Presigned PUT URL of `<key>/HEAD`. The agent adds `If-Match`, which the signature
        /// does not cover, so one URL serves every commit until it expires.
        head_put_url: String,
        /// Presigned GET URLs by object name relative to the key root (the manifest HEAD names
        /// and every pack it locates chunks in).
        reads: BTreeMap<String, String>,
        /// When the earliest of these expires; the binding renews before then.
        expires_at: DateTime<Utc>,
    },
    /// Azure: a user-delegation SAS scoped to the key's directory (`sr=d` on a
    /// hierarchical-namespace account) with read, add, create and write rights. It reaches no
    /// other key, and can neither list nor delete.
    #[serde(rename = "azureSas", rename_all = "camelCase")]
    AzureSas {
        /// `https://<account>.blob.core.windows.net/<container>/<key>/`, ending in `/`.
        key_url: String,
        /// The SAS query string, without the leading `?`.
        sas: SasToken,
        /// When the SAS (and the delegation key it is signed with) expires.
        expires_at: DateTime<Utc>,
    },
}

/// A SAS query string. A credential: serialized as is, never printed.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SasToken(pub String);

impl std::fmt::Debug for SasToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SasToken(..)")
    }
}

impl StorageGrant {
    /// When these rights expire.
    pub fn expires_at(&self) -> DateTime<Utc> {
        match self {
            Self::S3 { expires_at, .. } | Self::AzureSas { expires_at, .. } => *expires_at,
        }
    }
}

/// An S3 browser-upload (POST) policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct S3PostPolicy {
    /// The bucket's endpoint the form is posted to.
    pub url: String,
    /// Form fields to send before the file (`policy`, `x-amz-*`), as signed.
    pub fields: BTreeMap<String, String>,
    /// Object key prefix the policy allows (`<key>/`); the upload's `key` is this plus the
    /// object name.
    pub key_prefix: String,
}
