use crate::core::{LongRunningOperation, OperationResult};
use alien_client_core::{ErrorData as CloudClientErrorData, Result as CloudClientResult};
use alien_core::AzureClientConfig;
use alien_error::{AlienError, Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use azure_mgmt_app::package_preview_2024_08 as azure_app_2024_08;
use azure_mgmt_app::package_preview_2024_08::models::{
    Certificate, ContainerApp, DaprComponent, ManagedEnvironment, TrackedResource,
};
use futures_util::StreamExt;
use std::{sync::Arc, time::Duration};

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

pub(crate) async fn get_container_app(
    client: &azure_app_2024_08::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    container_app_name: &str,
) -> CloudClientResult<ContainerApp> {
    let result = client
        .container_apps_client()
        .get(
            config.subscription_id.clone(),
            resource_group_name.to_string(),
            container_app_name.to_string(),
        )
        .await;
    map_azure_core_021_sdk_error(
        "Azure Container Apps",
        result,
        "container app get",
        "Azure Container App",
        container_app_name,
    )
}

pub(crate) async fn create_or_update_container_app(
    client: &azure_app_2024_08::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    container_app_name: &str,
    container_app: &ContainerApp,
) -> CloudClientResult<OperationResult<ContainerApp>> {
    let result = client
        .container_apps_client()
        .create_or_update(
            config.subscription_id.clone(),
            resource_group_name.to_string(),
            container_app_name.to_string(),
            container_app.clone(),
        )
        .send()
        .await;
    map_azure_core_021_lro_response(
        "Azure Container Apps",
        result,
        "container app create or update",
        "Azure Container App",
        container_app_name,
        |response| response.into_body(),
    )
    .await
}

pub(crate) async fn update_container_app(
    client: &azure_app_2024_08::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    container_app_name: &str,
    container_app: &ContainerApp,
) -> CloudClientResult<OperationResult<ContainerApp>> {
    let result = client
        .container_apps_client()
        .update(
            config.subscription_id.clone(),
            resource_group_name.to_string(),
            container_app_name.to_string(),
            container_app.clone(),
        )
        .send()
        .await;
    map_azure_core_021_lro_response(
        "Azure Container Apps",
        result,
        "container app update",
        "Azure Container App",
        container_app_name,
        |response| response.into_body(),
    )
    .await
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

pub(crate) async fn get_managed_environment_certificate(
    client: &azure_app_2024_08::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    environment_name: &str,
    certificate_name: &str,
) -> CloudClientResult<Certificate> {
    let result = client
        .certificates_client()
        .get(
            config.subscription_id.clone(),
            resource_group_name.to_string(),
            environment_name.to_string(),
            certificate_name.to_string(),
        )
        .await;
    map_azure_core_021_sdk_error(
        "Azure Container Apps",
        result,
        "managed environment certificate get",
        "Azure Container Apps Managed Environment Certificate",
        certificate_name,
    )
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

#[derive(Debug)]
pub(crate) struct ContainerAppUserAssignedIdentityPolicy;

#[async_trait::async_trait]
impl azure_core_021::Policy for ContainerAppUserAssignedIdentityPolicy {
    async fn send(
        &self,
        ctx: &azure_core_021::Context,
        request: &mut azure_core_021::Request,
        next: &[Arc<dyn azure_core_021::Policy>],
    ) -> azure_core_021::PolicyResult {
        inject_user_assigned_identities(request)?;
        next[0].send(ctx, request, &next[1..]).await
    }
}

fn inject_user_assigned_identities(
    request: &mut azure_core_021::Request,
) -> azure_core_021::Result<()> {
    if !matches!(
        request.method(),
        &azure_core_021::Method::Put | &azure_core_021::Method::Patch
    ) || !request
        .url()
        .path()
        .contains("/providers/Microsoft.App/containerApps/")
    {
        return Ok(());
    }

    let azure_core_021::Body::Bytes(body) = request.body() else {
        return Ok(());
    };
    let mut body: serde_json::Value = serde_json::from_slice(body)?;
    let Some(identity_settings) = body
        .pointer("/properties/configuration/identitySettings")
        .and_then(serde_json::Value::as_array)
    else {
        return Ok(());
    };

    let mut user_assigned_identities = serde_json::Map::new();
    for identity in identity_settings.iter().filter_map(|identity_setting| {
        identity_setting
            .get("identity")
            .and_then(serde_json::Value::as_str)
            .filter(|identity| !identity.is_empty())
    }) {
        user_assigned_identities.insert(identity.to_string(), serde_json::json!({}));
    }

    if user_assigned_identities.is_empty() {
        return Ok(());
    }

    body["identity"] = serde_json::json!({
        "type": "UserAssigned",
        "userAssignedIdentities": user_assigned_identities,
    });
    let body = serde_json::to_vec(&body)?;
    request.insert_header(
        azure_core_021::headers::CONTENT_LENGTH,
        body.len().to_string(),
    );
    request.set_body(body);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injects_user_assigned_identity_map_from_generated_identity_settings() {
        let mut request = azure_core_021::Request::new(
            azure_core_021::Url::parse(
                "https://management.azure.com/subscriptions/sub/resourceGroups/rg/providers/Microsoft.App/containerApps/app?api-version=2024-08-02-preview",
            )
            .expect("test URL should parse"),
            azure_core_021::Method::Put,
        );
        request.set_body(
            serde_json::json!({
                "properties": {
                    "configuration": {
                        "identitySettings": [
                            {
                                "identity": "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/app-sa",
                                "lifecycle": "All"
                            }
                        ]
                    }
                }
            })
            .to_string(),
        );

        inject_user_assigned_identities(&mut request).expect("identity injection should succeed");

        let azure_core_021::Body::Bytes(body) = request.body() else {
            panic!("test request should use a byte body");
        };
        let body: serde_json::Value =
            serde_json::from_slice(body).expect("mutated request body should be JSON");
        assert_eq!(body["identity"]["type"], "UserAssigned");
        assert_eq!(
            body["identity"]["userAssignedIdentities"]
                ["/subscriptions/sub/resourceGroups/rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/app-sa"],
            serde_json::json!({})
        );
    }

    #[test]
    fn leaves_container_app_request_without_identity_settings_unchanged() {
        let body = serde_json::json!({
            "properties": {
                "configuration": {
                    "identitySettings": []
                }
            }
        })
        .to_string();
        let mut request = azure_core_021::Request::new(
            azure_core_021::Url::parse(
                "https://management.azure.com/subscriptions/sub/resourceGroups/rg/providers/Microsoft.App/containerApps/app?api-version=2024-08-02-preview",
            )
            .expect("test URL should parse"),
            azure_core_021::Method::Patch,
        );
        request.set_body(body.clone());

        inject_user_assigned_identities(&mut request).expect("identity injection should succeed");

        let azure_core_021::Body::Bytes(mutated_body) = request.body() else {
            panic!("test request should use a byte body");
        };
        assert_eq!(mutated_body.as_ref(), body.as_bytes());
    }
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
