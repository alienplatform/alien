use crate::core::map_azure_core_021_sdk_error;
use crate::error::Result;
use azure_mgmt_servicebus::package_2024_01 as azure_servicebus_2024_01;
use azure_mgmt_servicebus::package_2024_01::models::{SbNamespace, SbQueue};

pub(crate) async fn create_or_update_namespace(
    client: &azure_servicebus_2024_01::Client,
    subscription_id: &str,
    resource_group_name: &str,
    namespace_name: &str,
    parameters: SbNamespace,
) -> Result<SbNamespace> {
    let result = client
        .namespaces_client()
        .create_or_update(
            resource_group_name.to_string(),
            namespace_name.to_string(),
            parameters,
            subscription_id.to_string(),
        )
        .await;
    map_azure_core_021_sdk_error(
        "Azure Service Bus",
        result,
        "namespace create or update",
        "Azure Service Bus namespace",
        namespace_name,
    )
}

pub(crate) async fn get_namespace(
    client: &azure_servicebus_2024_01::Client,
    subscription_id: &str,
    resource_group_name: &str,
    namespace_name: &str,
) -> Result<SbNamespace> {
    let result = client
        .namespaces_client()
        .get(
            resource_group_name.to_string(),
            namespace_name.to_string(),
            subscription_id.to_string(),
        )
        .await;
    map_azure_core_021_sdk_error(
        "Azure Service Bus",
        result,
        "namespace get",
        "Azure Service Bus namespace",
        namespace_name,
    )
}

pub(crate) async fn delete_namespace(
    client: &azure_servicebus_2024_01::Client,
    subscription_id: &str,
    resource_group_name: &str,
    namespace_name: &str,
) -> Result<()> {
    let result = client
        .namespaces_client()
        .delete(
            resource_group_name.to_string(),
            namespace_name.to_string(),
            subscription_id.to_string(),
        )
        .send()
        .await
        .map(|_| ());
    map_azure_core_021_sdk_error(
        "Azure Service Bus",
        result,
        "namespace delete",
        "Azure Service Bus namespace",
        namespace_name,
    )
}

pub(crate) async fn create_or_update_queue(
    client: &azure_servicebus_2024_01::Client,
    subscription_id: &str,
    resource_group_name: &str,
    namespace_name: &str,
    queue_name: &str,
    parameters: SbQueue,
) -> Result<SbQueue> {
    let result = client
        .queues_client()
        .create_or_update(
            resource_group_name.to_string(),
            namespace_name.to_string(),
            queue_name.to_string(),
            parameters,
            subscription_id.to_string(),
        )
        .await;
    map_azure_core_021_sdk_error(
        "Azure Service Bus",
        result,
        "queue create or update",
        "Azure Service Bus queue",
        queue_name,
    )
}

pub(crate) async fn get_queue(
    client: &azure_servicebus_2024_01::Client,
    subscription_id: &str,
    resource_group_name: &str,
    namespace_name: &str,
    queue_name: &str,
) -> Result<SbQueue> {
    let result = client
        .queues_client()
        .get(
            resource_group_name.to_string(),
            namespace_name.to_string(),
            queue_name.to_string(),
            subscription_id.to_string(),
        )
        .await;
    map_azure_core_021_sdk_error(
        "Azure Service Bus",
        result,
        "queue get",
        "Azure Service Bus queue",
        queue_name,
    )
}

pub(crate) async fn delete_queue(
    client: &azure_servicebus_2024_01::Client,
    subscription_id: &str,
    resource_group_name: &str,
    namespace_name: &str,
    queue_name: &str,
) -> Result<()> {
    let result = client
        .queues_client()
        .delete(
            resource_group_name.to_string(),
            namespace_name.to_string(),
            queue_name.to_string(),
            subscription_id.to_string(),
        )
        .send()
        .await
        .map(|_| ());
    map_azure_core_021_sdk_error(
        "Azure Service Bus",
        result,
        "queue delete",
        "Azure Service Bus queue",
        queue_name,
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
    use azure_mgmt_servicebus::package_2024_01::models::SbQueueProperties;
    use serde_json::json;
    use std::sync::Arc;

    #[derive(Debug)]
    struct CreateQueueTransport;

    #[async_trait::async_trait]
    impl Policy for CreateQueueTransport {
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
                "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/test-rg/providers/Microsoft.ServiceBus/namespaces/test-ns/queues/test-queue"
            );
            assert_eq!(request.url().query(), Some("api-version=2024-01-01"));
            match request.body() {
                Body::Bytes(bytes) => {
                    let body: serde_json::Value =
                        serde_json::from_slice(bytes).expect("request body should be JSON");
                    assert_eq!(body["name"], "test-queue");
                }
                #[cfg(not(target_arch = "wasm32"))]
                Body::SeekableStream(_) => panic!("queue request should use JSON bytes"),
            }

            let response = json!({
                "id": "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/test-rg/providers/Microsoft.ServiceBus/namespaces/test-ns/queues/test-queue",
                "name": "test-queue",
                "type": "Microsoft.ServiceBus/namespaces/queues",
                "properties": { "status": "Active" }
            });
            Ok(Response::new(
                StatusCode::Ok,
                Headers::new(),
                Box::pin(BytesStream::new(response.to_string())),
            ))
        }
    }

    fn servicebus_client_with_transport(
        transport: impl Policy + 'static,
    ) -> azure_servicebus_2024_01::Client {
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

        azure_servicebus_2024_01::Client::builder(credential)
            .endpoint(azure_core_021::Url::parse("https://management.azure.com").unwrap())
            .transport(TransportOptions::new_custom_policy(Arc::new(transport)))
            .build()
            .expect("Azure Service Bus client should build")
    }

    #[tokio::test]
    async fn create_queue_helper_uses_generated_client_request() {
        let client = servicebus_client_with_transport(CreateQueueTransport);
        let created = create_or_update_queue(
            &client,
            "00000000-0000-0000-0000-000000000000",
            "test-rg",
            "test-ns",
            "test-queue",
            SbQueue {
                proxy_resource: azure_servicebus_2024_01::models::ProxyResource {
                    id: None,
                    name: Some("test-queue".to_string()),
                    type_: None,
                    location: None,
                },
                properties: Some(SbQueueProperties::default()),
                system_data: None,
            },
        )
        .await
        .expect("queue should be created");

        assert_eq!(created.proxy_resource.name.as_deref(), Some("test-queue"));
    }
}
