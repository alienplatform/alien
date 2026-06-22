use crate::core::{LongRunningOperation, OperationResult};
use alien_client_core::{ErrorData as CloudClientErrorData, Result as CloudClientResult};
use alien_core::AzureClientConfig;
use alien_error::{AlienError, Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use azure_core::credentials::{AccessToken, TokenCredential};
use azure_mgmt_app::package_preview_2024_08 as azure_app_2024_08;
use azure_mgmt_app::package_preview_2024_08::models::{
    container_app, Certificate, Configuration, DaprComponent, ManagedEnvironment, Template,
    TrackedResource,
};
use futures_util::StreamExt;
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use std::{collections::HashMap, fmt::Debug, sync::Arc, time::Duration};

#[cfg(any(test, feature = "test-utils"))]
use mockall::automock;

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait ContainerAppsApi: Send + Sync + Debug {
    async fn create_or_update_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
        container_app: &ContainerApp,
    ) -> CloudClientResult<OperationResult<ContainerApp>>;

    async fn update_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
        container_app: &ContainerApp,
    ) -> CloudClientResult<OperationResult<ContainerApp>>;

    async fn get_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
    ) -> CloudClientResult<ContainerApp>;

    async fn delete_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
    ) -> CloudClientResult<OperationResult<()>>;
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContainerApp {
    /// Fully qualified Azure resource ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Managed identity assigned to the app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<serde_json::Value>,
    /// Azure region.
    pub location: String,
    /// Optional resource name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Container App properties.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<ContainerAppProperties>,
    /// Resource tags.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tags: HashMap<String, String>,
    /// Resource type.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    #[serde(
        rename = "extendedLocation",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub extended_location: Option<serde_json::Value>,
    #[serde(rename = "managedBy", default, skip_serializing_if = "Option::is_none")]
    pub managed_by: Option<String>,
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub system_data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContainerAppProperties {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<Configuration>,
    #[serde(
        rename = "customDomainVerificationId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub custom_domain_verification_id: Option<String>,
    #[serde(
        rename = "environmentId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub environment_id: Option<String>,
    #[serde(
        rename = "eventStreamEndpoint",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub event_stream_endpoint: Option<String>,
    #[serde(
        rename = "latestReadyRevisionName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub latest_ready_revision_name: Option<String>,
    #[serde(
        rename = "latestRevisionFqdn",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub latest_revision_fqdn: Option<String>,
    #[serde(
        rename = "latestRevisionName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub latest_revision_name: Option<String>,
    #[serde(
        rename = "managedEnvironmentId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub managed_environment_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "null_to_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub outbound_ip_addresses: Vec<String>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub provisioning_state: Option<container_app::properties::ProvisioningState>,
    #[serde(
        rename = "runningStatus",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub running_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<Template>,
    #[serde(
        rename = "workloadProfileName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub workload_profile_name: Option<String>,
}

pub(crate) async fn create_or_update_managed_environment(
    client: &azure_app_2024_08::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    environment_name: &str,
    managed_environment: &ManagedEnvironment,
) -> CloudClientResult<OperationResult<ManagedEnvironment>> {
    let result = client
        .managed_environments_client()
        .create_or_update(
            config.subscription_id.clone(),
            resource_group_name.to_string(),
            environment_name.to_string(),
            managed_environment.clone(),
        )
        .send()
        .await;
    map_azure_core_021_lro_response(
        "Azure Container Apps",
        result,
        "managed environment create or update",
        "Azure Container Apps Managed Environment",
        environment_name,
        |response| response.into_body(),
    )
    .await
}

pub(crate) async fn get_managed_environment(
    client: &azure_app_2024_08::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    environment_name: &str,
) -> CloudClientResult<ManagedEnvironment> {
    let result = client
        .managed_environments_client()
        .get(
            config.subscription_id.clone(),
            resource_group_name.to_string(),
            environment_name.to_string(),
        )
        .await;
    map_azure_core_021_sdk_error(
        "Azure Container Apps",
        result,
        "managed environment get",
        "Azure Container Apps Managed Environment",
        environment_name,
    )
}

pub(crate) async fn delete_managed_environment(
    client: &azure_app_2024_08::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    environment_name: &str,
) -> CloudClientResult<OperationResult<()>> {
    let result = client
        .managed_environments_client()
        .delete(
            config.subscription_id.clone(),
            resource_group_name.to_string(),
            environment_name.to_string(),
        )
        .send()
        .await;
    map_azure_core_021_delete_lro_response(
        "Azure Container Apps",
        result,
        "managed environment delete",
        "Azure Container Apps Managed Environment",
        environment_name,
    )
    .await
}

pub(crate) async fn list_container_apps_by_resource_group(
    client: &azure_app_2024_08::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
) -> CloudClientResult<azure_app_2024_08::models::ContainerAppCollection> {
    let mut stream = client
        .container_apps_client()
        .list_by_resource_group(
            config.subscription_id.clone(),
            resource_group_name.to_string(),
        )
        .into_stream();
    let mut apps = Vec::new();
    while let Some(page) = stream.next().await {
        let page = map_azure_core_021_sdk_error(
            "Azure Container Apps",
            page,
            "container apps list by resource group",
            "Azure Container Apps",
            resource_group_name,
        )?;
        apps.extend(page.value);
    }
    Ok(azure_app_2024_08::models::ContainerAppCollection::new(apps))
}

pub(crate) async fn create_or_update_managed_environment_certificate(
    client: &azure_app_2024_08::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    environment_name: &str,
    certificate_name: &str,
    certificate: &Certificate,
) -> CloudClientResult<Certificate> {
    let result = client
        .certificates_client()
        .create_or_update(
            config.subscription_id.clone(),
            resource_group_name.to_string(),
            environment_name.to_string(),
            certificate_name.to_string(),
        )
        .certificate_envelope(certificate.clone())
        .send()
        .await;
    let response = map_azure_core_021_sdk_error(
        "Azure Container Apps",
        result,
        "managed environment certificate create or update",
        "Azure Container Apps Managed Environment Certificate",
        certificate_name,
    )?;
    parse_azure_core_021_response_body_or_default_certificate(
        response.into_raw_response(),
        "Azure Container Apps Managed Environment Certificate",
        certificate_name,
    )
    .await
}

pub(crate) async fn delete_managed_environment_certificate(
    client: &azure_app_2024_08::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    environment_name: &str,
    certificate_name: &str,
) -> CloudClientResult<OperationResult<()>> {
    let result = client
        .certificates_client()
        .delete(
            config.subscription_id.clone(),
            resource_group_name.to_string(),
            environment_name.to_string(),
            certificate_name.to_string(),
        )
        .send()
        .await;
    map_azure_core_021_delete_lro_response(
        "Azure Container Apps",
        result,
        "managed environment certificate delete",
        "Azure Container Apps Managed Environment Certificate",
        certificate_name,
    )
    .await
}

pub(crate) async fn create_or_update_dapr_component(
    client: &azure_app_2024_08::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    environment_name: &str,
    component_name: &str,
    dapr_component: &DaprComponent,
) -> CloudClientResult<OperationResult<DaprComponent>> {
    let result = client
        .dapr_components_client()
        .create_or_update(
            config.subscription_id.clone(),
            resource_group_name.to_string(),
            environment_name.to_string(),
            component_name.to_string(),
            dapr_component.clone(),
        )
        .send()
        .await;
    map_azure_core_021_lro_response(
        "Azure Container Apps",
        result,
        "Dapr component create or update",
        "Azure Container Apps Dapr Component",
        component_name,
        |response| response.into_body(),
    )
    .await
}

pub(crate) async fn delete_dapr_component(
    client: &azure_app_2024_08::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    environment_name: &str,
    component_name: &str,
) -> CloudClientResult<OperationResult<()>> {
    let result = client
        .dapr_components_client()
        .delete(
            config.subscription_id.clone(),
            resource_group_name.to_string(),
            environment_name.to_string(),
            component_name.to_string(),
        )
        .send()
        .await;
    map_azure_core_021_delete_lro_response(
        "Azure Container Apps",
        result,
        "Dapr component delete",
        "Azure Container Apps Dapr Component",
        component_name,
    )
    .await
}

pub struct OfficialAzureContainerAppsClient {
    config: AzureClientConfig,
    credential: Arc<dyn TokenCredential>,
    http_client: reqwest::Client,
}

impl Debug for OfficialAzureContainerAppsClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OfficialAzureContainerAppsClient")
            .field("subscription_id", &self.config.subscription_id)
            .finish_non_exhaustive()
    }
}

impl OfficialAzureContainerAppsClient {
    pub fn new(config: AzureClientConfig, credential: Arc<dyn TokenCredential>) -> Self {
        Self {
            config,
            credential,
            http_client: reqwest::Client::new(),
        }
    }

    fn base_url(&self) -> String {
        crate::core::azure_management_endpoint(&self.config)
            .trim_end_matches('/')
            .to_string()
    }

    fn container_app_url(&self, resource_group_name: &str, container_app_name: &str) -> String {
        format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/containerApps/{}?api-version=2025-01-01",
            self.base_url(), self.config.subscription_id, resource_group_name, container_app_name
        )
    }

    async fn request(
        &self,
        method: reqwest::Method,
        url: String,
        body: Option<String>,
        resource_type: &str,
        resource_name: &str,
    ) -> CloudClientResult<(reqwest::StatusCode, reqwest::header::HeaderMap, String)> {
        azure_arm_request(
            &self.http_client,
            self.credential.as_ref(),
            method,
            url,
            body,
            resource_type,
            resource_name,
        )
        .await
    }

    fn parse_response<T: DeserializeOwned>(
        &self,
        resource_type: &str,
        resource_name: &str,
        body: &str,
    ) -> CloudClientResult<T> {
        parse_response(resource_type, resource_name, body)
    }
}

#[async_trait::async_trait]
impl ContainerAppsApi for OfficialAzureContainerAppsClient {
    async fn create_or_update_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
        container_app: &ContainerApp,
    ) -> CloudClientResult<OperationResult<ContainerApp>> {
        self.put_lro(
            self.container_app_url(resource_group_name, container_app_name),
            container_app,
            "Azure Container App",
            container_app_name,
        )
        .await
    }

    async fn get_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
    ) -> CloudClientResult<ContainerApp> {
        let (_, _, body) = self
            .request(
                reqwest::Method::GET,
                self.container_app_url(resource_group_name, container_app_name),
                None,
                "Azure Container App",
                container_app_name,
            )
            .await?;
        self.parse_response("Azure Container App", container_app_name, &body)
    }

    async fn update_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
        container_app: &ContainerApp,
    ) -> CloudClientResult<OperationResult<ContainerApp>> {
        self.patch_lro(
            self.container_app_url(resource_group_name, container_app_name),
            container_app,
            "Azure Container App",
            container_app_name,
        )
        .await
    }

    async fn delete_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
    ) -> CloudClientResult<OperationResult<()>> {
        self.delete_lro(
            self.container_app_url(resource_group_name, container_app_name),
            "Azure Container App",
            container_app_name,
        )
        .await
    }
}

impl OfficialAzureContainerAppsClient {
    async fn put_lro<T>(
        &self,
        url: String,
        resource: &T,
        resource_type: &str,
        resource_name: &str,
    ) -> CloudClientResult<OperationResult<T>>
    where
        T: Serialize + DeserializeOwned,
    {
        let body = serialize_request(resource_type, resource_name, resource)?;
        let (status, headers, body) = self
            .request(
                reqwest::Method::PUT,
                url,
                Some(body),
                resource_type,
                resource_name,
            )
            .await?;
        operation_result_from_response(status, &headers, &body, resource_type, resource_name)
    }

    async fn patch_lro<T>(
        &self,
        url: String,
        resource: &T,
        resource_type: &str,
        resource_name: &str,
    ) -> CloudClientResult<OperationResult<T>>
    where
        T: Serialize + DeserializeOwned,
    {
        let body = serialize_request(resource_type, resource_name, resource)?;
        let (status, headers, body) = self
            .request(
                reqwest::Method::PATCH,
                url,
                Some(body),
                resource_type,
                resource_name,
            )
            .await?;
        operation_result_from_response(status, &headers, &body, resource_type, resource_name)
    }

    async fn delete_lro(
        &self,
        url: String,
        resource_type: &str,
        resource_name: &str,
    ) -> CloudClientResult<OperationResult<()>> {
        let (status, headers, _) = self
            .request(
                reqwest::Method::DELETE,
                url,
                None,
                resource_type,
                resource_name,
            )
            .await?;
        if status == reqwest::StatusCode::ACCEPTED {
            return Ok(OperationResult::LongRunning(
                long_running_operation_from_headers(&headers, resource_type, resource_name)?,
            ));
        }
        Ok(OperationResult::Completed(()))
    }
}

#[derive(Clone)]
pub struct AzureLongRunningOperationClient {
    credential: Arc<dyn azure_core_021::auth::TokenCredential>,
    scopes: Vec<String>,
    pipeline: Arc<azure_core_021::Pipeline>,
}

impl Debug for AzureLongRunningOperationClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AzureLongRunningOperationClient")
            .finish_non_exhaustive()
    }
}

impl AzureLongRunningOperationClient {
    pub fn new(
        credential: Arc<dyn azure_core_021::auth::TokenCredential>,
        scopes: Vec<String>,
        options: azure_core_021::ClientOptions,
    ) -> Self {
        let pipeline = azure_core_021::Pipeline::new(
            option_env!("CARGO_PKG_NAME"),
            option_env!("CARGO_PKG_VERSION"),
            options,
            Vec::new(),
            Vec::new(),
        );
        Self {
            credential,
            scopes,
            pipeline: Arc::new(pipeline),
        }
    }

    pub async fn check_status(
        &self,
        operation: &LongRunningOperation,
        operation_name: &str,
        resource_name: &str,
    ) -> CloudClientResult<Option<String>> {
        let scopes = self.scopes.iter().map(String::as_str).collect::<Vec<_>>();
        let token = self
            .credential
            .get_token(&scopes)
            .await
            .into_alien_error()
            .context(CloudClientErrorData::AuthenticationError {
                message: "Failed to get Azure management access token".to_string(),
            })?;
        let url = azure_core_021::Url::parse(&operation.url)
            .into_alien_error()
            .context(CloudClientErrorData::HttpRequestFailed {
                message: format!(
                    "Invalid Azure {operation_name} polling URL for '{resource_name}'"
                ),
            })?;
        let mut request = azure_core_021::Request::new(url, azure_core_021::Method::Get);
        request.insert_header(
            azure_core_021::headers::AUTHORIZATION,
            format!("Bearer {}", token.token.secret()),
        );

        let response = self
            .pipeline
            .send(&azure_core_021::Context::default(), &mut request)
            .await
            .into_alien_error()
            .context(CloudClientErrorData::HttpRequestFailed {
                message: format!(
                    "Azure {operation_name} polling request failed for '{resource_name}'"
                ),
            })?;
        let status = response.status();
        let body = response
            .into_body()
            .collect()
            .await
            .into_alien_error()
            .context(CloudClientErrorData::HttpRequestFailed {
                message: format!(
                    "Failed to read Azure {operation_name} polling response for '{resource_name}'"
                ),
            })?;
        let body = String::from_utf8(body.to_vec())
            .into_alien_error()
            .context(CloudClientErrorData::SerializationError {
                message: format!(
                    "Azure {operation_name} polling response for '{resource_name}' was not UTF-8"
                ),
            })?;

        match status {
            azure_core_021::StatusCode::Ok => {
                azure_operation_body_status(operation, operation_name, resource_name, body)
            }
            azure_core_021::StatusCode::NoContent => Ok(Some(String::new())),
            azure_core_021::StatusCode::Accepted => Ok(None),
            _ => Err(AlienError::new(CloudClientErrorData::HttpResponseError {
                message: format!(
                    "Azure {operation_name} for '{resource_name}' returned HTTP {}",
                    status as u16
                ),
                url: operation.url.clone(),
                http_status: status as u16,
                http_request_text: None,
                http_response_text: Some(body),
            })),
        }
    }
}

async fn azure_arm_request(
    http_client: &reqwest::Client,
    credential: &dyn TokenCredential,
    method: reqwest::Method,
    url: String,
    body: Option<String>,
    resource_type: &str,
    resource_name: &str,
) -> CloudClientResult<(reqwest::StatusCode, reqwest::header::HeaderMap, String)> {
    let token = azure_bearer_token(credential).await?;
    let mut request = http_client
        .request(method, &url)
        .bearer_auth(token.token.secret());

    if let Some(body) = body {
        request = request
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body);
    }

    let response = request.send().await.into_alien_error().context(
        CloudClientErrorData::HttpRequestFailed {
            message: format!("Azure ARM request failed for {resource_type} '{resource_name}'"),
        },
    )?;
    let status = response.status();
    let headers = response.headers().clone();
    let text = response.text().await.into_alien_error().context(
        CloudClientErrorData::HttpRequestFailed {
            message: format!(
                "Failed to read Azure ARM response for {resource_type} '{resource_name}'"
            ),
        },
    )?;

    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(AlienError::new(
            CloudClientErrorData::RemoteResourceNotFound {
                resource_type: resource_type.to_string(),
                resource_name: resource_name.to_string(),
            },
        ));
    }

    if status == reqwest::StatusCode::CONFLICT {
        return Err(AlienError::new(
            CloudClientErrorData::RemoteResourceConflict {
                resource_type: resource_type.to_string(),
                resource_name: resource_name.to_string(),
                message: text,
            },
        ));
    }

    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(AlienError::new(CloudClientErrorData::RemoteAccessDenied {
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
        }));
    }

    if !status.is_success() {
        return Err(AlienError::new(CloudClientErrorData::HttpResponseError {
            message: format!(
                "Azure ARM request for {resource_type} '{resource_name}' returned HTTP {}",
                status.as_u16()
            ),
            url,
            http_status: status.as_u16(),
            http_request_text: None,
            http_response_text: Some(text),
        }));
    }

    Ok((status, headers, text))
}

async fn azure_bearer_token(credential: &dyn TokenCredential) -> CloudClientResult<AccessToken> {
    credential
        .get_token(&["https://management.azure.com/.default"], None)
        .await
        .into_alien_error()
        .context(CloudClientErrorData::AuthenticationError {
            message: "Failed to get Azure management access token".to_string(),
        })
}

fn map_azure_core_021_sdk_error<T>(
    service_name: &str,
    result: azure_core_021::Result<T>,
    action: &str,
    resource_type: &str,
    resource_name: &str,
) -> CloudClientResult<T> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            let http_status = match error.kind() {
                azure_core_021::error::ErrorKind::HttpResponse { status, .. } => Some(*status),
                _ => None,
            };

            match http_status {
                Some(azure_core_021::StatusCode::NotFound) => Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: resource_type.to_string(),
                        resource_name: resource_name.to_string(),
                    },
                )),
                Some(azure_core_021::StatusCode::Conflict) => Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceConflict {
                        resource_type: resource_type.to_string(),
                        resource_name: resource_name.to_string(),
                        message: error.to_string(),
                    },
                )),
                Some(azure_core_021::StatusCode::Forbidden)
                | Some(azure_core_021::StatusCode::Unauthorized) => {
                    Err(AlienError::new(CloudClientErrorData::RemoteAccessDenied {
                        resource_type: resource_type.to_string(),
                        resource_name: resource_name.to_string(),
                    }))
                }
                Some(status) => Err(AlienError::new(CloudClientErrorData::HttpResponseError {
                    message: format!(
                        "{service_name} {action} for {resource_type} '{resource_name}' returned HTTP {}",
                        status as u16
                    ),
                    url: String::new(),
                    http_status: status as u16,
                    http_request_text: None,
                    http_response_text: Some(error.to_string()),
                })),
                None => Err(error.into_alien_error().context(
                    CloudClientErrorData::HttpRequestFailed {
                        message: format!(
                            "{service_name} {action} failed for {resource_type} '{resource_name}'"
                        ),
                    },
                )),
            }
        }
    }
}

async fn map_azure_core_021_lro_response<T, R, F, Fut>(
    service_name: &str,
    result: azure_core_021::Result<R>,
    action: &str,
    resource_type: &str,
    resource_name: &str,
    into_body: F,
) -> CloudClientResult<OperationResult<T>>
where
    R: AsRef<azure_core_021::Response>,
    F: FnOnce(R) -> Fut,
    Fut: std::future::Future<Output = azure_core_021::Result<T>>,
{
    let response =
        map_azure_core_021_sdk_error(service_name, result, action, resource_type, resource_name)?;
    if response.as_ref().status() == azure_core_021::StatusCode::Accepted {
        let operation = long_running_operation_from_azure_core_021_headers(
            response.as_ref().headers(),
            resource_type,
            resource_name,
        )?;
        Ok(OperationResult::LongRunning(operation))
    } else {
        let body = into_body(response).await.into_alien_error().context(
            CloudClientErrorData::SerializationError {
                message: format!(
                    "Failed to parse {service_name} {action} response for {resource_type} '{resource_name}'"
                ),
            },
        )?;
        Ok(OperationResult::Completed(body))
    }
}

async fn map_azure_core_021_delete_lro_response<R>(
    service_name: &str,
    result: azure_core_021::Result<R>,
    action: &str,
    resource_type: &str,
    resource_name: &str,
) -> CloudClientResult<OperationResult<()>>
where
    R: AsRef<azure_core_021::Response>,
{
    let response =
        map_azure_core_021_sdk_error(service_name, result, action, resource_type, resource_name)?;
    if response.as_ref().status() == azure_core_021::StatusCode::Accepted {
        let operation = long_running_operation_from_azure_core_021_headers(
            response.as_ref().headers(),
            resource_type,
            resource_name,
        )?;
        Ok(OperationResult::LongRunning(operation))
    } else {
        Ok(OperationResult::Completed(()))
    }
}

fn long_running_operation_from_azure_core_021_headers(
    headers: &azure_core_021::headers::Headers,
    resource_type: &str,
    resource_name: &str,
) -> CloudClientResult<LongRunningOperation> {
    let async_operation_url = headers
        .get_optional_str(&azure_core_021::headers::AZURE_ASYNCOPERATION)
        .map(ToString::to_string);
    let location_url = headers
        .get_optional_str(&azure_core_021::headers::LOCATION)
        .map(ToString::to_string);
    let url = async_operation_url
        .clone()
        .or_else(|| location_url.clone())
        .ok_or_else(|| {
            AlienError::new(CloudClientErrorData::GenericError {
                message: format!(
                    "{resource_type} '{resource_name}' returned 202 without Azure-AsyncOperation or Location header"
                ),
            })
        })?;
    let retry_after = headers
        .get_optional_str(&azure_core_021::headers::RETRY_AFTER)
        .map(|value| {
            value
                .parse::<u64>()
                .map(Duration::from_secs)
                .map_err(|error| {
                    AlienError::new(CloudClientErrorData::SerializationError {
                        message: format!(
                            "Failed to parse Azure Retry-After header '{value}': {error}"
                        ),
                    })
                })
        })
        .transpose()?;

    Ok(LongRunningOperation {
        url,
        retry_after,
        location_url: async_operation_url.and(location_url),
    })
}

async fn parse_azure_core_021_response_body_or_default_certificate(
    response: azure_core_021::Response,
    resource_type: &str,
    resource_name: &str,
) -> CloudClientResult<Certificate> {
    let body = response
        .into_body()
        .collect()
        .await
        .into_alien_error()
        .context(CloudClientErrorData::HttpRequestFailed {
            message: format!("Failed to read {resource_type} '{resource_name}' response"),
        })?;

    if body.is_empty() {
        return Ok(Certificate::new(TrackedResource::new(String::new())));
    }

    serde_json::from_slice(&body).into_alien_error().context(
        CloudClientErrorData::SerializationError {
            message: format!("Failed to parse {resource_type} '{resource_name}' response"),
        },
    )
}

fn operation_result_from_response<T>(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: &str,
    resource_type: &str,
    resource_name: &str,
) -> CloudClientResult<OperationResult<T>>
where
    T: DeserializeOwned,
{
    if status == reqwest::StatusCode::ACCEPTED {
        return Ok(OperationResult::LongRunning(
            long_running_operation_from_headers(headers, resource_type, resource_name)?,
        ));
    }

    parse_response(resource_type, resource_name, body).map(OperationResult::Completed)
}

fn long_running_operation_from_headers(
    headers: &reqwest::header::HeaderMap,
    resource_type: &str,
    resource_name: &str,
) -> CloudClientResult<LongRunningOperation> {
    let async_operation_url = header_to_string(headers, "azure-asyncoperation")?;
    let location_url = header_to_string(headers, "location")?;
    let url = async_operation_url
        .clone()
        .or_else(|| location_url.clone())
        .ok_or_else(|| {
            AlienError::new(CloudClientErrorData::GenericError {
                message: format!(
                    "{resource_type} '{resource_name}' returned 202 without Azure-AsyncOperation or Location header"
                ),
            })
        })?;
    let retry_after = header_to_string(headers, "retry-after")?
        .map(|value| {
            value
                .parse::<u64>()
                .map(Duration::from_secs)
                .map_err(|error| {
                    AlienError::new(CloudClientErrorData::SerializationError {
                        message: format!(
                            "Failed to parse Azure Retry-After header '{value}': {error}"
                        ),
                    })
                })
        })
        .transpose()?;

    Ok(LongRunningOperation {
        url,
        retry_after,
        location_url: async_operation_url.and(location_url),
    })
}

fn header_to_string(
    headers: &reqwest::header::HeaderMap,
    name: &'static str,
) -> CloudClientResult<Option<String>> {
    headers
        .get(name)
        .map(|value| {
            value.to_str().map(ToString::to_string).map_err(|error| {
                AlienError::new(CloudClientErrorData::SerializationError {
                    message: format!("Failed to parse Azure {name} header: {error}"),
                })
            })
        })
        .transpose()
}

fn serialize_request<T: Serialize>(
    resource_type: &str,
    resource_name: &str,
    request: &T,
) -> CloudClientResult<String> {
    serde_json::to_string(request).into_alien_error().context(
        CloudClientErrorData::SerializationError {
            message: format!("Failed to serialize {resource_type} '{resource_name}' request"),
        },
    )
}

fn parse_response<T: DeserializeOwned>(
    resource_type: &str,
    resource_name: &str,
    body: &str,
) -> CloudClientResult<T> {
    serde_json::from_str(body).into_alien_error().context(
        CloudClientErrorData::SerializationError {
            message: format!("Failed to parse {resource_type} '{resource_name}' response"),
        },
    )
}

fn azure_operation_body_status(
    operation: &LongRunningOperation,
    operation_name: &str,
    resource_name: &str,
    body: String,
) -> CloudClientResult<Option<String>> {
    if body.trim().is_empty() {
        return Ok(Some(body));
    }

    let value: serde_json::Value = serde_json::from_str(&body).into_alien_error().context(
        CloudClientErrorData::HttpResponseError {
            message: format!("Azure {operation_name}: failed to parse operation JSON"),
            url: operation.url.clone(),
            http_status: 200,
            http_request_text: None,
            http_response_text: Some(body.clone()),
        },
    )?;

    if let Some(status) = value.get("status").and_then(serde_json::Value::as_str) {
        match status.to_ascii_lowercase().as_str() {
            "succeeded" => return Ok(Some(body)),
            "failed" | "canceled" => {
                return Err(AlienError::new(CloudClientErrorData::GenericError {
                    message: format!(
                        "Azure {operation_name} for '{resource_name}' {}: {}",
                        status.to_ascii_lowercase(),
                        value
                            .get("error")
                            .map(ToString::to_string)
                            .unwrap_or_else(|| "no error details".to_string())
                    ),
                }));
            }
            _ => return Ok(None),
        }
    }

    if let Some(state) = value
        .get("properties")
        .and_then(|properties| properties.get("provisioningState"))
        .and_then(serde_json::Value::as_str)
    {
        match state.to_ascii_lowercase().as_str() {
            "succeeded" => return Ok(Some(body)),
            "failed" | "canceled" => {
                return Err(AlienError::new(CloudClientErrorData::GenericError {
                    message: format!(
                        "Azure {operation_name} for '{resource_name}' failed with provisioningState: {state}"
                    ),
                }));
            }
            _ => return Ok(None),
        }
    }

    Ok(Some(body))
}

fn null_to_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}
