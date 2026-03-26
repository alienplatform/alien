//! AWS Systems Manager (SSM) Client
//!
//! This module provides a client for interacting with AWS SSM APIs, including
//! Parameter Store for storing/retrieving secrets and Run Command for executing
//! commands on EC2 instances.
//!
//! # Example
//!
//! ```rust,ignore
//! use alien_aws_clients::ssm::{SsmClient, SsmApi, PutParameterRequest};
//! use reqwest::Client;
//!
//! let ssm_client = SsmClient::new(Client::new(), aws_config);
//! ssm_client.put_parameter(
//!     PutParameterRequest::builder()
//!         .name("/my/parameter".to_string())
//!         .value("secret-value".to_string())
//!         .parameter_type("SecureString".to_string())
//!         .build()
//! ).await?;
//! ```

use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, ContextError, IntoAlienError};
use async_trait::async_trait;
use bon::Builder;
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(feature = "test-utils")]
use mockall::automock;

// ---------------------------------------------------------------------------
// SSM API Trait
// ---------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait SsmApi: Send + Sync + std::fmt::Debug {
    // Parameter Store Operations
    async fn put_parameter(&self, request: PutParameterRequest) -> Result<PutParameterResponse>;
    async fn get_parameter(&self, request: GetParameterRequest) -> Result<GetParameterResponse>;
    async fn delete_parameter(&self, name: &str) -> Result<()>;
    async fn get_parameters(&self, request: GetParametersRequest) -> Result<GetParametersResponse>;
    async fn describe_parameters(
        &self,
        request: DescribeParametersRequest,
    ) -> Result<DescribeParametersResponse>;

    // Run Command Operations
    async fn send_command(&self, request: SendCommandRequest) -> Result<SendCommandResponse>;
    async fn get_command_invocation(
        &self,
        request: GetCommandInvocationRequest,
    ) -> Result<GetCommandInvocationResponse>;
    async fn list_command_invocations(
        &self,
        request: ListCommandInvocationsRequest,
    ) -> Result<ListCommandInvocationsResponse>;
}

// ---------------------------------------------------------------------------
// SSM Client
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SsmClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl SsmClient {
    /// Create a new SSM client.
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self { client, credentials }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "ssm".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self.credentials.get_service_endpoint_option("ssm") {
            override_url.to_string()
        } else {
            format!("https://ssm.{}.amazonaws.com", self.credentials.region())
        }
    }

    fn get_host(&self) -> String {
        format!("ssm.{}.amazonaws.com", self.credentials.region())
    }

    // ------------------------- Internal Helpers -------------------------

    async fn send_json<T: DeserializeOwned + Send + 'static>(
        &self,
        target: &str,
        body: String,
        operation: &str,
        resource: &str,
    ) -> Result<T> {
        self.credentials.ensure_fresh().await?;
        let url = self.get_base_url();

        let builder = self
            .client
            .request(Method::POST, &url)
            .host(&self.get_host())
            .header("X-Amz-Target", format!("AmazonSSM.{}", target))
            .header("Content-Type", "application/x-amz-json-1.1")
            .content_sha256(&body)
            .body(body.clone());

        let result =
            crate::aws::aws_request_utils::sign_send_json(builder, &self.sign_config()).await;

        Self::map_result(result, operation, resource, Some(&body))
    }

    async fn send_no_response(
        &self,
        target: &str,
        body: String,
        operation: &str,
        resource: &str,
    ) -> Result<()> {
        self.credentials.ensure_fresh().await?;
        let url = self.get_base_url();

        let builder = self
            .client
            .request(Method::POST, &url)
            .host(&self.get_host())
            .header("X-Amz-Target", format!("AmazonSSM.{}", target))
            .header("Content-Type", "application/x-amz-json-1.1")
            .content_sha256(&body)
            .body(body.clone());

        let result =
            crate::aws::aws_request_utils::sign_send_no_response(builder, &self.sign_config())
                .await;

        Self::map_result(result, operation, resource, Some(&body))
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
                        Self::map_ssm_error(status, text, operation, resource, request_body)
                    {
                        Err(e.context(mapped))
                    } else {
                        Err(e)
                    }
                } else {
                    Err(e)
                }
            }
        }
    }

    fn map_ssm_error(
        status: StatusCode,
        body: &str,
        _operation: &str,
        resource: &str,
        request_body: Option<&str>,
    ) -> Option<ErrorData> {
        // Try to parse SSM error JSON
        let parsed: std::result::Result<SsmErrorResponse, _> = serde_json::from_str(body);
        let (code, message) = match parsed {
            Ok(e) => {
                let c = e
                    .type_field
                    .or(e.code)
                    .unwrap_or_else(|| "UnknownErrorCode".into());
                let m = e.message.unwrap_or_else(|| "Unknown error".into());
                (c, m)
            }
            Err(_) => {
                return None;
            }
        };

        // Extract just the error code without the namespace prefix
        let error_code = code.split('#').last().unwrap_or(&code);

        Some(match error_code {
            // Access / Auth errors
            "AccessDeniedException" | "UnauthorizedOperation" => ErrorData::RemoteAccessDenied {
                resource_type: "SSM Resource".into(),
                resource_name: resource.into(),
            },
            // Throttling
            "ThrottlingException" | "TooManyUpdates" => ErrorData::RateLimitExceeded { message },
            // Service unavailable
            "InternalServerError" | "ServiceUnavailable" => {
                ErrorData::RemoteServiceUnavailable { message }
            }
            // Parameter not found
            "ParameterNotFound" | "ParameterVersionNotFound" => ErrorData::RemoteResourceNotFound {
                resource_type: "SSM Parameter".into(),
                resource_name: resource.into(),
            },
            // Command not found
            "InvocationDoesNotExist" => ErrorData::RemoteResourceNotFound {
                resource_type: "CommandInvocation".into(),
                resource_name: resource.into(),
            },
            // Parameter already exists
            "ParameterAlreadyExists" => ErrorData::RemoteResourceConflict {
                message,
                resource_type: "SSM Parameter".into(),
                resource_name: resource.into(),
            },
            // Invalid parameter errors
            "InvalidParameters" | "ParameterLimitExceeded" | "ParameterMaxVersionLimitExceeded" => {
                ErrorData::InvalidInput {
                    message,
                    field_name: None,
                }
            }
            // Invalid document errors
            "InvalidDocument" | "InvalidDocumentContent" | "InvalidDocumentVersion" => {
                ErrorData::InvalidInput {
                    message,
                    field_name: Some("document_name".into()),
                }
            }
            // Instance errors
            "InvalidInstanceId" | "InvalidTarget" => ErrorData::InvalidInput {
                message,
                field_name: Some("instance_id".into()),
            },
            // Duplicate instance error
            "DuplicateInstanceId" => ErrorData::RemoteResourceConflict {
                message,
                resource_type: "Instance".into(),
                resource_name: resource.into(),
            },
            // Quota errors
            "HierarchyLevelLimitExceededException" | "PoliciesLimitExceededException" => {
                ErrorData::QuotaExceeded { message }
            }
            // Default fallback based on status code
            _ => match status {
                StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
                    resource_type: "SSM Resource".into(),
                    resource_name: resource.into(),
                },
                StatusCode::CONFLICT => ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "SSM Resource".into(),
                    resource_name: resource.into(),
                },
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
                    resource_type: "SSM Resource".into(),
                    resource_name: resource.into(),
                },
                StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded { message },
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => ErrorData::RemoteServiceUnavailable { message },
                _ => ErrorData::HttpResponseError {
                    message: format!("SSM operation failed: {}", message),
                    url: "ssm.amazonaws.com".into(),
                    http_status: status.as_u16(),
                    http_response_text: Some(body.into()),
                    http_request_text: request_body.map(|s| s.to_string()),
                },
            },
        })
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl SsmApi for SsmClient {
    // ---------------------------------------------------------------------------
    // Parameter Store Operations
    // ---------------------------------------------------------------------------

    async fn put_parameter(&self, request: PutParameterRequest) -> Result<PutParameterResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize PutParameterRequest for '{}'",
                    request.name
                ),
            },
        )?;
        self.send_json("PutParameter", body, "PutParameter", &request.name)
            .await
    }

    async fn get_parameter(&self, request: GetParameterRequest) -> Result<GetParameterResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize GetParameterRequest for '{}'",
                    request.name
                ),
            },
        )?;
        self.send_json("GetParameter", body, "GetParameter", &request.name)
            .await
    }

    async fn delete_parameter(&self, name: &str) -> Result<()> {
        let request = DeleteParameterRequest {
            name: name.to_string(),
        };
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize DeleteParameterRequest for '{}'", name),
            },
        )?;
        // DeleteParameter returns an empty response on success
        let _: serde_json::Value = self
            .send_json("DeleteParameter", body, "DeleteParameter", name)
            .await?;
        Ok(())
    }

    async fn get_parameters(&self, request: GetParametersRequest) -> Result<GetParametersResponse> {
        let names_str = request.names.join(", ");
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize GetParametersRequest for [{}]",
                    names_str
                ),
            },
        )?;
        self.send_json("GetParameters", body, "GetParameters", &names_str)
            .await
    }

    async fn describe_parameters(
        &self,
        request: DescribeParametersRequest,
    ) -> Result<DescribeParametersResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: "Failed to serialize DescribeParametersRequest".to_string(),
            },
        )?;
        self.send_json(
            "DescribeParameters",
            body,
            "DescribeParameters",
            "parameters",
        )
        .await
    }

    // ---------------------------------------------------------------------------
    // Run Command Operations
    // ---------------------------------------------------------------------------

    async fn send_command(&self, request: SendCommandRequest) -> Result<SendCommandResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize SendCommandRequest for document '{}'",
                    request.document_name
                ),
            },
        )?;
        self.send_json("SendCommand", body, "SendCommand", &request.document_name)
            .await
    }

    async fn get_command_invocation(
        &self,
        request: GetCommandInvocationRequest,
    ) -> Result<GetCommandInvocationResponse> {
        let resource = format!("{}:{}", request.command_id, request.instance_id);
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize GetCommandInvocationRequest for '{}'",
                    resource
                ),
            },
        )?;
        self.send_json(
            "GetCommandInvocation",
            body,
            "GetCommandInvocation",
            &resource,
        )
        .await
    }

    async fn list_command_invocations(
        &self,
        request: ListCommandInvocationsRequest,
    ) -> Result<ListCommandInvocationsResponse> {
        let resource = request.command_id.as_deref().unwrap_or("all");
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: "Failed to serialize ListCommandInvocationsRequest".to_string(),
            },
        )?;
        self.send_json(
            "ListCommandInvocations",
            body,
            "ListCommandInvocations",
            resource,
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// Error Response Structs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SsmErrorResponse {
    #[serde(rename = "__type")]
    type_field: Option<String>,
    #[serde(rename = "Code")]
    code: Option<String>,
    #[serde(rename = "Message", alias = "message")]
    message: Option<String>,
}

// ---------------------------------------------------------------------------
// Parameter Store Request/Response Types
// ---------------------------------------------------------------------------

/// Request to put a parameter.
#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct PutParameterRequest {
    /// The name of the parameter.
    pub name: String,
    /// The value of the parameter.
    pub value: String,
    /// The type of parameter: String, StringList, or SecureString.
    #[serde(rename = "Type")]
    pub parameter_type: String,
    /// A description of the parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Overwrite an existing parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overwrite: Option<bool>,
    /// The KMS key ID to encrypt SecureString parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
    /// Tags to assign to the parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<SsmTag>>,
    /// The parameter tier: Standard, Advanced, or Intelligent-Tiering.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    /// The data type for a String parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_type: Option<String>,
}

/// Response from putting a parameter.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PutParameterResponse {
    /// The version number of the parameter.
    pub version: Option<i64>,
    /// The tier of the parameter.
    pub tier: Option<String>,
}

/// Request to get a parameter.
#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct GetParameterRequest {
    /// The name of the parameter.
    pub name: String,
    /// Whether to decrypt SecureString values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_decryption: Option<bool>,
}

/// Response from getting a parameter.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetParameterResponse {
    /// The parameter information.
    pub parameter: Option<Parameter>,
}

/// Request to delete a parameter.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct DeleteParameterRequest {
    /// The name of the parameter.
    pub name: String,
}

/// Request to get multiple parameters.
#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct GetParametersRequest {
    /// The names of the parameters.
    pub names: Vec<String>,
    /// Whether to decrypt SecureString values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_decryption: Option<bool>,
}

/// Response from getting multiple parameters.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetParametersResponse {
    /// The parameters.
    pub parameters: Option<Vec<Parameter>>,
    /// The names of parameters that weren't found.
    pub invalid_parameters: Option<Vec<String>>,
}

/// Request to describe parameters.
#[derive(Debug, Clone, Serialize, Builder, Default)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeParametersRequest {
    /// Filters for the parameter search.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameter_filters: Option<Vec<ParameterStringFilter>>,
    /// The maximum number of items to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    /// A token to start the list from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing parameters.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeParametersResponse {
    /// The parameter metadata.
    pub parameters: Option<Vec<ParameterMetadata>>,
    /// The token to use when requesting the next set of items.
    pub next_token: Option<String>,
}

/// A parameter value.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Parameter {
    /// The name of the parameter.
    pub name: Option<String>,
    /// The value of the parameter.
    pub value: Option<String>,
    /// The type of parameter.
    #[serde(rename = "Type")]
    pub parameter_type: Option<String>,
    /// The version number of the parameter.
    pub version: Option<i64>,
    /// The ARN of the parameter.
    #[serde(rename = "ARN")]
    pub arn: Option<String>,
    /// The last modified date.
    pub last_modified_date: Option<f64>,
    /// The data type for a String parameter.
    pub data_type: Option<String>,
}

/// Metadata about a parameter.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ParameterMetadata {
    /// The name of the parameter.
    pub name: Option<String>,
    /// The type of parameter.
    #[serde(rename = "Type")]
    pub parameter_type: Option<String>,
    /// The key ID.
    pub key_id: Option<String>,
    /// The last modified date.
    pub last_modified_date: Option<f64>,
    /// The user that last modified the parameter.
    pub last_modified_user: Option<String>,
    /// The description.
    pub description: Option<String>,
    /// The version.
    pub version: Option<i64>,
    /// The tier.
    pub tier: Option<String>,
    /// The data type.
    pub data_type: Option<String>,
}

/// A filter for describing parameters.
#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct ParameterStringFilter {
    /// The filter key.
    pub key: String,
    /// The filter option.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option: Option<String>,
    /// The filter values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<String>>,
}

/// A tag for an SSM resource.
#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct SsmTag {
    /// The tag key.
    pub key: String,
    /// The tag value.
    pub value: String,
}

// ---------------------------------------------------------------------------
// Run Command Request/Response Types
// ---------------------------------------------------------------------------

/// Request to send a command to instances.
#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct SendCommandRequest {
    /// The name of the document to run.
    pub document_name: String,
    /// The instance IDs to target.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_ids: Option<Vec<String>>,
    /// Targets to send the command to (alternative to instance_ids).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub targets: Option<Vec<Target>>,
    /// Parameters for the document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<HashMap<String, Vec<String>>>,
    /// A comment for the command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// The timeout in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<i32>,
    /// The S3 bucket to store output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_s3_bucket_name: Option<String>,
    /// The S3 key prefix for output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_s3_key_prefix: Option<String>,
    /// The CloudWatch log group name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_watch_output_config: Option<CloudWatchOutputConfig>,
}

/// Response from sending a command.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SendCommandResponse {
    /// The command information.
    pub command: Option<Command>,
}

/// Request to get a command invocation result.
#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct GetCommandInvocationRequest {
    /// The command ID.
    pub command_id: String,
    /// The instance ID.
    pub instance_id: String,
    /// The plugin name (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_name: Option<String>,
}

/// Response from getting a command invocation.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetCommandInvocationResponse {
    /// The command ID.
    pub command_id: Option<String>,
    /// The instance ID.
    pub instance_id: Option<String>,
    /// The comment.
    pub comment: Option<String>,
    /// The document name.
    pub document_name: Option<String>,
    /// The document version.
    pub document_version: Option<String>,
    /// The plugin name.
    pub plugin_name: Option<String>,
    /// The response code.
    pub response_code: Option<i32>,
    /// The execution start time.
    pub execution_start_date_time: Option<String>,
    /// The execution elapsed time.
    pub execution_elapsed_time: Option<String>,
    /// The execution end time.
    pub execution_end_date_time: Option<String>,
    /// The status.
    pub status: Option<String>,
    /// The status details.
    pub status_details: Option<String>,
    /// The standard output content.
    pub standard_output_content: Option<String>,
    /// The standard output URL.
    pub standard_output_url: Option<String>,
    /// The standard error content.
    pub standard_error_content: Option<String>,
    /// The standard error URL.
    pub standard_error_url: Option<String>,
    /// The CloudWatch output config.
    pub cloud_watch_output_config: Option<CloudWatchOutputConfig>,
}

/// Request to list command invocations.
#[derive(Debug, Clone, Serialize, Builder, Default)]
#[serde(rename_all = "PascalCase")]
pub struct ListCommandInvocationsRequest {
    /// The command ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_id: Option<String>,
    /// The instance ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    /// The maximum number of items to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    /// The token for the next page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
    /// Filters for the results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<CommandFilter>>,
    /// Whether to include command details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<bool>,
}

/// Response from listing command invocations.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ListCommandInvocationsResponse {
    /// The command invocations.
    pub command_invocations: Option<Vec<CommandInvocation>>,
    /// The token for the next page.
    pub next_token: Option<String>,
}

/// A command.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Command {
    /// The command ID.
    pub command_id: Option<String>,
    /// The document name.
    pub document_name: Option<String>,
    /// The document version.
    pub document_version: Option<String>,
    /// The comment.
    pub comment: Option<String>,
    /// The expires after time.
    pub expires_after: Option<f64>,
    /// The parameters.
    pub parameters: Option<HashMap<String, Vec<String>>>,
    /// The instance IDs.
    pub instance_ids: Option<Vec<String>>,
    /// The targets.
    pub targets: Option<Vec<Target>>,
    /// The requested date and time.
    pub requested_date_time: Option<f64>,
    /// The status.
    pub status: Option<String>,
    /// The status details.
    pub status_details: Option<String>,
    /// The output S3 bucket name.
    pub output_s3_bucket_name: Option<String>,
    /// The output S3 key prefix.
    pub output_s3_key_prefix: Option<String>,
    /// The max concurrency.
    pub max_concurrency: Option<String>,
    /// The max errors.
    pub max_errors: Option<String>,
    /// The target count.
    pub target_count: Option<i32>,
    /// The completed count.
    pub completed_count: Option<i32>,
    /// The error count.
    pub error_count: Option<i32>,
    /// The delivery timed out count.
    pub delivery_timed_out_count: Option<i32>,
}

/// A command invocation.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CommandInvocation {
    /// The command ID.
    pub command_id: Option<String>,
    /// The instance ID.
    pub instance_id: Option<String>,
    /// The instance name.
    pub instance_name: Option<String>,
    /// The comment.
    pub comment: Option<String>,
    /// The document name.
    pub document_name: Option<String>,
    /// The document version.
    pub document_version: Option<String>,
    /// The requested date and time.
    pub requested_date_time: Option<f64>,
    /// The status.
    pub status: Option<String>,
    /// The status details.
    pub status_details: Option<String>,
    /// The trace output.
    pub trace_output: Option<String>,
    /// The standard output URL.
    pub standard_output_url: Option<String>,
    /// The standard error URL.
    pub standard_error_url: Option<String>,
    /// The command plugins.
    pub command_plugins: Option<Vec<CommandPlugin>>,
}

/// A command plugin.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CommandPlugin {
    /// The plugin name.
    pub name: Option<String>,
    /// The status.
    pub status: Option<String>,
    /// The status details.
    pub status_details: Option<String>,
    /// The response code.
    pub response_code: Option<i32>,
    /// The response start date time.
    pub response_start_date_time: Option<f64>,
    /// The response finish date time.
    pub response_finish_date_time: Option<f64>,
    /// The output.
    pub output: Option<String>,
    /// The standard output URL.
    pub standard_output_url: Option<String>,
    /// The standard error URL.
    pub standard_error_url: Option<String>,
    /// The output S3 bucket name.
    pub output_s3_bucket_name: Option<String>,
    /// The output S3 key prefix.
    pub output_s3_key_prefix: Option<String>,
}

/// A target for a command.
#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct Target {
    /// The target key.
    pub key: Option<String>,
    /// The target values.
    pub values: Option<Vec<String>>,
}

/// A filter for commands.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CommandFilter {
    /// The filter key.
    pub key: String,
    /// The filter value.
    pub value: String,
}

/// CloudWatch output configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct CloudWatchOutputConfig {
    /// The CloudWatch log group name.
    pub cloud_watch_log_group_name: Option<String>,
    /// Whether CloudWatch output is enabled.
    pub cloud_watch_output_enabled: Option<bool>,
}
