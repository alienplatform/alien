use crate::core::map_azure_core_021_sdk_error;
use crate::error::Result;
use azure_mgmt_resources::package_resources_2021_04 as azure_resources_2021_04;
use azure_mgmt_resources::package_resources_2021_04::models::{Provider, ResourceGroup};

pub(crate) async fn create_or_update_resource_group(
    client: &azure_resources_2021_04::Client,
    subscription_id: &str,
    resource_group_name: &str,
    resource_group: ResourceGroup,
) -> Result<ResourceGroup> {
    let result = client
        .resource_groups_client()
        .create_or_update(
            resource_group_name.to_string(),
            resource_group,
            subscription_id.to_string(),
        )
        .await;
    map_azure_core_021_sdk_error(
        "Azure Resources",
        result,
        "resource group create or update",
        "Azure Resource Group",
        resource_group_name,
    )
}

pub(crate) async fn delete_resource_group(
    client: &azure_resources_2021_04::Client,
    subscription_id: &str,
    resource_group_name: &str,
) -> Result<()> {
    let result = client
        .resource_groups_client()
        .delete(resource_group_name.to_string(), subscription_id.to_string())
        .send()
        .await
        .map(|_| ());
    map_azure_core_021_sdk_error(
        "Azure Resources",
        result,
        "resource group delete",
        "Azure Resource Group",
        resource_group_name,
    )
}

pub(crate) async fn get_resource_group(
    client: &azure_resources_2021_04::Client,
    subscription_id: &str,
    resource_group_name: &str,
) -> Result<ResourceGroup> {
    let result = client
        .resource_groups_client()
        .get(resource_group_name.to_string(), subscription_id.to_string())
        .await;
    map_azure_core_021_sdk_error(
        "Azure Resources",
        result,
        "resource group get",
        "Azure Resource Group",
        resource_group_name,
    )
}

pub(crate) async fn get_provider(
    client: &azure_resources_2021_04::Client,
    subscription_id: &str,
    resource_provider_namespace: &str,
) -> Result<Provider> {
    let result = client
        .providers_client()
        .get(
            resource_provider_namespace.to_string(),
            subscription_id.to_string(),
        )
        .await;
    map_azure_core_021_sdk_error(
        "Azure Resources",
        result,
        "provider get",
        "Azure Resource Provider",
        resource_provider_namespace,
    )
}

pub(crate) async fn register_provider(
    client: &azure_resources_2021_04::Client,
    subscription_id: &str,
    resource_provider_namespace: &str,
) -> Result<Provider> {
    let result = client
        .providers_client()
        .register(
            resource_provider_namespace.to_string(),
            subscription_id.to_string(),
        )
        .await;
    map_azure_core_021_sdk_error(
        "Azure Resources",
        result,
        "provider register",
        "Azure Resource Provider",
        resource_provider_namespace,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{azure_credential_from_config, AzureCore021Credential};
    use alien_core::{AzureClientConfig, AzureCredentials};
    use azure_core_021::{
        headers::Headers, Body, BytesStream, Context, Method, Policy, PolicyResult, Request,
        Response, StatusCode, TransportOptions,
    };
    use serde_json::json;
    use std::sync::Arc;

    #[derive(Debug)]
    struct ResourceGroupTransport;

    #[async_trait::async_trait]
    impl Policy for ResourceGroupTransport {
        async fn send(
            &self,
            _ctx: &Context,
            request: &mut Request,
            next: &[Arc<dyn Policy>],
        ) -> PolicyResult {
            assert!(next.is_empty());
            assert_eq!(request.method(), &Method::Put);
            assert_eq!(
                request.url().path(),
                "/subscriptions/00000000-0000-0000-0000-000000000000/resourcegroups/test-rg"
            );
            assert_eq!(request.url().query(), Some("api-version=2021-04-01"));
            match request.body() {
                Body::Bytes(bytes) => {
                    let body: serde_json::Value =
                        serde_json::from_slice(bytes).expect("request body should be JSON");
                    assert_eq!(body["location"], "eastus");
                }
                #[cfg(not(target_arch = "wasm32"))]
                Body::SeekableStream(_) => panic!("resource group request should use JSON bytes"),
            }

            let response = json!({
                "id": "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/test-rg",
                "name": "test-rg",
                "location": "eastus",
                "properties": { "provisioningState": "Succeeded" }
            });
            Ok(Response::new(
                StatusCode::Ok,
                Headers::new(),
                Box::pin(BytesStream::new(response.to_string())),
            ))
        }
    }

    fn resources_client_with_transport(
        transport: impl Policy + 'static,
    ) -> azure_resources_2021_04::Client {
        let config = AzureClientConfig {
            subscription_id: "00000000-0000-0000-0000-000000000000".to_string(),
            tenant_id: "11111111-1111-1111-1111-111111111111".to_string(),
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::AccessToken {
                token: "test-token".to_string(),
            },
            service_overrides: None,
        };
        let credential = Arc::new(AzureCore021Credential::new(
            azure_credential_from_config(&config).expect("test credential should build"),
        ));

        azure_resources_2021_04::Client::builder(credential)
            .endpoint(azure_core_021::Url::parse("https://management.azure.com").unwrap())
            .transport(TransportOptions::new_custom_policy(Arc::new(transport)))
            .build()
            .expect("Azure Resources client should build")
    }

    #[tokio::test]
    async fn resource_group_helper_uses_generated_client_request() {
        let client = resources_client_with_transport(ResourceGroupTransport);
        let created = create_or_update_resource_group(
            &client,
            "00000000-0000-0000-0000-000000000000",
            "test-rg",
            ResourceGroup::new("eastus".to_string()),
        )
        .await
        .expect("resource group should be created");

        assert_eq!(created.name.as_deref(), Some("test-rg"));
        assert_eq!(
            created
                .properties
                .and_then(|properties| properties.provisioning_state),
            Some("Succeeded".to_string())
        );
    }
}
