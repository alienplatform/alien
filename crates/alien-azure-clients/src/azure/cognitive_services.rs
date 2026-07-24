use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::long_running_operation::OperationResult;
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};

#[cfg(feature = "test-utils")]
use mockall::automock;

const COGNITIVE_SERVICES_API_VERSION: &str = "2024-10-01";

// -------------------------------------------------------------------------
// ARM resource models
// -------------------------------------------------------------------------

/// Properties of a CognitiveServices account
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CognitiveServicesAccountProperties {
    /// The endpoint URL of the account once provisioned
    pub endpoint: Option<String>,
    /// The provisioning state of the account
    pub provisioning_state: Option<String>,
}

/// SKU for a CognitiveServices account
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CognitiveServicesSku {
    /// The SKU name (e.g. "S0")
    pub name: String,
}

/// An Azure CognitiveServices account resource
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CognitiveServicesAccount {
    /// The ARM resource kind (e.g. "AIServices")
    pub kind: Option<String>,
    /// The Azure region where the account lives
    pub location: Option<String>,
    /// The SKU
    pub sku: Option<CognitiveServicesSku>,
    /// The account properties, including endpoint and provisioning state
    pub properties: Option<CognitiveServicesAccountProperties>,
}

/// Request body for creating a CognitiveServices account
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CognitiveServicesAccountCreateParameters {
    /// Azure region
    pub location: String,
    /// Account kind — must be "AIServices" for Azure AI Services
    pub kind: String,
    /// SKU
    pub sku: CognitiveServicesSku,
    /// Account properties
    pub properties: CognitiveServicesAccountCreateProperties,
}

/// Properties included in the create request body
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CognitiveServicesAccountCreateProperties {
    /// Custom subdomain name used to build the endpoint URL
    pub custom_sub_domain_name: String,
}

/// Create may complete synchronously or return an Azure long-running operation.
pub type CognitiveServicesAccountOperationResult = OperationResult<CognitiveServicesAccount>;

/// The model a deployment serves.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CognitiveServicesDeploymentModel {
    /// Model format (e.g. "OpenAI")
    pub format: String,
    /// Model name (e.g. "gpt-4.1")
    pub name: String,
    /// Model version (e.g. "2025-04-14")
    pub version: String,
}

/// SKU (throughput tier + capacity) of a deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CognitiveServicesDeploymentSku {
    /// SKU name (e.g. "GlobalStandard")
    pub name: String,
    /// Provisioned throughput units
    pub capacity: i32,
}

/// Properties of a deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CognitiveServicesDeploymentProperties {
    /// The deployed model
    pub model: CognitiveServicesDeploymentModel,
    /// The provisioning state (response only)
    pub provisioning_state: Option<String>,
}

/// A CognitiveServices model deployment resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CognitiveServicesDeployment {
    /// The SKU
    pub sku: Option<CognitiveServicesDeploymentSku>,
    /// The deployment properties, including the model and provisioning state
    pub properties: Option<CognitiveServicesDeploymentProperties>,
}

/// Properties included in the deployment create request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CognitiveServicesDeploymentCreateProperties {
    /// The model to deploy
    pub model: CognitiveServicesDeploymentModel,
}

/// Request body for creating a deployment (PUT).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CognitiveServicesDeploymentCreateParameters {
    /// The SKU (throughput tier + capacity)
    pub sku: CognitiveServicesDeploymentSku,
    /// The deployment properties
    pub properties: CognitiveServicesDeploymentCreateProperties,
}

/// Deployment create may complete synchronously or return a long-running operation.
pub type CognitiveServicesDeploymentOperationResult = OperationResult<CognitiveServicesDeployment>;

// -------------------------------------------------------------------------
// CognitiveServices Accounts API trait
// -------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait CognitiveServicesAccountsApi: Send + Sync + std::fmt::Debug {
    /// Create a CognitiveServices account (PUT). May return a long-running operation.
    async fn create_account(
        &self,
        resource_group_name: &str,
        account_name: &str,
        parameters: &CognitiveServicesAccountCreateParameters,
    ) -> Result<CognitiveServicesAccountOperationResult>;

    /// Get the properties of a CognitiveServices account.
    async fn get_account(
        &self,
        resource_group_name: &str,
        account_name: &str,
    ) -> Result<CognitiveServicesAccount>;

    /// Delete a CognitiveServices account.
    async fn delete_account(
        &self,
        resource_group_name: &str,
        account_name: &str,
    ) -> Result<()>;

    /// Create (or update) a model deployment under an account (PUT). May return a
    /// long-running operation.
    async fn create_deployment(
        &self,
        resource_group_name: &str,
        account_name: &str,
        deployment_name: &str,
        parameters: &CognitiveServicesDeploymentCreateParameters,
    ) -> Result<CognitiveServicesDeploymentOperationResult>;

    /// Get a model deployment's properties, including its provisioning state.
    async fn get_deployment(
        &self,
        resource_group_name: &str,
        account_name: &str,
        deployment_name: &str,
    ) -> Result<CognitiveServicesDeployment>;
}

// -------------------------------------------------------------------------
// CognitiveServices Accounts client struct
// -------------------------------------------------------------------------

#[derive(Debug)]
pub struct AzureCognitiveServicesClient {
    pub base: AzureClientBase,
    pub token_cache: AzureTokenCache,
}

impl AzureCognitiveServicesClient {
    pub fn new(client: Client, token_cache: AzureTokenCache) -> Self {
        let endpoint = token_cache.management_endpoint().to_string();
        Self {
            base: AzureClientBase::with_client_config(
                client,
                endpoint,
                token_cache.config().clone(),
            ),
            token_cache,
        }
    }

    fn resource_url(&self, resource_group_name: &str, account_name: &str) -> String {
        format!(
            "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.CognitiveServices/accounts/{}",
            self.token_cache.config().subscription_id,
            resource_group_name,
            account_name
        )
    }

    fn deployment_url(
        &self,
        resource_group_name: &str,
        account_name: &str,
        deployment_name: &str,
    ) -> String {
        format!(
            "{}/deployments/{}",
            self.resource_url(resource_group_name, account_name),
            deployment_name
        )
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl CognitiveServicesAccountsApi for AzureCognitiveServicesClient {
    /// Create a CognitiveServices account
    async fn create_account(
        &self,
        resource_group_name: &str,
        account_name: &str,
        parameters: &CognitiveServicesAccountCreateParameters,
    ) -> Result<CognitiveServicesAccountOperationResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &self.resource_url(resource_group_name, account_name),
            Some(vec![("api-version", COGNITIVE_SERVICES_API_VERSION.into())]),
        );

        let body = serde_json::to_string(parameters)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize CognitiveServices account create parameters for resource: {}",
                    account_name
                ),
            })?;

        let builder = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(signed, "CreateCognitiveServicesAccount", account_name)
            .await
    }

    /// Get a CognitiveServices account's properties
    async fn get_account(
        &self,
        resource_group_name: &str,
        account_name: &str,
    ) -> Result<CognitiveServicesAccount> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &self.resource_url(resource_group_name, account_name),
            Some(vec![("api-version", COGNITIVE_SERVICES_API_VERSION.into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetCognitiveServicesAccount", account_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetCognitiveServicesAccount: failed to read response body for {}",
                    account_name
                ),
                url: url.clone(),
                http_status: 200,
                http_request_text: None,
                http_response_text: None,
            })?;

        let account: CognitiveServicesAccount = serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetCognitiveServicesAccount: JSON parse error for {}",
                    account_name
                ),
                url: url.clone(),
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(body),
            })?;

        Ok(account)
    }

    /// Delete a CognitiveServices account
    async fn delete_account(
        &self,
        resource_group_name: &str,
        account_name: &str,
    ) -> Result<()> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &self.resource_url(resource_group_name, account_name),
            Some(vec![("api-version", COGNITIVE_SERVICES_API_VERSION.into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let _resp = self
            .base
            .execute_request(signed, "DeleteCognitiveServicesAccount", account_name)
            .await?;

        Ok(())
    }

    /// Create (or update) a model deployment under an account
    async fn create_deployment(
        &self,
        resource_group_name: &str,
        account_name: &str,
        deployment_name: &str,
        parameters: &CognitiveServicesDeploymentCreateParameters,
    ) -> Result<CognitiveServicesDeploymentOperationResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &self.deployment_url(resource_group_name, account_name, deployment_name),
            Some(vec![("api-version", COGNITIVE_SERVICES_API_VERSION.into())]),
        );

        let body = serde_json::to_string(parameters)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize CognitiveServices deployment create parameters for: {}",
                    deployment_name
                ),
            })?;

        let builder = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(
                signed,
                "CreateCognitiveServicesDeployment",
                deployment_name,
            )
            .await
    }

    /// Get a model deployment's properties
    async fn get_deployment(
        &self,
        resource_group_name: &str,
        account_name: &str,
        deployment_name: &str,
    ) -> Result<CognitiveServicesDeployment> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &self.deployment_url(resource_group_name, account_name, deployment_name),
            Some(vec![("api-version", COGNITIVE_SERVICES_API_VERSION.into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetCognitiveServicesDeployment", deployment_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetCognitiveServicesDeployment: failed to read response body for {}",
                    deployment_name
                ),
                url: url.clone(),
                http_status: 200,
                http_request_text: None,
                http_response_text: None,
            })?;

        let deployment: CognitiveServicesDeployment = serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetCognitiveServicesDeployment: JSON parse error for {}",
                    deployment_name
                ),
                url: url.clone(),
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(body),
            })?;

        Ok(deployment)
    }
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_current_cognitive_services_api_version() {
        assert_eq!(COGNITIVE_SERVICES_API_VERSION, "2024-10-01");
    }

    /// Verifies that the ARM GET response for a CognitiveServices account deserializes
    /// correctly into our hand-written model. This catches camelCase/field-name mistakes.
    #[test]
    fn deserializes_arm_get_response() {
        let json = r#"{
            "kind": "AIServices",
            "location": "eastus",
            "sku": { "name": "S0" },
            "properties": {
                "endpoint": "https://my-account.cognitiveservices.azure.com/",
                "provisioningState": "Succeeded"
            }
        }"#;

        let account: CognitiveServicesAccount =
            serde_json::from_str(json).expect("ARM GET response should deserialize");

        assert_eq!(account.kind.as_deref(), Some("AIServices"));
        assert_eq!(account.location.as_deref(), Some("eastus"));
        assert_eq!(account.sku.as_ref().map(|s| s.name.as_str()), Some("S0"));

        let props = account.properties.expect("properties should be present");
        assert_eq!(
            props.endpoint.as_deref(),
            Some("https://my-account.cognitiveservices.azure.com/")
        );
        assert_eq!(props.provisioning_state.as_deref(), Some("Succeeded"));
    }

    /// Verifies that the create parameters serialize to the expected ARM request body shape.
    #[test]
    fn serializes_create_parameters() {
        let params = CognitiveServicesAccountCreateParameters {
            location: "eastus".to_string(),
            kind: "AIServices".to_string(),
            sku: CognitiveServicesSku {
                name: "S0".to_string(),
            },
            properties: CognitiveServicesAccountCreateProperties {
                custom_sub_domain_name: "my-account".to_string(),
            },
        };

        let json = serde_json::to_string(&params).expect("should serialize");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["kind"], "AIServices");
        assert_eq!(value["location"], "eastus");
        assert_eq!(value["sku"]["name"], "S0");
        assert_eq!(value["properties"]["customSubDomainName"], "my-account");
    }

    /// The deployment create body must match the ARM shape: sku {name,capacity} and
    /// properties.model {format,name,version}.
    #[test]
    fn serializes_deployment_create_parameters() {
        let params = CognitiveServicesDeploymentCreateParameters {
            sku: CognitiveServicesDeploymentSku {
                name: "GlobalStandard".to_string(),
                capacity: 1,
            },
            properties: CognitiveServicesDeploymentCreateProperties {
                model: CognitiveServicesDeploymentModel {
                    format: "OpenAI".to_string(),
                    name: "gpt-4.1".to_string(),
                    version: "2025-04-14".to_string(),
                },
            },
        };

        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&params).unwrap()).unwrap();

        assert_eq!(value["sku"]["name"], "GlobalStandard");
        assert_eq!(value["sku"]["capacity"], 1);
        assert_eq!(value["properties"]["model"]["format"], "OpenAI");
        assert_eq!(value["properties"]["model"]["name"], "gpt-4.1");
        assert_eq!(value["properties"]["model"]["version"], "2025-04-14");
    }

    /// The ARM GET response for a deployment must deserialize, including the
    /// provisioning state the controller polls on.
    #[test]
    fn deserializes_deployment_get_response() {
        let json = r#"{
            "sku": { "name": "GlobalStandard", "capacity": 1 },
            "properties": {
                "model": { "format": "OpenAI", "name": "gpt-4.1", "version": "2025-04-14" },
                "provisioningState": "Succeeded"
            }
        }"#;

        let deployment: CognitiveServicesDeployment =
            serde_json::from_str(json).expect("ARM GET deployment response should deserialize");

        let props = deployment.properties.expect("properties should be present");
        assert_eq!(props.model.name, "gpt-4.1");
        assert_eq!(props.model.version, "2025-04-14");
        assert_eq!(props.provisioning_state.as_deref(), Some("Succeeded"));
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod mock_tests {
    use super::*;

    /// Happy-path round-trip: create, get, delete using the mock. Confirms the mock
    /// generated by automock compiles and satisfies the trait surface.
    #[tokio::test]
    async fn mock_create_get_delete_round_trip() {
        let mut mock = MockCognitiveServicesAccountsApi::new();

        mock.expect_create_account()
            .returning(|_, _, _| {
                Ok(OperationResult::Completed(CognitiveServicesAccount {
                    kind: Some("AIServices".to_string()),
                    location: Some("eastus".to_string()),
                    sku: Some(CognitiveServicesSku {
                        name: "S0".to_string(),
                    }),
                    properties: Some(CognitiveServicesAccountProperties {
                        endpoint: Some(
                            "https://my-account.cognitiveservices.azure.com/".to_string(),
                        ),
                        provisioning_state: Some("Succeeded".to_string()),
                    }),
                }))
            });

        mock.expect_get_account().returning(|_, _| {
            Ok(CognitiveServicesAccount {
                kind: Some("AIServices".to_string()),
                location: Some("eastus".to_string()),
                sku: Some(CognitiveServicesSku {
                    name: "S0".to_string(),
                }),
                properties: Some(CognitiveServicesAccountProperties {
                    endpoint: Some(
                        "https://my-account.cognitiveservices.azure.com/".to_string(),
                    ),
                    provisioning_state: Some("Succeeded".to_string()),
                }),
            })
        });

        mock.expect_delete_account().returning(|_, _| Ok(()));

        let params = CognitiveServicesAccountCreateParameters {
            location: "eastus".to_string(),
            kind: "AIServices".to_string(),
            sku: CognitiveServicesSku {
                name: "S0".to_string(),
            },
            properties: CognitiveServicesAccountCreateProperties {
                custom_sub_domain_name: "my-account".to_string(),
            },
        };

        let create_result = mock
            .create_account("my-rg", "my-account", &params)
            .await
            .expect("create should succeed");

        let account = match create_result {
            OperationResult::Completed(a) => a,
            OperationResult::LongRunning(_) => panic!("expected completed result from mock"),
        };

        let props = account.properties.expect("properties should be present");
        assert_eq!(
            props.endpoint.as_deref(),
            Some("https://my-account.cognitiveservices.azure.com/")
        );
        assert_eq!(props.provisioning_state.as_deref(), Some("Succeeded"));

        let gotten = mock
            .get_account("my-rg", "my-account")
            .await
            .expect("get should succeed");

        assert_eq!(
            gotten
                .properties
                .as_ref()
                .and_then(|p| p.endpoint.as_deref()),
            Some("https://my-account.cognitiveservices.azure.com/")
        );

        mock.delete_account("my-rg", "my-account")
            .await
            .expect("delete should succeed");
    }
}
