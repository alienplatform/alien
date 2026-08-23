//! Azure Container Apps SandboxGroups (`Microsoft.App/sandboxGroups`).
//!
//! The ARM control plane only. Sandboxes themselves live on a **separate** ADC data plane at
//! `management.<region>.azuredevcompute.io`, gated by the `Container Apps SandboxGroup Data
//! Owner` role — subscription Owner returns 403 against it, measured. Management
//! actions alone provision cleanly and then fail at first exec, which is why the role assignment
//! is emitted alongside the group rather than left to the operator.

use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};

#[cfg(feature = "test-utils")]
use mockall::automock;

/// ARM API version for sandbox groups.
const API_VERSION: &str = "2025-02-02-preview";

/// Scope every ARM call is signed for.
const ARM_SCOPE: &str = "https://management.azure.com/.default";

/// A sandbox group: the top-level boundary every sandbox, image, snapshot and secret sits under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxGroup {
    /// ARM resource id
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Group name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Region the group lives in
    pub location: String,
    /// Provisioning state; deletion is async, so this is not a substitute for polling to 404
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<SandboxGroupProperties>,
}

/// Observed state of a sandbox group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxGroupProperties {
    /// `Succeeded`, `Failed`, `Creating`, `Deleting`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisioning_state: Option<String>,
}

#[cfg_attr(feature = "test-utils", automock)]
#[async_trait]
pub trait SandboxGroupsApi: Send + Sync + std::fmt::Debug {
    /// Creates or updates a sandbox group.
    async fn create_or_update_sandbox_group(
        &self,
        resource_group: &str,
        name: &str,
        location: &str,
    ) -> Result<SandboxGroup>;

    /// Reads a sandbox group. A 404 surfaces as an error, which is how deletion is confirmed.
    async fn get_sandbox_group(&self, resource_group: &str, name: &str) -> Result<SandboxGroup>;

    /// Issues a delete. Returns before the group is gone — Azure deletes asynchronously, so the
    /// caller confirms by polling `get_sandbox_group` to 404 rather than trusting this.
    async fn delete_sandbox_group(&self, resource_group: &str, name: &str) -> Result<()>;
}

/// Client for `Microsoft.App/sandboxGroups`.
#[derive(Debug)]
pub struct AzureSandboxGroupsClient {
    pub base: AzureClientBase,
    pub token_cache: AzureTokenCache,
}

impl AzureSandboxGroupsClient {
    /// Builds a client against the ARM management endpoint.
    pub fn new(client: reqwest::Client, token_cache: AzureTokenCache) -> Self {
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

    fn group_path(&self, resource_group: &str, name: &str) -> String {
        format!(
            "/subscriptions/{}/resourceGroups/{resource_group}/providers/Microsoft.App/sandboxGroups/{name}",
            self.token_cache.config().subscription_id
        )
    }
}

impl AzureSandboxGroupsClient {
    /// Reads a response body and parses it, naming the operation so a parse failure says which
    /// call produced the body rather than only that some JSON was wrong.
    async fn parse<T: serde::de::DeserializeOwned>(
        response: reqwest::Response,
        operation: &str,
        name: &str,
    ) -> Result<T> {
        let body = response
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::GenericError {
                message: format!("Azure {operation}: failed to read response body for {name}"),
            })?;

        serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::GenericError {
                message: format!("Azure {operation}: unexpected response body for {name}: {body}"),
            })
    }
}

#[async_trait]
impl SandboxGroupsApi for AzureSandboxGroupsClient {
    async fn create_or_update_sandbox_group(
        &self,
        resource_group: &str,
        name: &str,
        location: &str,
    ) -> Result<SandboxGroup> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope(ARM_SCOPE)
            .await?;

        let url = self.base.build_url(
            &self.group_path(resource_group, name),
            Some(vec![("api-version", API_VERSION.into())]),
        );

        let group = SandboxGroup {
            id: None,
            name: None,
            location: location.to_string(),
            properties: None,
        };

        let body = serde_json::to_string(&group).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize sandbox group '{name}'"),
            },
        )?;

        let request = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body)
            .build()?;

        let signed = self.base.sign_request(request, &bearer_token).await?;
        let response = self
            .base
            .execute_request(signed, "CreateOrUpdateSandboxGroup", name)
            .await?;
        Self::parse(response, "CreateOrUpdateSandboxGroup", name).await
    }

    async fn get_sandbox_group(&self, resource_group: &str, name: &str) -> Result<SandboxGroup> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope(ARM_SCOPE)
            .await?;

        let url = self.base.build_url(
            &self.group_path(resource_group, name),
            Some(vec![("api-version", API_VERSION.into())]),
        );

        let request = AzureRequestBuilder::new(Method::GET, url).build()?;
        let signed = self.base.sign_request(request, &bearer_token).await?;
        let response = self
            .base
            .execute_request(signed, "GetSandboxGroup", name)
            .await?;
        Self::parse(response, "GetSandboxGroup", name).await
    }

    async fn delete_sandbox_group(&self, resource_group: &str, name: &str) -> Result<()> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope(ARM_SCOPE)
            .await?;

        let url = self.base.build_url(
            &self.group_path(resource_group, name),
            Some(vec![("api-version", API_VERSION.into())]),
        );

        let request = AzureRequestBuilder::new(Method::DELETE, url).build()?;
        let signed = self.base.sign_request(request, &bearer_token).await?;
        // The response is discarded on purpose: it reports that the delete *started*. Deletion
        // is confirmed by polling get_sandbox_group to 404, never by this call.
        self.base
            .execute_request(signed, "DeleteSandboxGroup", name)
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_deserializes_from_an_arm_response() {
        let group: SandboxGroup = serde_json::from_str(
            r#"{"id":"/subscriptions/s/resourceGroups/rg/providers/Microsoft.App/sandboxGroups/sbg1",
                "name":"sbg1","location":"swedencentral",
                "properties":{"provisioningState":"Succeeded"}}"#,
        )
        .expect("deserializes");

        assert_eq!(group.name.as_deref(), Some("sbg1"));
        assert_eq!(
            group
                .properties
                .and_then(|p| p.provisioning_state)
                .as_deref(),
            Some("Succeeded")
        );
    }

    /// A create body carries only the location; ARM rejects a read-only id or name on write.
    #[test]
    fn a_create_body_omits_read_only_fields() {
        let group = SandboxGroup {
            id: None,
            name: None,
            location: "swedencentral".to_string(),
            properties: None,
        };

        let body = serde_json::to_value(&group).expect("serializes");
        assert_eq!(body["location"], "swedencentral");
        assert!(body.get("id").is_none());
        assert!(body.get("name").is_none());
        assert!(body.get("properties").is_none());
    }
}
