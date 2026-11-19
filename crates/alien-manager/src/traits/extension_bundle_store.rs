//! Where a manager keeps the objects of extension bundles: each core zip and controller binary
//! by its project and sha256.
//!
//! A standalone manager stores them under its state directory
//! ([`crate::providers::StateDirExtensionBundles`]). An embedder whose bundles live in an object
//! store plugs in its own, which answers a download with a URL.

use async_trait::async_trait;

use alien_error::AlienError;

use crate::auth::Subject;

/// A stored bundle object, as a download gets it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleObject {
    /// The object's bytes.
    Bytes(Vec<u8>),
    /// A URL the caller downloads the object from.
    Url(String),
}

/// Objects of extension bundles by project and sha256, as 64 lowercase hex characters. A project
/// reads only the objects stored under it, so knowing another project's digest reads nothing.
#[async_trait]
pub trait ExtensionBundleStore: Send + Sync {
    /// Stores `bytes` under `sha256` in `project_id`. The caller has checked that they hash to
    /// it, so a second put of the same digest stores the same bytes.
    async fn put(
        &self,
        caller: &Subject,
        project_id: &str,
        sha256: &str,
        bytes: Vec<u8>,
    ) -> Result<(), AlienError>;

    /// The object stored under `sha256` in `project_id`, or where `caller` downloads it. `None`
    /// when there is none.
    async fn get(
        &self,
        caller: &Subject,
        project_id: &str,
        sha256: &str,
    ) -> Result<Option<BundleObject>, AlienError>;
}
