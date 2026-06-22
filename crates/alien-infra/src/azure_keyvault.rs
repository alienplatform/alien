use crate::core::map_azure_core_021_sdk_error;
use crate::error::Result;
use azure_mgmt_keyvault::package_preview_2022_02 as azure_keyvault_2022_02;
use azure_mgmt_keyvault::package_preview_2022_02::models::{Vault, VaultCreateOrUpdateParameters};

pub(crate) async fn create_or_update_vault(
    client: &azure_keyvault_2022_02::Client,
    subscription_id: &str,
    resource_group_name: &str,
    vault_name: &str,
    parameters: VaultCreateOrUpdateParameters,
) -> Result<Vault> {
    let result = client
        .vaults_client()
        .create_or_update(
            resource_group_name.to_string(),
            vault_name.to_string(),
            parameters,
            subscription_id.to_string(),
        )
        .await;
    map_azure_core_021_sdk_error(
        "Azure Key Vault",
        result,
        "vault create or update",
        "Azure Key Vault",
        vault_name,
    )
}

pub(crate) async fn delete_vault(
    client: &azure_keyvault_2022_02::Client,
    subscription_id: &str,
    resource_group_name: &str,
    vault_name: &str,
) -> Result<()> {
    let result = client
        .vaults_client()
        .delete(
            resource_group_name.to_string(),
            vault_name.to_string(),
            subscription_id.to_string(),
        )
        .send()
        .await
        .map(|_| ());
    map_azure_core_021_sdk_error(
        "Azure Key Vault",
        result,
        "vault delete",
        "Azure Key Vault",
        vault_name,
    )
}

pub(crate) async fn get_vault(
    client: &azure_keyvault_2022_02::Client,
    subscription_id: &str,
    resource_group_name: &str,
    vault_name: &str,
) -> Result<Vault> {
    let result = client
        .vaults_client()
        .get(
            resource_group_name.to_string(),
            vault_name.to_string(),
            subscription_id.to_string(),
        )
        .await;
    map_azure_core_021_sdk_error(
        "Azure Key Vault",
        result,
        "vault get",
        "Azure Key Vault",
        vault_name,
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
    use azure_mgmt_keyvault::package_preview_2022_02::models::{
        sku::{Family as AzureKeyVaultSkuFamily, Name as AzureKeyVaultSkuName},
        Sku, VaultProperties,
    };
    use serde_json::json;
    use std::sync::Arc;

    #[derive(Debug)]
    struct CreateVaultTransport;

    #[async_trait::async_trait]
    impl Policy for CreateVaultTransport {
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
                "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/test-rg/providers/Microsoft.KeyVault/vaults/test-vault"
            );
            assert_eq!(
                request.url().query(),
                Some("api-version=2022-02-01-preview")
            );
            match request.body() {
                Body::Bytes(bytes) => {
                    let body: serde_json::Value =
                        serde_json::from_slice(bytes).expect("request body should be JSON");
                    assert_eq!(body["location"], "eastus");
                    assert_eq!(body["properties"]["tenantId"], "test-tenant");
                    assert_eq!(body["properties"]["enableRbacAuthorization"], true);
                }
                #[cfg(not(target_arch = "wasm32"))]
                Body::SeekableStream(_) => panic!("vault request should use JSON bytes"),
            }

            let response = json!({
                "id": "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/test-rg/providers/Microsoft.KeyVault/vaults/test-vault",
                "name": "test-vault",
                "type": "Microsoft.KeyVault/vaults",
                "location": "eastus",
                "properties": {
                    "tenantId": "test-tenant",
                    "sku": { "family": "A", "name": "standard" },
                    "accessPolicies": [],
                    "vaultUri": "https://test-vault.vault.azure.net/",
                    "provisioningState": "Succeeded"
                }
            });
            Ok(Response::new(
                StatusCode::Ok,
                Headers::new(),
                Box::pin(BytesStream::new(response.to_string())),
            ))
        }
    }

    fn keyvault_client_with_transport(
        transport: impl Policy + 'static,
    ) -> azure_keyvault_2022_02::Client {
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

        azure_keyvault_2022_02::Client::builder(credential)
            .endpoint(azure_core_021::Url::parse("https://management.azure.com").unwrap())
            .transport(TransportOptions::new_custom_policy(Arc::new(transport)))
            .build()
            .expect("Azure Key Vault client should build")
    }

    #[tokio::test]
    async fn create_vault_helper_uses_generated_client_request() {
        let client = keyvault_client_with_transport(CreateVaultTransport);
        let mut properties = VaultProperties::new(
            "test-tenant".to_string(),
            Sku::new(AzureKeyVaultSkuFamily::A, AzureKeyVaultSkuName::Standard),
        );
        properties.enable_rbac_authorization = Some(true);

        let created = create_or_update_vault(
            &client,
            "00000000-0000-0000-0000-000000000000",
            "test-rg",
            "test-vault",
            VaultCreateOrUpdateParameters::new("eastus".to_string(), properties),
        )
        .await
        .expect("vault should be created");

        assert_eq!(created.name.as_deref(), Some("test-vault"));
        assert_eq!(
            created.properties.vault_uri.as_deref(),
            Some("https://test-vault.vault.azure.net/")
        );
    }
}
