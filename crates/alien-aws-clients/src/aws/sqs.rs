use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};

use alien_error::ContextError;
use bon::Builder;
use form_urlencoded;
use quick_xml;
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;

// ---------------------------------------------------------------------------
// Error struct
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SqsErrorResponse {
    pub error: SqsErrorDetails,
    pub request_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SqsErrorDetails {
    #[serde(rename = "Type")]
    pub error_type: Option<String>,
    #[serde(rename = "Code")]
    pub code: String,
    #[serde(rename = "Message")]
    pub message: String,
    #[serde(rename = "Detail")]
    pub detail: Option<String>,
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait SqsApi: Send + Sync + std::fmt::Debug {
    async fn create_queue(&self, request: CreateQueueRequest) -> Result<CreateQueueResponse>;
    async fn delete_queue(&self, queue_url: &str) -> Result<()>;
    async fn delete_message(&self, queue_url: &str, request: DeleteMessageRequest) -> Result<()>;
    async fn send_message(
        &self,
        queue_url: &str,
        request: SendMessageRequest,
    ) -> Result<SendMessageResponse>;
    async fn get_queue_url(&self, request: GetQueueUrlRequest) -> Result<GetQueueUrlResponse>;
    async fn get_queue_attributes(
        &self,
        queue_url: &str,
        request: GetQueueAttributesRequest,
    ) -> Result<GetQueueAttributesResponse>;
    async fn add_permission(&self, queue_url: &str, request: AddPermissionRequest) -> Result<()>;
    async fn purge_queue(&self, queue_url: &str) -> Result<()>;
    async fn receive_message(
        &self,
        queue_url: &str,
        request: ReceiveMessageRequest,
    ) -> Result<ReceiveMessageResponse>;
    async fn remove_permission(
        &self,
        queue_url: &str,
        request: RemovePermissionRequest,
    ) -> Result<()>;
    async fn set_queue_attributes(
        &self,
        queue_url: &str,
        request: SetQueueAttributesRequest,
    ) -> Result<()>;
}

// ---------------------------------------------------------------------------
// SQS client using new request helpers.
// ---------------------------------------------------------------------------
#[derive(Debug, Clone)]
pub struct SqsClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl SqsClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self { client, credentials }
    }

    /// Get the account ID for this SQS client (used by tests)
    pub fn account_id(&self) -> &str {
        self.credentials.account_id()
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "sqs".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self.credentials.get_service_endpoint_option("sqs") {
            override_url.to_string()
        } else {
            format!("https://sqs.{}.amazonaws.com", self.credentials.region())
        }
    }

    // ------------------------- internal helpers -------------------------

    async fn send_form<T: DeserializeOwned + Send + 'static>(
        &self,
        method: Method,
        path: &str,
        form_data: HashMap<String, String>,
        operation: &str,
        resource: &str,
    ) -> Result<T> {
        self.credentials.ensure_fresh().await?;
        let base_url = self.get_base_url();
        let url = format!("{}{}", base_url.trim_end_matches('/'), path);

        let form_body = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form_data.iter())
            .finish();

        let builder = self
            .client
            .request(method.clone(), &url)
            .host(&format!("sqs.{}.amazonaws.com", self.credentials.region()))
            .content_type_form()
            .content_sha256(&form_body)
            .body(form_body.clone());

        let result =
            crate::aws::aws_request_utils::sign_send_xml(builder, &self.sign_config()).await;

        Self::map_result(result, operation, resource, Some(&form_body))
    }

    async fn send_form_no_body(
        &self,
        method: Method,
        path: &str,
        form_data: HashMap<String, String>,
        operation: &str,
        resource: &str,
    ) -> Result<()> {
        self.credentials.ensure_fresh().await?;
        let base_url = self.get_base_url();
        let url = format!("{}{}", base_url.trim_end_matches('/'), path);

        let form_body = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form_data.iter())
            .finish();

        let builder = self
            .client
            .request(method, &url)
            .host(&format!("sqs.{}.amazonaws.com", self.credentials.region()))
            .content_type_form()
            .content_sha256(&form_body)
            .body(form_body.clone());

        let result =
            crate::aws::aws_request_utils::sign_send_no_response(builder, &self.sign_config())
                .await;

        Self::map_result(result, operation, resource, Some(&form_body))
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
                        Self::map_sqs_error(status, text, operation, resource, request_body)
                    {
                        Err(e.context(mapped))
                    } else {
                        // Couldn't parse SQS error, use original error
                        Err(e)
                    }
                } else {
                    Err(e)
                }
            }
        }
    }

    fn map_sqs_error(
        status: StatusCode,
        body: &str,
        _operation: &str,
        resource: &str,
        request_body: Option<&str>,
    ) -> Option<ErrorData> {
        // Handle empty response bodies for specific status codes
        if body.trim().is_empty() {
            return match status {
                StatusCode::NOT_FOUND => Some(ErrorData::RemoteResourceNotFound {
                    resource_type: "Queue".into(),
                    resource_name: resource.into(),
                }),
                StatusCode::CONFLICT => Some(ErrorData::RemoteResourceConflict {
                    message: "Resource conflict".into(),
                    resource_type: "Queue".into(),
                    resource_name: resource.into(),
                }),
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => {
                    Some(ErrorData::RemoteAccessDenied {
                        resource_type: "Queue".into(),
                        resource_name: resource.into(),
                    })
                }
                StatusCode::TOO_MANY_REQUESTS => Some(ErrorData::RateLimitExceeded {
                    message: "Too many requests".into(),
                }),
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => Some(ErrorData::RemoteServiceUnavailable {
                    message: "Service unavailable".into(),
                }),
                _ => None, // Let the original error be used
            };
        }

        // Try to parse SQS error XML: <ErrorResponse><Error><Code>...</Code><Message>...</Message></Error></ErrorResponse>
        let parsed: std::result::Result<SqsErrorResponse, _> = quick_xml::de::from_str(body);
        let (code, message) = match parsed {
            Ok(e) => (e.error.code, e.error.message),
            Err(_) => {
                // If we can't parse the response, fall back to status code mapping
                let default_message = "Unknown error".to_string();
                return match status {
                    StatusCode::NOT_FOUND => Some(ErrorData::RemoteResourceNotFound {
                        resource_type: "Queue".into(),
                        resource_name: resource.into(),
                    }),
                    StatusCode::CONFLICT => Some(ErrorData::RemoteResourceConflict {
                        message: default_message,
                        resource_type: "Queue".into(),
                        resource_name: resource.into(),
                    }),
                    StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => {
                        Some(ErrorData::RemoteAccessDenied {
                            resource_type: "Queue".into(),
                            resource_name: resource.into(),
                        })
                    }
                    StatusCode::TOO_MANY_REQUESTS => Some(ErrorData::RateLimitExceeded {
                        message: default_message,
                    }),
                    StatusCode::SERVICE_UNAVAILABLE
                    | StatusCode::BAD_GATEWAY
                    | StatusCode::GATEWAY_TIMEOUT => Some(ErrorData::RemoteServiceUnavailable {
                        message: default_message,
                    }),
                    _ => None, // Let the original error be used
                };
            }
        };

        Some(match code.as_str() {
            // Access / auth
            "AccessDenied" | "NotAuthorized" | "UnrecognizedClient" | "ExpiredToken" => {
                ErrorData::RemoteAccessDenied {
                    resource_type: "Queue".into(),
                    resource_name: resource.into(),
                }
            }
            // Throttling
            "Throttling" | "TooManyRequests" => ErrorData::RateLimitExceeded { message },
            // Service unavailable
            "ServiceUnavailable" | "InternalFailure" | "ServiceException" => {
                ErrorData::RemoteServiceUnavailable { message }
            }
            // Timeout
            "RequestTimeout" => ErrorData::Timeout { message },
            // Resource not found - SQS uses different error codes
            "AWS.SimpleQueueService.NonExistentQueue"
            | "ResourceNotFound"
            | "QueueDoesNotExist" => ErrorData::RemoteResourceNotFound {
                resource_type: "Queue".into(),
                resource_name: resource.into(),
            },
            // Conflict / already exists
            "AWS.SimpleQueueService.QueueAlreadyExists"
            | "QueueAlreadyExists"
            | "QueueNameExists" => ErrorData::RemoteResourceConflict {
                message,
                resource_type: "Queue".into(),
                resource_name: resource.into(),
            },
            _ => match status {
                StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
                    resource_type: "Queue".into(),
                    resource_name: resource.into(),
                },
                StatusCode::CONFLICT => ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "Queue".into(),
                    resource_name: resource.into(),
                },
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
                    resource_type: "Queue".into(),
                    resource_name: resource.into(),
                },
                StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded { message },
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => ErrorData::RemoteServiceUnavailable { message },
                _ => ErrorData::HttpResponseError {
                    message: format!("SQS operation failed: {}", message),
                    url: "sqs.amazonaws.com".into(),
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
impl SqsApi for SqsClient {
    async fn create_queue(&self, request: CreateQueueRequest) -> Result<CreateQueueResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateQueue".to_string());
        form_data.insert("QueueName".to_string(), request.queue_name.clone());
        form_data.insert("Version".to_string(), "2012-11-05".to_string());

        // Add attributes if provided
        if let Some(attributes) = request.attributes {
            for (i, (key, value)) in attributes.iter().enumerate() {
                form_data.insert(format!("Attribute.{}.Name", i + 1), key.clone());
                form_data.insert(format!("Attribute.{}.Value", i + 1), value.clone());
            }
        }

        // Add tags if provided
        if let Some(tags) = request.tags {
            for (i, (key, value)) in tags.iter().enumerate() {
                form_data.insert(format!("Tag.{}.Key", i + 1), key.clone());
                form_data.insert(format!("Tag.{}.Value", i + 1), value.clone());
            }
        }

        self.send_form(
            Method::POST,
            "/",
            form_data,
            "CreateQueue",
            &request.queue_name,
        )
        .await
    }

    async fn delete_queue(&self, queue_url: &str) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteQueue".to_string());
        form_data.insert("QueueUrl".to_string(), queue_url.to_string());
        form_data.insert("Version".to_string(), "2012-11-05".to_string());

        self.send_form_no_body(Method::POST, "/", form_data, "DeleteQueue", queue_url)
            .await
    }

    async fn delete_message(&self, queue_url: &str, request: DeleteMessageRequest) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteMessage".to_string());
        form_data.insert("QueueUrl".to_string(), queue_url.to_string());
        form_data.insert("ReceiptHandle".to_string(), request.receipt_handle.clone());
        form_data.insert("Version".to_string(), "2012-11-05".to_string());

        self.send_form_no_body(Method::POST, "/", form_data, "DeleteMessage", queue_url)
            .await
    }

    async fn send_message(
        &self,
        queue_url: &str,
        request: SendMessageRequest,
    ) -> Result<SendMessageResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "SendMessage".to_string());
        form_data.insert("QueueUrl".to_string(), queue_url.to_string());
        form_data.insert("MessageBody".to_string(), request.message_body.clone());
        form_data.insert("Version".to_string(), "2012-11-05".to_string());

        if let Some(delay_seconds) = request.delay_seconds {
            form_data.insert("DelaySeconds".to_string(), delay_seconds.to_string());
        }

        if let Some(message_attributes) = request.message_attributes {
            for (i, (key, value)) in message_attributes.iter().enumerate() {
                form_data.insert(format!("MessageAttribute.{}.Name", i + 1), key.clone());
                form_data.insert(
                    format!("MessageAttribute.{}.Value.StringValue", i + 1),
                    value.string_value.clone(),
                );
                form_data.insert(
                    format!("MessageAttribute.{}.Value.DataType", i + 1),
                    value.data_type.clone(),
                );
            }
        }

        if let Some(message_system_attributes) = request.message_system_attributes {
            for (i, (key, value)) in message_system_attributes.iter().enumerate() {
                form_data.insert(
                    format!("MessageSystemAttribute.{}.Name", i + 1),
                    key.clone(),
                );
                form_data.insert(
                    format!("MessageSystemAttribute.{}.Value.StringValue", i + 1),
                    value.string_value.clone(),
                );
                form_data.insert(
                    format!("MessageSystemAttribute.{}.Value.DataType", i + 1),
                    value.data_type.clone(),
                );
            }
        }

        if let Some(message_deduplication_id) = request.message_deduplication_id {
            form_data.insert(
                "MessageDeduplicationId".to_string(),
                message_deduplication_id,
            );
        }

        if let Some(message_group_id) = request.message_group_id {
            form_data.insert("MessageGroupId".to_string(), message_group_id);
        }

        self.send_form(Method::POST, "/", form_data, "SendMessage", queue_url)
            .await
    }

    async fn get_queue_url(&self, request: GetQueueUrlRequest) -> Result<GetQueueUrlResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "GetQueueUrl".to_string());
        form_data.insert("QueueName".to_string(), request.queue_name.clone());
        form_data.insert("Version".to_string(), "2012-11-05".to_string());

        if let Some(queue_owner_aws_account_id) = request.queue_owner_aws_account_id {
            form_data.insert(
                "QueueOwnerAWSAccountId".to_string(),
                queue_owner_aws_account_id,
            );
        }

        self.send_form(
            Method::POST,
            "/",
            form_data,
            "GetQueueUrl",
            &request.queue_name,
        )
        .await
    }

    async fn get_queue_attributes(
        &self,
        queue_url: &str,
        request: GetQueueAttributesRequest,
    ) -> Result<GetQueueAttributesResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "GetQueueAttributes".to_string());
        form_data.insert("QueueUrl".to_string(), queue_url.to_string());
        form_data.insert("Version".to_string(), "2012-11-05".to_string());

        if let Some(attribute_names) = request.attribute_names {
            for (i, name) in attribute_names.iter().enumerate() {
                form_data.insert(format!("AttributeName.{}", i + 1), name.clone());
            }
        }

        self.send_form(
            Method::POST,
            "/",
            form_data,
            "GetQueueAttributes",
            queue_url,
        )
        .await
    }

    async fn add_permission(&self, queue_url: &str, request: AddPermissionRequest) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "AddPermission".to_string());
        form_data.insert("QueueUrl".to_string(), queue_url.to_string());
        form_data.insert("Label".to_string(), request.label.clone());
        form_data.insert("Version".to_string(), "2012-11-05".to_string());

        // Add AWS account IDs as separate list elements
        for (i, account_id) in request.aws_account_ids.iter().enumerate() {
            form_data.insert(format!("AWSAccountId.{}", i + 1), account_id.clone());
        }

        // Add actions as separate list elements
        for (i, action) in request.actions.iter().enumerate() {
            form_data.insert(format!("ActionName.{}", i + 1), action.clone());
        }

        self.send_form_no_body(Method::POST, "/", form_data, "AddPermission", queue_url)
            .await
    }

    async fn purge_queue(&self, queue_url: &str) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "PurgeQueue".to_string());
        form_data.insert("QueueUrl".to_string(), queue_url.to_string());
        form_data.insert("Version".to_string(), "2012-11-05".to_string());

        self.send_form_no_body(Method::POST, "/", form_data, "PurgeQueue", queue_url)
            .await
    }

    async fn receive_message(
        &self,
        queue_url: &str,
        request: ReceiveMessageRequest,
    ) -> Result<ReceiveMessageResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "ReceiveMessage".to_string());
        form_data.insert("QueueUrl".to_string(), queue_url.to_string());
        form_data.insert("Version".to_string(), "2012-11-05".to_string());

        if let Some(attribute_names) = request.attribute_names {
            for (i, name) in attribute_names.iter().enumerate() {
                form_data.insert(format!("AttributeName.{}", i + 1), name.clone());
            }
        }

        if let Some(message_attribute_names) = request.message_attribute_names {
            for (i, name) in message_attribute_names.iter().enumerate() {
                form_data.insert(format!("MessageAttributeName.{}", i + 1), name.clone());
            }
        }

        if let Some(max_number_of_messages) = request.max_number_of_messages {
            form_data.insert(
                "MaxNumberOfMessages".to_string(),
                max_number_of_messages.to_string(),
            );
        }

        if let Some(receive_request_attempt_id) = request.receive_request_attempt_id {
            form_data.insert(
                "ReceiveRequestAttemptId".to_string(),
                receive_request_attempt_id,
            );
        }

        if let Some(visibility_timeout) = request.visibility_timeout {
            form_data.insert(
                "VisibilityTimeout".to_string(),
                visibility_timeout.to_string(),
            );
        }

        if let Some(wait_time_seconds) = request.wait_time_seconds {
            form_data.insert("WaitTimeSeconds".to_string(), wait_time_seconds.to_string());
        }

        self.send_form(Method::POST, "/", form_data, "ReceiveMessage", queue_url)
            .await
    }

    async fn remove_permission(
        &self,
        queue_url: &str,
        request: RemovePermissionRequest,
    ) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "RemovePermission".to_string());
        form_data.insert("QueueUrl".to_string(), queue_url.to_string());
        form_data.insert("Label".to_string(), request.label.clone());
        form_data.insert("Version".to_string(), "2012-11-05".to_string());

        self.send_form_no_body(Method::POST, "/", form_data, "RemovePermission", queue_url)
            .await
    }

    async fn set_queue_attributes(
        &self,
        queue_url: &str,
        request: SetQueueAttributesRequest,
    ) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "SetQueueAttributes".to_string());
        form_data.insert("QueueUrl".to_string(), queue_url.to_string());
        form_data.insert("Version".to_string(), "2012-11-05".to_string());

        for (i, (key, value)) in request.attributes.iter().enumerate() {
            form_data.insert(format!("Attribute.{}.Name", i + 1), key.clone());
            form_data.insert(format!("Attribute.{}.Value", i + 1), value.clone());
        }

        self.send_form_no_body(
            Method::POST,
            "/",
            form_data,
            "SetQueueAttributes",
            queue_url,
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// Request / response payloads
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct CreateQueueRequest {
    pub queue_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributes: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateQueueResponse {
    pub create_queue_result: CreateQueueResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateQueueResult {
    pub queue_url: String,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct SendMessageRequest {
    pub message_body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay_seconds: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_attributes: Option<HashMap<String, MessageAttributeValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_system_attributes: Option<HashMap<String, MessageSystemAttributeValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_deduplication_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_group_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SendMessageResponse {
    pub send_message_result: SendMessageResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SendMessageResult {
    #[serde(rename = "MD5OfMessageBody")]
    pub md5_of_body: String,
    #[serde(rename = "MD5OfMessageAttributes")]
    pub md5_of_message_attributes: Option<String>,
    #[serde(rename = "MD5OfMessageSystemAttributes")]
    pub md5_of_message_system_attributes: Option<String>,
    #[serde(rename = "MessageId")]
    pub message_id: String,
    #[serde(rename = "SequenceNumber")]
    pub sequence_number: Option<String>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct GetQueueUrlRequest {
    pub queue_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_owner_aws_account_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetQueueUrlResponse {
    pub get_queue_url_result: GetQueueUrlResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetQueueUrlResult {
    pub queue_url: String,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct GetQueueAttributesRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribute_names: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetQueueAttributesResponse {
    pub get_queue_attributes_result: GetQueueAttributesResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetQueueAttributesResult {
    #[serde(rename = "Attribute", default)]
    pub attributes: Vec<Attribute>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Attribute {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct AddPermissionRequest {
    pub label: String,
    pub aws_account_ids: Vec<String>,
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct ReceiveMessageRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribute_names: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_attribute_names: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_number_of_messages: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receive_request_attempt_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility_timeout: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_time_seconds: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ReceiveMessageResponse {
    pub receive_message_result: ReceiveMessageResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ReceiveMessageResult {
    #[serde(rename = "Message", default)]
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct RemovePermissionRequest {
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct SetQueueAttributesRequest {
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct DeleteMessageRequest {
    pub receipt_handle: String,
}

// ---------------------------------------------------------------------------
// Supporting data types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct MessageAttributeValue {
    pub string_value: String,
    pub data_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct MessageSystemAttributeValue {
    pub string_value: String,
    pub data_type: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Message {
    pub attributes: Option<HashMap<String, String>>,
    pub body: String,
    #[serde(rename = "MD5OfBody")]
    pub md5_of_body: String,
    #[serde(rename = "MD5OfMessageAttributes")]
    pub md5_of_message_attributes: Option<String>,
    pub message_attributes: Option<HashMap<String, MessageAttributeValue>>,
    pub message_id: String,
    pub receipt_handle: String,
}
