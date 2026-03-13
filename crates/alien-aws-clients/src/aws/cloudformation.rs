use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::AwsClientConfig;
use crate::aws::AwsClientConfigExt;
use alien_client_core::{ErrorData, Result};

use alien_error::{AlienError, ContextError};
use aws_credential_types::Credentials;
use bon::Builder;
use form_urlencoded;
use quick_xml;
use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait CloudFormationApi: Send + Sync + std::fmt::Debug {
    async fn create_stack(&self, request: CreateStackRequest) -> Result<CreateStackResponse>;
    async fn describe_stacks(
        &self,
        request: DescribeStacksRequest,
    ) -> Result<DescribeStacksResponse>;
    async fn delete_stack(&self, request: DeleteStackRequest) -> Result<DeleteStackResponse>;
    async fn describe_stack_resources(
        &self,
        request: DescribeStackResourcesRequest,
    ) -> Result<DescribeStackResourcesResponse>;
    async fn describe_stack_resource(
        &self,
        request: DescribeStackResourceRequest,
    ) -> Result<DescribeStackResourceResponse>;
    async fn describe_stack_events(
        &self,
        request: DescribeStackEventsRequest,
    ) -> Result<DescribeStackEventsResponse>;
}

/// AWS CloudFormation client implemented with the new Alien request & error utilities.
#[derive(Debug, Clone)]
pub struct CloudFormationClient {
    client: Client,
    config: AwsClientConfig,
}

impl CloudFormationClient {
    pub fn new(client: Client, config: AwsClientConfig) -> Self {
        Self { client, config }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "cloudformation".into(),
            region: self.config.region.clone(),
            credentials: self.config.get_credentials(),
            signing_region: None,
        }
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self.config.get_service_endpoint_option("cloudformation") {
            override_url.to_string()
        } else {
            format!(
                "https://cloudformation.{}.amazonaws.com",
                self.config.region
            )
        }
    }

    fn build_form_body(action: &str, version: &str, params: Vec<(String, String)>) -> String {
        let mut all = vec![
            ("Action".to_string(), action.to_string()),
            ("Version".to_string(), version.to_string()),
        ];
        all.extend(params);
        all.into_iter()
            .map(|(k, v)| {
                format!(
                    "{}={}",
                    k,
                    form_urlencoded::byte_serialize(v.as_bytes()).collect::<String>()
                )
            })
            .collect::<Vec<String>>()
            .join("&")
    }

    async fn post_xml<T: DeserializeOwned + Send + 'static>(
        &self,
        body: String,
        operation: &str,
        resource_name: &str,
    ) -> Result<T> {
        let base_url = self.get_base_url();
        let url = format!("{}/", base_url.trim_end_matches('/'));
        let body_for_error = body.clone();
        let builder = self
            .client
            .post(&url)
            .host(&format!(
                "cloudformation.{}.amazonaws.com",
                self.config.region
            ))
            .content_type_form()
            .body(body);

        let result =
            crate::aws::aws_request_utils::sign_send_xml(builder, &self.sign_config()).await;

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
                    if let Some(mapped) = Self::map_cfn_error(
                        status,
                        text,
                        operation,
                        resource_name,
                        Some(&body_for_error),
                    ) {
                        Err(e.context(mapped))
                    } else {
                        // Couldn't parse CloudFormation error, use original error
                        Err(e)
                    }
                } else {
                    Err(e)
                }
            }
        }
    }

    fn map_cfn_error(
        status: StatusCode,
        error_body: &str,
        operation: &str,
        resource_name: &str,
        request_body: Option<&str>,
    ) -> Option<ErrorData> {
        // Try to parse CloudFormation error xml: <ErrorResponse><Error><Code>...</Code><Message>...</Message></Error></ErrorResponse>
        let parsed: std::result::Result<CloudFormationErrorResponse, _> =
            quick_xml::de::from_str(error_body);
        let (code, message) = match parsed {
            Ok(e) => (
                e.error.code.unwrap_or_else(|| "UnknownErrorCode".into()),
                e.error.message.unwrap_or_else(|| "Unknown error".into()),
            ),
            Err(_) => {
                // If we can't parse the response, return None to use original error
                return None;
            }
        };

        Some(match code.as_str() {
            "AccessDenied"
            | "AccessDeniedException"
            | "UnauthorizedOperation"
            | "AuthFailure"
            | "SignatureDoesNotMatch" => ErrorData::RemoteAccessDenied {
                resource_type: "CloudFormation Stack".into(),
                resource_name: resource_name.into(),
            },
            "Throttling" | "ThrottlingException" | "RequestLimitExceeded" => {
                ErrorData::RateLimitExceeded { message }
            }
            "ServiceUnavailable" | "InternalFailure" => {
                ErrorData::RemoteServiceUnavailable { message }
            }
            "AlreadyExists" | "AlreadyExistsException" => ErrorData::RemoteResourceConflict {
                message,
                resource_type: "CloudFormation Stack".into(),
                resource_name: resource_name.into(),
            },
            "LimitExceeded" | "LimitExceededException" => ErrorData::QuotaExceeded { message },
            "ValidationError" if message.contains("does not exist") => {
                ErrorData::RemoteResourceNotFound {
                    resource_type: "CloudFormation Stack".into(),
                    resource_name: resource_name.into(),
                }
            }
            _ => match status {
                StatusCode::CONFLICT => ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "CloudFormation Stack".into(),
                    resource_name: resource_name.into(),
                },
                StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
                    resource_type: "CloudFormation Stack".into(),
                    resource_name: resource_name.into(),
                },
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
                    resource_type: "CloudFormation Stack".into(),
                    resource_name: resource_name.into(),
                },
                StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded { message },
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => ErrorData::RemoteServiceUnavailable { message },
                _ => ErrorData::HttpResponseError {
                    message: format!("CloudFormation operation failed: {}", message),
                    url: format!("cloudformation.amazonaws.com"),
                    http_status: status.as_u16(),
                    http_request_text: request_body.map(|s| s.to_string()),
                    http_response_text: Some(error_body.into()),
                },
            },
        })
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl CloudFormationApi for CloudFormationClient {
    async fn create_stack(&self, request: CreateStackRequest) -> Result<CreateStackResponse> {
        let mut params = vec![
            ("StackName".to_string(), request.stack_name.clone()),
            ("TemplateBody".to_string(), request.template_body),
        ];

        if let Some(description) = request.description {
            params.push(("Description".to_string(), description));
        }

        if let Some(timeout) = request.timeout_in_minutes {
            params.push(("TimeoutInMinutes".to_string(), timeout.to_string()));
        }

        if let Some(ref capabilities) = request.capabilities {
            for (i, capability) in capabilities.iter().enumerate() {
                params.push((format!("Capabilities.member.{}", i + 1), capability.clone()));
            }
        }

        let body = Self::build_form_body("CreateStack", "2010-05-15", params);
        let stack_name = &request.stack_name;
        self.post_xml(body, "CreateStack", stack_name).await
    }

    async fn describe_stacks(
        &self,
        request: DescribeStacksRequest,
    ) -> Result<DescribeStacksResponse> {
        let mut params = Vec::new();
        if let Some(ref stack_name) = request.stack_name {
            params.push(("StackName".to_string(), stack_name.clone()));
        }

        let body = Self::build_form_body("DescribeStacks", "2010-05-15", params);
        let resource_name = request.stack_name.as_deref().unwrap_or("(unknown)");
        self.post_xml(body, "DescribeStacks", resource_name).await
    }

    async fn delete_stack(&self, request: DeleteStackRequest) -> Result<DeleteStackResponse> {
        let params = vec![("StackName".to_string(), request.stack_name.clone())];

        let body = Self::build_form_body("DeleteStack", "2010-05-15", params);
        self.post_xml(body, "DeleteStack", &request.stack_name)
            .await
    }

    async fn describe_stack_resources(
        &self,
        request: DescribeStackResourcesRequest,
    ) -> Result<DescribeStackResourcesResponse> {
        let mut params = Vec::new();
        if let Some(ref stack_name) = request.stack_name {
            params.push(("StackName".to_string(), stack_name.clone()));
        }
        if let Some(ref logical_id) = request.logical_resource_id {
            params.push(("LogicalResourceId".to_string(), logical_id.clone()));
        }
        if let Some(ref physical_id) = request.physical_resource_id {
            params.push(("PhysicalResourceId".to_string(), physical_id.clone()));
        }

        let body = Self::build_form_body("DescribeStackResources", "2010-05-15", params);
        self.post_xml(
            body,
            "DescribeStackResources",
            request.stack_name.as_deref().unwrap_or("(unknown)"),
        )
        .await
    }

    async fn describe_stack_resource(
        &self,
        request: DescribeStackResourceRequest,
    ) -> Result<DescribeStackResourceResponse> {
        let params = vec![
            ("StackName".to_string(), request.stack_name.clone()),
            (
                "LogicalResourceId".to_string(),
                request.logical_resource_id.clone(),
            ),
        ];

        let body = Self::build_form_body("DescribeStackResource", "2010-05-15", params);
        self.post_xml(body, "DescribeStackResource", &request.stack_name)
            .await
    }

    async fn describe_stack_events(
        &self,
        request: DescribeStackEventsRequest,
    ) -> Result<DescribeStackEventsResponse> {
        let params = vec![("StackName".to_string(), request.stack_name.clone())];

        let body = Self::build_form_body("DescribeStackEvents", "2010-05-15", params);
        self.post_xml(body, "DescribeStackEvents", &request.stack_name)
            .await
    }
}

// -------------------------------------------------------------------------
// Error XML structs
// -------------------------------------------------------------------------

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct CloudFormationErrorResponse {
    pub error: CloudFormationErrorDetails,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct CloudFormationErrorDetails {
    pub code: Option<String>,
    pub message: Option<String>,
}

// -------------------------------------------------------------------------
// Request / response payloads
// -------------------------------------------------------------------------

#[derive(Serialize, Debug, Clone, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct CreateStackRequest {
    pub stack_name: String,
    pub template_body: String,
    pub description: Option<String>,
    pub timeout_in_minutes: Option<i32>,
    pub capabilities: Option<Vec<String>>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct CreateStackResponse {
    pub create_stack_result: CreateStackResult,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct CreateStackResult {
    pub stack_id: String,
}

#[derive(Serialize, Debug, Clone, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeStacksRequest {
    pub stack_name: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeStacksResponse {
    pub describe_stacks_result: DescribeStacksResult,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeStacksResult {
    pub stacks: Stacks,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct Stacks {
    #[serde(rename = "member", default)]
    pub member: Vec<Stack>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct Stack {
    pub stack_id: String,
    pub stack_name: String,
    pub stack_status: String,
    pub creation_time: String,
    pub description: Option<String>,
    pub capabilities: Option<Capabilities>,
    pub outputs: Option<Outputs>,
    pub parameters: Option<Parameters>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct Capabilities {
    #[serde(rename = "member", default)]
    pub member: Vec<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct Outputs {
    #[serde(rename = "member", default)]
    pub member: Vec<Output>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct Output {
    pub output_key: String,
    pub output_value: String,
    pub description: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct Parameters {
    #[serde(rename = "member", default)]
    pub member: Vec<Parameter>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct Parameter {
    pub parameter_key: String,
    pub parameter_value: Option<String>,
}

#[derive(Serialize, Debug, Clone, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct DeleteStackRequest {
    pub stack_name: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct DeleteStackResponse {
    // DeleteStack returns an empty response on success
}

#[derive(Serialize, Debug, Clone, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeStackResourcesRequest {
    pub stack_name: Option<String>,
    pub logical_resource_id: Option<String>,
    pub physical_resource_id: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeStackResourcesResponse {
    pub describe_stack_resources_result: DescribeStackResourcesResult,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeStackResourcesResult {
    pub stack_resources: StackResources,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct StackResources {
    #[serde(rename = "member", default)]
    pub member: Vec<StackResource>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct StackResource {
    pub stack_name: Option<String>,
    pub stack_id: Option<String>,
    pub logical_resource_id: String,
    pub physical_resource_id: Option<String>,
    pub resource_type: String,
    pub timestamp: String,
    pub resource_status: String,
    pub resource_status_reason: Option<String>,
    pub description: Option<String>,
    pub drift_information: Option<StackResourceDriftInformation>,
    pub module_info: Option<ModuleInfo>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct StackResourceDriftInformation {
    pub stack_resource_drift_status: String,
    pub last_check_timestamp: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct ModuleInfo {
    pub type_hierarchy: Option<String>,
    pub logical_id_hierarchy: Option<String>,
}

#[derive(Serialize, Debug, Clone, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeStackResourceRequest {
    pub stack_name: String,
    pub logical_resource_id: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeStackResourceResponse {
    pub describe_stack_resource_result: DescribeStackResourceResult,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeStackResourceResult {
    pub stack_resource_detail: StackResourceDetail,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct StackResourceDetail {
    pub stack_name: Option<String>,
    pub stack_id: Option<String>,
    pub logical_resource_id: String,
    pub physical_resource_id: Option<String>,
    pub resource_type: String,
    pub last_updated_timestamp: String,
    pub resource_status: String,
    pub resource_status_reason: Option<String>,
    pub description: Option<String>,
    pub metadata: Option<String>,
    pub drift_information: Option<StackResourceDriftInformation>,
    pub module_info: Option<ModuleInfo>,
}

#[derive(Serialize, Debug, Clone, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeStackEventsRequest {
    pub stack_name: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeStackEventsResponse {
    pub describe_stack_events_result: DescribeStackEventsResult,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeStackEventsResult {
    pub stack_events: StackEvents,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct StackEvents {
    #[serde(rename = "member", default)]
    pub member: Vec<StackEvent>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct StackEvent {
    pub stack_id: String,
    pub event_id: String,
    pub stack_name: String,
    pub logical_resource_id: Option<String>,
    pub physical_resource_id: Option<String>,
    pub resource_type: Option<String>,
    pub timestamp: String,
    pub resource_status: Option<String>,
    pub resource_status_reason: Option<String>,
    pub resource_properties: Option<String>,
    pub client_request_token: Option<String>,
}
