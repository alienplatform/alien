//! Extension bundle objects in the manager's state directory: `extensions/sha256-<hex>`.

use std::path::PathBuf;

use async_trait::async_trait;

use alien_error::{AlienError, Context, GenericError, IntoAlienError};

use crate::auth::Subject;
use crate::traits::{BundleObject, ExtensionBundleStore};

/// Bundle objects as files under `<state dir>/extensions/`, one per sha256.
pub struct StateDirExtensionBundles {
    dir: PathBuf,
}

impl StateDirExtensionBundles {
    /// The store under `state_dir`.
    pub fn new(state_dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: state_dir.into().join("extensions"),
        }
    }

    fn path(&self, sha256: &str) -> PathBuf {
        self.dir.join(format!("sha256-{sha256}"))
    }
}

#[async_trait]
impl ExtensionBundleStore for StateDirExtensionBundles {
    async fn put(&self, _caller: &Subject, sha256: &str, bytes: Vec<u8>) -> Result<(), AlienError> {
        let path = self.path(sha256);
        let part = self.dir.join(format!("sha256-{sha256}.part"));
        let failed = |operation: &str| GenericError {
            message: format!(
                "Failed to {operation} extension bundle object {}",
                path.display()
            ),
        };
        tokio::fs::create_dir_all(&self.dir)
            .await
            .into_alien_error()
            .context(failed("create the folder of"))?;
        // Written whole, then renamed: a reader sees the complete object or none.
        tokio::fs::write(&part, &bytes)
            .await
            .into_alien_error()
            .context(failed("write"))?;
        tokio::fs::rename(&part, &path)
            .await
            .into_alien_error()
            .context(failed("store"))?;
        Ok(())
    }

    /// A standalone manager has one store for its projects: a digest names the same bytes in
    /// any of them.
    async fn get(
        &self,
        _caller: &Subject,
        _project_id: &str,
        sha256: &str,
    ) -> Result<Option<BundleObject>, AlienError> {
        match tokio::fs::read(self.path(sha256)).await {
            Ok(bytes) => Ok(Some(BundleObject::Bytes(bytes))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).into_alien_error().context(GenericError {
                message: format!("Failed to read extension bundle object sha256-{sha256}"),
            }),
        }
    }
}
