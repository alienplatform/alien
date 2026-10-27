//! Selects the Operator image advertised to a connected deployment.

use alien_error::AlienError;
use async_trait::async_trait;

use crate::{auth::Subject, traits::DeploymentRecord};

/// Resolves an image only after the caller is authorized to sync this deployment.
/// Return `None` to leave the installed Operator unchanged. Images should be
/// pinned by digest so validation and installation use the same bytes.
#[async_trait]
pub trait OperatorImageProvider: Send + Sync {
    async fn target_image(
        &self,
        subject: &Subject,
        deployment: &DeploymentRecord,
    ) -> Result<Option<String>, AlienError>;
}
