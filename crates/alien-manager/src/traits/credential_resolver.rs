use async_trait::async_trait;

use alien_core::{AwsClientConfig, ClientConfig, GcpClientConfig, ManagementConfig, Platform};
use alien_error::AlienError;

use super::deployment_store::DeploymentRecord;

/// Credentials resolved for a deployment plus the lifecycle phases they may
/// drive safely.
#[derive(Debug, Clone)]
pub struct ResolvedCredentials {
    /// Platform client configuration to pass into the deployment runner.
    pub client_config: ClientConfig,
    /// Whether these credentials may create the bootstrap layer-2 resources.
    pub has_provision_capability: bool,
}

/// Manager-proven GCP source and exact access-boundary role.
///
/// Private fields prevent external credential resolvers from asserting this
/// provenance; they must use `Direct`, which the GCP materializer rejects.
pub struct GcpCredentialAccessBoundarySource {
    pub(crate) source: Box<GcpClientConfig>,
    pub(crate) available_role: String,
}

/// Credential authority retained specifically for remote Storage attenuation.
///
/// The AWS cross-account variant keeps the pre-handoff source and target role
/// so the bucket policy is applied by STS on the target-role session itself.
pub enum RemoteStorageCredentialSource {
    /// A direct provider config. The materializer accepts it only when the
    /// provider can prove resource attenuation from this form.
    Direct(ClientConfig),
    /// AWS source credentials plus the exact target role for a policy-bearing
    /// AssumeRole handoff.
    AwsAssumeRole {
        source: Box<AwsClientConfig>,
        role_arn: String,
        role_session_name: String,
        target_account_id: String,
        target_region: String,
    },
    /// A GCP source whose Credential Access Boundary is capped by the exact
    /// custom role generated for `storage/remote-data-write`.
    GcpCredentialAccessBoundary(GcpCredentialAccessBoundarySource),
}

impl std::fmt::Debug for RemoteStorageCredentialSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Direct(config) => f
                .debug_struct("RemoteStorageCredentialSource::Direct")
                .field("platform", &config.platform())
                .field("credentials", &"[REDACTED]")
                .finish(),
            Self::AwsAssumeRole {
                role_arn,
                target_account_id,
                target_region,
                ..
            } => f
                .debug_struct("RemoteStorageCredentialSource::AwsAssumeRole")
                .field("role_arn", role_arn)
                .field("target_account_id", target_account_id)
                .field("target_region", target_region)
                .field("credentials", &"[REDACTED]")
                .finish(),
            Self::GcpCredentialAccessBoundary(source) => f
                .debug_struct("RemoteStorageCredentialSource::GcpCredentialAccessBoundary")
                .field("available_role", &source.available_role)
                .field("credentials", &"[REDACTED]")
                .finish(),
        }
    }
}

/// Resolves cloud credentials for push-model deployments.
///
/// In push mode, alien-manager needs credentials to call cloud APIs in the remote
/// environment. The resolver reads base credentials and optionally impersonates
/// a service account in the target environment.
#[async_trait]
pub trait CredentialResolver: Send + Sync {
    /// Resolve credentials for a deployment's target environment.
    ///
    /// For single-account setups, returns the server's own credentials.
    /// For cross-account setups, impersonates the target role/service account.
    async fn resolve(&self, deployment: &DeploymentRecord) -> Result<ClientConfig, AlienError>;

    /// Resolve credentials with their lifecycle capabilities.
    ///
    /// The default resolver behavior is suitable for direct environment/local
    /// credentials: if a resolver can return a client config, those credentials
    /// are allowed to provision. Cross-account resolvers override this for
    /// post-bootstrap impersonation credentials, which can manage layer-3 work
    /// and updates but must not create the initial customer-owned layer-2 stack.
    async fn resolve_with_capability(
        &self,
        deployment: &DeploymentRecord,
    ) -> Result<ResolvedCredentials, AlienError> {
        let client_config = self.resolve(deployment).await?;
        Ok(ResolvedCredentials {
            client_config,
            has_provision_capability: true,
        })
    }

    /// Resolve authority for a purpose-specific remote Storage lease.
    ///
    /// Custom resolvers default to their direct config. Provider materializers
    /// still fail closed when that form cannot be attenuated cryptographically.
    async fn resolve_remote_storage_source(
        &self,
        deployment: &DeploymentRecord,
    ) -> Result<RemoteStorageCredentialSource, AlienError> {
        Ok(RemoteStorageCredentialSource::Direct(
            self.resolve(deployment).await?,
        ))
    }

    /// Resolve the management identity for a target platform.
    ///
    /// Returns the ManagementConfig describing which identity should be granted
    /// cross-account access in the customer's cloud. Derived from the management
    /// ServiceAccount binding for the given platform's target provider.
    ///
    /// Returns `Ok(None)` when no management binding is available (e.g. standalone mode).
    async fn resolve_management_config(
        &self,
        _platform: Platform,
    ) -> Result<Option<ManagementConfig>, AlienError> {
        Ok(None)
    }
}
