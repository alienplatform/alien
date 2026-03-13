use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::models::managed_identity::{CloudError, Identity, IdentityUpdate};
use crate::azure::AzureClientConfig;
use crate::azure::AzureClientConfigExt;
use alien_client_core::{ErrorData, Result};

use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use reqwest::{Client, Method, StatusCode};
use serde::Deserialize;

#[cfg(feature = "test-utils")]
use mockall::automock;

// -----------------------------------------------------------------------------
// Managed Identity API trait
// -----------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait ManagedIdentityApi: Send + Sync + std::fmt::Debug {
    async fn create_or_update_user_assigned_identity(
        &self,
        resource_group_name: &str,
        resource_name: &str,
        identity: &Identity,
    ) -> Result<Identity>;

    async fn delete_user_assigned_identity(
        &self,
        resource_group_name: &str,
        resource_name: &str,
    ) -> Result<()>;

    async fn get_user_assigned_identity(
        &self,
        resource_group_name: &str,
        resource_name: &str,
    ) -> Result<Identity>;

    async fn update_user_assigned_identity(
        &self,
        resource_group_name: &str,
        resource_name: &str,
        identity_update: &IdentityUpdate,
    ) -> Result<Identity>;

    fn build_user_assigned_identity_id(
        &self,
        resource_group_name: &str,
        resource_name: &str,
    ) -> String;
}

// -----------------------------------------------------------------------------
// Managed Identity client struct
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct AzureManagedIdentityClient {
    pub base: AzureClientBase,
    pub client_config: AzureClientConfig,
}

impl AzureManagedIdentityClient {
    pub fn new(client: Client, client_config: AzureClientConfig) -> Self {
        let endpoint = client_config.management_endpoint().to_string();
        Self {
            base: AzureClientBase::with_client_config(client, endpoint, client_config.clone()),
            client_config,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl ManagedIdentityApi for AzureManagedIdentityClient {
    /// Create or update a user assigned identity
    async fn create_or_update_user_assigned_identity(
        &self,
        resource_group_name: &str,
        resource_name: &str,
        identity: &Identity,
    ) -> Result<Identity> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{}",
                &self.client_config.subscription_id, 
                resource_group_name, 
                resource_name
            ),
            Some(vec![("api-version", "2023-01-31".into())]),
        );

        let body = serde_json::to_string(identity).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize user assigned identity: {}",
                    resource_name
                ),
            },
        )?;

        let request_body = body.clone();
        let builder = AzureRequestBuilder::new(Method::PUT, url.clone())
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "CreateOrUpdateUserAssignedIdentity", resource_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure CreateOrUpdateUserAssignedIdentity: failed to read response body for {}",
                    resource_name
                ),
                url: url.clone(),
                http_status: 200,
                http_request_text: Some(request_body.clone()),
                http_response_text: None,
            })?;

        let identity: Identity = serde_json::from_str(&body).into_alien_error().context(
            ErrorData::HttpResponseError {
                message: format!(
                    "Azure CreateOrUpdateUserAssignedIdentity: JSON parse error for {}",
                    resource_name
                ),
                url: url,
                http_status: 200,
                http_request_text: Some(request_body),
                http_response_text: Some(body),
            },
        )?;

        Ok(identity)
    }

    /// Delete a user assigned identity
    async fn delete_user_assigned_identity(
        &self,
        resource_group_name: &str,
        resource_name: &str,
    ) -> Result<()> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{}",
                &self.client_config.subscription_id, 
                resource_group_name, 
                resource_name
            ),
            Some(vec![("api-version", "2023-01-31".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let _resp = self
            .base
            .execute_request(signed, "DeleteUserAssignedIdentity", resource_name)
            .await?;

        Ok(())
    }

    /// Get a user assigned identity
    async fn get_user_assigned_identity(
        &self,
        resource_group_name: &str,
        resource_name: &str,
    ) -> Result<Identity> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{}",
                &self.client_config.subscription_id, 
                resource_group_name, 
                resource_name
            ),
            Some(vec![("api-version", "2023-01-31".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetUserAssignedIdentity", resource_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetUserAssignedIdentity: failed to read response body for {}",
                    resource_name
                ),
                url: url.clone(),
                http_status: 200,
                http_request_text: None,
                http_response_text: None,
            })?;

        let identity: Identity = serde_json::from_str(&body).into_alien_error().context(
            ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetUserAssignedIdentity: JSON parse error for {}",
                    resource_name
                ),
                url: url,
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(body),
            },
        )?;

        Ok(identity)
    }

    /// Update a user assigned identity
    async fn update_user_assigned_identity(
        &self,
        resource_group_name: &str,
        resource_name: &str,
        identity_update: &IdentityUpdate,
    ) -> Result<Identity> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{}",
                &self.client_config.subscription_id, 
                resource_group_name, 
                resource_name
            ),
            Some(vec![("api-version", "2023-01-31".into())]),
        );

        let body = serde_json::to_string(identity_update)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize user assigned identity update: {}",
                    resource_name
                ),
            })?;

        let request_body = body.clone();
        let builder = AzureRequestBuilder::new(Method::PATCH, url.clone())
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "UpdateUserAssignedIdentity", resource_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure UpdateUserAssignedIdentity: failed to read response body for {}",
                    resource_name
                ),
                url: url.clone(),
                http_status: 200,
                http_request_text: Some(request_body.clone()),
                http_response_text: None,
            })?;

        let identity: Identity = serde_json::from_str(&body).into_alien_error().context(
            ErrorData::HttpResponseError {
                message: format!(
                    "Azure UpdateUserAssignedIdentity: JSON parse error for {}",
                    resource_name
                ),
                url: url,
                http_status: 200,
                http_request_text: Some(request_body),
                http_response_text: Some(body),
            },
        )?;

        Ok(identity)
    }

    /// Build the Azure resource ID for a user assigned identity
    fn build_user_assigned_identity_id(
        &self,
        resource_group_name: &str,
        resource_name: &str,
    ) -> String {
        format!(
            "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{}",
            &self.client_config.subscription_id, 
            resource_group_name, 
            resource_name
        )
    }
}
