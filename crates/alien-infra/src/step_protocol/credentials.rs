//! The credentials of a step whose controller runs as a module in Alien's own process: the
//! extension identity's, minted for each step. The host signs the module's cloud calls with
//! them, and the module never gets them.

use super::protocol::Credentials;
use super::proxy::CredentialSource;
use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{
    AwsCredentials, AzureCredentials, ClientConfig, ImpersonationConfig, Platform, ServiceAccount,
    ServiceAccountOutputs, StackState,
};
use alien_error::{AlienError, Context};
use async_trait::async_trait;
use std::time::Duration;

/// How long credentials minted for one step last: STS's shortest session, and longer than a
/// step's deadline plus the host's grace.
const MINTED_LIFETIME: Duration = Duration::from_secs(900);

const GCP_CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const AZURE_MANAGEMENT_SCOPE: &str = "https://management.azure.com/.default";

/// Where a deployment runs, which its extension identities must be in: its AWS account, GCP
/// project or Azure subscription.
#[derive(Debug, Clone, Copy)]
pub enum IdentityHome<'a> {
    Aws { account_id: &'a str },
    Gcp { project_id: &'a str },
    Azure { subscription_id: &'a str },
}

/// Why `identity` is not the identity setup creates for `extension` (resource `identity_id`) in
/// the deployment whose state is `state`, which runs in `home`: on AWS the role
/// `extension_identity_aws_role_name` names in its account, on GCP the service account
/// `extension_identity_gcp_account_id` names in its project. Checked before any cloud call;
/// `state` is what the deployment's setup reported.
///
/// On Azure a client id names nothing to check it against without an ARM read, so the record
/// `state` holds for the identity is checked instead: it must be set up for `extension` and
/// name the managed identity `<prefix>-<id>` in the deployment's subscription. The Operator
/// reads the client id from that same record and nothing reads it back from ARM, so beyond this
/// check it relies on Entra ID's federated exchange: it mints only from its workload identity
/// token, which Entra ID exchanges only for an identity whose federated credential trusts the
/// Operator's service account.
pub fn check_extension_identity(
    state: &StackState,
    home: IdentityHome<'_>,
    extension: &str,
    identity_id: &str,
    identity: &str,
) -> std::result::Result<(), String> {
    let prefix = state.resource_prefix.as_str();
    match home {
        IdentityHome::Aws { account_id } => {
            let mut parts = identity.splitn(6, ':');
            let account = parts.nth(4);
            let resource = parts.next();
            if account != Some(account_id) {
                return Err(format!(
                    "its role {identity} is not in the deployment's account {account_id}"
                ));
            }
            let expected = format!(
                "role/{}",
                alien_core::extension_identity_aws_role_name(prefix, identity_id)
            );
            if resource != Some(expected.as_str()) {
                return Err(format!(
                    "its role {identity} is not the role setup creates for it ({expected})"
                ));
            }
        }
        IdentityHome::Gcp { project_id } => {
            if !identity.ends_with(&format!("@{project_id}.iam.gserviceaccount.com")) {
                return Err(format!(
                    "its service account {identity} is not in the deployment's project {project_id}"
                ));
            }
            let expected = format!(
                "{}@{project_id}.iam.gserviceaccount.com",
                alien_core::extension_identity_gcp_account_id(prefix, identity_id)
            );
            if identity != expected {
                return Err(format!(
                    "its service account {identity} is not the one setup creates for it ({expected})"
                ));
            }
        }
        IdentityHome::Azure { subscription_id } => {
            let recorded = state
                .resources
                .get(identity_id)
                .filter(|resource| {
                    resource
                        .config
                        .downcast_ref::<ServiceAccount>()
                        .is_some_and(|account| account.extension.as_deref() == Some(extension))
                })
                .and_then(|resource| resource.outputs.as_ref())
                .and_then(|outputs| outputs.downcast_ref::<ServiceAccountOutputs>())
                .ok_or_else(|| {
                    format!("the deployment records no identity '{identity_id}' set up for it")
                })?;
            let name = crate::get_azure_managed_identity_name(prefix, identity_id);
            let segments: Vec<&str> = recorded.resource_id.split('/').collect();
            let in_home = match segments.as_slice() {
                ["", subscriptions, subscription, groups, group, providers, namespace, kind, named] => {
                    [
                        (*subscriptions, "subscriptions"),
                        (*subscription, subscription_id),
                        (*groups, "resourceGroups"),
                        (*providers, "providers"),
                        (*namespace, "Microsoft.ManagedIdentity"),
                        (*kind, "userAssignedIdentities"),
                        (*named, name.as_str()),
                    ]
                    .iter()
                    .all(|(segment, expected)| segment.eq_ignore_ascii_case(expected))
                        && !group.is_empty()
                }
                _ => false,
            };
            if !in_home {
                return Err(format!(
                    "the deployment records its identity '{identity_id}' as {}, not the managed identity '{name}' setup creates in subscription {subscription_id}",
                    recorded.resource_id
                ));
            }
        }
    }
    Ok(())
}

/// Credentials minted for each step for the identity setup creates for an extension, from the
/// Operator's own.
///
/// The host signs the controller module's cloud calls with them, so the calls carry the
/// extension identity's permissions and show apart from the Operator's in the customer's audit
/// log.
pub struct ExtensionIdentityCredentials {
    extension: String,
    identity_id: String,
    caller: ClientConfig,
    /// The Operator pod's web identity token on EKS, where setup lets only that token assume
    /// the identity, so the Operator's own role can't.
    pod_token_file: Option<String>,
}

impl ExtensionIdentityCredentials {
    /// The source for the extension named in `manifest`, minting from `caller`, the Operator's
    /// client config. `None` when `caller` reaches no cloud, as on the local platform, where
    /// setup creates no identity and the controller makes no cloud call.
    pub fn new(manifest: &alien_core::ExtensionManifest, caller: &ClientConfig) -> Option<Self> {
        let (cloud, pod_token_file) = match caller {
            ClientConfig::KubernetesCloud { cloud, .. } => {
                let pod_token_file = match cloud.as_ref() {
                    ClientConfig::Aws(aws) => match &aws.credentials {
                        AwsCredentials::WebIdentity { config } => {
                            Some(config.web_identity_token_file.clone())
                        }
                        _ => None,
                    },
                    _ => None,
                };
                (cloud.as_ref(), pod_token_file)
            }
            other => (other, None),
        };
        matches!(
            cloud,
            ClientConfig::Aws(_) | ClientConfig::Gcp(_) | ClientConfig::Azure(_)
        )
        .then(|| Self {
            extension: manifest.name.clone(),
            identity_id: manifest.identity_id(),
            caller: cloud.clone(),
            pod_token_file,
        })
    }

    fn impersonation(&self, identity: String) -> Result<ImpersonationConfig> {
        let lifetime = MINTED_LIFETIME.as_secs();
        match &self.caller {
            ClientConfig::Aws(_) => Ok(ImpersonationConfig::Aws(
                alien_core::AwsImpersonationConfig {
                    role_arn: identity,
                    session_name: Some(format!("alien-extension-{}", self.extension)),
                    duration_seconds: Some(lifetime as i32),
                    external_id: None,
                    target_region: None,
                },
            )),
            ClientConfig::Gcp(_) => Ok(ImpersonationConfig::Gcp(
                alien_core::GcpImpersonationConfig {
                    service_account_email: identity,
                    scopes: vec![GCP_CLOUD_PLATFORM_SCOPE.to_string()],
                    delegates: None,
                    lifetime: Some(format!("{lifetime}s")),
                    target_project_id: None,
                    target_region: None,
                },
            )),
            ClientConfig::Azure(_) => Ok(ImpersonationConfig::Azure(
                alien_core::AzureImpersonationConfig {
                    client_id: identity,
                    scope: AZURE_MANAGEMENT_SCOPE.to_string(),
                    tenant_id: None,
                    target_subscription_id: None,
                    target_region: None,
                },
            )),
            other => Err(AlienError::new(ErrorData::ClientConfigInvalid {
                platform: other.platform(),
                message: format!(
                    "Extension '{}': credentials for its identity are minted only on a cloud",
                    self.extension
                ),
            })),
        }
    }
}

impl std::fmt::Debug for ExtensionIdentityCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtensionIdentityCredentials")
            .field("extension", &self.extension)
            .field("identity_id", &self.identity_id)
            .field("platform", &self.caller.platform())
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl CredentialSource for ExtensionIdentityCredentials {
    async fn credentials(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Option<Credentials>> {
        if !super::has_extension_identity(ctx, &self.identity_id) {
            return Ok(None);
        }
        // Only the service account setup created for this extension is minted for: another
        // resource at its id could name any identity the Operator can act as.
        let state = ctx.state.resources.get(&self.identity_id);
        let created_for_it = state.map(|state| {
            state
                .config
                .downcast_ref::<ServiceAccount>()
                .is_some_and(|account| {
                    account.extension.as_deref() == Some(self.extension.as_str())
                })
        });
        if created_for_it == Some(false) {
            return Err(AlienError::new(ErrorData::AuthenticationFailed {
                message: format!(
                    "Extension '{}': the resource '{}' is not the identity setup created for it, so its controller can't get credentials; run its initial setup again",
                    self.extension, self.identity_id
                ),
                method: Some("extension_identity".to_string()),
            }));
        }
        // Until its identity is set up the controller must not get the Operator's credentials
        // instead.
        let identity = state
            .and_then(|state| state.outputs.as_ref())
            .and_then(|outputs| outputs.downcast_ref::<ServiceAccountOutputs>())
            .map(|outputs| outputs.identity.clone())
            .ok_or_else(|| {
                AlienError::new(ErrorData::AuthenticationFailed {
                    message: format!(
                        "Extension '{}': its identity '{}' has no outputs in this deployment, so its controller can't get credentials; the deployment's initial setup creates it, so run that setup",
                        self.extension, self.identity_id
                    ),
                    method: Some("extension_identity".to_string()),
                })
            })?;
        let home = match &self.caller {
            ClientConfig::Aws(aws) => IdentityHome::Aws {
                account_id: &aws.account_id,
            },
            ClientConfig::Gcp(gcp) => IdentityHome::Gcp {
                project_id: &gcp.project_id,
            },
            ClientConfig::Azure(azure) => {
                if !exchanges_for_another_identity(&azure.credentials) {
                    return Err(AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Azure,
                        message: format!(
                            "Extension '{}': the Operator runs on {}, which can't mint credentials for the extension's identity, so its controller can't get credentials; run the Operator with Azure workload identity",
                            self.extension,
                            credential_kind(&azure.credentials)
                        ),
                    }));
                }
                if own_client_id(&azure.credentials)
                    .is_some_and(|own| own.eq_ignore_ascii_case(&identity))
                {
                    return Err(AlienError::new(ErrorData::AuthenticationFailed {
                        message: format!(
                            "Extension '{}': its identity's client id {identity} is the Operator's own, so its controller can't get credentials",
                            self.extension
                        ),
                        method: Some("extension_identity".to_string()),
                    }));
                }
                IdentityHome::Azure {
                    subscription_id: &azure.subscription_id,
                }
            }
            other => {
                return Err(AlienError::new(ErrorData::ClientConfigInvalid {
                    platform: other.platform(),
                    message: format!(
                        "Extension '{}': credentials for its identity are minted only on a cloud",
                        self.extension
                    ),
                }))
            }
        };
        check_extension_identity(
            ctx.state,
            home,
            &self.extension,
            &self.identity_id,
            &identity,
        )
        .map_err(|reason| {
            AlienError::new(ErrorData::AuthenticationFailed {
                message: format!(
                    "Extension '{}': {reason}, so its controller can't get credentials",
                    self.extension
                ),
                method: Some("extension_identity".to_string()),
            })
        })?;
        let minted = match (
            &self.caller,
            &self.pod_token_file,
            self.impersonation(identity)?,
        ) {
            #[cfg(feature = "aws")]
            (ClientConfig::Aws(caller), Some(token_file), ImpersonationConfig::Aws(identity)) => {
                use alien_aws_clients::AwsClientConfigExt;
                alien_core::AwsClientConfig {
                    credentials: AwsCredentials::WebIdentity {
                        config: alien_core::AwsWebIdentityConfig {
                            role_arn: identity.role_arn,
                            session_name: identity.session_name,
                            web_identity_token_file: token_file.clone(),
                            duration_seconds: identity.duration_seconds,
                        },
                    },
                    ..(**caller).clone()
                }
                .get_web_identity_credentials()
                .await
                .map(|minted| ClientConfig::Aws(Box::new(minted)))
            }
            (_, _, impersonation) => {
                crate::ClientConfigExt::impersonate(&self.caller, impersonation).await
            }
        }
        .context(ErrorData::AuthenticationFailed {
            message: format!(
                "Extension '{}': credentials for its identity '{}' could not be minted",
                self.extension, self.identity_id
            ),
            method: Some("extension_identity".to_string()),
        })?;
        credentials_of(&minted, MINTED_LIFETIME).await.map(Some)
    }
}

/// Whether impersonating from `credentials` goes through Entra ID's federated exchange, which
/// issues a token only for an identity that trusts the Operator. From a client secret it gets
/// the Operator's own token, and an access token can't get another identity's at all. A managed
/// identity asks the host's identity endpoint or IMDS, which issues a token for any identity
/// attached to the host, whether or not that identity trusts the Operator.
fn exchanges_for_another_identity(credentials: &AzureCredentials) -> bool {
    matches!(credentials, AzureCredentials::WorkloadIdentity { .. })
}

fn credential_kind(credentials: &AzureCredentials) -> &'static str {
    match credentials {
        AzureCredentials::ServicePrincipal { .. } => "a client secret",
        AzureCredentials::AccessToken { .. } => "an access token",
        AzureCredentials::ScopedAccessTokens { .. } => "scoped access tokens",
        AzureCredentials::VmManagedIdentity { .. } => "a VM managed identity",
        AzureCredentials::WorkloadIdentity { .. } => "workload identity",
        AzureCredentials::ManagedIdentity { .. } => {
            "an App Service or Container Apps managed identity"
        }
    }
}

/// The client id `credentials` authenticate as, when they name one.
fn own_client_id(credentials: &AzureCredentials) -> Option<&str> {
    match credentials {
        AzureCredentials::ServicePrincipal { client_id, .. }
        | AzureCredentials::VmManagedIdentity { client_id, .. }
        | AzureCredentials::WorkloadIdentity { client_id, .. }
        | AzureCredentials::ManagedIdentity { client_id, .. } => Some(client_id),
        AzureCredentials::AccessToken { .. } | AzureCredentials::ScopedAccessTokens { .. } => None,
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        Platform, Resource, ResourceLifecycle, ResourceOutputs, ResourceStatus, StackResourceState,
    };

    const SUBSCRIPTION: &str = "customer-subscription";
    const ACME_CLIENT_ID: &str = "0a7d6c8e-3b1f-4c2a-8e9d-111111111111";

    fn identity_resource_id(subscription: &str, name: &str) -> String {
        format!("/subscriptions/{subscription}/resourceGroups/rg-a/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{name}")
    }

    /// A deployment with prefix `test-prefix` whose state records `resource_id` as the identity
    /// of the extension `owner`.
    fn azure_state(owner: Option<&str>, resource_id: &str) -> StackState {
        let mut state =
            StackState::with_resource_prefix(Platform::Azure, "test-prefix".to_string());
        let mut account = ServiceAccount::new("acme-ext".to_string()).build();
        account.extension = owner.map(str::to_string);
        state.resources.insert(
            "acme-ext".to_string(),
            StackResourceState::builder()
                .resource_type(ServiceAccount::RESOURCE_TYPE.to_string())
                .status(ResourceStatus::Running)
                .config(Resource::new(account))
                .outputs(ResourceOutputs::new(ServiceAccountOutputs {
                    identity: ACME_CLIENT_ID.to_string(),
                    resource_id: resource_id.to_string(),
                }))
                .lifecycle(ResourceLifecycle::Frozen)
                .build(),
        );
        state
    }

    fn check_azure(state: &StackState) -> std::result::Result<(), String> {
        check_extension_identity(
            state,
            IdentityHome::Azure {
                subscription_id: SUBSCRIPTION,
            },
            "acme",
            "acme-ext",
            ACME_CLIENT_ID,
        )
    }

    /// A record of the managed identity setup names `<prefix>-<id>` in the deployment's
    /// subscription, set up for the extension, is accepted.
    #[test]
    fn the_azure_identity_setup_records_for_the_deployment_is_accepted() {
        check_azure(&azure_state(
            Some("acme"),
            &identity_resource_id(SUBSCRIPTION, "test-prefix-acme-ext"),
        ))
        .unwrap();
    }

    /// A record that names another deployment's or subscription's managed identity, as one
    /// copied from another deployment's state does, or that is not set up for the extension, is
    /// refused.
    #[test]
    fn an_azure_record_copied_from_another_deployment_is_refused() {
        let ours = identity_resource_id(SUBSCRIPTION, "test-prefix-acme-ext");
        for (label, owner, resource_id) in [
            (
                "another deployment's managed identity",
                Some("acme"),
                identity_resource_id(SUBSCRIPTION, "other-prefix-acme-ext"),
            ),
            (
                "another subscription",
                Some("acme"),
                identity_resource_id("other-subscription", "test-prefix-acme-ext"),
            ),
            ("a longer path", Some("acme"), format!("{ours}/extra")),
            ("not set up for the extension", None, ours.clone()),
            ("another extension's", Some("billing"), ours.clone()),
        ] {
            assert!(
                check_azure(&azure_state(owner, &resource_id)).is_err(),
                "{label} was accepted"
            );
        }
    }
}
