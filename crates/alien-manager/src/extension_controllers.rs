//! How the deployment loop steps a custom resource whose extension runs its controller on the
//! developer's server: `POST /v1/step` to the `controller-url` the release's bundle pins, with
//! a token this manager signs, and credentials minted for the extension's identity for each step.
//!
//! Each deployment loop run builds its own proxies, so two deployments with the same extension
//! reach the controller server their own releases pin.

use std::sync::Arc;

use alien_core::{ExtensionManifest, ServiceAccount, ServiceAccountOutputs};
use alien_error::{AlienError, Context};
use alien_infra::extension_bundle::ExtensionBundle;
use alien_infra::step_protocol::protocol::Credentials;
use alien_infra::step_protocol::{
    CredentialSource, HttpTransport, ProxyFactory, TokenSigner, Transport, TransportFailure,
    HOST_GRACE, STEP_DEADLINE,
};
use alien_infra::ResourceControllerContext;
use async_trait::async_trait;
use ed25519_compact::KeyPair;

use crate::traits::{CredentialResolver, DeploymentRecord, ExtensionIdentity};

/// Where a manager publishes the public keys of its step tokens, as a JWKS document. A
/// controller's server fetches it from the manager it takes steps from.
pub const STEP_TOKEN_JWKS_PATH: &str = "/.well-known/alien-step-jwks.json";

/// What a manager's deployment loop reaches extensions' controller servers with.
pub struct ExtensionControllers {
    /// The `iss` of every token: the manager's own URL, without a trailing `/`, under which its
    /// JWKS is published. A controller's server compares it to its `--issuer` the same way.
    issuer: String,
    key: Arc<KeyPair>,
    credential_resolver: Arc<dyn CredentialResolver>,
    /// Admits a controller server on this machine, with these roots for its certificate.
    #[cfg(test)]
    local: Option<Vec<reqwest::Certificate>>,
}

impl ExtensionControllers {
    pub fn new(
        issuer: String,
        key: Arc<KeyPair>,
        credential_resolver: Arc<dyn CredentialResolver>,
    ) -> Self {
        Self {
            issuer: issuer.trim_end_matches('/').to_string(),
            key,
            credential_resolver,
            #[cfg(test)]
            local: None,
        }
    }

    /// The `iss` of every token this manager signs.
    #[cfg(test)]
    pub(crate) fn issuer(&self) -> &str {
        &self.issuer
    }

    #[cfg(test)]
    pub(crate) fn allowing_local(mut self, roots: Vec<reqwest::Certificate>) -> Self {
        self.local = Some(roots);
        self
    }

    /// How `deployment`'s proxies reach the controller server of `extension`, or `None` when
    /// its bundle names no server, so its custom resources fail naming the extension.
    pub(crate) fn factory(
        &self,
        deployment: &DeploymentRecord,
        extension: &ExtensionManifest,
        bundle: &ExtensionBundle,
    ) -> Option<ProxyFactory> {
        let controller_url = bundle.manifest().controller_url.clone()?;
        let transport = HttpTransport::new(
            controller_url,
            TokenSigner {
                issuer: self.issuer.clone(),
                project: deployment.project_id.clone(),
                secret_key: self.key.sk.clone(),
            },
        );
        #[cfg(test)]
        let transport = match &self.local {
            Some(roots) => transport.allowing_local(roots.clone()),
            None => transport,
        };
        Some(ProxyFactory::new(
            deployment.id.clone(),
            Arc::new(transport),
            Some(Arc::new(IdentityCredentials {
                credential_resolver: self.credential_resolver.clone(),
                deployment: deployment.clone(),
                extension: extension.clone(),
            })),
        ))
    }
}

/// The code of a step a manager without a step-token key can't send.
pub const STEP_TOKEN_KEY_MISSING: &str = "EXTENSION_STEP_TOKEN_KEY_MISSING";

/// How `deployment`'s proxies fail for `bundle`'s controller server on a manager that has no
/// step-token key: each step says why, asks for no credentials and calls nothing. `None` when
/// the bundle names no server.
pub(crate) fn unsigned_factory(
    deployment: &DeploymentRecord,
    bundle: &ExtensionBundle,
) -> Option<ProxyFactory> {
    bundle.manifest().controller_url.as_ref()?;
    Some(ProxyFactory::new(
        deployment.id.clone(),
        Arc::new(NoStepTokenKey),
        None,
    ))
}

/// The transport of a manager with no step-token key.
#[derive(Debug)]
struct NoStepTokenKey;

impl NoStepTokenKey {
    fn failure() -> TransportFailure {
        TransportFailure {
            code: STEP_TOKEN_KEY_MISSING,
            message: "this manager has no step-token signing key, so it can't call an extension's controller server; give the manager one".to_string(),
            retryable: false,
        }
    }
}

#[async_trait]
impl Transport for NoStepTokenKey {
    async fn ready(&self) -> Result<(), TransportFailure> {
        Err(Self::failure())
    }

    async fn step(
        &self,
        _request: &alien_infra::step_protocol::protocol::StepRequest,
        _timeout: std::time::Duration,
    ) -> Result<alien_infra::step_protocol::protocol::StepResponse, TransportFailure> {
        Err(Self::failure())
    }
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
    ) -> alien_infra::Result<Credentials> {
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
        let lasts_the_step = expires_at
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
            .is_some_and(|at| {
                at > chrono::Utc::now()
                    + chrono::Duration::seconds((STEP_DEADLINE + HOST_GRACE).as_secs() as i64)
            });
        if !lasts_the_step {
            return Err(AlienError::new(failed(format!(
                "the credentials minted for extension '{}' expire before the step's deadline",
                self.extension.name
            ))));
        }
        Ok(credentials)
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

/// The JWKS document of `key`, as [`STEP_TOKEN_JWKS_PATH`] serves it.
pub fn step_token_jwks(key: &KeyPair) -> serde_json::Value {
    serde_json::json!({ "keys": [alien_infra::step_protocol::token::jwk(&key.pk)] })
}

/// Serves the JWKS of `key` at [`STEP_TOKEN_JWKS_PATH`], to anyone: it holds only public keys.
pub fn jwks_router(key: &KeyPair) -> axum::Router {
    let jwks = step_token_jwks(key);
    axum::Router::new().route(
        STEP_TOKEN_JWKS_PATH,
        axum::routing::get(move || async move { axum::Json(jwks) }),
    )
}
