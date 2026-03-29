use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};

use alien_error::{Context, ContextError, IntoAlienError};
use bon::Builder;
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait DynamoDbApi: Send + Sync + std::fmt::Debug {
    async fn create_table(&self, request: CreateTableRequest) -> Result<CreateTableResponse>;
    async fn delete_table(&self, request: DeleteTableRequest) -> Result<DeleteTableResponse>;
    async fn describe_table(&self, request: DescribeTableRequest) -> Result<DescribeTableResponse>;
    async fn get_item(&self, request: GetItemRequest) -> Result<GetItemResponse>;
    async fn put_item(&self, request: PutItemRequest) -> Result<PutItemResponse>;
    async fn delete_item(&self, request: DeleteItemRequest) -> Result<DeleteItemResponse>;
    async fn update_item(&self, request: UpdateItemRequest) -> Result<UpdateItemResponse>;
    async fn query(&self, request: QueryRequest) -> Result<QueryResponse>;
    async fn update_time_to_live(
        &self,
        request: UpdateTimeToLiveRequest,
    ) -> Result<UpdateTimeToLiveResponse>;
    async fn describe_time_to_live(
        &self,
        request: DescribeTimeToLiveRequest,
    ) -> Result<DescribeTimeToLiveResponse>;
}

// ---------------------------------------------------------------------------
// DynamoDB client using new request helpers.
// ---------------------------------------------------------------------------
#[derive(Debug, Clone)]
pub struct DynamoDbClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl DynamoDbClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
        }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "dynamodb".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self.credentials.get_service_endpoint_option("dynamodb") {
            override_url.to_string()
        } else {
            format!(
                "https://dynamodb.{}.amazonaws.com",
                self.credentials.region()
            )
        }
    }

    // ------------------------- internal helpers -------------------------

    async fn send_json<T: DeserializeOwned + Send + 'static>(
        &self,
        target: &str,
        body: Option<String>,
        operation: &str,
        resource: &str,
    ) -> Result<T> {
        self.credentials.ensure_fresh().await?;
        let base_url = self.get_base_url();
        let url = base_url.trim_end_matches('/');

        let builder = self
            .client
            .request(Method::POST, url)
            .host(&format!(
                "dynamodb.{}.amazonaws.com",
                self.credentials.region()
            ))
            .header("X-Amz-Target", target)
            .content_type_json();

        let builder = if let Some(ref b) = body {
            builder.content_sha256(b).body(b.clone())
        } else {
            builder.content_sha256("")
        };

        let result =
            crate::aws::aws_request_utils::sign_send_json(builder, &self.sign_config()).await;

        Self::map_result(result, operation, resource, body.as_deref())
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
                        Self::map_dynamodb_error(status, text, operation, resource, request_body)
                    {
                        Err(e.context(mapped))
                    } else {
                        // Couldn't parse DynamoDB error, use original error
                        Err(e)
                    }
                } else {
                    Err(e)
                }
            }
        }
    }

    fn map_dynamodb_error(
        status: StatusCode,
        body: &str,
        _operation: &str,
        resource: &str,
        request_body: Option<&str>,
    ) -> Option<ErrorData> {
        let parsed: std::result::Result<DynamoDbErrorResponse, _> = serde_json::from_str(body);
        let (code, message) = match parsed {
            Ok(e) => {
                let raw_code = e
                    .type_field
                    .or(e.type_field_underscore)
                    .unwrap_or_else(|| "UnknownErrorCode".into());

                // Extract the actual error type from the service-prefixed format
                // e.g., "com.amazonaws.dynamodb.v20120810#ConditionalCheckFailedException" -> "ConditionalCheckFailedException"
                let c = if let Some(hash_pos) = raw_code.rfind('#') {
                    raw_code[hash_pos + 1..].to_string()
                } else {
                    raw_code
                };

                let m = e.message.unwrap_or_else(|| "Unknown error".into());
                (c, m)
            }
            Err(_) => {
                // If we can't parse the response, return None to use original error
                return None;
            }
        };

        Some(match code.as_str() {
            // Access / auth
            "AccessDeniedException" | "UnrecognizedClientException" | "ExpiredTokenException" => {
                ErrorData::RemoteAccessDenied {
                    resource_type: "Table".into(),
                    resource_name: resource.into(),
                }
            }
            // Throttling
            "ThrottlingException" | "ProvisionedThroughputExceededException" => {
                ErrorData::RateLimitExceeded { message }
            }
            // Service unavailable
            "ServiceUnavailable" | "InternalServerError" => {
                ErrorData::RemoteServiceUnavailable { message }
            }
            // Resource not found
            "ResourceNotFoundException" => ErrorData::RemoteResourceNotFound {
                resource_type: "Table".into(),
                resource_name: resource.into(),
            },
            // Conflict / conditional check failed
            "ConditionalCheckFailedException" => ErrorData::RemoteResourceConflict {
                message,
                resource_type: "Item".into(),
                resource_name: resource.into(),
            },
            // Validation errors
            "ValidationException" => ErrorData::GenericError { message },
            _ => match status {
                StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
                    resource_type: "Table".into(),
                    resource_name: resource.into(),
                },
                StatusCode::CONFLICT => ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "Item".into(),
                    resource_name: resource.into(),
                },
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
                    resource_type: "Table".into(),
                    resource_name: resource.into(),
                },
                StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded { message },
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => ErrorData::RemoteServiceUnavailable { message },
                _ => ErrorData::HttpResponseError {
                    message: format!("DynamoDB operation failed: {}", message),
                    url: format!("dynamodb.amazonaws.com"),
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
impl DynamoDbApi for DynamoDbClient {
    async fn create_table(&self, request: CreateTableRequest) -> Result<CreateTableResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize CreateTableRequest for table '{}'",
                    request.table_name
                ),
            },
        )?;
        self.send_json(
            "DynamoDB_20120810.CreateTable",
            Some(body),
            "CreateTable",
            &request.table_name,
        )
        .await
    }

    async fn delete_table(&self, request: DeleteTableRequest) -> Result<DeleteTableResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize DeleteTableRequest for table '{}'",
                    request.table_name
                ),
            },
        )?;
        self.send_json(
            "DynamoDB_20120810.DeleteTable",
            Some(body),
            "DeleteTable",
            &request.table_name,
        )
        .await
    }

    async fn describe_table(&self, request: DescribeTableRequest) -> Result<DescribeTableResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize DescribeTableRequest for table '{}'",
                    request.table_name
                ),
            },
        )?;
        self.send_json(
            "DynamoDB_20120810.DescribeTable",
            Some(body),
            "DescribeTable",
            &request.table_name,
        )
        .await
    }

    async fn get_item(&self, request: GetItemRequest) -> Result<GetItemResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize GetItemRequest for table '{}'",
                    request.table_name
                ),
            },
        )?;
        self.send_json(
            "DynamoDB_20120810.GetItem",
            Some(body),
            "GetItem",
            &request.table_name,
        )
        .await
    }

    async fn put_item(&self, request: PutItemRequest) -> Result<PutItemResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize PutItemRequest for table '{}'",
                    request.table_name
                ),
            },
        )?;
        self.send_json(
            "DynamoDB_20120810.PutItem",
            Some(body),
            "PutItem",
            &request.table_name,
        )
        .await
    }

    async fn delete_item(&self, request: DeleteItemRequest) -> Result<DeleteItemResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize DeleteItemRequest for table '{}'",
                    request.table_name
                ),
            },
        )?;
        self.send_json(
            "DynamoDB_20120810.DeleteItem",
            Some(body),
            "DeleteItem",
            &request.table_name,
        )
        .await
    }

    async fn update_item(&self, request: UpdateItemRequest) -> Result<UpdateItemResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize UpdateItemRequest for table '{}'",
                    request.table_name
                ),
            },
        )?;
        self.send_json(
            "DynamoDB_20120810.UpdateItem",
            Some(body),
            "UpdateItem",
            &request.table_name,
        )
        .await
    }

    async fn query(&self, request: QueryRequest) -> Result<QueryResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize QueryRequest for table '{}'",
                    request.table_name
                ),
            },
        )?;
        self.send_json(
            "DynamoDB_20120810.Query",
            Some(body),
            "Query",
            &request.table_name,
        )
        .await
    }

    async fn update_time_to_live(
        &self,
        request: UpdateTimeToLiveRequest,
    ) -> Result<UpdateTimeToLiveResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize UpdateTimeToLiveRequest for table '{}'",
                    request.table_name
                ),
            },
        )?;
        self.send_json(
            "DynamoDB_20120810.UpdateTimeToLive",
            Some(body),
            "UpdateTimeToLive",
            &request.table_name,
        )
        .await
    }

    async fn describe_time_to_live(
        &self,
        request: DescribeTimeToLiveRequest,
    ) -> Result<DescribeTimeToLiveResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize DescribeTimeToLiveRequest for table '{}'",
                    request.table_name
                ),
            },
        )?;
        self.send_json(
            "DynamoDB_20120810.DescribeTimeToLive",
            Some(body),
            "DescribeTimeToLive",
            &request.table_name,
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// Error JSON mapping structs
// ---------------------------------------------------------------------------
#[derive(Debug, Deserialize)]
struct DynamoDbErrorResponse {
    #[serde(rename = "Type")]
    type_field: Option<String>,
    #[serde(rename = "__type")]
    type_field_underscore: Option<String>,
    #[serde(rename = "message")]
    message: Option<String>,
}

// ---------------------------------------------------------------------------
// Constants for DynamoDB API
// ---------------------------------------------------------------------------

/// Billing modes for DynamoDB tables
pub mod billing_modes {
    pub const PROVISIONED: &str = "PROVISIONED";
    pub const PAY_PER_REQUEST: &str = "PAY_PER_REQUEST";
}

/// Key types for DynamoDB key schema
pub mod key_types {
    pub const HASH: &str = "HASH";
    pub const RANGE: &str = "RANGE";
}

/// Attribute types for DynamoDB attributes
pub mod attribute_types {
    pub const STRING: &str = "S";
    pub const NUMBER: &str = "N";
    pub const BINARY: &str = "B";
}

/// Table status values
pub mod table_status {
    pub const CREATING: &str = "CREATING";
    pub const UPDATING: &str = "UPDATING";
    pub const DELETING: &str = "DELETING";
    pub const ACTIVE: &str = "ACTIVE";
    pub const INACCESSIBLE_ENCRYPTION_CREDENTIALS: &str = "INACCESSIBLE_ENCRYPTION_CREDENTIALS";
    pub const ARCHIVING: &str = "ARCHIVING";
    pub const ARCHIVED: &str = "ARCHIVED";
}

/// Return value constants for PutItem, UpdateItem, DeleteItem
pub mod return_values {
    pub const NONE: &str = "NONE";
    pub const ALL_OLD: &str = "ALL_OLD";
    pub const UPDATED_OLD: &str = "UPDATED_OLD";
    pub const ALL_NEW: &str = "ALL_NEW";
    pub const UPDATED_NEW: &str = "UPDATED_NEW";
}

// ---------------------------------------------------------------------------
// Request / response payloads for DynamoDB operations
// ---------------------------------------------------------------------------

/// Represents an attribute value in DynamoDB
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AttributeValue {
    /// An attribute of type String
    #[serde(rename = "S", skip_serializing_if = "Option::is_none")]
    pub s: Option<String>,
    /// An attribute of type Number
    #[serde(rename = "N", skip_serializing_if = "Option::is_none")]
    pub n: Option<String>,
    /// An attribute of type Binary (Base64 encoded)
    #[serde(rename = "B", skip_serializing_if = "Option::is_none")]
    pub b: Option<String>,
    /// An attribute of type Boolean
    #[serde(rename = "BOOL", skip_serializing_if = "Option::is_none")]
    pub bool: Option<bool>,
    /// An attribute of type Null
    #[serde(rename = "NULL", skip_serializing_if = "Option::is_none")]
    pub null: Option<bool>,
    /// An attribute of type List
    #[serde(rename = "L", skip_serializing_if = "Option::is_none")]
    pub l: Option<Vec<AttributeValue>>,
    /// An attribute of type Map
    #[serde(rename = "M", skip_serializing_if = "Option::is_none")]
    pub m: Option<HashMap<String, AttributeValue>>,
    /// An attribute of type String Set
    #[serde(rename = "SS", skip_serializing_if = "Option::is_none")]
    pub ss: Option<Vec<String>>,
    /// An attribute of type Number Set
    #[serde(rename = "NS", skip_serializing_if = "Option::is_none")]
    pub ns: Option<Vec<String>>,
    /// An attribute of type Binary Set (Base64 encoded)
    #[serde(rename = "BS", skip_serializing_if = "Option::is_none")]
    pub bs: Option<Vec<String>>,
}

impl AttributeValue {
    pub fn s(value: String) -> Self {
        Self {
            s: Some(value),
            n: None,
            b: None,
            bool: None,
            null: None,
            l: None,
            m: None,
            ss: None,
            ns: None,
            bs: None,
        }
    }

    pub fn n(value: String) -> Self {
        Self {
            s: None,
            n: Some(value),
            b: None,
            bool: None,
            null: None,
            l: None,
            m: None,
            ss: None,
            ns: None,
            bs: None,
        }
    }

    pub fn b(value: String) -> Self {
        Self {
            s: None,
            n: None,
            b: Some(value),
            bool: None,
            null: None,
            l: None,
            m: None,
            ss: None,
            ns: None,
            bs: None,
        }
    }

    pub fn bool(value: bool) -> Self {
        Self {
            s: None,
            n: None,
            b: None,
            bool: Some(value),
            null: None,
            l: None,
            m: None,
            ss: None,
            ns: None,
            bs: None,
        }
    }

    pub fn null() -> Self {
        Self {
            s: None,
            n: None,
            b: None,
            bool: None,
            null: Some(true),
            l: None,
            m: None,
            ss: None,
            ns: None,
            bs: None,
        }
    }

    pub fn l(value: Vec<AttributeValue>) -> Self {
        Self {
            s: None,
            n: None,
            b: None,
            bool: None,
            null: None,
            l: Some(value),
            m: None,
            ss: None,
            ns: None,
            bs: None,
        }
    }

    pub fn m(value: HashMap<String, AttributeValue>) -> Self {
        Self {
            s: None,
            n: None,
            b: None,
            bool: None,
            null: None,
            l: None,
            m: Some(value),
            ss: None,
            ns: None,
            bs: None,
        }
    }

    pub fn ss(value: Vec<String>) -> Self {
        Self {
            s: None,
            n: None,
            b: None,
            bool: None,
            null: None,
            l: None,
            m: None,
            ss: Some(value),
            ns: None,
            bs: None,
        }
    }

    pub fn ns(value: Vec<String>) -> Self {
        Self {
            s: None,
            n: None,
            b: None,
            bool: None,
            null: None,
            l: None,
            m: None,
            ss: None,
            ns: Some(value),
            bs: None,
        }
    }

    pub fn bs(value: Vec<String>) -> Self {
        Self {
            s: None,
            n: None,
            b: None,
            bool: None,
            null: None,
            l: None,
            m: None,
            ss: None,
            ns: None,
            bs: Some(value),
        }
    }
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct GetItemRequest {
    pub table_name: String,
    pub key: HashMap<String, AttributeValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projection_expression: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression_attribute_names: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consistent_read: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetItemResponse {
    pub item: Option<HashMap<String, AttributeValue>>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct PutItemRequest {
    pub table_name: String,
    pub item: HashMap<String, AttributeValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition_expression: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression_attribute_names: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression_attribute_values: Option<HashMap<String, AttributeValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_values: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PutItemResponse {
    pub attributes: Option<HashMap<String, AttributeValue>>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct DeleteItemRequest {
    pub table_name: String,
    pub key: HashMap<String, AttributeValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition_expression: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression_attribute_names: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression_attribute_values: Option<HashMap<String, AttributeValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_values: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DeleteItemResponse {
    pub attributes: Option<HashMap<String, AttributeValue>>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct UpdateItemRequest {
    pub table_name: String,
    pub key: HashMap<String, AttributeValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_expression: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition_expression: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression_attribute_names: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression_attribute_values: Option<HashMap<String, AttributeValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_values: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct UpdateItemResponse {
    pub attributes: Option<HashMap<String, AttributeValue>>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct QueryRequest {
    pub table_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_condition_expression: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression_attribute_names: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression_attribute_values: Option<HashMap<String, AttributeValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projection_expression: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclusive_start_key: Option<HashMap<String, AttributeValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scan_index_forward: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct QueryResponse {
    pub items: Vec<HashMap<String, AttributeValue>>,
    pub count: i32,
    pub scanned_count: i32,
    pub last_evaluated_key: Option<HashMap<String, AttributeValue>>,
}

// ---------------------------------------------------------------------------
// Table management operations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct CreateTableRequest {
    pub table_name: String,
    pub key_schema: Vec<KeySchemaElement>,
    pub attribute_definitions: Vec<AttributeDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisioned_throughput: Option<ProvisionedThroughput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct KeySchemaElement {
    pub attribute_name: String,
    pub key_type: String, // "HASH" or "RANGE"
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct AttributeDefinition {
    pub attribute_name: String,
    pub attribute_type: String, // "S", "N", "B"
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct ProvisionedThroughput {
    pub read_capacity_units: i64,
    pub write_capacity_units: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateTableResponse {
    pub table_description: TableDescription,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct DeleteTableRequest {
    pub table_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DeleteTableResponse {
    pub table_description: TableDescription,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeTableRequest {
    pub table_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct BillingModeSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_mode: Option<String>, // "PROVISIONED" or "PAY_PER_REQUEST"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_update_to_pay_per_request_date_time: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ProvisionedThroughputDescription {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_capacity_units: Option<i64>, // AWS docs show as Double, using i64 for consistency
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_capacity_units: Option<i64>, // AWS docs show as Double, using i64 for consistency
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_increase_date_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_decrease_date_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number_of_decreases_today: Option<i64>, // AWS docs show as Long
}

// ---------------------------------------------------------------------------
// Supporting types for TableDescription (defined before TableDescription)
// Note: These are minimal implementations to support the TableDescription struct.
// They can be expanded as needed for specific use cases.
// ---------------------------------------------------------------------------

/// Contains information about the table archive
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ArchivalSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archival_date_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archival_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archival_backup_arn: Option<String>,
}

/// Represents attributes that are projected into the index
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Projection {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projection_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub non_key_attributes: Option<Vec<String>>,
}

/// Represents the properties of a global secondary index
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GlobalSecondaryIndexDescription {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_schema: Option<Vec<KeySchemaElement>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projection: Option<Projection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backfilling: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisioned_throughput: Option<ProvisionedThroughputDescription>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_size_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_arn: Option<String>,
}

/// Represents the properties of a local secondary index
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LocalSecondaryIndexDescription {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_schema: Option<Vec<KeySchemaElement>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projection: Option<Projection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_size_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_arn: Option<String>,
}

/// Replica-specific provisioned throughput override
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ProvisionedThroughputOverride {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_capacity_units: Option<i64>,
}

/// Contains details about the table class
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TableClassSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_update_date_time: Option<f64>,
}

/// Represents a replica global secondary index
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ReplicaGlobalSecondaryIndexDescription {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisioned_throughput_override: Option<ProvisionedThroughputOverride>,
}

/// Contains the details of the replica
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ReplicaDescription {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replica_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replica_status_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replica_status_percent_progress: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kms_master_key_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisioned_throughput_override: Option<ProvisionedThroughputOverride>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global_secondary_indexes: Option<Vec<ReplicaGlobalSecondaryIndexDescription>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replica_inaccessible_date_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replica_table_class_summary: Option<TableClassSummary>,
}

/// Contains details about the restore source
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RestoreSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_backup_arn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_table_arn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restore_date_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restore_in_progress: Option<bool>,
}

/// The description of the server-side encryption status on the specified table
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SSEDescription {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sse_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kms_master_key_arn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inaccessible_encryption_date_time: Option<f64>,
}

/// Represents the DynamoDB Streams configuration for a table
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct StreamSpecification {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_view_type: Option<String>,
}

/// Describes the warm throughput value of the base table
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TableWarmThroughputDescription {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_units_per_second: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_units_per_second: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TableDescription {
    /// Contains information about the table archive
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archival_summary: Option<ArchivalSummary>,
    /// An array of AttributeDefinition objects
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribute_definitions: Option<Vec<AttributeDefinition>>,
    /// Contains the details for the read/write capacity mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_mode_summary: Option<BillingModeSummary>,
    /// The date and time when the table was created, in UNIX epoch time format
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_date_time: Option<f64>,
    /// Indicates whether deletion protection is enabled (true) or disabled (false) on the table
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deletion_protection_enabled: Option<bool>,
    /// The global secondary indexes, if any, on the table
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global_secondary_indexes: Option<Vec<GlobalSecondaryIndexDescription>>,
    /// Represents the version of global tables in use, if the table is replicated across AWS Regions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global_table_version: Option<String>,
    /// The number of items in the specified table
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_count: Option<i64>,
    /// The primary key structure for the table
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_schema: Option<Vec<KeySchemaElement>>,
    /// The Amazon Resource Name (ARN) that uniquely identifies the latest stream for this table
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_stream_arn: Option<String>,
    /// A timestamp, in ISO 8601 format, for this stream
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_stream_label: Option<String>,
    /// Represents one or more local secondary indexes on the table
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_secondary_indexes: Option<Vec<LocalSecondaryIndexDescription>>,
    /// The provisioned throughput settings for the table
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisioned_throughput: Option<ProvisionedThroughputDescription>,
    /// Represents replicas of the table
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replicas: Option<Vec<ReplicaDescription>>,
    /// Contains information about the table's restore source
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restore_summary: Option<RestoreSummary>,
    /// The description of the server-side encryption status on the specified table
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sse_description: Option<SSEDescription>,
    /// The current DynamoDB Streams configuration for the table
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_specification: Option<StreamSpecification>,
    /// The Amazon Resource Name (ARN) that uniquely identifies the table
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_arn: Option<String>,
    /// Contains details of the table class
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_class_summary: Option<TableClassSummary>,
    /// Unique identifier for the table for which the backup was created
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_id: Option<String>,
    /// The name of the table
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_name: Option<String>,
    /// The total size of the specified table, in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_size_bytes: Option<i64>,
    /// The current state of the table
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_status: Option<String>,
    /// Describes the warm throughput value of the base table
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warm_throughput: Option<TableWarmThroughputDescription>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeTableResponse {
    pub table: TableDescription,
}

// ---------------------------------------------------------------------------
// TTL (Time To Live) operations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct TimeToLiveSpecification {
    pub attribute_name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct UpdateTimeToLiveRequest {
    pub table_name: String,
    pub time_to_live_specification: TimeToLiveSpecification,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TimeToLiveDescription {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_to_live_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribute_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct UpdateTimeToLiveResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_to_live_specification: Option<TimeToLiveSpecification>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeTimeToLiveRequest {
    pub table_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeTimeToLiveResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_to_live_description: Option<TimeToLiveDescription>,
}
