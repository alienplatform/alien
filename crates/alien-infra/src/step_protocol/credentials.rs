//! The credentials a step carries when the extension's controller runs as a process beside the
//! host that steps it: the setup CLI forwards the admin's own, and the host in a controller's
//! pod on Kubernetes forwards the pod's own identity. Either way they reach the controller only
//! on its stdin.

use super::protocol::Credentials;
use super::proxy::CredentialSource;
use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::ClientConfig;
use alien_error::{AlienError, Context};
use async_trait::async_trait;
use std::time::Duration;

const GCP_CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const AZURE_MANAGEMENT_SCOPE: &str = "https://management.azure.com/.default";

/// The host's own credentials, forwarded to the controller it runs beside it. The setup CLI
/// forwards the admin's during initial setup, so the controller's calls show in the audit log as
/// the admin's, as Alien's own setup calls do. The host in a controller's pod on Kubernetes
/// forwards the pod's own identity, which is the extension's.
pub struct ForwardedCredentials {
    config: ClientConfig,
}

impl ForwardedCredentials {
    /// The credentials in `config`, which the host loaded from its own environment.
    pub fn new(config: ClientConfig) -> Self {
        Self { config }
    }

    /// The credentials as a step carries them, resolved now, so a refreshable source hands
    /// over a current session.
    pub async fn resolve(&self) -> Result<Credentials> {
        credentials_of(&self.config, STEP_VALIDITY).await
    }
}

impl std::fmt::Debug for ForwardedCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForwardedCredentials")
            .field("platform", &self.config.platform())
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl CredentialSource for ForwardedCredentials {
    async fn credentials(&self, _ctx: &ResourceControllerContext<'_>) -> Result<Credentials> {
        self.resolve().await
    }
}

/// How long forwarded credentials are said to last when their source doesn't say: the step
/// they are sent for has to end by then.
const STEP_VALIDITY: Duration = super::proxy::STEP_DEADLINE;

/// The credentials in `config` as a step carries them, valid for at least `validity` when the
/// source doesn't say how long.
#[cfg_attr(not(any(feature = "gcp", feature = "azure")), allow(unused_variables))]
async fn credentials_of(config: &ClientConfig, validity: Duration) -> Result<Credentials> {
    match config {
        #[cfg(feature = "aws")]
        ClientConfig::Aws(aws) => {
            let resolved = alien_aws_clients::AwsCredentialProvider::from_config((**aws).clone())
                .await
                .context(ErrorData::AuthenticationFailed {
                    message: "the AWS credentials could not be resolved".to_string(),
                    method: None,
                })?
                .get_credentials();
            let expires_at = match &aws.credentials {
                alien_core::AwsCredentials::SessionCredentials { expires_at, .. } => {
                    Some(expires_at.clone())
                }
                _ => None,
            };
            Ok(Credentials::Aws {
                account_id: aws.account_id.clone(),
                region: aws.region.clone(),
                access_key_id: resolved.access_key_id().to_string(),
                secret_access_key: resolved.secret_access_key().to_string(),
                session_token: resolved.session_token().map(str::to_string),
                expires_at,
            })
        }
        #[cfg(feature = "gcp")]
        ClientConfig::Gcp(gcp) => {
            use alien_gcp_clients::GcpClientConfigExt;
            let access_token = gcp
                .get_bearer_token(GCP_CLOUD_PLATFORM_SCOPE)
                .await
                .context(ErrorData::AuthenticationFailed {
                    message: "a GCP access token could not be obtained".to_string(),
                    method: None,
                })?;
            Ok(Credentials::Gcp {
                project_id: gcp.project_id.clone(),
                region: gcp.region.clone(),
                access_token,
                expires_at: expiry_after(validity),
            })
        }
        #[cfg(feature = "azure")]
        ClientConfig::Azure(azure) => {
            use alien_azure_clients::AzureClientConfigExt;
            let access_token = azure
                .get_bearer_token_with_scope(AZURE_MANAGEMENT_SCOPE)
                .await
                .context(ErrorData::AuthenticationFailed {
                    message: "an Azure access token could not be obtained".to_string(),
                    method: None,
                })?;
            Ok(Credentials::Azure {
                subscription_id: azure.subscription_id.clone(),
                tenant_id: azure.tenant_id.clone(),
                access_token,
                expires_at: expiry_after(validity),
            })
        }
        other => Err(AlienError::new(ErrorData::ClientConfigInvalid {
            platform: other.platform(),
            message: "a step carries credentials only for AWS, GCP or Azure".to_string(),
        })),
    }
}

/// An access token's expiry, which the token endpoints Alien calls don't return with it.
#[cfg(any(feature = "gcp", feature = "azure"))]
fn expiry_after(validity: Duration) -> String {
    (chrono::Utc::now() + chrono::Duration::seconds(validity.as_secs() as i64)).to_rfc3339()
}
