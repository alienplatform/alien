use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};

use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use bon::Builder;
use form_urlencoded;
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait CodeBuildApi: Send + Sync + std::fmt::Debug {
    async fn create_project(&self, request: CreateProjectRequest) -> Result<CreateProjectResponse>;
    async fn delete_project(&self, request: DeleteProjectRequest) -> Result<DeleteProjectResponse>;
    async fn update_project(&self, request: UpdateProjectRequest) -> Result<UpdateProjectResponse>;
    async fn batch_get_projects(
        &self,
        request: BatchGetProjectsRequest,
    ) -> Result<BatchGetProjectsResponse>;
    async fn start_build(&self, request: StartBuildRequest) -> Result<StartBuildResponse>;
    async fn stop_build(&self, request: StopBuildRequest) -> Result<StopBuildResponse>;
    async fn batch_get_builds(
        &self,
        request: BatchGetBuildsRequest,
    ) -> Result<BatchGetBuildsResponse>;
    async fn batch_delete_builds(
        &self,
        request: BatchDeleteBuildsRequest,
    ) -> Result<BatchDeleteBuildsResponse>;
    async fn retry_build(&self, request: RetryBuildRequest) -> Result<RetryBuildResponse>;
}

// ---------------------------------------------------------------------------
// CodeBuild client
// ---------------------------------------------------------------------------
#[derive(Debug, Clone)]
pub struct CodeBuildClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl CodeBuildClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self { client, credentials }
    }

    /// Get the region for this CodeBuild client
    pub fn region(&self) -> &str {
        self.credentials.region()
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "codebuild".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self.credentials.get_service_endpoint_option("codebuild") {
            override_url.to_string()
        } else {
            format!("https://codebuild.{}.amazonaws.com", self.credentials.region())
        }
    }

    // ------------------------- internal helpers -------------------------

    async fn post_json<T: DeserializeOwned + Send + 'static>(
        &self,
        action: &str,
        body: String,
        resource: &str,
    ) -> Result<T> {
        self.credentials.ensure_fresh().await?;
        let base_url = self.get_base_url();
        let url = format!("{}/", base_url.trim_end_matches('/'));

        let builder = self
            .client
            .post(&url)
            .host(&format!("codebuild.{}.amazonaws.com", self.credentials.region()))
            .header("X-Amz-Target", format!("CodeBuild_20161006.{}", action))
            .header("Content-Type", "application/x-amz-json-1.1")
            .body(body.clone());

        let result =
            crate::aws::aws_request_utils::sign_send_json(builder, &self.sign_config()).await;

        Self::map_result(result, action, resource, Some(&body))
    }

    async fn post_empty_response(&self, action: &str, body: String, resource: &str) -> Result<()> {
        self.credentials.ensure_fresh().await?;
        let base_url = self.get_base_url();
        let url = format!("{}/", base_url.trim_end_matches('/'));

        let builder = self
            .client
            .post(&url)
            .host(&format!("codebuild.{}.amazonaws.com", self.credentials.region()))
            .header("X-Amz-Target", format!("CodeBuild_20161006.{}", action))
            .header("Content-Type", "application/x-amz-json-1.1")
            .body(body.clone());

        let result =
            crate::aws::aws_request_utils::sign_send_no_response(builder, &self.sign_config())
                .await;

        Self::map_result(result, action, resource, Some(&body))
    }

    fn map_result<T>(
        result: Result<T>,
        operation: &str,
        resource: &str,
        request_body: Option<&str>,
    ) -> Result<T> {
        match result {
            Ok(v) => Ok(v),
            Err(e) => {
                if let Some(ErrorData::HttpResponseError {
                    http_status,
                    http_response_text: Some(ref text),
                    ..
                }) = &e.error
                {
                    let status = StatusCode::from_u16(*http_status)
                        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                    if let Some(mapped) =
                        Self::map_codebuild_error(status, text, operation, resource, request_body)
                    {
                        Err(e.context(mapped))
                    } else {
                        // Couldn't parse CodeBuild error, use original error
                        Err(e)
                    }
                } else {
                    Err(e)
                }
            }
        }
    }

    fn map_codebuild_error(
        status: StatusCode,
        body: &str,
        _operation: &str,
        resource: &str,
        request_body: Option<&str>,
    ) -> Option<ErrorData> {
        let parsed: std::result::Result<CodeBuildErrorResponse, _> = serde_json::from_str(body);
        let (code, message) = match parsed {
            Ok(e) => {
                let c = e.type_field.unwrap_or_else(|| "UnknownErrorCode".into());
                let m = e.message.unwrap_or_else(|| "Unknown error".into());
                (c, m)
            }
            Err(_) => {
                // If we can't parse the response, return None to use original error
                return None;
            }
        };

        Some(match code.as_str() {
            "AccessDeniedException" => ErrorData::RemoteAccessDenied {
                resource_type: "Project".into(),
                resource_name: resource.into(),
            },
            "AccountLimitExceededException" => ErrorData::QuotaExceeded { message },
            "ThrottlingException" | "TooManyRequestsException" => {
                ErrorData::RateLimitExceeded { message }
            }
            "ServiceUnavailable" | "InternalFailure" | "ServiceException" => {
                ErrorData::RemoteServiceUnavailable { message }
            }
            "RequestTimeoutException" => ErrorData::Timeout { message },
            "ResourceNotFoundException" => ErrorData::RemoteResourceNotFound {
                resource_type: "Project".into(),
                resource_name: resource.into(),
            },
            "ResourceAlreadyExistsException" => ErrorData::RemoteResourceConflict {
                message,
                resource_type: "Project".into(),
                resource_name: resource.into(),
            },
            "InvalidInputException" | "NotAuthorized" | "ValidationError" => {
                ErrorData::InvalidInput {
                    message,
                    field_name: None,
                }
            }
            _ => match status {
                StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
                    resource_type: "Project".into(),
                    resource_name: resource.into(),
                },
                StatusCode::CONFLICT => ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "Project".into(),
                    resource_name: resource.into(),
                },
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
                    resource_type: "Project".into(),
                    resource_name: resource.into(),
                },
                StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded { message },
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => ErrorData::RemoteServiceUnavailable { message },
                _ => ErrorData::HttpResponseError {
                    message: format!("CodeBuild operation failed: {}", message),
                    url: format!("codebuild.amazonaws.com"),
                    http_status: status.as_u16(),
                    http_response_text: Some(body.into()),
                    http_request_text: request_body.map(|s| s.to_string()),
                },
            },
        })
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl CodeBuildApi for CodeBuildClient {
    async fn create_project(&self, request: CreateProjectRequest) -> Result<CreateProjectResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize CreateProjectRequest for project '{}'",
                    request.name
                ),
            },
        )?;
        self.post_json("CreateProject", body, &request.name).await
    }

    async fn delete_project(&self, request: DeleteProjectRequest) -> Result<DeleteProjectResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize DeleteProjectRequest for project '{}'",
                    request.name
                ),
            },
        )?;
        self.post_empty_response("DeleteProject", body, &request.name)
            .await?;
        Ok(DeleteProjectResponse {})
    }

    async fn update_project(&self, request: UpdateProjectRequest) -> Result<UpdateProjectResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize UpdateProjectRequest for project '{}'",
                    request.name
                ),
            },
        )?;
        self.post_json("UpdateProject", body, &request.name).await
    }

    async fn batch_get_projects(
        &self,
        request: BatchGetProjectsRequest,
    ) -> Result<BatchGetProjectsResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize BatchGetProjectsRequest"),
            },
        )?;
        self.post_json("BatchGetProjects", body, "").await
    }

    async fn start_build(&self, request: StartBuildRequest) -> Result<StartBuildResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize StartBuildRequest for project '{}'",
                    request.project_name
                ),
            },
        )?;
        self.post_json("StartBuild", body, &request.project_name)
            .await
    }

    async fn stop_build(&self, request: StopBuildRequest) -> Result<StopBuildResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize StopBuildRequest for build '{}'",
                    request.id
                ),
            },
        )?;
        self.post_json("StopBuild", body, &request.id).await
    }

    async fn batch_get_builds(
        &self,
        request: BatchGetBuildsRequest,
    ) -> Result<BatchGetBuildsResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize BatchGetBuildsRequest"),
            },
        )?;
        self.post_json("BatchGetBuilds", body, "").await
    }

    async fn batch_delete_builds(
        &self,
        request: BatchDeleteBuildsRequest,
    ) -> Result<BatchDeleteBuildsResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize BatchDeleteBuildsRequest"),
            },
        )?;
        self.post_json("BatchDeleteBuilds", body, "").await
    }

    async fn retry_build(&self, request: RetryBuildRequest) -> Result<RetryBuildResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize RetryBuildRequest"),
            },
        )?;
        self.post_json("RetryBuild", body, &request.id.unwrap_or_default())
            .await
    }
}

// ---------------------------------------------------------------------------
// Request / response payloads
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CodeBuildErrorResponse {
    #[serde(rename = "__type")]
    type_field: Option<String>,
    #[serde(rename = "message")]
    message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectRequest {
    pub name: String,
    pub source: ProjectSource,
    pub artifacts: ProjectArtifacts,
    pub environment: ProjectEnvironment,
    pub service_role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logs_config: Option<LogsConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<Tag>>,
}

#[derive(Debug, Clone, Serialize, Builder, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSource {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buildspec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_clone_depth: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insecure_ssl: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Builder, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectArtifacts {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packaging: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_identifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bucket_owner_access: Option<String>,
}

#[derive(Debug, Clone, Serialize, Builder, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEnvironment {
    pub r#type: String,
    pub image: String,
    pub compute_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_variables: Option<Vec<EnvironmentVariable>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub privileged_mode: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_pull_credentials_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Builder, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentVariable {
    pub name: String,
    pub value: String,
    pub r#type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Builder, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectResponse {
    pub project: Project,
}

#[derive(Debug, Deserialize, Clone, bon::Builder)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub name: Option<String>,
    pub arn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<ProjectSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<ProjectArtifacts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<ProjectEnvironment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<Tag>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vpc_config: Option<VpcConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook: Option<Webhook>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logs_config: Option<LogsConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<ProjectCache>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_in_minutes: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queued_timeout_in_minutes: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary_sources: Option<Vec<ProjectSource>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary_artifacts: Option<Vec<ProjectArtifacts>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct DeleteProjectRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct DeleteProjectResponse {}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProjectRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<ProjectSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<ProjectArtifacts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<ProjectEnvironment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<Tag>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vpc_config: Option<VpcConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook: Option<Webhook>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logs_config: Option<LogsConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<ProjectCache>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_in_minutes: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queued_timeout_in_minutes: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_key: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProjectResponse {
    pub project: Project,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct StartBuildRequest {
    pub project_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buildspec_override: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_variables_override: Option<Vec<EnvironmentVariable>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compute_type_override: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_type_override: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_override: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_in_minutes_override: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartBuildResponse {
    pub build: Build,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Build {
    pub id: Option<String>,
    pub arn: Option<String>,
    pub build_number: Option<i64>,
    pub build_status: Option<String>,
    pub start_time: Option<f64>,
    pub end_time: Option<f64>,
    pub current_phase: Option<String>,
    pub build_complete: bool,
    pub initiator: Option<String>,
    pub source_version: Option<String>,
    pub project_name: Option<String>,
    pub phases: Option<Vec<BuildPhase>>,
    pub source: Option<ProjectSource>,
    pub secondary_sources: Option<Vec<ProjectSource>>,
    pub secondary_artifacts: Option<Vec<BuildArtifacts>>,
    pub artifacts: Option<BuildArtifacts>,
    pub logs: Option<LogsLocation>,
    pub vpc_config: Option<VpcConfig>,
    pub environment: Option<ProjectEnvironment>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct StopBuildRequest {
    pub id: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StopBuildResponse {
    pub build: Build,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BatchGetBuildsRequest {
    pub ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchGetBuildsResponse {
    pub builds: Option<Vec<Build>>,
    pub builds_not_found: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BatchDeleteBuildsRequest {
    pub ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchDeleteBuildsResponse {
    pub builds_deleted: Option<Vec<String>>,
    pub builds_not_deleted: Option<Vec<BuildNotDeleted>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildNotDeleted {
    pub id: Option<String>,
    pub status_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RetryBuildRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryBuildResponse {
    pub build: Build,
}

#[derive(Debug, Clone, Serialize, Builder, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpcConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vpc_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnets: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_group_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Builder, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Webhook {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_groups: Option<Vec<Vec<WebhookFilter>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Builder, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookFilter {
    pub r#type: String,
    pub pattern: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_matched_pattern: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Builder, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_watch_logs: Option<CloudWatchLogsConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s3_logs: Option<S3LogsConfig>,
}

#[derive(Debug, Clone, Serialize, Builder, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudWatchLogsConfig {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Builder, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct S3LogsConfig {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bucket_owner_access: Option<String>,
}

#[derive(Debug, Clone, Serialize, Builder, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCache {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modes: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BuildPhase {
    pub phase_type: Option<String>,
    pub phase_status: Option<String>,
    pub start_time: Option<f64>,
    pub end_time: Option<f64>,
    pub duration_in_seconds: Option<i64>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BuildArtifacts {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256sum: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub md5sum: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub override_artifact_name: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_identifier: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LogsLocation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deep_link: Option<String>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BatchGetProjectsRequest {
    pub names: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchGetProjectsResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projects: Option<Vec<Project>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projects_not_found: Option<Vec<String>>,
}
