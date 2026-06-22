use crate::core::map_azure_core_021_sdk_error;
use crate::error::Result;
use azure_mgmt_storage::package_2023_05 as azure_storage_2023_05;

pub(crate) async fn create_table(
    client: &azure_storage_2023_05::Client,
    subscription_id: &str,
    resource_group_name: &str,
    storage_account_name: &str,
    table_name: &str,
) -> Result<()> {
    let result = client
        .table_client()
        .create(
            resource_group_name.to_string(),
            storage_account_name.to_string(),
            subscription_id.to_string(),
            table_name.to_string(),
        )
        .await
        .map(|_| ());
    map_azure_core_021_sdk_error(
        "Azure Storage",
        result,
        "table create",
        "Azure Table",
        table_name,
    )
}

pub(crate) async fn delete_table(
    client: &azure_storage_2023_05::Client,
    subscription_id: &str,
    resource_group_name: &str,
    storage_account_name: &str,
    table_name: &str,
) -> Result<()> {
    let result = client
        .table_client()
        .delete(
            resource_group_name.to_string(),
            storage_account_name.to_string(),
            subscription_id.to_string(),
            table_name.to_string(),
        )
        .send()
        .await
        .map(|_| ());
    map_azure_core_021_sdk_error(
        "Azure Storage",
        result,
        "table delete",
        "Azure Table",
        table_name,
    )
}

pub(crate) async fn get_table_signed_identifier_count(
    client: &azure_storage_2023_05::Client,
    subscription_id: &str,
    resource_group_name: &str,
    storage_account_name: &str,
    table_name: &str,
) -> Result<usize> {
    let result = client
        .table_client()
        .get(
            resource_group_name.to_string(),
            storage_account_name.to_string(),
            subscription_id.to_string(),
            table_name.to_string(),
        )
        .await;
    let table = map_azure_core_021_sdk_error(
        "Azure Storage",
        result,
        "table get",
        "Azure Table",
        table_name,
    )?;
    Ok(table
        .properties
        .map(|properties| properties.signed_identifiers.len())
        .unwrap_or_default())
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
    struct CreateTableTransport;

    #[async_trait::async_trait]
    impl Policy for CreateTableTransport {
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
                "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/test-rg/providers/Microsoft.Storage/storageAccounts/teststorage/tableServices/default/tables/testtable"
            );
            assert_eq!(request.url().query(), Some("api-version=2023-05-01"));
            match request.body() {
                Body::Bytes(bytes) => assert!(
                    bytes.is_empty(),
                    "table create should not add a request body"
                ),
                #[cfg(not(target_arch = "wasm32"))]
                Body::SeekableStream(_) => panic!("table create should use an empty body"),
            }

            let response = json!({
                "properties": {
                    "tableName": "testtable",
                    "signedIdentifiers": []
                }
            });
            Ok(Response::new(
                StatusCode::Ok,
                Headers::new(),
                Box::pin(BytesStream::new(response.to_string())),
            ))
        }
    }

    fn storage_client_with_transport(
        transport: impl Policy + 'static,
    ) -> azure_storage_2023_05::Client {
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

        azure_storage_2023_05::Client::builder(credential)
            .endpoint(azure_core_021::Url::parse("https://management.azure.com").unwrap())
            .transport(TransportOptions::new_custom_policy(Arc::new(transport)))
            .build()
            .expect("Azure Storage client should build")
    }

    #[tokio::test]
    async fn create_table_helper_uses_generated_client_request() {
        let client = storage_client_with_transport(CreateTableTransport);

        create_table(
            &client,
            "00000000-0000-0000-0000-000000000000",
            "test-rg",
            "teststorage",
            "testtable",
        )
        .await
        .expect("table should be created");
    }
}
