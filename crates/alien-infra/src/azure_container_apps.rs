use crate::core::{
    map_azure_core_021_delete_lro_response, map_azure_core_021_lro_response,
    map_azure_core_021_sdk_error, OperationResult,
};
use crate::error::{ErrorData, Result};
use alien_core::AzureClientConfig;
use alien_error::{Context, IntoAlienError};
use azure_mgmt_app::package_preview_2024_08 as azure_app_2024_08;
use azure_mgmt_app::package_preview_2024_08::models::{
    Certificate, DaprComponent, TrackedResource,
};
use std::sync::Arc;

pub(crate) async fn create_or_update_managed_environment_certificate(
    client: &azure_app_2024_08::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    environment_name: &str,
    certificate_name: &str,
    certificate: &Certificate,
) -> Result<Certificate> {
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
) -> Result<OperationResult<()>> {
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
) -> Result<Certificate> {
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
) -> Result<OperationResult<DaprComponent>> {
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
) -> Result<OperationResult<()>> {
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

async fn parse_azure_core_021_response_body_or_default_certificate(
    response: azure_core_021::Response,
    resource_type: &str,
    resource_name: &str,
) -> Result<Certificate> {
    let body = response
        .into_body()
        .collect()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to read {resource_type} '{resource_name}' response"),
            resource_id: None,
        })?;

    if body.is_empty() {
        return Ok(Certificate::new(TrackedResource::new(String::new())));
    }

    serde_json::from_slice(&body)
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to parse {resource_type} '{resource_name}' response"),
            resource_id: None,
        })
}
