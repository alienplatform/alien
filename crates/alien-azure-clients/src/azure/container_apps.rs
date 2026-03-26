use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::long_running_operation::OperationResult;
use crate::azure::models::{
    container_apps::ContainerApp,
    jobs::{Job, JobExecution},
    managed_environments::ManagedEnvironment,
    managed_environments_dapr_components::{
        DaprComponent, DaprComponentsCollection, DaprSecretsCollection,
    },
};
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};

use alien_error::{Context, IntoAlienError};
use async_trait::async_trait;
use reqwest::{Client, Method};

#[cfg(feature = "test-utils")]
use mockall::automock;

/// Result of a container app create or update operation
pub type ContainerAppOperationResult = OperationResult<ContainerApp>;

/// Result of a managed environment create or update operation
pub type ManagedEnvironmentOperationResult = OperationResult<ManagedEnvironment>;

/// Result of a job create or update operation
pub type JobOperationResult = OperationResult<Job>;

/// Result of a job execution start operation
pub type JobExecutionOperationResult = OperationResult<JobExecution>;

/// Result of a dapr component create or update operation
pub type DaprComponentOperationResult = OperationResult<DaprComponent>;

// -------------------------------------------------------------------------
// Container Apps API trait
// -------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait ContainerAppsApi: Send + Sync + std::fmt::Debug {
    async fn create_or_update_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
        container_app: &ContainerApp,
    ) -> Result<ContainerAppOperationResult>;

    async fn update_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
        container_app: &ContainerApp,
    ) -> Result<ContainerAppOperationResult>;

    async fn get_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
    ) -> Result<ContainerApp>;

    async fn delete_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
    ) -> Result<OperationResult<()>>;

    async fn create_or_update_managed_environment(
        &self,
        resource_group_name: &str,
        environment_name: &str,
        managed_environment: &ManagedEnvironment,
    ) -> Result<ManagedEnvironmentOperationResult>;

    async fn update_managed_environment(
        &self,
        resource_group_name: &str,
        environment_name: &str,
        managed_environment: &ManagedEnvironment,
    ) -> Result<ManagedEnvironmentOperationResult>;

    async fn get_managed_environment(
        &self,
        resource_group_name: &str,
        environment_name: &str,
    ) -> Result<ManagedEnvironment>;

    async fn delete_managed_environment(
        &self,
        resource_group_name: &str,
        environment_name: &str,
    ) -> Result<OperationResult<()>>;

    // -------------------------------------------------------------------------
    // Jobs API
    // -------------------------------------------------------------------------

    async fn create_or_update_job(
        &self,
        resource_group_name: &str,
        job_name: &str,
        job: &Job,
    ) -> Result<JobOperationResult>;

    async fn get_job(&self, resource_group_name: &str, job_name: &str) -> Result<Job>;

    async fn delete_job(
        &self,
        resource_group_name: &str,
        job_name: &str,
    ) -> Result<OperationResult<()>>;

    async fn start_job(
        &self,
        resource_group_name: &str,
        job_name: &str,
    ) -> Result<JobExecutionOperationResult>;

    async fn stop_job_execution(
        &self,
        resource_group_name: &str,
        job_name: &str,
        job_execution_name: &str,
    ) -> Result<OperationResult<()>>;

    // -------------------------------------------------------------------------
    // DAPR Components API
    // -------------------------------------------------------------------------

    async fn create_or_update_dapr_component(
        &self,
        resource_group_name: &str,
        environment_name: &str,
        component_name: &str,
        dapr_component: &DaprComponent,
    ) -> Result<DaprComponentOperationResult>;

    async fn get_dapr_component(
        &self,
        resource_group_name: &str,
        environment_name: &str,
        component_name: &str,
    ) -> Result<DaprComponent>;

    async fn list_dapr_components(
        &self,
        resource_group_name: &str,
        environment_name: &str,
    ) -> Result<DaprComponentsCollection>;

    async fn list_dapr_component_secrets(
        &self,
        resource_group_name: &str,
        environment_name: &str,
        component_name: &str,
    ) -> Result<DaprSecretsCollection>;
}

// -------------------------------------------------------------------------
// Container Apps client struct
// -------------------------------------------------------------------------

#[derive(Debug)]
pub struct AzureContainerAppsClient {
    pub base: AzureClientBase,
    pub token_cache: AzureTokenCache,
}

impl AzureContainerAppsClient {
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
impl ContainerAppsApi for AzureContainerAppsClient {
    /// Create or update a Container App
    ///
    /// This method handles the Azure Container Apps API for both creating new apps
    /// and updating existing ones. Azure uses PUT semantics for both operations.
    ///
    /// The operation may complete synchronously (201/200 with result) or be long-running
    /// (202 with polling URLs). Use the returned OperationResult to handle both cases.
    ///
    /// # Arguments
    /// * `resource_group_name` - Name of the Azure Resource Group
    /// * `container_app_name` - Name of the Container App
    /// * `container_app` - Complete Container App definition
    async fn create_or_update_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
        container_app: &ContainerApp,
    ) -> Result<OperationResult<ContainerApp>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/containerApps/{}",
                &self.token_cache.config().subscription_id, resource_group_name, container_app_name
            ),
            Some(vec![("api-version", "2025-01-01".into())]),
        );

        let body = serde_json::to_string(container_app)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Failed to serialize container app: {}", container_app_name),
            })?;

        let builder = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        // Use the generic long-running operation support
        self.base
            .execute_request_with_long_running_support(
                signed,
                "CreateOrUpdateContainerApp",
                container_app_name,
            )
            .await
    }

    /// Update a Container App using JSON Merge Patch
    ///
    /// This method uses HTTP PATCH with JSON Merge Patch semantics, which allows
    /// partial updates to the Container App resource. Only the properties specified
    /// in the request body will be updated, while unspecified properties remain unchanged.
    ///
    /// Use this method when:
    /// - Updating specific properties of an existing Container App
    /// - You want to preserve existing configuration not mentioned in the request
    /// - Making incremental changes to a Container App
    ///
    /// The operation may complete synchronously (200/201 with result) or be long-running
    /// (202 with polling URLs). Use the returned OperationResult to handle both cases.
    ///
    /// # Arguments
    /// * `resource_group_name` - Name of the Azure Resource Group
    /// * `container_app_name` - Name of the existing Container App to update
    /// * `container_app` - Partial Container App definition with only the fields to update
    async fn update_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
        container_app: &ContainerApp,
    ) -> Result<OperationResult<ContainerApp>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/containerApps/{}",
                &self.token_cache.config().subscription_id, resource_group_name, container_app_name
            ),
            Some(vec![("api-version", "2025-01-01".into())]),
        );

        let body = serde_json::to_string(container_app)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Failed to serialize container app: {}", container_app_name),
            })?;

        let builder = AzureRequestBuilder::new(Method::PATCH, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        // Use the generic long-running operation support
        self.base
            .execute_request_with_long_running_support(
                signed,
                "UpdateContainerApp",
                container_app_name,
            )
            .await
    }

    /// Get a Container App by name
    async fn get_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
    ) -> Result<ContainerApp> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/containerApps/{}",
                &self.token_cache.config().subscription_id, resource_group_name, container_app_name
            ),
            Some(vec![("api-version", "2025-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetContainerApp", container_app_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetContainerApp: failed to read response body for {}",
                    container_app_name
                ),
                url: url.clone(),
                http_status: 200,
                http_response_text: None,
                http_request_text: None,
            })?;

        let container_app: ContainerApp = serde_json::from_str(&body).into_alien_error().context(
            ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetContainerApp: JSON parse error for {}",
                    container_app_name
                ),
                url: url,
                http_status: 200,
                http_response_text: Some(body.clone()),
                http_request_text: None,
            },
        )?;

        Ok(container_app)
    }

    /// Delete a Container App
    ///
    /// This method deletes a Container App. The operation may complete synchronously with
    /// a 204 status code if the deletion is immediate, or asynchronously returning
    /// a 202 status code if the deletion is in progress. Returns an OperationResult
    /// that can be used to wait for completion if needed.
    async fn delete_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
    ) -> Result<OperationResult<()>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/containerApps/{}",
                &self.token_cache.config().subscription_id, resource_group_name, container_app_name
            ),
            Some(vec![("api-version", "2025-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        // Use the generic long-running operation support (now supports NO_CONTENT)
        self.base
            .execute_request_with_long_running_support(
                signed,
                "DeleteContainerApp",
                container_app_name,
            )
            .await
    }

    /// Create or update a Managed Environment
    ///
    /// This method handles the Azure Container Apps Managed Environment API for both creating
    /// new environments and updating existing ones. Azure uses PUT semantics for both operations.
    ///
    /// The operation may complete synchronously (201/200 with result) or be long-running
    /// (202 with polling URLs). Use the returned OperationResult to handle both cases.
    ///
    /// # Arguments
    /// * `resource_group_name` - Name of the Azure Resource Group
    /// * `environment_name` - Name of the Managed Environment
    /// * `managed_environment` - Complete Managed Environment definition
    async fn create_or_update_managed_environment(
        &self,
        resource_group_name: &str,
        environment_name: &str,
        managed_environment: &ManagedEnvironment,
    ) -> Result<ManagedEnvironmentOperationResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/managedEnvironments/{}", 
                     &self.token_cache.config().subscription_id, resource_group_name, environment_name),
            Some(vec![("api-version", "2025-01-01".into())]),
        );

        let body = serde_json::to_string(managed_environment)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize managed environment: {}",
                    environment_name
                ),
            })?;

        let builder = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        // Use the generic long-running operation support
        self.base
            .execute_request_with_long_running_support(
                signed,
                "CreateOrUpdateManagedEnvironment",
                environment_name,
            )
            .await
    }

    /// Update a Managed Environment using JSON Merge Patch
    ///
    /// This method uses HTTP PATCH with JSON Merge Patch semantics, which allows
    /// partial updates to the Managed Environment resource. Only the properties specified
    /// in the request body will be updated, while unspecified properties remain unchanged.
    ///
    /// Use this method when:
    /// - Updating specific properties of an existing Managed Environment
    /// - You want to preserve existing configuration not mentioned in the request
    /// - Making incremental changes to a Managed Environment
    ///
    /// The operation may complete synchronously (200/201 with result) or be long-running
    /// (202 with polling URLs). Use the returned OperationResult to handle both cases.
    ///
    /// # Arguments
    /// * `resource_group_name` - Name of the Azure Resource Group
    /// * `environment_name` - Name of the existing Managed Environment to update
    /// * `managed_environment` - Partial Managed Environment definition with only the fields to update
    async fn update_managed_environment(
        &self,
        resource_group_name: &str,
        environment_name: &str,
        managed_environment: &ManagedEnvironment,
    ) -> Result<ManagedEnvironmentOperationResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/managedEnvironments/{}", 
                     &self.token_cache.config().subscription_id, resource_group_name, environment_name),
            Some(vec![("api-version", "2025-01-01".into())]),
        );

        let body = serde_json::to_string(managed_environment)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize managed environment: {}",
                    environment_name
                ),
            })?;

        let builder = AzureRequestBuilder::new(Method::PATCH, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        // Use the generic long-running operation support
        self.base
            .execute_request_with_long_running_support(
                signed,
                "UpdateManagedEnvironment",
                environment_name,
            )
            .await
    }

    /// Get a Managed Environment by name
    async fn get_managed_environment(
        &self,
        resource_group_name: &str,
        environment_name: &str,
    ) -> Result<ManagedEnvironment> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/managedEnvironments/{}", 
                     &self.token_cache.config().subscription_id, resource_group_name, environment_name),
            Some(vec![("api-version", "2025-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetManagedEnvironment", environment_name)
            .await?;

        let url_clone = url.clone();
        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetManagedEnvironment: failed to read response body for {}",
                    environment_name
                ),
                url: url_clone,
                http_status: 200,
                http_request_text: None,
                http_response_text: None,
            })?;

        let managed_environment: ManagedEnvironment = serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetManagedEnvironment: JSON parse error for {}",
                    environment_name
                ),
                url: url,
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(body.clone()),
            })?;

        Ok(managed_environment)
    }

    /// Delete a Managed Environment
    ///
    /// This method deletes a Managed Environment. The operation may complete synchronously with
    /// a 204 status code if the deletion is immediate, or asynchronously returning
    /// a 202 status code if the deletion is in progress. Returns an OperationResult
    /// that can be used to wait for completion if needed.
    async fn delete_managed_environment(
        &self,
        resource_group_name: &str,
        environment_name: &str,
    ) -> Result<OperationResult<()>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/managedEnvironments/{}", 
                     &self.token_cache.config().subscription_id, resource_group_name, environment_name),
            Some(vec![("api-version", "2025-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        // Use the generic long-running operation support
        self.base
            .execute_request_with_long_running_support(
                signed,
                "DeleteManagedEnvironment",
                environment_name,
            )
            .await
    }

    // -------------------------------------------------------------------------
    // Jobs API implementation
    // -------------------------------------------------------------------------

    /// Create or update a Container Apps Job
    ///
    /// This method handles the Azure Container Apps Jobs API for both creating new jobs
    /// and updating existing ones. Azure uses PUT semantics for both operations.
    ///
    /// The operation may complete synchronously (201/200 with result) or be long-running
    /// (202 with polling URLs). Use the returned OperationResult to handle both cases.
    ///
    /// # Arguments
    /// * `resource_group_name` - Name of the Azure Resource Group
    /// * `job_name` - Name of the Container Apps Job
    /// * `job` - Complete Job definition
    async fn create_or_update_job(
        &self,
        resource_group_name: &str,
        job_name: &str,
        job: &Job,
    ) -> Result<JobOperationResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/jobs/{}",
                &self.token_cache.config().subscription_id, resource_group_name, job_name
            ),
            Some(vec![("api-version", "2025-01-01".into())]),
        );

        let body = serde_json::to_string(job).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize job: {}", job_name),
            },
        )?;

        let builder = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        // Use the generic long-running operation support
        self.base
            .execute_request_with_long_running_support(signed, "CreateOrUpdateJob", job_name)
            .await
    }

    /// Get a Container Apps Job by name
    async fn get_job(&self, resource_group_name: &str, job_name: &str) -> Result<Job> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/jobs/{}",
                &self.token_cache.config().subscription_id, resource_group_name, job_name
            ),
            Some(vec![("api-version", "2025-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetJob", job_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetJob: failed to read response body for {}",
                    job_name
                ),
                url: url.clone(),
                http_status: 200,
                http_response_text: None,
                http_request_text: None,
            })?;

        let job: Job = serde_json::from_str(&body).into_alien_error().context(
            ErrorData::HttpResponseError {
                message: format!("Azure GetJob: JSON parse error for {}", job_name),
                url: url,
                http_status: 200,
                http_response_text: Some(body.clone()),
                http_request_text: None,
            },
        )?;

        Ok(job)
    }

    /// Delete a Container Apps Job
    ///
    /// This method deletes a Container Apps Job. The operation may complete synchronously with
    /// a 204 status code if the deletion is immediate, or asynchronously returning
    /// a 202 status code if the deletion is in progress. Returns an OperationResult
    /// that can be used to wait for completion if needed.
    async fn delete_job(
        &self,
        resource_group_name: &str,
        job_name: &str,
    ) -> Result<OperationResult<()>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/jobs/{}",
                &self.token_cache.config().subscription_id, resource_group_name, job_name
            ),
            Some(vec![("api-version", "2025-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        // Use the generic long-running operation support
        self.base
            .execute_request_with_long_running_support(signed, "DeleteJob", job_name)
            .await
    }

    /// Start a Container Apps Job
    ///
    /// This method starts a new execution of a Container Apps Job. The operation
    /// returns a job execution that can be monitored for completion.
    ///
    /// # Arguments
    /// * `resource_group_name` - Name of the Azure Resource Group
    /// * `job_name` - Name of the Container Apps Job to start
    async fn start_job(
        &self,
        resource_group_name: &str,
        job_name: &str,
    ) -> Result<JobExecutionOperationResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/jobs/{}/start",
                &self.token_cache.config().subscription_id, resource_group_name, job_name
            ),
            Some(vec![("api-version", "2025-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::POST, url).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        // Use the generic long-running operation support
        self.base
            .execute_request_with_long_running_support(signed, "StartJob", job_name)
            .await
    }

    /// Stop a Container Apps Job Execution
    ///
    /// This method stops a specific execution of a Container Apps Job.
    ///
    /// # Arguments
    /// * `resource_group_name` - Name of the Azure Resource Group
    /// * `job_name` - Name of the Container Apps Job
    /// * `job_execution_name` - Name of the specific job execution to stop
    async fn stop_job_execution(
        &self,
        resource_group_name: &str,
        job_name: &str,
        job_execution_name: &str,
    ) -> Result<OperationResult<()>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/jobs/{}/executions/{}/stop", 
                     &self.token_cache.config().subscription_id, resource_group_name, job_name, job_execution_name),
            Some(vec![("api-version", "2025-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::POST, url).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        // Use the generic long-running operation support
        self.base
            .execute_request_with_long_running_support(
                signed,
                "StopJobExecution",
                &format!("{}/{}", job_name, job_execution_name),
            )
            .await
    }

    // -------------------------------------------------------------------------
    // DAPR Components API implementation
    // -------------------------------------------------------------------------

    /// Create or update a DAPR Component in a Managed Environment
    ///
    /// This method handles the Azure Container Apps DAPR Components API for both creating new components
    /// and updating existing ones. Azure uses PUT semantics for both operations.
    ///
    /// The operation may complete synchronously (201/200 with result) or be long-running
    /// (202 with polling URLs). Use the returned OperationResult to handle both cases.
    ///
    /// # Arguments
    /// * `resource_group_name` - Name of the Azure Resource Group
    /// * `environment_name` - Name of the Managed Environment
    /// * `component_name` - Name of the DAPR Component
    /// * `dapr_component` - Complete DAPR Component definition
    async fn create_or_update_dapr_component(
        &self,
        resource_group_name: &str,
        environment_name: &str,
        component_name: &str,
        dapr_component: &DaprComponent,
    ) -> Result<DaprComponentOperationResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/managedEnvironments/{}/daprComponents/{}", 
                     &self.token_cache.config().subscription_id, resource_group_name, environment_name, component_name),
            Some(vec![("api-version", "2025-01-01".into())]),
        );

        let body = serde_json::to_string(dapr_component)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Failed to serialize DAPR component: {}", component_name),
            })?;

        let builder = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        // Use the generic long-running operation support
        self.base
            .execute_request_with_long_running_support(
                signed,
                "CreateOrUpdateDaprComponent",
                component_name,
            )
            .await
    }

    /// Get a DAPR Component by name
    async fn get_dapr_component(
        &self,
        resource_group_name: &str,
        environment_name: &str,
        component_name: &str,
    ) -> Result<DaprComponent> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/managedEnvironments/{}/daprComponents/{}", 
                     &self.token_cache.config().subscription_id, resource_group_name, environment_name, component_name),
            Some(vec![("api-version", "2025-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetDaprComponent", component_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetDaprComponent: failed to read response body for {}",
                    component_name
                ),
                url: url.clone(),
                http_status: 200,
                http_response_text: None,
                http_request_text: None,
            })?;

        let dapr_component: DaprComponent = serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetDaprComponent: JSON parse error for {}",
                    component_name
                ),
                url: url,
                http_status: 200,
                http_response_text: Some(body.clone()),
                http_request_text: None,
            })?;

        Ok(dapr_component)
    }

    /// List all DAPR Components in a Managed Environment
    async fn list_dapr_components(
        &self,
        resource_group_name: &str,
        environment_name: &str,
    ) -> Result<DaprComponentsCollection> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/managedEnvironments/{}/daprComponents", 
                     &self.token_cache.config().subscription_id, resource_group_name, environment_name),
            Some(vec![("api-version", "2025-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "ListDaprComponents", environment_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure ListDaprComponents: failed to read response body for environment {}",
                    environment_name
                ),
                url: url.clone(),
                http_status: 200,
                http_response_text: None,
                http_request_text: None,
            })?;

        let dapr_components: DaprComponentsCollection = serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure ListDaprComponents: JSON parse error for environment {}",
                    environment_name
                ),
                url: url,
                http_status: 200,
                http_response_text: Some(body.clone()),
                http_request_text: None,
            })?;

        Ok(dapr_components)
    }

    /// List secrets for a DAPR Component
    ///
    /// This method retrieves all secrets associated with a specific DAPR component.
    /// Note that this operation requires a POST request to the listSecrets endpoint.
    async fn list_dapr_component_secrets(
        &self,
        resource_group_name: &str,
        environment_name: &str,
        component_name: &str,
    ) -> Result<DaprSecretsCollection> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/managedEnvironments/{}/daprComponents/{}/listSecrets", 
                     &self.token_cache.config().subscription_id, resource_group_name, environment_name, component_name),
            Some(vec![("api-version", "2025-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::POST, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "ListDaprComponentSecrets", component_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure ListDaprComponentSecrets: failed to read response body for {}",
                    component_name
                ),
                url: url.clone(),
                http_status: 200,
                http_response_text: None,
                http_request_text: None,
            })?;

        let dapr_secrets: DaprSecretsCollection = serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure ListDaprComponentSecrets: JSON parse error for {}",
                    component_name
                ),
                url: url,
                http_status: 200,
                http_response_text: Some(body.clone()),
                http_request_text: None,
            })?;

        Ok(dapr_secrets)
    }
}
