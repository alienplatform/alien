use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::models::managedservices::{RegistrationAssignment, RegistrationDefinition};
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};

use alien_error::{Context, IntoAlienError};
use async_trait::async_trait;
use reqwest::{Client, Method, StatusCode};

#[cfg(feature = "test-utils")]
use mockall::automock;

// -----------------------------------------------------------------------------
// Azure Managed Services (Lighthouse) API trait
// -----------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait ManagedServicesApi: Send + Sync + std::fmt::Debug {
    // Registration Definition APIs
    async fn get_registration_definition(
        &self,
        scope: &str,
        registration_definition_id: &str,
    ) -> Result<RegistrationDefinition>;

    async fn create_or_update_registration_definition(
        &self,
        scope: &str,
        registration_definition_id: &str,
        registration_definition: &RegistrationDefinition,
    ) -> Result<RegistrationDefinition>;

    async fn delete_registration_definition(
        &self,
        scope: &str,
        registration_definition_id: &str,
    ) -> Result<Option<RegistrationDefinition>>;

    // Registration Assignment APIs
    async fn get_registration_assignment(
        &self,
        scope: &str,
        registration_assignment_id: &str,
    ) -> Result<RegistrationAssignment>;

    async fn create_or_update_registration_assignment(
        &self,
        scope: &str,
        registration_assignment_id: &str,
        registration_assignment: &RegistrationAssignment,
    ) -> Result<RegistrationAssignment>;

    async fn delete_registration_assignment(
        &self,
        scope: &str,
        registration_assignment_id: &str,
    ) -> Result<Option<RegistrationAssignment>>;

    // Helper methods for scope building
    fn build_subscription_scope(&self, subscription_id: &str) -> String;
    fn build_resource_group_scope(
        &self,
        subscription_id: &str,
        resource_group_name: &str,
    ) -> String;
}

// -----------------------------------------------------------------------------
// Azure Managed Services client struct
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct AzureManagedServicesClient {
    pub base: AzureClientBase,
    pub token_cache: AzureTokenCache,
}

impl AzureManagedServicesClient {
    pub fn new(client: Client, token_cache: AzureTokenCache) -> Self {
        // Azure Resource Manager endpoint
        let endpoint = token_cache.management_endpoint().to_string();

        Self {
            base: AzureClientBase::with_client_config(client, endpoint, token_cache.config().clone()),
            token_cache,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl ManagedServicesApi for AzureManagedServicesClient {
    /// Get a registration definition
    async fn get_registration_definition(
        &self,
        scope: &str,
        registration_definition_id: &str,
    ) -> Result<RegistrationDefinition> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/{}/providers/Microsoft.ManagedServices/registrationDefinitions/{}",
                scope, registration_definition_id
            ),
            Some(vec![("api-version", "2022-10-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(
                signed,
                "GetRegistrationDefinition",
                registration_definition_id,
            )
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Azure GetRegistrationDefinition: failed to read response body"
                        .to_string(),
                })?;

        let registration_definition: RegistrationDefinition = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetRegistrationDefinition: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.clone(),
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(response_body),
            })?;

        Ok(registration_definition)
    }

    /// Create or update a registration definition
    async fn create_or_update_registration_definition(
        &self,
        scope: &str,
        registration_definition_id: &str,
        registration_definition: &RegistrationDefinition,
    ) -> Result<RegistrationDefinition> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/{}/providers/Microsoft.ManagedServices/registrationDefinitions/{}",
                scope, registration_definition_id
            ),
            Some(vec![("api-version", "2022-10-01".into())]),
        );

        let body = serde_json::to_string(registration_definition)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: "Failed to serialize registration definition".to_string(),
            })?;

        let builder = AzureRequestBuilder::new(Method::PUT, url.clone())
            .content_type_json()
            .content_length(&body)
            .body(body.clone());

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(
                signed,
                "CreateOrUpdateRegistrationDefinition",
                registration_definition_id,
            )
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message:
                        "Azure CreateOrUpdateRegistrationDefinition: failed to read response body"
                            .to_string(),
                })?;

        let registration_definition: RegistrationDefinition = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure CreateOrUpdateRegistrationDefinition: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.clone(),
                http_status: 200,
                http_request_text: Some(body),
                http_response_text: Some(response_body),
            })?;

        Ok(registration_definition)
    }

    /// Delete a registration definition
    async fn delete_registration_definition(
        &self,
        scope: &str,
        registration_definition_id: &str,
    ) -> Result<Option<RegistrationDefinition>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/{}/providers/Microsoft.ManagedServices/registrationDefinitions/{}",
                scope, registration_definition_id
            ),
            Some(vec![("api-version", "2022-10-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(
                signed,
                "DeleteRegistrationDefinition",
                registration_definition_id,
            )
            .await?;

        let status = resp.status();
        let registration_definition = if status == StatusCode::NO_CONTENT {
            None
        } else {
            let response_body =
                resp.text()
                    .await
                    .into_alien_error()
                    .context(ErrorData::HttpRequestFailed {
                        message: "Azure DeleteRegistrationDefinition: failed to read response body"
                            .to_string(),
                    })?;

            if response_body.is_empty() {
                None
            } else {
                Some(
                    serde_json::from_str(&response_body)
                        .into_alien_error()
                        .context(ErrorData::HttpResponseError {
                            message: format!(
                                "Azure DeleteRegistrationDefinition: JSON parse error. Body: {}",
                                response_body
                            ),
                            url: url.clone(),
                            http_status: status.as_u16(),
                            http_request_text: None,
                            http_response_text: Some(response_body),
                        })?,
                )
            }
        };

        Ok(registration_definition)
    }

    /// Get a registration assignment
    async fn get_registration_assignment(
        &self,
        scope: &str,
        registration_assignment_id: &str,
    ) -> Result<RegistrationAssignment> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/{}/providers/Microsoft.ManagedServices/registrationAssignments/{}",
                scope, registration_assignment_id
            ),
            Some(vec![("api-version", "2022-10-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(
                signed,
                "GetRegistrationAssignment",
                registration_assignment_id,
            )
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Azure GetRegistrationAssignment: failed to read response body"
                        .to_string(),
                })?;

        let registration_assignment: RegistrationAssignment = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetRegistrationAssignment: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.clone(),
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(response_body),
            })?;

        Ok(registration_assignment)
    }

    /// Create or update a registration assignment
    async fn create_or_update_registration_assignment(
        &self,
        scope: &str,
        registration_assignment_id: &str,
        registration_assignment: &RegistrationAssignment,
    ) -> Result<RegistrationAssignment> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/{}/providers/Microsoft.ManagedServices/registrationAssignments/{}",
                scope, registration_assignment_id
            ),
            Some(vec![("api-version", "2022-10-01".into())]),
        );

        let body = serde_json::to_string(registration_assignment)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize registration assignment: {}",
                    registration_assignment_id
                ),
            })?;

        let builder = AzureRequestBuilder::new(Method::PUT, url.clone())
            .content_type_json()
            .content_length(&body)
            .body(body.clone());

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(
                signed,
                "CreateOrUpdateRegistrationAssignment",
                registration_assignment_id,
            )
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message:
                        "Azure CreateOrUpdateRegistrationAssignment: failed to read response body"
                            .to_string(),
                })?;

        let registration_assignment: RegistrationAssignment = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure CreateOrUpdateRegistrationAssignment: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.clone(),
                http_status: 200,
                http_request_text: Some(body),
                http_response_text: Some(response_body),
            })?;

        Ok(registration_assignment)
    }

    /// Delete a registration assignment
    async fn delete_registration_assignment(
        &self,
        scope: &str,
        registration_assignment_id: &str,
    ) -> Result<Option<RegistrationAssignment>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/{}/providers/Microsoft.ManagedServices/registrationAssignments/{}",
                scope, registration_assignment_id
            ),
            Some(vec![("api-version", "2022-10-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(
                signed,
                "DeleteRegistrationAssignment",
                registration_assignment_id,
            )
            .await?;

        let status = resp.status();
        let registration_assignment = if status == StatusCode::NO_CONTENT {
            None
        } else {
            let response_body =
                resp.text()
                    .await
                    .into_alien_error()
                    .context(ErrorData::HttpRequestFailed {
                        message: "Azure DeleteRegistrationAssignment: failed to read response body"
                            .to_string(),
                    })?;

            if response_body.is_empty() {
                None
            } else {
                Some(
                    serde_json::from_str(&response_body)
                        .into_alien_error()
                        .context(ErrorData::HttpResponseError {
                            message: format!(
                                "Azure DeleteRegistrationAssignment: JSON parse error. Body: {}",
                                response_body
                            ),
                            url: url.clone(),
                            http_status: status.as_u16(),
                            http_request_text: None,
                            http_response_text: Some(response_body),
                        })?,
                )
            }
        };

        Ok(registration_assignment)
    }

    /// Helper method to build a subscription scope
    fn build_subscription_scope(&self, subscription_id: &str) -> String {
        format!("subscriptions/{}", subscription_id)
    }

    /// Helper method to build a resource group scope
    fn build_resource_group_scope(
        &self,
        subscription_id: &str,
        resource_group_name: &str,
    ) -> String {
        format!(
            "subscriptions/{}/resourceGroups/{}",
            subscription_id, resource_group_name
        )
    }
}
