use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::long_running_operation::OperationResult;
use crate::azure::models::managed_clusters::{CredentialResults, ManagedCluster};
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use reqwest::{Client, Method};

#[cfg(feature = "test-utils")]
use mockall::automock;

const AKS_API_VERSION: &str = "2026-02-01";

pub type ManagedClusterOperationResult = OperationResult<ManagedCluster>;
pub type DeleteManagedClusterOperationResult = OperationResult<()>;

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait ManagedClustersApi: Send + Sync + std::fmt::Debug {
    async fn create_or_update_managed_cluster(
        &self,
        resource_group_name: &str,
        managed_cluster_name: &str,
        managed_cluster: &ManagedCluster,
    ) -> Result<ManagedClusterOperationResult>;

    async fn get_managed_cluster(
        &self,
        resource_group_name: &str,
        managed_cluster_name: &str,
    ) -> Result<ManagedCluster>;

    async fn delete_managed_cluster(
        &self,
        resource_group_name: &str,
        managed_cluster_name: &str,
    ) -> Result<DeleteManagedClusterOperationResult>;

    async fn list_cluster_admin_credentials(
        &self,
        resource_group_name: &str,
        managed_cluster_name: &str,
    ) -> Result<CredentialResults>;

    async fn list_cluster_user_credentials(
        &self,
        resource_group_name: &str,
        managed_cluster_name: &str,
    ) -> Result<CredentialResults>;
}

#[derive(Debug)]
pub struct AzureManagedClustersClient {
    pub base: AzureClientBase,
    pub token_cache: AzureTokenCache,
}

impl AzureManagedClustersClient {
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

    fn managed_cluster_path(
        &self,
        resource_group_name: &str,
        managed_cluster_name: &str,
    ) -> String {
        format!(
            "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerService/managedClusters/{}",
            &self.token_cache.config().subscription_id,
            resource_group_name,
            managed_cluster_name
        )
    }

    fn managed_cluster_url(&self, resource_group_name: &str, managed_cluster_name: &str) -> String {
        self.base.build_url(
            &self.managed_cluster_path(resource_group_name, managed_cluster_name),
            Some(vec![("api-version", AKS_API_VERSION.into())]),
        )
    }

    async fn bearer_token(&self) -> Result<String> {
        self.token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl ManagedClustersApi for AzureManagedClustersClient {
    async fn create_or_update_managed_cluster(
        &self,
        resource_group_name: &str,
        managed_cluster_name: &str,
        managed_cluster: &ManagedCluster,
    ) -> Result<ManagedClusterOperationResult> {
        let bearer_token = self.bearer_token().await?;
        let url = self.managed_cluster_url(resource_group_name, managed_cluster_name);
        let body = serde_json::to_string(managed_cluster)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize AKS managed cluster: {}",
                    managed_cluster_name
                ),
            })?;

        let req = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body)
            .build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(
                signed,
                "CreateOrUpdateManagedCluster",
                managed_cluster_name,
            )
            .await
    }

    async fn get_managed_cluster(
        &self,
        resource_group_name: &str,
        managed_cluster_name: &str,
    ) -> Result<ManagedCluster> {
        let bearer_token = self.bearer_token().await?;
        let url = self.managed_cluster_url(resource_group_name, managed_cluster_name);
        let req = AzureRequestBuilder::new(Method::GET, url).build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        let response = self
            .base
            .execute_request(signed, "GetManagedCluster", managed_cluster_name)
            .await?;
        let status = response.status().as_u16();
        let body =
            response
                .text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!(
                        "Azure GetManagedCluster: failed to read response body for {}",
                        managed_cluster_name
                    ),
                })?;

        serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetManagedCluster: JSON parse error for {}",
                    managed_cluster_name
                ),
                url: self.managed_cluster_url(resource_group_name, managed_cluster_name),
                http_status: status,
                http_request_text: None,
                http_response_text: None,
            })
    }

    async fn delete_managed_cluster(
        &self,
        resource_group_name: &str,
        managed_cluster_name: &str,
    ) -> Result<DeleteManagedClusterOperationResult> {
        let bearer_token = self.bearer_token().await?;
        let url = self.managed_cluster_url(resource_group_name, managed_cluster_name);
        let req = AzureRequestBuilder::new(Method::DELETE, url).build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(
                signed,
                "DeleteManagedCluster",
                managed_cluster_name,
            )
            .await
    }

    async fn list_cluster_admin_credentials(
        &self,
        resource_group_name: &str,
        managed_cluster_name: &str,
    ) -> Result<CredentialResults> {
        self.list_cluster_credentials(
            resource_group_name,
            managed_cluster_name,
            "listClusterAdminCredential",
            "ListClusterAdminCredentials",
        )
        .await
    }

    async fn list_cluster_user_credentials(
        &self,
        resource_group_name: &str,
        managed_cluster_name: &str,
    ) -> Result<CredentialResults> {
        self.list_cluster_credentials(
            resource_group_name,
            managed_cluster_name,
            "listClusterUserCredential",
            "ListClusterUserCredentials",
        )
        .await
    }
}

impl AzureManagedClustersClient {
    async fn list_cluster_credentials(
        &self,
        resource_group_name: &str,
        managed_cluster_name: &str,
        action: &str,
        operation: &str,
    ) -> Result<CredentialResults> {
        let bearer_token = self.bearer_token().await?;
        let url = self.base.build_url(
            &format!(
                "{}/{}",
                self.managed_cluster_path(resource_group_name, managed_cluster_name),
                action
            ),
            Some(vec![("api-version", AKS_API_VERSION.into())]),
        );
        let req = AzureRequestBuilder::new(Method::POST, url.clone())
            .content_length("")
            .build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        let response = self
            .base
            .execute_request(signed, operation, managed_cluster_name)
            .await?;
        let status = response.status().as_u16();
        let body =
            response
                .text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!(
                        "Azure {operation}: failed to read response body for {}",
                        managed_cluster_name
                    ),
                })?;

        serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure {operation}: JSON parse error for {}",
                    managed_cluster_name
                ),
                url,
                http_status: status,
                http_request_text: None,
                http_response_text: None,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_current_aks_api_version() {
        assert_eq!(AKS_API_VERSION, "2026-02-01");
    }
}
