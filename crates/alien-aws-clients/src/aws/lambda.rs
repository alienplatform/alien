use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsRequestSigner, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};

use alien_error::{Context, ContextError, IntoAlienError};
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
pub trait LambdaApi: Send + Sync + std::fmt::Debug {
    async fn create_function(
        &self,
        request: CreateFunctionRequest,
    ) -> Result<FunctionConfiguration>;
    async fn create_function_url_config(
        &self,
        function_name: &str,
        request: CreateFunctionUrlConfigRequest,
    ) -> Result<CreateFunctionUrlConfigResponse>;
    async fn add_permission(
        &self,
        function_name: &str,
        request: AddPermissionRequest,
    ) -> Result<AddPermissionResponse>;
    async fn update_function_code(
        &self,
        function_name: &str,
        request: UpdateFunctionCodeRequest,
    ) -> Result<FunctionConfiguration>;
    async fn update_function_configuration(
        &self,
        function_name: &str,
        request: UpdateFunctionConfigurationRequest,
    ) -> Result<FunctionConfiguration>;
    async fn get_function_configuration(
        &self,
        function_name: &str,
        qualifier: Option<String>,
    ) -> Result<FunctionConfiguration>;
    async fn delete_function_url_config(
        &self,
        function_name: &str,
        qualifier: Option<String>,
    ) -> Result<()>;
    async fn get_function_url_config(
        &self,
        function_name: &str,
        qualifier: Option<String>,
    ) -> Result<FunctionUrlConfig>;
    async fn delete_function(&self, function_name: &str, qualifier: Option<String>) -> Result<()>;
    async fn get_policy(
        &self,
        function_name: &str,
        qualifier: Option<String>,
    ) -> Result<GetPolicyResponse>;

    // Function invocation operations
    async fn invoke(&self, request: InvokeRequest) -> Result<InvokeResponse>;

    // Event Source Mapping operations
    async fn create_event_source_mapping(
        &self,
        request: CreateEventSourceMappingRequest,
    ) -> Result<EventSourceMapping>;
    async fn get_event_source_mapping(&self, uuid: &str) -> Result<EventSourceMapping>;
    async fn update_event_source_mapping(
        &self,
        uuid: &str,
        request: UpdateEventSourceMappingRequest,
    ) -> Result<EventSourceMapping>;
    async fn delete_event_source_mapping(&self, uuid: &str) -> Result<EventSourceMapping>;
    async fn list_event_source_mappings(
        &self,
        request: ListEventSourceMappingsRequest,
    ) -> Result<ListEventSourceMappingsResponse>;

    // Concurrency operations
    async fn put_function_concurrency(
        &self,
        function_name: &str,
        reserved_concurrent_executions: u32,
    ) -> Result<()>;
    async fn delete_function_concurrency(&self, function_name: &str) -> Result<()>;
}

// ---------------------------------------------------------------------------
// Lambda client using new request helpers.
// ---------------------------------------------------------------------------
#[derive(Debug, Clone)]
pub struct LambdaClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl LambdaClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
        }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "lambda".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self.credentials.get_service_endpoint_option("lambda") {
            override_url.to_string()
        } else {
            format!("https://lambda.{}.amazonaws.com", self.credentials.region())
        }
    }

    // ------------------------- internal helpers -------------------------

    async fn send_json<T: DeserializeOwned + Send + 'static>(
        &self,
        method: Method,
        path: &str,
        query_params: Option<Vec<(&str, String)>>,
        body: Option<String>,
        operation: &str,
        resource: &str,
    ) -> Result<T> {
        self.credentials.ensure_fresh().await?;
        let base_url = self.get_base_url();
        let mut url = format!("{}{}", base_url.trim_end_matches('/'), path);
        if let Some(qs) = query_params {
            if !qs.is_empty() {
                url.push('?');
                url.push_str(
                    &qs.iter()
                        .map(|(k, v)| {
                            format!(
                                "{}={}",
                                k,
                                form_urlencoded::byte_serialize(v.as_bytes()).collect::<String>()
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("&"),
                );
            }
        }

        let builder = self
            .client
            .request(method.clone(), &url)
            .host(&format!(
                "lambda.{}.amazonaws.com",
                self.credentials.region()
            ))
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

    async fn send_no_body(
        &self,
        method: Method,
        path: &str,
        query_params: Option<Vec<(&str, String)>>,
        operation: &str,
        resource: &str,
    ) -> Result<()> {
        self.credentials.ensure_fresh().await?;
        let base_url = self.get_base_url();
        let mut url = format!("{}{}", base_url.trim_end_matches('/'), path);
        if let Some(qs) = query_params {
            if !qs.is_empty() {
                url.push('?');
                url.push_str(
                    &qs.iter()
                        .map(|(k, v)| {
                            format!(
                                "{}={}",
                                k,
                                form_urlencoded::byte_serialize(v.as_bytes()).collect::<String>()
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("&"),
                );
            }
        }

        let builder = self
            .client
            .request(method, &url)
            .host(&format!(
                "lambda.{}.amazonaws.com",
                self.credentials.region()
            ))
            .content_sha256("");

        let result =
            crate::aws::aws_request_utils::sign_send_no_response(builder, &self.sign_config())
                .await;

        Self::map_result(result, operation, resource, None)
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
                        Self::map_lambda_error(status, text, operation, resource, request_body)
                    {
                        Err(e.context(mapped))
                    } else {
                        // Couldn't parse Lambda error, use original error
                        Err(e)
                    }
                } else {
                    Err(e)
                }
            }
        }
    }

    fn map_lambda_error(
        status: StatusCode,
        body: &str,
        operation: &str,
        resource: &str,
        request_body: Option<&str>,
    ) -> Option<ErrorData> {
        let parsed: std::result::Result<LambdaErrorResponse, _> = serde_json::from_str(body);
        let (code, message) = match parsed {
            Ok(e) => {
                let c = e
                    .type_field_underscore
                    .or(e.type_field)
                    .or_else(|| e.error.as_ref().and_then(|d| d.code.clone()))
                    .unwrap_or_else(|| "UnknownErrorCode".into());
                let m = e
                    .message
                    .or(e.message_capital)
                    .or_else(|| e.error.as_ref().and_then(|d| d.message.clone()))
                    .unwrap_or_else(|| "Unknown error".into());
                (c, m)
            }
            Err(_) => {
                // If we can't parse the response, return None to use original error
                return None;
            }
        };

        Some(match code.as_str() {
            // Access / auth
            "AccessDeniedException"
            | "NotAuthorized"
            | "UnrecognizedClientException"
            | "ExpiredTokenException" => ErrorData::RemoteAccessDenied {
                resource_type: "Function".into(),
                resource_name: resource.into(),
            },
            // Throttling
            "ThrottlingException" | "TooManyRequestsException" => {
                ErrorData::RateLimitExceeded { message }
            }
            // Service unavailable
            "ServiceUnavailable" | "InternalFailure" | "ServiceException" => {
                ErrorData::RemoteServiceUnavailable { message }
            }
            // Timeout
            "RequestTimeoutException" => ErrorData::Timeout { message },
            // Resource not found
            "ResourceNotFoundException" => ErrorData::RemoteResourceNotFound {
                resource_type: "Function".into(),
                resource_name: resource.into(),
            },
            // Conflict / already exists
            "ResourceConflictException" => ErrorData::RemoteResourceConflict {
                message,
                resource_type: "Function".into(),
                resource_name: resource.into(),
            },
            // Lambda invoke-specific errors
            "EC2AccessDeniedException" | "EC2ThrottledException" | "EC2UnexpectedException" => {
                ErrorData::RemoteServiceUnavailable { message }
            }
            "EFSIOException"
            | "EFSMountConnectivityException"
            | "EFSMountFailureException"
            | "EFSMountTimeoutException" => ErrorData::RemoteServiceUnavailable { message },
            "ENILimitReachedException"
            | "InvalidSubnetIDException"
            | "InvalidSecurityGroupIDException"
            | "SubnetIPAddressLimitReachedException" => {
                ErrorData::RemoteServiceUnavailable { message }
            }
            "InvalidParameterValueException"
            | "InvalidRequestContentException"
            | "UnsupportedMediaTypeException" => {
                // IAM eventual consistency: Lambda may return InvalidParameterValueException
                // when it cannot yet assume a just-created execution role, or when ECR
                // cross-account permissions haven't propagated yet.
                if message.contains("cannot be assumed")
                    || message.contains("not authorized")
                    || message.contains("does not have permission to access the ECR image")
                {
                    ErrorData::RemoteServiceUnavailable { message }
                } else {
                    ErrorData::InvalidInput {
                        message,
                        field_name: None,
                    }
                }
            }
            "InvalidRuntimeException" | "InvalidZipFileException" => {
                ErrorData::RemoteServiceUnavailable { message }
            }
            "KMSAccessDeniedException"
            | "KMSDisabledException"
            | "KMSInvalidStateException"
            | "KMSNotFoundException" => ErrorData::RemoteAccessDenied {
                resource_type: "KMS Key".into(),
                resource_name: resource.into(),
            },
            "RecursiveInvocationException" => ErrorData::InvalidInput {
                message,
                field_name: None,
            },
            "RequestTooLargeException" => ErrorData::InvalidInput {
                message,
                field_name: None,
            },
            "ResourceNotReadyException" => ErrorData::RemoteServiceUnavailable { message },
            "SnapStartException" | "SnapStartNotReadyException" | "SnapStartTimeoutException" => {
                ErrorData::RemoteServiceUnavailable { message }
            }
            _ => match status {
                StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
                    resource_type: "Function".into(),
                    resource_name: resource.into(),
                },
                StatusCode::CONFLICT => ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "Function".into(),
                    resource_name: resource.into(),
                },
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
                    resource_type: "Function".into(),
                    resource_name: resource.into(),
                },
                StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded { message },
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => ErrorData::RemoteServiceUnavailable { message },
                _ => ErrorData::HttpResponseError {
                    message: format!("Lambda operation failed: {}", message),
                    url: format!("lambda.amazonaws.com"),
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
impl LambdaApi for LambdaClient {
    async fn create_function(
        &self,
        request: CreateFunctionRequest,
    ) -> Result<FunctionConfiguration> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize CreateFunctionRequest for function '{}'",
                    request.function_name
                ),
            },
        )?;
        self.send_json(
            Method::POST,
            "/2015-03-31/functions",
            None,
            Some(body),
            "CreateFunction",
            &request.function_name,
        )
        .await
    }

    async fn create_function_url_config(
        &self,
        function_name: &str,
        request: CreateFunctionUrlConfigRequest,
    ) -> Result<CreateFunctionUrlConfigResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize CreateFunctionUrlConfigRequest for function '{}'",
                    function_name
                ),
            },
        )?;
        let path = format!("/2021-10-31/functions/{}/url", function_name);
        self.send_json(
            Method::POST,
            &path,
            None,
            Some(body),
            "CreateFunctionUrlConfig",
            function_name,
        )
        .await
    }

    async fn add_permission(
        &self,
        function_name: &str,
        request: AddPermissionRequest,
    ) -> Result<AddPermissionResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize AddPermissionRequest for function '{}'",
                    function_name
                ),
            },
        )?;
        let path = format!("/2015-03-31/functions/{}/policy", function_name);
        self.send_json(
            Method::POST,
            &path,
            None,
            Some(body),
            "AddPermission",
            function_name,
        )
        .await
    }

    async fn update_function_code(
        &self,
        function_name: &str,
        request: UpdateFunctionCodeRequest,
    ) -> Result<FunctionConfiguration> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize UpdateFunctionCodeRequest for function '{}'",
                    function_name
                ),
            },
        )?;
        let path = format!("/2015-03-31/functions/{}/code", function_name);
        self.send_json(
            Method::PUT,
            &path,
            None,
            Some(body),
            "UpdateFunctionCode",
            function_name,
        )
        .await
    }

    async fn update_function_configuration(
        &self,
        function_name: &str,
        request: UpdateFunctionConfigurationRequest,
    ) -> Result<FunctionConfiguration> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize UpdateFunctionConfigurationRequest for function '{}'",
                    function_name
                ),
            },
        )?;
        let path = format!("/2015-03-31/functions/{}/configuration", function_name);
        self.send_json(
            Method::PUT,
            &path,
            None,
            Some(body),
            "UpdateFunctionConfiguration",
            function_name,
        )
        .await
    }

    async fn get_function_configuration(
        &self,
        function_name: &str,
        qualifier: Option<String>,
    ) -> Result<FunctionConfiguration> {
        let path = format!("/2015-03-31/functions/{}", function_name);
        let mut qp = Vec::new();
        if let Some(q) = qualifier {
            qp.push(("Qualifier", q));
        }
        let resp: GetFunctionResponse = self
            .send_json(
                Method::GET,
                &path,
                Some(qp),
                None,
                "GetFunctionConfiguration",
                function_name,
            )
            .await?;
        Ok(resp.configuration)
    }

    async fn delete_function_url_config(
        &self,
        function_name: &str,
        qualifier: Option<String>,
    ) -> Result<()> {
        let path = format!("/2021-10-31/functions/{}/url", function_name);
        let mut qp = Vec::new();
        if let Some(q) = qualifier {
            qp.push(("Qualifier", q));
        }
        self.send_no_body(
            Method::DELETE,
            &path,
            if qp.is_empty() { None } else { Some(qp) },
            "DeleteFunctionUrlConfig",
            function_name,
        )
        .await
    }

    async fn get_function_url_config(
        &self,
        function_name: &str,
        qualifier: Option<String>,
    ) -> Result<FunctionUrlConfig> {
        let path = format!("/2021-10-31/functions/{}/url", function_name);
        let mut qp = Vec::new();
        if let Some(q) = qualifier {
            qp.push(("Qualifier", q));
        }
        self.send_json(
            Method::GET,
            &path,
            if qp.is_empty() { None } else { Some(qp) },
            None,
            "GetFunctionUrlConfig",
            function_name,
        )
        .await
    }

    async fn delete_function(&self, function_name: &str, qualifier: Option<String>) -> Result<()> {
        let path = format!("/2015-03-31/functions/{}", function_name);
        let mut qp = Vec::new();
        if let Some(q) = qualifier {
            qp.push(("Qualifier", q));
        }
        self.send_no_body(
            Method::DELETE,
            &path,
            if qp.is_empty() { None } else { Some(qp) },
            "DeleteFunction",
            function_name,
        )
        .await
    }

    async fn get_policy(
        &self,
        function_name: &str,
        qualifier: Option<String>,
    ) -> Result<GetPolicyResponse> {
        let path = format!("/2015-03-31/functions/{}/policy", function_name);
        let mut qp = Vec::new();
        if let Some(q) = qualifier {
            qp.push(("Qualifier", q));
        }
        self.send_json(
            Method::GET,
            &path,
            if qp.is_empty() { None } else { Some(qp) },
            None,
            "GetPolicy",
            function_name,
        )
        .await
    }

    async fn invoke(&self, request: InvokeRequest) -> Result<InvokeResponse> {
        self.credentials.ensure_fresh().await?;
        let function_name = &request.function_name;
        let path = format!("/2015-03-31/functions/{}/invocations", function_name);

        // Build URL with query parameters
        let base_url = self.get_base_url();
        let mut url = format!("{}{}", base_url.trim_end_matches('/'), path);

        if let Some(ref qualifier) = request.qualifier {
            url.push('?');
            url.push_str(&format!(
                "Qualifier={}",
                form_urlencoded::byte_serialize(qualifier.as_bytes()).collect::<String>()
            ));
        }

        let mut builder = self.client.request(Method::POST, &url).host(&format!(
            "lambda.{}.amazonaws.com",
            self.credentials.region()
        ));

        // Set invocation type header
        match request.invocation_type {
            InvocationType::RequestResponse => {
                builder = builder.header("X-Amz-Invocation-Type", "RequestResponse");
            }
            InvocationType::Event => {
                builder = builder.header("X-Amz-Invocation-Type", "Event");
            }
            InvocationType::DryRun => {
                builder = builder.header("X-Amz-Invocation-Type", "DryRun");
            }
        }

        // Set log type header if specified
        if let Some(ref log_type) = request.log_type {
            builder = builder.header("X-Amz-Log-Type", log_type);
        }

        // Set client context if specified
        if let Some(ref client_context) = request.client_context {
            builder = builder.header("X-Amz-Client-Context", client_context);
        }

        // Set payload as binary data (not JSON)
        builder = builder.content_sha256_bytes(&request.payload);
        if !request.payload.is_empty() {
            builder = builder.body(reqwest::Body::from(request.payload.clone()));
        }

        let signed_builder = builder.sign_aws_request(&self.sign_config())?;
        let response = signed_builder.send().await.into_alien_error().context(
            ErrorData::HttpRequestFailed {
                message: format!("Failed to invoke Lambda function '{}'", function_name),
            },
        )?;

        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let payload =
            response
                .bytes()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!(
                        "Failed to read invoke response body for function '{}'",
                        function_name
                    ),
                })?;

        // Parse function error from headers
        let function_error = headers
            .get("X-Amz-Function-Error")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let log_result = headers
            .get("X-Amz-Log-Result")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let executed_version = headers
            .get("X-Amz-Executed-Version")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        Ok(InvokeResponse {
            status_code: status,
            function_error,
            log_result,
            payload: payload.to_vec(),
            executed_version,
        })
    }

    async fn create_event_source_mapping(
        &self,
        request: CreateEventSourceMappingRequest,
    ) -> Result<EventSourceMapping> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize CreateEventSourceMappingRequest for function '{}'",
                    request.function_name
                ),
            },
        )?;
        self.send_json(
            Method::POST,
            "/2015-03-31/event-source-mappings",
            None,
            Some(body),
            "CreateEventSourceMapping",
            &request.function_name,
        )
        .await
    }

    async fn get_event_source_mapping(&self, uuid: &str) -> Result<EventSourceMapping> {
        let path = format!("/2015-03-31/event-source-mappings/{}", uuid);
        self.send_json(
            Method::GET,
            &path,
            None,
            None,
            "GetEventSourceMapping",
            uuid,
        )
        .await
    }

    async fn update_event_source_mapping(
        &self,
        uuid: &str,
        request: UpdateEventSourceMappingRequest,
    ) -> Result<EventSourceMapping> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize UpdateEventSourceMappingRequest for UUID '{}'",
                    uuid
                ),
            },
        )?;
        let path = format!("/2015-03-31/event-source-mappings/{}", uuid);
        self.send_json(
            Method::PUT,
            &path,
            None,
            Some(body),
            "UpdateEventSourceMapping",
            uuid,
        )
        .await
    }

    async fn delete_event_source_mapping(&self, uuid: &str) -> Result<EventSourceMapping> {
        let path = format!("/2015-03-31/event-source-mappings/{}", uuid);
        self.send_json(
            Method::DELETE,
            &path,
            None,
            None,
            "DeleteEventSourceMapping",
            uuid,
        )
        .await
    }

    async fn list_event_source_mappings(
        &self,
        request: ListEventSourceMappingsRequest,
    ) -> Result<ListEventSourceMappingsResponse> {
        let mut qp = Vec::new();
        if let Some(ref arn) = request.event_source_arn {
            qp.push(("EventSourceArn", arn.clone()));
        }
        if let Some(ref func) = request.function_name {
            qp.push(("FunctionName", func.clone()));
        }
        if let Some(ref marker) = request.marker {
            qp.push(("Marker", marker.clone()));
        }
        if let Some(max_items) = request.max_items {
            qp.push(("MaxItems", max_items.to_string()));
        }

        let resource_name = request.function_name.as_deref().unwrap_or("unknown");
        self.send_json(
            Method::GET,
            "/2015-03-31/event-source-mappings",
            if qp.is_empty() { None } else { Some(qp) },
            None,
            "ListEventSourceMappings",
            resource_name,
        )
        .await
    }

    async fn put_function_concurrency(
        &self,
        function_name: &str,
        reserved_concurrent_executions: u32,
    ) -> Result<()> {
        let body =
            serde_json::json!({ "ReservedConcurrentExecutions": reserved_concurrent_executions })
                .to_string();
        let path = format!("/2017-10-31/functions/{}/concurrency", function_name);
        let _: serde_json::Value = self
            .send_json(
                Method::PUT,
                &path,
                None,
                Some(body),
                "PutFunctionConcurrency",
                function_name,
            )
            .await?;
        Ok(())
    }

    async fn delete_function_concurrency(&self, function_name: &str) -> Result<()> {
        let path = format!("/2017-10-31/functions/{}/concurrency", function_name);
        self.send_no_body(
            Method::DELETE,
            &path,
            None,
            "DeleteFunctionConcurrency",
            function_name,
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// Error JSON mapping structs
// ---------------------------------------------------------------------------
#[derive(Debug, Deserialize)]
struct LambdaErrorResponse {
    #[serde(rename = "Type")]
    type_field: Option<String>,
    #[serde(rename = "__type")]
    type_field_underscore: Option<String>,
    #[serde(rename = "message")]
    message: Option<String>,
    #[serde(rename = "Message")]
    message_capital: Option<String>,
    #[serde(rename = "Error")]
    error: Option<LambdaErrorDetails>,
}

#[derive(Debug, Deserialize)]
struct LambdaErrorDetails {
    #[serde(rename = "Code")]
    code: Option<String>,
    #[serde(rename = "Message")]
    message: Option<String>,
}

// ---------------------------------------------------------------------------
// Request / response payloads (subset required by infra/tests)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct CreateFunctionRequest {
    pub function_name: String,
    pub role: String,
    pub code: FunctionCode,
    #[builder(default = "Image".to_string())]
    pub package_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_size: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publish: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<Environment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub architectures: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracing_config: Option<TracingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<std::collections::HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ephemeral_storage: Option<EphemeralStorage>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "KMSKeyArn")]
    pub kms_key_arn: Option<String>,
    /// VPC configuration for running the function inside a VPC
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vpc_config: Option<VpcConfig>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct FunctionCode {
    pub image_uri: Option<String>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct Environment {
    pub variables: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct FunctionConfiguration {
    pub function_name: Option<String>,
    pub function_arn: Option<String>,
    pub state: Option<String>,
    pub last_update_status: Option<String>,
    #[serde(rename = "KMSKeyArn")]
    pub kms_key_arn: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetFunctionResponse {
    pub configuration: FunctionConfiguration,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct CreateFunctionUrlConfigRequest {
    pub auth_type: String,
    pub cors: Option<Cors>,
    pub invoke_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateFunctionUrlConfigResponse {
    pub function_url: String,
    pub function_arn: String,
    pub auth_type: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct FunctionUrlConfig {
    pub function_url: String,
    pub auth_type: String,
    pub cors: Option<Cors>,
}

#[derive(Debug, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct AddPermissionRequest {
    pub statement_id: String,
    pub action: String,
    pub principal: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_url_auth_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_arn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_account: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AddPermissionResponse {
    pub statement: Option<String>,
}

#[derive(Debug, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct UpdateFunctionCodeRequest {
    pub image_uri: String,
    pub publish: Option<bool>,
}

#[derive(Debug, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct UpdateFunctionConfigurationRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_size: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<Environment>,
    /// VPC configuration for running the function inside a VPC
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vpc_config: Option<VpcConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetPolicyResponse {
    pub policy: Option<String>,
}

// ---------------------------------------------------------------------------
// Supporting structs newly added for test compilation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct TracingConfig {
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct EphemeralStorage {
    pub size: i32,
}

/// VPC configuration for Lambda function.
///
/// When configured, the Lambda function runs inside the specified VPC with access
/// to the specified subnets and security groups.
#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct VpcConfig {
    /// A list of VPC subnet IDs where the function will run.
    /// Lambda creates an elastic network interface for each subnet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnet_ids: Option<Vec<String>>,

    /// A list of VPC security group IDs for the function's network interfaces.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_group_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct Cors {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_credentials: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_headers: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_methods: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_origins: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_age: Option<i32>,
}

// ---------------------------------------------------------------------------
// Event Source Mapping structs
// ---------------------------------------------------------------------------

/// Request to create an event source mapping
#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct CreateEventSourceMappingRequest {
    /// The Amazon Resource Name (ARN) of the event source (e.g., SQS queue)
    pub event_source_arn: String,

    /// The name or ARN of the Lambda function
    pub function_name: String,

    /// The maximum number of records in each batch that Lambda pulls from your queue and sends to your function
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch_size: Option<i32>,

    /// When true, the event source mapping is active. When false, Lambda pauses polling and invocation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    /// The maximum amount of time, in seconds, that Lambda spends gathering records before invoking the function
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_batching_window_in_seconds: Option<i32>,

    /// A list of current response type enums applied to the event source mapping
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_response_types: Option<Vec<String>>,

    /// An object that defines the filter criteria that determine whether Lambda should process an event
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_criteria: Option<FilterCriteria>,

    /// The maximum concurrency setting limits the number of concurrent instances of the function that an Amazon SQS event source can invoke
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scaling_config: Option<ScalingConfig>,
}

/// Request to update an event source mapping
#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct UpdateEventSourceMappingRequest {
    /// The maximum number of records in each batch that Lambda pulls from your queue and sends to your function
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch_size: Option<i32>,

    /// When true, the event source mapping is active. When false, Lambda pauses polling and invocation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    /// The name or ARN of the Lambda function
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_name: Option<String>,

    /// The maximum amount of time, in seconds, that Lambda spends gathering records before invoking the function
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_batching_window_in_seconds: Option<i32>,

    /// A list of current response type enums applied to the event source mapping
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_response_types: Option<Vec<String>>,

    /// An object that defines the filter criteria that determine whether Lambda should process an event
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_criteria: Option<FilterCriteria>,

    /// The maximum concurrency setting limits the number of concurrent instances of the function that an Amazon SQS event source can invoke
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scaling_config: Option<ScalingConfig>,
}

/// Request to list event source mappings
#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct ListEventSourceMappingsRequest {
    /// The Amazon Resource Name (ARN) of the event source
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_source_arn: Option<String>,

    /// The name or ARN of the Lambda function
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_name: Option<String>,

    /// A pagination token returned by a previous call
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,

    /// The maximum number of event source mappings to return
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_items: Option<i32>,
}

/// Response from listing event source mappings
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ListEventSourceMappingsResponse {
    /// A list of event source mappings
    pub event_source_mappings: Option<Vec<EventSourceMapping>>,

    /// A pagination token that's returned when the response doesn't contain all event source mappings
    pub next_marker: Option<String>,
}

/// Represents an event source mapping
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct EventSourceMapping {
    /// The identifier of the event source mapping
    #[serde(rename = "UUID")]
    pub uuid: Option<String>,

    /// The Amazon Resource Name (ARN) of the event source
    pub event_source_arn: Option<String>,

    /// The ARN of the Lambda function
    pub function_arn: Option<String>,

    /// The maximum number of records in each batch that Lambda pulls from your queue and sends to your function
    pub batch_size: Option<i32>,

    /// The date that the event source mapping was last updated or that its state changed
    pub last_modified: Option<f64>,

    /// The result of the last AWS Lambda invocation of your Lambda function
    pub last_processing_result: Option<String>,

    /// The state of the event source mapping
    pub state: Option<String>,

    /// Indicates whether a user or Lambda made the last change to the event source mapping
    pub state_transition_reason: Option<String>,

    /// The maximum amount of time, in seconds, that Lambda spends gathering records before invoking the function
    pub maximum_batching_window_in_seconds: Option<i32>,

    /// A list of current response type enums applied to the event source mapping
    pub function_response_types: Option<Vec<String>>,

    /// An object that defines the filter criteria that determine whether Lambda should process an event
    pub filter_criteria: Option<FilterCriteria>,

    /// The maximum concurrency setting limits the number of concurrent instances of the function that an Amazon SQS event source can invoke
    pub scaling_config: Option<ScalingConfig>,
}

/// Filter criteria for event source mapping
#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct FilterCriteria {
    /// A list of filters to apply to the event
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
}

/// Individual filter in filter criteria
#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct Filter {
    /// A filter pattern
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
}

/// Scaling configuration for SQS event source mapping
#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct ScalingConfig {
    /// The maximum number of concurrent instances of the function that an Amazon SQS event source can invoke
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_concurrency: Option<i32>,
}

// ---------------------------------------------------------------------------
// Lambda invocation types and structs
// ---------------------------------------------------------------------------

/// Lambda invocation type
#[derive(Debug, Clone)]
pub enum InvocationType {
    /// Synchronous invocation
    RequestResponse,
    /// Asynchronous invocation
    Event,
    /// Dry run validation
    DryRun,
}

impl Default for InvocationType {
    fn default() -> Self {
        InvocationType::RequestResponse
    }
}

/// Request for invoking a Lambda function
#[derive(Debug, Clone, Builder)]
pub struct InvokeRequest {
    /// The name of the Lambda function
    pub function_name: String,
    /// The invocation type
    #[builder(default)]
    pub invocation_type: InvocationType,
    /// For versioned functions, the version to invoke
    pub qualifier: Option<String>,
    /// Up to 3,583 bytes of base64-encoded data about the invoking client to pass to the function
    pub client_context: Option<String>,
    /// Set to Tail to include the execution log in the response
    pub log_type: Option<String>,
    /// The JSON that you want to provide to your Lambda function as input
    #[builder(default)]
    pub payload: Vec<u8>,
}

/// Response from invoking a Lambda function
#[derive(Debug, Clone)]
pub struct InvokeResponse {
    /// The HTTP status code for the invoke response
    pub status_code: u16,
    /// If present, indicates that an error occurred during function execution
    pub function_error: Option<String>,
    /// The last 4 KB of the execution log (only if you set log_type to Tail)
    pub log_result: Option<String>,
    /// The response from the function
    pub payload: Vec<u8>,
    /// The version of the function that was executed
    pub executed_version: Option<String>,
}
