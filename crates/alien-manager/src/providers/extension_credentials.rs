//! Mints the short-lived credentials an extension's controller gets for one step, for the
//! identity initial setup created for the extension. That identity trusts the manager's own
//! principal, as the Remote Bindings identity does: the managing role on AWS, the deployment's
//! management federation on GCP, the manager's workload identity on Azure. The deployment's
//! management identity's credentials are never minted for or sent to a controller; on Azure it
//! only reads the extension's managed identity from ARM.

use std::collections::HashMap;

use alien_azure_clients::AzureClientConfigExt;
use alien_core::{
    AwsCredentials, AzureClientConfig, AzureCredentials, ClientConfig, EnvironmentInfo,
    GcpClientConfig, GcpCredentials, GcpFederationAccess, GcpFederationSource,
    ManagementExternalId, RemoteStackManagement, RemoteStackManagementOutputs, StackState,
};
use alien_error::{AlienError, Context, GenericError, IntoAlienError};
use alien_gcp_clients::GcpClientConfigExt;
use alien_infra::step_protocol::protocol::Credentials;
use base64::Engine as _;

use crate::traits::{DeploymentRecord, ExtensionIdentity};

/// How long minted AWS credentials last: STS's minimum, which still outlives a step's deadline.
const AWS_SESSION_SECONDS: i32 = 900;

/// The token audience of Azure Resource Manager, which an Azure controller calls.
const AZURE_ARM_SCOPE: &str = "https://management.azure.com/.default";

/// What the manager mints an extension's credentials from, per cloud.
pub enum MintSource<'a> {
    /// The managing role's credentials, which the identity's trust names, and the ExternalId
    /// the trust checks when the deployment has one.
    Aws {
        base: ClientConfig,
        external_id: Option<&'a ManagementExternalId>,
    },
    /// The manager's assertion source, exchanged through the deployment's management pool.
    Gcp { source: GcpFederationSource },
    /// The manager's environment, whose federated token the identity's credential trusts, and
    /// the deployment's management credentials, which read the identity from ARM.
    Azure {
        environment: &'a HashMap<String, String>,
        management: ClientConfig,
    },
}

/// Credentials for `identity` in `deployment`, minted from `source`.
pub async fn mint(
    deployment: &DeploymentRecord,
    identity: &ExtensionIdentity,
    source: MintSource<'_>,
) -> Result<Credentials, AlienError> {
    check_identity(deployment, identity)?;
    let environment = deployment.environment_info.as_ref().ok_or_else(|| {
        refused(
            identity,
            "the deployment has no recorded environment".to_string(),
        )
    })?;
    match (source, environment) {
        (MintSource::Aws { base, external_id }, EnvironmentInfo::Aws(environment)) => {
            mint_aws(identity, base, external_id, environment).await
        }
        (MintSource::Gcp { source }, EnvironmentInfo::Gcp(environment)) => {
            mint_gcp(deployment, identity, source, environment).await
        }
        (
            MintSource::Azure {
                environment: env,
                management,
            },
            EnvironmentInfo::Azure(environment),
        ) => mint_azure(deployment, identity, env, management, environment).await,
        _ => Err(refused(
            identity,
            format!(
                "the manager mints for another cloud than the deployment's {}",
                deployment.platform
            ),
        )),
    }
}

async fn mint_aws(
    identity: &ExtensionIdentity,
    base: ClientConfig,
    external_id: Option<&ManagementExternalId>,
    environment: &alien_core::AwsEnvironmentInfo,
) -> Result<Credentials, AlienError> {
    use alien_aws_clients::AwsClientConfigExt;
    let ClientConfig::Aws(base) = base else {
        return Err(refused(
            identity,
            "the managing credentials are not AWS's".to_string(),
        ));
    };
    // AWS's own impersonation, not `ClientConfig`'s, which flattens STS's answer into a message
    // and so would hide its error code from the retry decision.
    let minted = cloud_call(
        format!(
            "Cannot assume the identity of extension '{}' ({})",
            identity.extension, identity.identity
        ),
        base.impersonate(alien_core::AwsImpersonationConfig {
            role_arn: identity.identity.clone(),
            session_name: Some(format!("alien-extension-{}", identity.extension)),
            duration_seconds: Some(AWS_SESSION_SECONDS),
            external_id: external_id.map(|value| value.expose().to_string()),
            target_region: Some(environment.region.clone()),
        }),
    )
    .await?;
    let AwsCredentials::SessionCredentials {
        access_key_id,
        secret_access_key,
        session_token,
        expires_at,
    } = minted.credentials
    else {
        return Err(refused(
            identity,
            "AWS returned credentials that are not a session".to_string(),
        ));
    };
    Ok(Credentials::Aws {
        account_id: environment.account_id.clone(),
        region: environment.region.clone(),
        access_key_id,
        secret_access_key,
        session_token: Some(session_token),
        expires_at: Some(expires_at),
    })
}

async fn mint_gcp(
    deployment: &DeploymentRecord,
    identity: &ExtensionIdentity,
    source: GcpFederationSource,
    environment: &alien_core::GcpEnvironmentInfo,
) -> Result<Credentials, AlienError> {
    // The identity trusts the principal of the deployment's management pool, so the exchange
    // goes through that pool's access and only the account it targets changes.
    let management = management_outputs(deployment.stack_state.as_ref()).ok_or_else(|| {
        refused(
            identity,
            "the deployment has no management identity".to_string(),
        )
    })?;
    let mut access: GcpFederationAccess = serde_json::from_str(&management.access_configuration)
        .into_alien_error()
        .context(GenericError {
            message: "The deployment's GCP management access is not a federation descriptor"
                .to_string(),
        })?;
    deployment
        .gcp_federation_scope()
        .validate_access(&access, &RemoteStackManagement::RESOURCE_TYPE)
        .context(GenericError {
            message: "The deployment's GCP management access does not match the deployment"
                .to_string(),
        })?;
    access.service_account_email = identity.identity.clone();
    // The manager's own endpoints, as its assertion source reaches them.
    let service_overrides = match &source {
        GcpFederationSource::GoogleServiceAccount { source, .. } => {
            source.service_overrides.clone()
        }
        GcpFederationSource::KubernetesServiceAccount { .. } => None,
    };
    let config = GcpClientConfig {
        project_id: environment.project_id.clone(),
        region: environment.region.clone(),
        project_number: Some(environment.project_number.clone()),
        service_overrides,
        credentials: GcpCredentials::Federated {
            access: Box::new(access),
            source: Box::new(source),
            lifetime_seconds: None,
        },
    };
    let client = http_client(identity)?;
    let minted = cloud_call(
        format!(
            "Cannot impersonate the identity of extension '{}' ({})",
            identity.extension, identity.identity
        ),
        config.acquire_federated_token(&client),
    )
    .await?;
    Ok(Credentials::Gcp {
        project_id: environment.project_id.clone(),
        region: environment.region.clone(),
        access_token: minted.token,
        expires_at: minted.expires_at.to_rfc3339(),
    })
}

async fn mint_azure(
    deployment: &DeploymentRecord,
    identity: &ExtensionIdentity,
    env: &HashMap<String, String>,
    management: ClientConfig,
    environment: &alien_core::AzureEnvironmentInfo,
) -> Result<Credentials, AlienError> {
    pin_azure_identity(deployment, identity, management, environment).await?;
    let federated_token_file = env.get("AZURE_FEDERATED_TOKEN_FILE").ok_or_else(|| {
        refused(
            identity,
            "AZURE_FEDERATED_TOKEN_FILE is not set for the manager".to_string(),
        )
    })?;
    let authority_host = env
        .get("AZURE_AUTHORITY_HOST")
        .cloned()
        .unwrap_or_else(|| "https://login.microsoftonline.com/".to_string());
    // The identity lives in the customer's tenant, which `check_identity` matched against the
    // management access.
    let tenant_id = environment.tenant_id.as_str();
    let config = AzureClientConfig {
        subscription_id: environment.subscription_id.clone(),
        tenant_id: tenant_id.to_string(),
        region: Some(environment.location.clone()),
        credentials: AzureCredentials::WorkloadIdentity {
            client_id: identity.identity.clone(),
            tenant_id: tenant_id.to_string(),
            federated_token_file: federated_token_file.clone(),
            authority_host,
        },
        service_overrides: None,
    };
    let token = cloud_call(
        format!(
            "Cannot exchange the manager's token for the identity of extension '{}' ({})",
            identity.extension, identity.identity
        ),
        config.get_bearer_token_with_scope(AZURE_ARM_SCOPE),
    )
    .await?;
    let expires_at = jwt_expiry(&token).ok_or_else(|| {
        refused(
            identity,
            "Entra ID returned a token without an expiry".to_string(),
        )
    })?;
    Ok(Credentials::Azure {
        subscription_id: environment.subscription_id.clone(),
        tenant_id: tenant_id.to_string(),
        access_token: token,
        expires_at: expires_at.to_rfc3339(),
    })
}

/// Refuses `identity` unless it is the one initial setup created for its extension in
/// `deployment`: never the deployment's management identity, and on AWS and GCP the role or
/// service account setup names after the deployment's prefix, in the deployment's own account
/// or project. Its outputs come from the deployment's imported state, so nothing else vouches
/// for them. Runs before any cloud call.
pub fn check_identity(
    deployment: &DeploymentRecord,
    identity: &ExtensionIdentity,
) -> Result<(), AlienError> {
    let (Some(state), Some(environment)) = (
        deployment.stack_state.as_ref(),
        deployment.environment_info.as_ref(),
    ) else {
        return Err(refused(
            identity,
            "the deployment has no installed stack or recorded environment".to_string(),
        ));
    };
    let management = management_outputs(Some(state)).ok_or_else(|| {
        refused(
            identity,
            "the deployment has no management identity".to_string(),
        )
    })?;
    let management_access: Option<serde_json::Value> =
        serde_json::from_str(&management.access_configuration).ok();
    let is_management = [
        Some(management.management_resource_id.as_str()),
        Some(management.access_configuration.as_str()),
        management_access
            .as_ref()
            .and_then(|access| access["uamiClientId"].as_str()),
        management_access
            .as_ref()
            .and_then(|access| access["serviceAccountEmail"].as_str()),
    ]
    .into_iter()
    .flatten()
    .any(|management| management == identity.identity);
    if is_management {
        return Err(refused(
            identity,
            format!(
                "{} is the deployment's management identity",
                identity.identity
            ),
        ));
    }
    let prefix = state.resource_prefix.as_str();
    match environment {
        EnvironmentInfo::Aws(environment) => {
            let mut parts = identity.identity.splitn(6, ':');
            let account = parts.nth(4);
            let resource = parts.next();
            if account != Some(environment.account_id.as_str()) {
                return Err(refused(
                    identity,
                    format!(
                        "its role {} is not in the deployment's account {}",
                        identity.identity, environment.account_id
                    ),
                ));
            }
            let expected = format!(
                "role/{}",
                alien_core::extension_identity_aws_role_name(prefix, &identity.id)
            );
            if resource != Some(expected.as_str()) {
                return Err(refused(
                    identity,
                    format!(
                        "its role {} is not the role setup creates for it ({expected})",
                        identity.identity
                    ),
                ));
            }
        }
        EnvironmentInfo::Gcp(environment) => {
            let project_number = management_access
                .as_ref()
                .and_then(|access| access["projectNumber"].as_str());
            let in_project = identity.identity.ends_with(&format!(
                "@{}.iam.gserviceaccount.com",
                environment.project_id
            ));
            if project_number != Some(environment.project_number.as_str()) || !in_project {
                return Err(refused(
                    identity,
                    format!(
                        "its service account {} is not in the deployment's project {} ({})",
                        identity.identity, environment.project_id, environment.project_number
                    ),
                ));
            }
            let expected = format!(
                "{}@{}.iam.gserviceaccount.com",
                alien_core::extension_identity_gcp_account_id(prefix, &identity.id),
                environment.project_id
            );
            if identity.identity != expected {
                return Err(refused(
                    identity,
                    format!(
                        "its service account {} is not the one setup creates for it ({expected})",
                        identity.identity
                    ),
                ));
            }
        }
        EnvironmentInfo::Azure(environment) => {
            // A managed identity's client id is a GUID setup can't choose, so here only its tenant
            // and its not being the management identity are checked; minting then reads the
            // identity from ARM (`pin_azure_identity`).
            let tenant = management_access
                .as_ref()
                .and_then(|access| access["tenantId"].as_str());
            if tenant != Some(environment.tenant_id.as_str()) {
                return Err(refused(
                    identity,
                    format!(
                        "the management access names tenant {} but the deployment is in tenant {}",
                        tenant.unwrap_or("none"),
                        environment.tenant_id
                    ),
                ));
            }
        }
        _ => {
            return Err(refused(
                identity,
                format!("extensions have no identity on {}", deployment.platform),
            ))
        }
    }
    Ok(())
}

/// Refuses `identity` unless ARM says its client id is the managed identity setup creates for
/// an extension of this name: `<prefix>-<extension>-ext`, read in the stack's resource group as
/// the deployment's management identity. Every identity Alien creates in a tenant trusts the
/// manager's one federated token, so the client id the deployment's state names is not enough
/// on its own: with this read it is never a management identity, and never an identity setup
/// didn't create for an extension of this name.
///
/// It does not bind the identity to this deployment. The subscription, prefix, resource group
/// and management identity all come from what the deployment's setup reported, so that needs a
/// federated credential per deployment, which setup doesn't create today.
async fn pin_azure_identity(
    deployment: &DeploymentRecord,
    identity: &ExtensionIdentity,
    management: ClientConfig,
    environment: &alien_core::AzureEnvironmentInfo,
) -> Result<(), AlienError> {
    use alien_azure_clients::managed_identity::{AzureManagedIdentityClient, ManagedIdentityApi};

    let state = deployment.stack_state.as_ref().ok_or_else(|| {
        refused(
            identity,
            "the deployment has no installed stack".to_string(),
        )
    })?;
    let ClientConfig::Azure(management) = management else {
        return Err(refused(
            identity,
            "the deployment's management credentials are not Azure's".to_string(),
        ));
    };
    let resource_group =
        alien_infra::azure_utils::get_resource_group_name(state).map_err(|error| {
            refused(
                identity,
                format!("the deployment has no resource group: {error}"),
            )
        })?;
    let name = alien_infra::get_azure_managed_identity_name(&state.resource_prefix, &identity.id);
    let client = AzureManagedIdentityClient::new(
        http_client(identity)?,
        alien_azure_clients::AzureTokenCache::new(*management),
    );
    let read = cloud_call(
        format!(
            "Cannot read the managed identity '{name}' of extension '{}' in resource group '{resource_group}'",
            identity.extension
        ),
        client.get_user_assigned_identity(&resource_group, &name),
    )
    .await?;
    let properties = read.properties.as_ref();
    let client_id = properties.and_then(|properties| properties.client_id.as_ref());
    let tenant_id = properties.and_then(|properties| properties.tenant_id.as_ref());
    let claimed = uuid::Uuid::parse_str(&identity.identity).ok();
    if claimed.is_none() || client_id.map(|id| id.0) != claimed {
        return Err(refused(
            identity,
            format!(
                "{} is not the client id of its managed identity '{name}' ({})",
                identity.identity,
                client_id.map_or("none".to_string(), ToString::to_string)
            ),
        ));
    }
    if tenant_id.map(ToString::to_string) != Some(environment.tenant_id.to_lowercase()) {
        return Err(refused(
            identity,
            format!(
                "its managed identity '{name}' is in tenant {}, not the deployment's {}",
                tenant_id.map_or("none".to_string(), ToString::to_string),
                environment.tenant_id
            ),
        ));
    }
    Ok(())
}

/// How long one call to a cloud API that minting makes may take, retries included, so a stalled
/// one fails the step, for a retry, before the step's deadline.
const CLOUD_CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

fn http_client(identity: &ExtensionIdentity) -> Result<reqwest::Client, AlienError> {
    reqwest::Client::builder()
        .timeout(CLOUD_CALL_TIMEOUT)
        .build()
        .map_err(|error| refused(identity, format!("cannot build an HTTP client: {error}")))
}

/// One call minting makes to a cloud, failing as `failure`. The call gets
/// [`CLOUD_CALL_TIMEOUT`] with the client's own retries included, and a stall is retryable;
/// otherwise [`classify_credential_error`](super::credential_errors::classify_credential_error)
/// says whether a later step can get past the failure.
async fn cloud_call<T, E>(
    failure: String,
    call: impl std::future::Future<Output = Result<T, AlienError<E>>>,
) -> Result<T, AlienError>
where
    E: alien_error::AlienErrorData + Clone + std::fmt::Debug + serde::Serialize,
{
    let outcome = tokio::time::timeout(CLOUD_CALL_TIMEOUT, call)
        .await
        .map_err(|_| {
            let mut error = AlienError::new(GenericError {
                message: format!("{failure}: no answer within {CLOUD_CALL_TIMEOUT:?}"),
            });
            error.retryable = true;
            error
        })?;
    outcome.map_err(|source| {
        let source = source.into_generic();
        let mut error = AlienError::new(GenericError { message: failure });
        error.retryable =
            super::credential_errors::classify_credential_error(source.clone()).retryable;
        error.source = Some(Box::new(source));
        error
    })
}

fn management_outputs(state: Option<&StackState>) -> Option<&RemoteStackManagementOutputs> {
    state?.resources.values().find_map(|resource| {
        (resource.resource_type == RemoteStackManagement::RESOURCE_TYPE.as_ref())
            .then(|| resource.outputs.as_ref()?.downcast_ref())
            .flatten()
    })
}

/// The `exp` of an Entra ID access token, which is a JWT. Entra doesn't say how long it lasts
/// otherwise, and the time it gives is the one the token is checked against.
fn jwt_expiry(token: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let payload = token.split('.').nth(1)?;
    let claims: serde_json::Value = serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload.trim_end_matches('='))
            .ok()?,
    )
    .ok()?;
    chrono::DateTime::from_timestamp(claims["exp"].as_i64()?, 0)
}

fn refused(identity: &ExtensionIdentity, reason: String) -> AlienError {
    AlienError::new(GenericError {
        message: format!(
            "Cannot mint credentials for the identity of extension '{}': {reason}",
            identity.extension
        ),
    })
}
