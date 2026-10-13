//! ECS control-plane operations. Batch failures and pagination remain visible to callers.
//! Request fields follow the ECS 2014-11-13 API; cluster selection is always explicit.

use alien_client_core::{ErrorData, Result};
use alien_error::{Context, ContextError, IntoAlienError};
#[cfg(feature = "test-utils")]
use mockall::automock;
use reqwest::Client;
use serde::{de::DeserializeOwned, Serialize};

use crate::aws::{
    aws_request_utils::{sign_send_json_once, AwsRequestBuilderExt, AwsSignConfig},
    credential_provider::AwsCredentialProvider,
};

mod types;
pub use types::*;

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait EcsApi: Send + Sync + std::fmt::Debug {
    async fn create_cluster(&self, request: CreateClusterRequest) -> Result<ClusterResponse>;
    async fn describe_clusters(
        &self,
        request: DescribeClustersRequest,
    ) -> Result<DescribeClustersResponse>;
    async fn delete_cluster(&self, request: ClusterRequest) -> Result<ClusterResponse>;
    async fn register_task_definition(
        &self,
        request: RegisterTaskDefinitionRequest,
    ) -> Result<TaskDefinitionResponse>;
    async fn describe_task_definition(
        &self,
        request: DescribeTaskDefinitionRequest,
    ) -> Result<TaskDefinitionResponse>;
    async fn list_task_definitions(
        &self,
        request: ListTaskDefinitionsRequest,
    ) -> Result<ListTaskDefinitionsResponse>;
    async fn deregister_task_definition(
        &self,
        request: TaskDefinitionRequest,
    ) -> Result<TaskDefinitionResponse>;
    async fn delete_task_definitions(
        &self,
        request: DeleteTaskDefinitionsRequest,
    ) -> Result<DeleteTaskDefinitionsResponse>;
    async fn create_service(&self, request: CreateServiceRequest) -> Result<ServiceResponse>;
    async fn update_service(&self, request: UpdateServiceRequest) -> Result<ServiceResponse>;
    async fn describe_services(
        &self,
        request: DescribeServicesRequest,
    ) -> Result<DescribeServicesResponse>;
    async fn delete_service(&self, request: DeleteServiceRequest) -> Result<ServiceResponse>;
    async fn list_tasks(&self, request: ListTasksRequest) -> Result<ListTasksResponse>;
    async fn describe_tasks(&self, request: DescribeTasksRequest) -> Result<DescribeTasksResponse>;
}

#[derive(Debug, Clone)]
pub struct EcsClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl EcsClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
        }
    }

    async fn send<T: DeserializeOwned + Send + 'static>(
        &self,
        operation: &str,
        request: &(impl Serialize + Sync),
    ) -> Result<T> {
        self.credentials.ensure_fresh().await?;
        let body = serde_json::to_string(request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Encode ECS {operation} request"),
            },
        )?;
        let host = format!("ecs.{}.amazonaws.com", self.credentials.region());
        let base = self
            .credentials
            .get_service_endpoint_option("ecs")
            .map(str::to_string)
            .unwrap_or_else(|| format!("https://{host}"));
        let sign = AwsSignConfig {
            service_name: "ecs".into(),
            region: self.credentials.region().into(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        };
        let result = sign_send_json_once(
            self.client
                .post(format!("{}/", base.trim_end_matches('/')))
                .host(&host)
                .header("Content-Type", "application/x-amz-json-1.1")
                .header(
                    "X-Amz-Target",
                    format!("AmazonEC2ContainerServiceV20141113.{operation}"),
                )
                .body(body),
            &sign,
        )
        .await;
        result.map_err(|error| {
            let Some(ErrorData::HttpResponseError {
                http_response_text: Some(body),
                ..
            }) = &error.error
            else {
                return error;
            };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
                return error;
            };
            let code = value
                .get("__type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .rsplit('#')
                .next()
                .unwrap_or("");
            let detail = value
                .get("message")
                .or_else(|| value.get("Message"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let message = format!("{code}: {detail}");
            let mapped = match code {
                // ECS has no dedicated not-found code for this operation. Match the
                // native missing-definition response narrowly; other ClientException
                // failures must never make a durable owner forget a resource.
                "ClientException"
                    if operation == "DescribeTaskDefinition"
                        && detail == "Unable to describe task definition." =>
                {
                    ErrorData::RemoteResourceNotFound {
                        resource_type: "ECS task definition".into(),
                        resource_name: operation.into(),
                    }
                }
                "ClusterNotFoundException" | "ServiceNotFoundException" => {
                    ErrorData::RemoteResourceNotFound {
                        resource_type: "ECS resource".into(),
                        resource_name: operation.into(),
                    }
                }
                "AccessDeniedException" => ErrorData::RemoteAccessDenied {
                    resource_type: "ECS resource".into(),
                    resource_name: operation.into(),
                },
                "ThrottlingException" | "TooManyRequestsException" => {
                    ErrorData::RateLimitExceeded { message }
                }
                "ServerException" | "ServiceUnavailableException" => {
                    ErrorData::RemoteServiceUnavailable { message }
                }
                "LimitExceededException" => ErrorData::QuotaExceeded { message },
                "ClusterContainsContainerInstancesException"
                | "ClusterContainsServicesException"
                | "ClusterContainsCapacityProviderException"
                | "ClusterContainsTasksException"
                | "UpdateInProgressException"
                | "ServiceNotActiveException" => ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "ECS resource".into(),
                    resource_name: operation.into(),
                },
                "InvalidParameterException"
                | "ClientException"
                | "PlatformUnknownException"
                | "PlatformTaskDefinitionIncompatibilityException"
                | "UnsupportedFeatureException" => ErrorData::InvalidInput {
                    message,
                    field_name: None,
                },
                _ => return error,
            };
            error.context(mapped)
        })
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl EcsApi for EcsClient {
    async fn create_cluster(&self, request: CreateClusterRequest) -> Result<ClusterResponse> {
        self.send("CreateCluster", &request).await
    }
    async fn describe_clusters(
        &self,
        request: DescribeClustersRequest,
    ) -> Result<DescribeClustersResponse> {
        self.send("DescribeClusters", &request).await
    }
    async fn delete_cluster(&self, request: ClusterRequest) -> Result<ClusterResponse> {
        self.send("DeleteCluster", &request).await
    }
    async fn register_task_definition(
        &self,
        request: RegisterTaskDefinitionRequest,
    ) -> Result<TaskDefinitionResponse> {
        self.send("RegisterTaskDefinition", &request).await
    }
    async fn describe_task_definition(
        &self,
        request: DescribeTaskDefinitionRequest,
    ) -> Result<TaskDefinitionResponse> {
        self.send("DescribeTaskDefinition", &request).await
    }
    async fn list_task_definitions(
        &self,
        request: ListTaskDefinitionsRequest,
    ) -> Result<ListTaskDefinitionsResponse> {
        self.send("ListTaskDefinitions", &request).await
    }
    async fn deregister_task_definition(
        &self,
        request: TaskDefinitionRequest,
    ) -> Result<TaskDefinitionResponse> {
        self.send("DeregisterTaskDefinition", &request).await
    }
    async fn delete_task_definitions(
        &self,
        request: DeleteTaskDefinitionsRequest,
    ) -> Result<DeleteTaskDefinitionsResponse> {
        self.send("DeleteTaskDefinitions", &request).await
    }
    async fn create_service(&self, request: CreateServiceRequest) -> Result<ServiceResponse> {
        if request.client_token.is_empty() || !request.client_token.is_ascii() {
            return Err(alien_error::AlienError::new(ErrorData::InvalidInput {
                message: "ECS service creation requires a persisted ASCII client token".into(),
                field_name: Some("clientToken".into()),
            }));
        }
        self.send("CreateService", &request).await
    }
    async fn update_service(&self, request: UpdateServiceRequest) -> Result<ServiceResponse> {
        self.send("UpdateService", &request).await
    }
    async fn describe_services(
        &self,
        request: DescribeServicesRequest,
    ) -> Result<DescribeServicesResponse> {
        self.send("DescribeServices", &request).await
    }
    async fn delete_service(&self, request: DeleteServiceRequest) -> Result<ServiceResponse> {
        self.send("DeleteService", &request).await
    }
    async fn list_tasks(&self, request: ListTasksRequest) -> Result<ListTasksResponse> {
        self.send("ListTasks", &request).await
    }
    async fn describe_tasks(&self, request: DescribeTasksRequest) -> Result<DescribeTasksResponse> {
        self.send("DescribeTasks", &request).await
    }
}

#[cfg(test)]
mod tests;
