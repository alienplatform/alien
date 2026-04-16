//! AWS API Gateway V2 (HTTP API) Client
//!
//! Minimal HTTP API operations needed for custom domains and Lambda integrations.

use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, ContextError, IntoAlienError};
use async_trait::async_trait;
use bon::Builder;
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[cfg(feature = "test-utils")]
use mockall::automock;

// ---------------------------------------------------------------------------
// API Gateway Error Response Parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ApiGatewayErrorResponse {
    pub message: Option<String>,
    pub code: Option<String>,
    #[serde(rename = "__type")]
    pub type_field: Option<String>,
}

// ---------------------------------------------------------------------------
// API Gateway V2 API Trait
// ---------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait ApiGatewayV2Api: Send + Sync + std::fmt::Debug {
    async fn create_api(&self, request: CreateApiRequest) -> Result<Api>;
    async fn get_api(&self, api_id: &str) -> Result<Api>;
    async fn delete_api(&self, api_id: &str) -> Result<()>;

    async fn create_integration(
        &self,
        api_id: &str,
        request: CreateIntegrationRequest,
    ) -> Result<Integration>;
    async fn create_route(&self, api_id: &str, request: CreateRouteRequest) -> Result<Route>;
    async fn create_stage(&self, api_id: &str, request: CreateStageRequest) -> Result<Stage>;

    async fn create_domain_name(&self, request: CreateDomainNameRequest) -> Result<DomainName>;
    async fn get_domain_name(&self, domain_name: &str) -> Result<DomainName>;
    async fn delete_domain_name(&self, domain_name: &str) -> Result<()>;

    async fn create_api_mapping(
        &self,
        domain_name: &str,
        request: CreateApiMappingRequest,
    ) -> Result<ApiMapping>;
    async fn get_api_mappings(&self, domain_name: &str) -> Result<GetApiMappingsResponse>;
    async fn delete_api_mapping(&self, domain_name: &str, api_mapping_id: &str) -> Result<()>;
}

// ---------------------------------------------------------------------------
// API Gateway V2 Client
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ApiGatewayV2Client {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl ApiGatewayV2Client {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
        }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "apigateway".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self.credentials.get_service_endpoint_option("apigateway") {
            override_url.to_string()
        } else {
            format!(
                "https://apigateway.{}.amazonaws.com",
                self.credentials.region()
            )
        }
    }

    async fn send_json<T: DeserializeOwned + Send + 'static>(
        &self,
        method: Method,
        path: &str,
        body: Option<String>,
        operation: &str,
        resource: &str,
    ) -> Result<T> {
        self.credentials.ensure_fresh().await?;
        let base_url = self.get_base_url();
        let url = format!("{}{}", base_url.trim_end_matches('/'), path);

        let mut builder = self
            .client
            .request(method, &url)
            .host(&format!(
                "apigateway.{}.amazonaws.com",
                self.credentials.region()
            ))
            .content_type_json();

        if let Some(body) = body {
            builder = builder.content_sha256(&body).body(body.clone());
            let result =
                crate::aws::aws_request_utils::sign_send_json(builder, &self.sign_config()).await;
            return Self::map_result(result, operation, resource, Some(&body));
        }

        builder = builder.content_sha256("");
        let result =
            crate::aws::aws_request_utils::sign_send_json(builder, &self.sign_config()).await;
        Self::map_result(result, operation, resource, None)
    }

    async fn send_no_response(
        &self,
        method: Method,
        path: &str,
        operation: &str,
        resource: &str,
    ) -> Result<()> {
        self.credentials.ensure_fresh().await?;
        let base_url = self.get_base_url();
        let url = format!("{}{}", base_url.trim_end_matches('/'), path);

        let builder = self
            .client
            .request(method, &url)
            .host(&format!(
                "apigateway.{}.amazonaws.com",
                self.credentials.region()
            ))
            .content_type_json()
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
                        Self::map_apigw_error(status, text, operation, resource, request_body)
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

    fn map_apigw_error(
        status: StatusCode,
        body: &str,
        _operation: &str,
        resource: &str,
        request_body: Option<&str>,
    ) -> Option<ErrorData> {
        let parsed: std::result::Result<ApiGatewayErrorResponse, _> = serde_json::from_str(body);
        let (code, message) = match parsed {
            Ok(e) => {
                let code = e
                    .type_field
                    .or(e.code)
                    .unwrap_or_else(|| "UnknownError".into());
                let message = e.message.unwrap_or_else(|| "Unknown error".into());
                (code, message)
            }
            Err(_) => return None,
        };

        Some(match code.as_str() {
            "AccessDeniedException" | "UnauthorizedException" => ErrorData::RemoteAccessDenied {
                resource_type: "ApiGateway".into(),
                resource_name: resource.into(),
            },
            "NotFoundException" => ErrorData::RemoteResourceNotFound {
                resource_type: "ApiGateway".into(),
                resource_name: resource.into(),
            },
            "TooManyRequestsException" | "ThrottlingException" => {
                ErrorData::RateLimitExceeded { message }
            }
            "BadRequestException" | "ValidationException" => ErrorData::InvalidInput {
                message,
                field_name: None,
            },
            _ => match status {
                StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
                    resource_type: "ApiGateway".into(),
                    resource_name: resource.into(),
                },
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
                    resource_type: "ApiGateway".into(),
                    resource_name: resource.into(),
                },
                StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded { message },
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => ErrorData::RemoteServiceUnavailable { message },
                _ => ErrorData::HttpResponseError {
                    message: format!("ApiGateway operation failed: {}", message),
                    url: format!("apigateway.amazonaws.com"),
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
impl ApiGatewayV2Api for ApiGatewayV2Client {
    async fn create_api(&self, request: CreateApiRequest) -> Result<Api> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: "Failed to serialize CreateApiRequest".to_string(),
            },
        )?;
        self.send_json(
            Method::POST,
            "/v2/apis",
            Some(body),
            "CreateApi",
            &request.name,
        )
        .await
    }

    async fn get_api(&self, api_id: &str) -> Result<Api> {
        let path = format!("/v2/apis/{}", api_id);
        self.send_json(Method::GET, &path, None, "GetApi", api_id)
            .await
    }

    async fn delete_api(&self, api_id: &str) -> Result<()> {
        let path = format!("/v2/apis/{}", api_id);
        self.send_no_response(Method::DELETE, &path, "DeleteApi", api_id)
            .await
    }

    async fn create_integration(
        &self,
        api_id: &str,
        request: CreateIntegrationRequest,
    ) -> Result<Integration> {
        let path = format!("/v2/apis/{}/integrations", api_id);
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: "Failed to serialize CreateIntegrationRequest".to_string(),
            },
        )?;
        self.send_json(Method::POST, &path, Some(body), "CreateIntegration", api_id)
            .await
    }

    async fn create_route(&self, api_id: &str, request: CreateRouteRequest) -> Result<Route> {
        let path = format!("/v2/apis/{}/routes", api_id);
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: "Failed to serialize CreateRouteRequest".to_string(),
            },
        )?;
        self.send_json(Method::POST, &path, Some(body), "CreateRoute", api_id)
            .await
    }

    async fn create_stage(&self, api_id: &str, request: CreateStageRequest) -> Result<Stage> {
        let path = format!("/v2/apis/{}/stages", api_id);
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: "Failed to serialize CreateStageRequest".to_string(),
            },
        )?;
        self.send_json(Method::POST, &path, Some(body), "CreateStage", api_id)
            .await
    }

    async fn create_domain_name(&self, request: CreateDomainNameRequest) -> Result<DomainName> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: "Failed to serialize CreateDomainNameRequest".to_string(),
            },
        )?;
        self.send_json(
            Method::POST,
            "/v2/domainnames",
            Some(body),
            "CreateDomainName",
            &request.domain_name,
        )
        .await
    }

    async fn get_domain_name(&self, domain_name: &str) -> Result<DomainName> {
        let path = format!("/v2/domainnames/{}", domain_name);
        self.send_json(Method::GET, &path, None, "GetDomainName", domain_name)
            .await
    }

    async fn delete_domain_name(&self, domain_name: &str) -> Result<()> {
        let path = format!("/v2/domainnames/{}", domain_name);
        self.send_no_response(Method::DELETE, &path, "DeleteDomainName", domain_name)
            .await
    }

    async fn create_api_mapping(
        &self,
        domain_name: &str,
        request: CreateApiMappingRequest,
    ) -> Result<ApiMapping> {
        let path = format!("/v2/domainnames/{}/apimappings", domain_name);
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: "Failed to serialize CreateApiMappingRequest".to_string(),
            },
        )?;
        self.send_json(
            Method::POST,
            &path,
            Some(body),
            "CreateApiMapping",
            domain_name,
        )
        .await
    }

    async fn get_api_mappings(&self, domain_name: &str) -> Result<GetApiMappingsResponse> {
        let path = format!("/v2/domainnames/{}/apimappings", domain_name);
        self.send_json(Method::GET, &path, None, "GetApiMappings", domain_name)
            .await
    }

    async fn delete_api_mapping(&self, domain_name: &str, api_mapping_id: &str) -> Result<()> {
        let path = format!(
            "/v2/domainnames/{}/apimappings/{}",
            domain_name, api_mapping_id
        );
        self.send_no_response(Method::DELETE, &path, "DeleteApiMapping", domain_name)
            .await
    }
}

// ---------------------------------------------------------------------------
// API Gateway V2 Request/Response Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateApiRequest {
    pub name: String,
    pub protocol_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Api {
    pub api_id: Option<String>,
    pub api_endpoint: Option<String>,
    pub name: Option<String>,
    pub protocol_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateIntegrationRequest {
    pub integration_type: String,
    pub integration_uri: String,
    pub payload_format_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Integration {
    pub integration_id: Option<String>,
    pub integration_type: Option<String>,
    pub integration_uri: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateRouteRequest {
    pub route_key: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    pub route_id: Option<String>,
    pub route_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateStageRequest {
    pub stage_name: String,
    pub auto_deploy: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stage {
    pub stage_name: Option<String>,
    pub auto_deploy: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateDomainNameRequest {
    pub domain_name: String,
    pub domain_name_configurations: Vec<DomainNameConfiguration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainName {
    pub domain_name: Option<String>,
    pub domain_name_configurations: Option<Vec<DomainNameConfiguration>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct DomainNameConfiguration {
    pub certificate_arn: String,
    pub endpoint_type: String,
    pub security_policy: String,
    pub api_gateway_domain_name: Option<String>,
    pub hosted_zone_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateApiMappingRequest {
    pub api_id: String,
    pub stage: String,
    pub api_mapping_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiMapping {
    pub api_mapping_id: Option<String>,
    pub api_mapping_key: Option<String>,
    pub stage: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetApiMappingsResponse {
    pub items: Option<Vec<ApiMapping>>,
    pub next_token: Option<String>,
}
