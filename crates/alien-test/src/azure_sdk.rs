use std::{fmt, path::PathBuf, sync::Arc};

use alien_core::{AzureClientConfig, AzureCredentials};
use anyhow::Context;
use azure_core::{
    cloud::{CloudConfiguration, CustomConfiguration},
    credentials::{AccessToken, Secret, TokenCredential, TokenRequestOptions},
    http::ClientOptions,
    time::{Duration as AzureDuration, OffsetDateTime},
};
use azure_identity::{
    ClientAssertionCredentialOptions, ClientSecretCredential, ClientSecretCredentialOptions,
    ManagedIdentityCredential, ManagedIdentityCredentialOptions, UserAssignedId,
    WorkloadIdentityCredential, WorkloadIdentityCredentialOptions,
};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};

const ARM_SCOPE: &str = "https://management.azure.com/.default";

#[derive(Debug, Clone)]
pub(crate) enum Scope {
    Resource {
        resource_group_name: String,
        resource_provider: String,
        parent_resource_path: Option<String>,
        resource_type: String,
        resource_name: String,
    },
}

impl Scope {
    pub(crate) fn to_scope_string(&self, client_config: &AzureClientConfig) -> String {
        match self {
            Scope::Resource {
                resource_group_name,
                resource_provider,
                parent_resource_path,
                resource_type,
                resource_name,
            } => {
                let base = format!(
                    "subscriptions/{}/resourceGroups/{}/providers/{}",
                    client_config.subscription_id, resource_group_name, resource_provider
                );

                if let Some(parent_path) = parent_resource_path {
                    format!(
                        "{}/{}/{}/{}",
                        base, parent_path, resource_type, resource_name
                    )
                } else {
                    format!("{}/{}/{}", base, resource_type, resource_name)
                }
            }
        }
    }

    pub(crate) fn to_resource_id_string(&self, client_config: &AzureClientConfig) -> String {
        format!(
            "/{}",
            self.to_scope_string(client_config).trim_start_matches('/')
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RoleAssignment {
    /// Fully qualified Azure role assignment resource ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) id: Option<String>,
    /// Azure role assignment resource name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    /// Azure role assignment resource type.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub(crate) type_: Option<String>,
    /// Role assignment properties.
    pub(crate) properties: RoleAssignmentProperties,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RoleAssignmentProperties {
    /// Principal object ID receiving the role assignment.
    pub(crate) principal_id: String,
    /// Fully qualified Azure role definition ID.
    pub(crate) role_definition_id: String,
    /// Scope where the role assignment applies.
    pub(crate) scope: String,
    /// Azure principal type.
    pub(crate) principal_type: String,
    /// Optional Azure role assignment condition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) condition: Option<String>,
    /// Optional Azure role assignment condition version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) condition_version: Option<String>,
    /// Optional delegated managed identity resource ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) delegated_managed_identity_resource_id: Option<String>,
    /// Optional human-readable role assignment description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<String>,
}

#[derive(Debug)]
pub(crate) struct AzureRestError {
    status: Option<StatusCode>,
    message: String,
}

impl AzureRestError {
    fn request(message: String) -> Self {
        Self {
            status: None,
            message,
        }
    }

    fn response(status: StatusCode, message: String) -> Self {
        Self {
            status: Some(status),
            message,
        }
    }

    pub(crate) fn should_retry_permission_probe(&self) -> bool {
        self.status.map_or(true, |status| {
            matches!(
                status.as_u16(),
                401 | 403 | 408 | 409 | 429 | 500 | 502 | 503 | 504
            )
        })
    }
}

impl fmt::Display for AzureRestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for AzureRestError {}

#[derive(Clone)]
pub(crate) struct AzureArmClient {
    config: AzureClientConfig,
    credential: Arc<dyn TokenCredential>,
    http_client: reqwest::Client,
}

impl AzureArmClient {
    pub(crate) fn new(config: AzureClientConfig) -> anyhow::Result<Self> {
        Ok(Self {
            credential: azure_credential_from_config(&config)?,
            config,
            http_client: reqwest::Client::new(),
        })
    }

    pub(crate) fn role_assignment_id(&self, scope: &Scope, role_assignment_name: String) -> String {
        format!(
            "/{}/providers/Microsoft.Authorization/roleAssignments/{}",
            scope.to_scope_string(&self.config),
            role_assignment_name
        )
    }

    pub(crate) async fn create_or_update_role_assignment(
        &self,
        role_assignment_id: String,
        role_assignment: &RoleAssignment,
    ) -> Result<(), AzureRestError> {
        let body = serde_json::to_string(role_assignment).map_err(|error| {
            AzureRestError::request(format!(
                "Failed to serialize Azure role assignment request: {error}"
            ))
        })?;

        self.request(
            Method::PUT,
            self.arm_url(&role_assignment_id, "2022-04-01"),
            Some(body),
            "Azure role assignment",
            &role_assignment_id,
        )
        .await?;
        Ok(())
    }

    pub(crate) async fn create_or_update_service_bus_queue(
        &self,
        resource_group_name: &str,
        namespace_name: &str,
        queue_name: &str,
    ) -> Result<(), AzureRestError> {
        let body = serde_json::to_string(&AzureServiceBusQueue {
            id: None,
            location: None,
            name: Some(queue_name.to_string()),
            properties: Some(AzureServiceBusQueueProperties::default()),
            type_: None,
        })
        .map_err(|error| {
            AzureRestError::request(format!(
                "Failed to serialize Azure Service Bus queue '{queue_name}' request: {error}"
            ))
        })?;

        self.request(
            Method::PUT,
            self.service_bus_queue_url(resource_group_name, namespace_name, queue_name),
            Some(body),
            "Azure Service Bus queue",
            queue_name,
        )
        .await?;
        Ok(())
    }

    pub(crate) async fn delete_service_bus_queue(
        &self,
        resource_group_name: &str,
        namespace_name: &str,
        queue_name: &str,
    ) -> Result<(), AzureRestError> {
        self.request(
            Method::DELETE,
            self.service_bus_queue_url(resource_group_name, namespace_name, queue_name),
            None,
            "Azure Service Bus queue",
            queue_name,
        )
        .await?;
        Ok(())
    }

    fn arm_url(&self, path: &str, api_version: &str) -> String {
        format!(
            "{}/{}?api-version={}",
            azure_management_endpoint(&self.config).trim_end_matches('/'),
            path.trim_start_matches('/'),
            api_version
        )
    }

    fn service_bus_queue_url(
        &self,
        resource_group_name: &str,
        namespace_name: &str,
        queue_name: &str,
    ) -> String {
        self.arm_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ServiceBus/namespaces/{}/queues/{}",
                self.config.subscription_id, resource_group_name, namespace_name, queue_name
            ),
            "2024-01-01",
        )
    }

    async fn bearer_token(&self) -> Result<AccessToken, AzureRestError> {
        self.credential
            .get_token(&[ARM_SCOPE], None)
            .await
            .map_err(|error| {
                AzureRestError::request(format!(
                    "Failed to get Azure management access token: {error}"
                ))
            })
    }

    async fn request(
        &self,
        method: Method,
        url: String,
        body: Option<String>,
        resource_type: &str,
        resource_name: &str,
    ) -> Result<String, AzureRestError> {
        let token = self.bearer_token().await?;
        let mut request = self
            .http_client
            .request(method, &url)
            .bearer_auth(token.token.secret());

        if let Some(body) = body {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body);
        }

        let response = request.send().await.map_err(|error| {
            AzureRestError::request(format!(
                "Azure ARM request failed for {resource_type} '{resource_name}': {error}"
            ))
        })?;
        let status = response.status();
        let text = response.text().await.map_err(|error| {
            AzureRestError::request(format!(
                "Failed to read Azure ARM response for {resource_type} '{resource_name}': {error}"
            ))
        })?;

        if !status.is_success() {
            return Err(AzureRestError::response(
                status,
                format!(
                    "Azure ARM request for {resource_type} '{resource_name}' returned HTTP {}: {}",
                    status.as_u16(),
                    text
                ),
            ));
        }

        Ok(text)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AzureServiceBusQueue {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    properties: Option<AzureServiceBusQueueProperties>,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    type_: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AzureServiceBusQueueProperties {}

#[derive(Debug)]
struct StaticAzureAccessTokenCredential {
    token: String,
}

#[async_trait::async_trait]
impl TokenCredential for StaticAzureAccessTokenCredential {
    async fn get_token(
        &self,
        scopes: &[&str],
        _options: Option<TokenRequestOptions<'_>>,
    ) -> azure_core::Result<AccessToken> {
        if scopes.is_empty() {
            return Err(azure_core::Error::with_message(
                azure_core::error::ErrorKind::Credential,
                "no scopes specified",
            ));
        }

        Ok(AccessToken::new(
            self.token.clone(),
            OffsetDateTime::now_utc() + AzureDuration::days(365),
        ))
    }
}

fn azure_credential_from_config(
    config: &AzureClientConfig,
) -> anyhow::Result<Arc<dyn TokenCredential>> {
    match &config.credentials {
        AzureCredentials::AccessToken { token } => Ok(Arc::new(StaticAzureAccessTokenCredential {
            token: token.clone(),
        })),
        AzureCredentials::ServicePrincipal {
            client_id,
            client_secret,
        } => ClientSecretCredential::new(
            &config.tenant_id,
            client_id.clone(),
            Secret::new(client_secret.clone()),
            Some(ClientSecretCredentialOptions {
                client_options: azure_client_options(None),
            }),
        )
        .map(|credential| credential as Arc<dyn TokenCredential>)
        .context("Failed to build official Azure service principal credentials"),
        AzureCredentials::WorkloadIdentity {
            client_id,
            tenant_id,
            federated_token_file,
            authority_host,
        } => WorkloadIdentityCredential::new(Some(WorkloadIdentityCredentialOptions {
            credential_options: ClientAssertionCredentialOptions {
                client_options: azure_client_options(Some(authority_host)),
            },
            client_id: Some(client_id.clone()),
            tenant_id: Some(tenant_id.clone()),
            token_file_path: Some(PathBuf::from(federated_token_file)),
        }))
        .map(|credential| credential as Arc<dyn TokenCredential>)
        .context("Failed to build official Azure workload identity credentials"),
        AzureCredentials::VmManagedIdentity {
            client_id,
            identity_endpoint,
        } => {
            if let Some(identity_endpoint) = identity_endpoint {
                anyhow::bail!(
                    "Official Azure ManagedIdentityCredential does not support per-config IMDS endpoint override '{}'; use the standard IMDS endpoint or provide an access token",
                    identity_endpoint
                );
            }

            ManagedIdentityCredential::new(Some(ManagedIdentityCredentialOptions {
                user_assigned_id: Some(UserAssignedId::ClientId(client_id.clone())),
                client_options: azure_client_options(None),
            }))
            .map(|credential| credential as Arc<dyn TokenCredential>)
            .context("Failed to build official Azure VM managed identity credentials")
        }
        AzureCredentials::ManagedIdentity {
            client_id,
            identity_endpoint,
            ..
        } => anyhow::bail!(
            "Official Azure ManagedIdentityCredential cannot be constructed from explicit App Service identity endpoint '{}' for client '{}'; use workload identity, VM managed identity, or provide an access token",
            identity_endpoint,
            client_id
        ),
    }
}

fn azure_client_options(authority_host: Option<&str>) -> ClientOptions {
    let cloud = authority_host.map(|authority_host| {
        let mut custom = CustomConfiguration::default();
        custom.authority_host = authority_host.to_string();
        Arc::new(CloudConfiguration::Custom(custom))
    });

    ClientOptions {
        cloud,
        ..Default::default()
    }
}

fn azure_management_endpoint(config: &AzureClientConfig) -> &str {
    config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("management"))
        .map(String::as_str)
        .unwrap_or("https://management.azure.com")
}
