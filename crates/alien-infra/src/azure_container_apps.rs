use crate::core::{LongRunningOperation, OperationResult};
use alien_client_core::{ErrorData as CloudClientErrorData, Result as CloudClientResult};
use alien_core::AzureClientConfig;
use alien_error::{AlienError, Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use azure_mgmt_app::package_preview_2024_08 as azure_app_2024_08;
use azure_mgmt_app::package_preview_2024_08::models::{
    container_app, Certificate, DaprComponent, ExtendedLocation, ManagedEnvironment,
    TrackedResource,
};
use futures_util::StreamExt;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{fmt::Debug, ops, sync::Arc, time::Duration};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerApp {
    #[serde(flatten)]
    pub tracked_resource: TrackedResource,
    /// Managed identity assigned to the app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<serde_json::Value>,
    /// Container App properties.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<ContainerAppProperties>,
    #[serde(
        rename = "extendedLocation",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub extended_location: Option<ExtendedLocation>,
    #[serde(rename = "managedBy", default, skip_serializing_if = "Option::is_none")]
    pub managed_by: Option<String>,
}

impl ops::Deref for ContainerApp {
    type Target = TrackedResource;

    fn deref(&self) -> &Self::Target {
        &self.tracked_resource
    }
}

impl ops::DerefMut for ContainerApp {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.tracked_resource
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContainerAppProperties {
    #[serde(flatten)]
    pub sdk: container_app::Properties,
    #[serde(
        rename = "runningStatus",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub running_status: Option<String>,
}

impl ops::Deref for ContainerAppProperties {
    type Target = container_app::Properties;

    fn deref(&self) -> &Self::Target {
        &self.sdk
    }
}

impl ops::DerefMut for ContainerAppProperties {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.sdk
    }
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

pub(crate) async fn delete_container_app(
    client: &azure_app_2024_08::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    container_app_name: &str,
) -> CloudClientResult<OperationResult<()>> {
    let result = client
        .container_apps_client()
        .delete(
            config.subscription_id.clone(),
            resource_group_name.to_string(),
            container_app_name.to_string(),
        )
        .send()
        .await;
    map_azure_core_021_delete_lro_response(
        "Azure Container Apps",
        result,
        "container app delete",
        "Azure Container App",
        container_app_name,
    )
    .await
}

pub struct OfficialAzureContainerAppsClient {
    config: AzureClientConfig,
    credential: Arc<dyn azure_core_021::auth::TokenCredential>,
    scopes: Vec<String>,
    pipeline: Arc<azure_core_021::Pipeline>,
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
    pub fn new(
        config: AzureClientConfig,
        credential: Arc<dyn azure_core_021::auth::TokenCredential>,
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
            config,
            credential,
            scopes: vec!["https://management.azure.com/.default".to_string()],
            pipeline: Arc::new(pipeline),
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
        method: azure_core_021::Method,
        url: String,
        body: Option<String>,
        resource_type: &str,
        resource_name: &str,
    ) -> CloudClientResult<AzureCoreArmResponse> {
        azure_core_arm_request(self, method, url, body, resource_type, resource_name).await
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

impl OfficialAzureContainerAppsClient {
    pub async fn create_or_update_container_app(
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

    pub async fn get_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
    ) -> CloudClientResult<ContainerApp> {
        let response = self
            .request(
                azure_core_021::Method::Get,
                self.container_app_url(resource_group_name, container_app_name),
                None,
                "Azure Container App",
                container_app_name,
            )
            .await?;
        self.parse_response("Azure Container App", container_app_name, &response.body)
    }

    pub async fn update_container_app(
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
        let response = self
            .request(
                azure_core_021::Method::Put,
                url,
                Some(body),
                resource_type,
                resource_name,
            )
            .await?;
        operation_result_from_response(
            response.status,
            &response.headers,
            &response.body,
            resource_type,
            resource_name,
        )
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
        let response = self
            .request(
                azure_core_021::Method::Patch,
                url,
                Some(body),
                resource_type,
                resource_name,
            )
            .await?;
        operation_result_from_response(
            response.status,
            &response.headers,
            &response.body,
            resource_type,
            resource_name,
        )
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

struct AzureCoreArmResponse {
    status: azure_core_021::StatusCode,
    headers: azure_core_021::headers::Headers,
    body: String,
}

async fn azure_core_arm_request(
    client: &OfficialAzureContainerAppsClient,
    method: azure_core_021::Method,
    url: String,
    body: Option<String>,
    resource_type: &str,
    resource_name: &str,
) -> CloudClientResult<AzureCoreArmResponse> {
    let scopes = client.scopes.iter().map(String::as_str).collect::<Vec<_>>();
    let token = client
        .credential
        .get_token(&scopes)
        .await
        .into_alien_error()
        .context(CloudClientErrorData::AuthenticationError {
            message: "Failed to get Azure management access token".to_string(),
        })?;
    let url = azure_core_021::Url::parse(&url)
        .into_alien_error()
        .context(CloudClientErrorData::HttpRequestFailed {
            message: format!("Invalid Azure ARM request URL for {resource_type} '{resource_name}'"),
        })?;
    let mut request = azure_core_021::Request::new(url.clone(), method);
    request.insert_header(
        azure_core_021::headers::AUTHORIZATION,
        format!("Bearer {}", token.token.secret()),
    );

    if let Some(body) = body {
        request.insert_header(azure_core_021::headers::CONTENT_TYPE, "application/json");
        request.insert_header(
            azure_core_021::headers::CONTENT_LENGTH,
            body.len().to_string(),
        );
        request.set_body(body);
    } else {
        request.set_body(azure_core_021::EMPTY_BODY);
    }

    let response = match client
        .pipeline
        .send(&azure_core_021::Context::default(), &mut request)
        .await
    {
        Ok(response) => response,
        Err(error) => {
            let mapped: CloudClientResult<azure_core_021::Response> = map_azure_core_021_sdk_error(
                "Azure Container Apps",
                Err(error),
                "ARM request",
                resource_type,
                resource_name,
            );
            return match mapped {
                Ok(_) => unreachable!("Azure Core send error unexpectedly mapped to success"),
                Err(error) => Err(error),
            };
        }
    };
    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .into_body()
        .collect()
        .await
        .into_alien_error()
        .context(CloudClientErrorData::HttpRequestFailed {
            message: format!(
                "Failed to read Azure ARM response for {resource_type} '{resource_name}'"
            ),
        })?;
    let text = String::from_utf8(body.to_vec())
        .into_alien_error()
        .context(CloudClientErrorData::SerializationError {
            message: format!(
                "Azure ARM response for {resource_type} '{resource_name}' was not UTF-8"
            ),
        })?;

    if status == azure_core_021::StatusCode::NotFound {
        return Err(AlienError::new(
            CloudClientErrorData::RemoteResourceNotFound {
                resource_type: resource_type.to_string(),
                resource_name: resource_name.to_string(),
            },
        ));
    }

    if status == azure_core_021::StatusCode::Conflict {
        return Err(AlienError::new(
            CloudClientErrorData::RemoteResourceConflict {
                resource_type: resource_type.to_string(),
                resource_name: resource_name.to_string(),
                message: text,
            },
        ));
    }

    if status == azure_core_021::StatusCode::Forbidden
        || status == azure_core_021::StatusCode::Unauthorized
    {
        return Err(AlienError::new(CloudClientErrorData::RemoteAccessDenied {
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
        }));
    }

    if !matches!(
        status,
        azure_core_021::StatusCode::Ok
            | azure_core_021::StatusCode::Created
            | azure_core_021::StatusCode::Accepted
            | azure_core_021::StatusCode::NoContent
    ) {
        return Err(AlienError::new(CloudClientErrorData::HttpResponseError {
            message: format!(
                "Azure ARM request for {resource_type} '{resource_name}' returned HTTP {}",
                status as u16
            ),
            url: url.to_string(),
            http_status: status as u16,
            http_request_text: None,
            http_response_text: Some(text),
        }));
    }

    Ok(AzureCoreArmResponse {
        status,
        headers,
        body: text,
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
    status: azure_core_021::StatusCode,
    headers: &azure_core_021::headers::Headers,
    body: &str,
    resource_type: &str,
    resource_name: &str,
) -> CloudClientResult<OperationResult<T>>
where
    T: DeserializeOwned,
{
    if status == azure_core_021::StatusCode::Accepted {
        return Ok(OperationResult::LongRunning(
            long_running_operation_from_azure_core_021_headers(
                headers,
                resource_type,
                resource_name,
            )?,
        ));
    }

    parse_response(resource_type, resource_name, body).map(OperationResult::Completed)
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
