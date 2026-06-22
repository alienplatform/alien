use crate::core::map_azure_core_021_sdk_error;
use crate::error::Result;
use azure_mgmt_msi::package_2023_01_31 as azure_msi_2023_01_31;
use azure_mgmt_msi::package_2023_01_31::models::{FederatedIdentityCredential, Identity};

pub(crate) async fn create_or_update_user_assigned_identity(
    client: &azure_msi_2023_01_31::Client,
    subscription_id: &str,
    resource_group_name: &str,
    resource_name: &str,
    identity: &Identity,
) -> Result<Identity> {
    let result = client
        .user_assigned_identities_client()
        .create_or_update(
            subscription_id.to_string(),
            resource_group_name.to_string(),
            resource_name.to_string(),
            identity.clone(),
        )
        .await;
    map_azure_core_021_sdk_error(
        "Azure Managed Identity",
        result,
        "user assigned identity create or update",
        "Azure managed identity",
        resource_name,
    )
}

pub(crate) async fn delete_user_assigned_identity(
    client: &azure_msi_2023_01_31::Client,
    subscription_id: &str,
    resource_group_name: &str,
    resource_name: &str,
) -> Result<()> {
    let result = client
        .user_assigned_identities_client()
        .delete(
            subscription_id.to_string(),
            resource_group_name.to_string(),
            resource_name.to_string(),
        )
        .send()
        .await
        .map(|_| ());
    map_azure_core_021_sdk_error(
        "Azure Managed Identity",
        result,
        "user assigned identity delete",
        "Azure managed identity",
        resource_name,
    )
}

pub(crate) async fn get_user_assigned_identity(
    client: &azure_msi_2023_01_31::Client,
    subscription_id: &str,
    resource_group_name: &str,
    resource_name: &str,
) -> Result<Identity> {
    let result = client
        .user_assigned_identities_client()
        .get(
            subscription_id.to_string(),
            resource_group_name.to_string(),
            resource_name.to_string(),
        )
        .await;
    map_azure_core_021_sdk_error(
        "Azure Managed Identity",
        result,
        "user assigned identity get",
        "Azure managed identity",
        resource_name,
    )
}

pub(crate) async fn create_or_update_federated_credential(
    client: &azure_msi_2023_01_31::Client,
    subscription_id: &str,
    resource_group_name: &str,
    identity_name: &str,
    credential_name: &str,
    credential: &FederatedIdentityCredential,
) -> Result<FederatedIdentityCredential> {
    let result = client
        .federated_identity_credentials_client()
        .create_or_update(
            subscription_id.to_string(),
            resource_group_name.to_string(),
            identity_name.to_string(),
            credential_name.to_string(),
            credential.clone(),
        )
        .await;
    map_azure_core_021_sdk_error(
        "Azure Managed Identity",
        result,
        "federated credential create or update",
        "Azure federated identity credential",
        credential_name,
    )
}

pub(crate) async fn delete_federated_credential(
    client: &azure_msi_2023_01_31::Client,
    subscription_id: &str,
    resource_group_name: &str,
    identity_name: &str,
    credential_name: &str,
) -> Result<()> {
    let result = client
        .federated_identity_credentials_client()
        .delete(
            subscription_id.to_string(),
            resource_group_name.to_string(),
            identity_name.to_string(),
            credential_name.to_string(),
        )
        .send()
        .await
        .map(|_| ());
    map_azure_core_021_sdk_error(
        "Azure Managed Identity",
        result,
        "federated credential delete",
        "Azure federated identity credential",
        credential_name,
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
    use azure_mgmt_msi::package_2023_01_31::models::{
        FederatedIdentityCredentialProperties, TrackedResource,
    };
    use serde_json::json;
    use std::sync::Arc;

    #[derive(Debug)]
    struct ManagedIdentityTransport;

    #[async_trait::async_trait]
    impl Policy for ManagedIdentityTransport {
        async fn send(
            &self,
            _ctx: &Context,
            request: &mut Request,
            next: &[Arc<dyn Policy>],
        ) -> PolicyResult {
            assert!(next.is_empty());
            assert_eq!(request.method(), &Method::Put);
            assert_eq!(request.url().query(), Some("api-version=2023-01-31"));

            match request.url().path() {
                "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/test-rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/test-identity" => {
                    assert_json_body(request, |body| {
                        assert_eq!(body["location"], "eastus");
                    });
                    Ok(json_response(json!({
                        "id": "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/test-rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/test-identity",
                        "name": "test-identity",
                        "type": "Microsoft.ManagedIdentity/userAssignedIdentities",
                        "location": "eastus",
                        "properties": {
                            "clientId": "22222222-2222-2222-2222-222222222222",
                            "principalId": "33333333-3333-3333-3333-333333333333",
                            "tenantId": "11111111-1111-1111-1111-111111111111"
                        }
                    })))
                }
                "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/test-rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/test-identity/federatedIdentityCredentials/test-fic" => {
                    assert_json_body(request, |body| {
                        assert_eq!(body["properties"]["issuer"], "https://issuer.example");
                        assert_eq!(body["properties"]["subject"], "system:serviceaccount:ns:name");
                        assert_eq!(body["properties"]["audiences"][0], "api://AzureADTokenExchange");
                    });
                    Ok(json_response(json!({
                        "id": "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/test-rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/test-identity/federatedIdentityCredentials/test-fic",
                        "name": "test-fic",
                        "type": "Microsoft.ManagedIdentity/userAssignedIdentities/federatedIdentityCredentials",
                        "properties": {
                            "issuer": "https://issuer.example",
                            "subject": "system:serviceaccount:ns:name",
                            "audiences": ["api://AzureADTokenExchange"]
                        }
                    })))
                }
                path => panic!("unexpected Azure Managed Identity path: {path}"),
            }
        }
    }

    fn assert_json_body(request: &Request, assert_body: impl FnOnce(serde_json::Value)) {
        match request.body() {
            Body::Bytes(bytes) => {
                let body: serde_json::Value =
                    serde_json::from_slice(bytes).expect("MSI request body should be JSON");
                assert_body(body);
            }
            #[cfg(not(target_arch = "wasm32"))]
            Body::SeekableStream(_) => panic!("MSI request should use JSON bytes"),
        }
    }

    fn json_response(value: serde_json::Value) -> Response {
        Response::new(
            StatusCode::Ok,
            Headers::new(),
            Box::pin(BytesStream::new(value.to_string())),
        )
    }

    fn msi_client_with_transport(transport: impl Policy + 'static) -> azure_msi_2023_01_31::Client {
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

        azure_msi_2023_01_31::Client::builder(credential)
            .endpoint(azure_core_021::Url::parse("https://management.azure.com").unwrap())
            .transport(TransportOptions::new_custom_policy(Arc::new(transport)))
            .build()
            .expect("Azure Managed Identity client should build")
    }

    #[tokio::test]
    async fn msi_helpers_use_generated_client_requests() {
        let client = msi_client_with_transport(ManagedIdentityTransport);

        let identity = Identity::new(TrackedResource::new("eastus".to_string()));
        let created = create_or_update_user_assigned_identity(
            &client,
            "00000000-0000-0000-0000-000000000000",
            "test-rg",
            "test-identity",
            &identity,
        )
        .await
        .expect("identity should be created");
        assert_eq!(
            created.tracked_resource.resource.name.as_deref(),
            Some("test-identity")
        );

        let mut credential = FederatedIdentityCredential::new();
        credential.properties = Some(FederatedIdentityCredentialProperties::new(
            "https://issuer.example".to_string(),
            "system:serviceaccount:ns:name".to_string(),
            vec!["api://AzureADTokenExchange".to_string()],
        ));
        let created = create_or_update_federated_credential(
            &client,
            "00000000-0000-0000-0000-000000000000",
            "test-rg",
            "test-identity",
            "test-fic",
            &credential,
        )
        .await
        .expect("federated credential should be created");
        assert_eq!(
            created.proxy_resource.resource.name.as_deref(),
            Some("test-fic")
        );
    }
}
