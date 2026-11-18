//! The credentials of the steps a manager's deployment loop runs in an extension's controller
//! module: minted for each step, for the identity setup created for that extension in that
//! deployment. The module never gets them: its host signs the module's cloud calls with them.

use std::sync::Arc;

use alien_core::{ExtensionManifest, ServiceAccount, ServiceAccountOutputs};
use alien_error::{AlienError, Context};
use alien_infra::step_protocol::protocol::Credentials;
use alien_infra::step_protocol::{CredentialSource, HOST_GRACE, STEP_DEADLINE};
use alien_infra::ResourceControllerContext;
use async_trait::async_trait;

use crate::traits::{CredentialResolver, DeploymentRecord, ExtensionIdentity};

/// The source of each step's credentials for `extension`'s controller in `deployment`, minted
/// through `credential_resolver`.
pub(crate) fn identity_credentials(
    credential_resolver: Arc<dyn CredentialResolver>,
    deployment: &DeploymentRecord,
    extension: &ExtensionManifest,
) -> Arc<dyn CredentialSource> {
    Arc::new(IdentityCredentials {
        credential_resolver,
        deployment: deployment.clone(),
        extension: extension.clone(),
    })
}

/// Credentials minted for each step, for the identity setup created for one extension in one
/// deployment.
struct IdentityCredentials {
    credential_resolver: Arc<dyn CredentialResolver>,
    deployment: DeploymentRecord,
    extension: ExtensionManifest,
}

impl std::fmt::Debug for IdentityCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentityCredentials")
            .field("deployment", &self.deployment.id)
            .field("extension", &self.extension.name)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl CredentialSource for IdentityCredentials {
    async fn credentials(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> alien_infra::Result<Option<Credentials>> {
        if !alien_infra::step_protocol::has_extension_identity(ctx, &self.extension.identity_id()) {
            return Ok(None);
        }
        let failed = |message: String| alien_infra::ErrorData::AuthenticationFailed {
            message,
            method: Some("extension_identity".to_string()),
        };
        let identity = self
            .identity(ctx)
            .map_err(|message| AlienError::new(failed(message)))?;
        let credentials = self
            .credential_resolver
            .resolve_extension_credentials(&self.deployment, &identity)
            .await
            .context(failed(format!(
                "cannot mint credentials for the identity of extension '{}'",
                self.extension.name
            )))?;
        let expires_at = match &credentials {
            Credentials::Aws { expires_at, .. } => expires_at.as_deref(),
            Credentials::Gcp { expires_at, .. } | Credentials::Azure { expires_at, .. } => {
                Some(expires_at.as_str())
            }
        };
        let Some(expires_at) =
            expires_at.and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
        else {
            return Err(AlienError::new(failed(format!(
                "the credentials minted for extension '{}' have no readable expiry",
                self.extension.name
            ))));
        };
        let lasts_the_step = expires_at
            > chrono::Utc::now()
                + chrono::Duration::seconds((STEP_DEADLINE + HOST_GRACE).as_secs() as i64);
        if !lasts_the_step {
            return Err(AlienError::new(failed(format!(
                "the credentials minted for extension '{}' expire before the step's deadline",
                self.extension.name
            ))));
        }
        Ok(Some(credentials))
    }
}

impl IdentityCredentials {
    /// The extension's identity as the deployment's state holds it now: the service account
    /// setup created for the extension, once it has outputs.
    fn identity(&self, ctx: &ResourceControllerContext<'_>) -> Result<ExtensionIdentity, String> {
        let id = self.extension.identity_id();
        let missing = || {
            format!(
                "the identity of extension '{}' ('{id}') is not set up in this deployment; run its initial setup again",
                self.extension.name
            )
        };
        let state = ctx.state.resources.get(&id).ok_or_else(missing)?;
        let created_for_it = state
            .config
            .downcast_ref::<ServiceAccount>()
            .is_some_and(|account| account.extension.as_deref() == Some(&self.extension.name));
        if !created_for_it {
            return Err(missing());
        }
        let outputs = state
            .outputs
            .as_ref()
            .and_then(|outputs| outputs.downcast_ref::<ServiceAccountOutputs>())
            .ok_or_else(missing)?;
        Ok(ExtensionIdentity {
            extension: self.extension.name.clone(),
            id,
            identity: outputs.identity.clone(),
        })
    }
}
