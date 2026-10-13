//! AWS API Gateway V1 (REST API) Client
//!
//! Minimal REST API control-plane operations for a streaming Lambda proxy with a
//! REGIONAL or PRIVATE custom domain. REST V1 (unlike the V2 HTTP API) supports response
//! streaming via `responseTransferMode: STREAM`, which is why a streaming worker
//! that needs a custom domain is served here rather than through a Function URL.

use crate::aws::aws_request_utils::{
    is_json_throttling, is_transient_json_error, sign_send_json_retrying_when,
    AwsRequestBuilderExt, AwsSignConfig,
};
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

#[derive(Debug, Deserialize)]
struct ApiGatewayErrorResponse {
    pub message: Option<String>,
    pub code: Option<String>,
    #[serde(rename = "__type")]
    pub type_field: Option<String>,
}

// ---------------------------------------------------------------------------
// API Gateway V1 (REST) Trait
// ---------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait ApiGatewayApi: Send + Sync + std::fmt::Debug {
    async fn create_rest_api(&self, request: CreateRestApiRequest) -> Result<RestApi>;
    async fn create_private_rest_api(
        &self,
        request: CreatePrivateRestApiRequest,
    ) -> Result<RestApi>;
    async fn get_rest_api(&self, rest_api_id: &str) -> Result<RestApi>;
    async fn get_rest_api_details(&self, rest_api_id: &str) -> Result<RestApiDetails>;
    async fn get_rest_apis(&self, position: Option<String>) -> Result<RestApisPage>;
    /// Account-owned domains, including public entries without a domainNameId.
    async fn get_domain_names(&self, position: Option<String>) -> Result<DomainNamesPage>;
    async fn delete_rest_api(&self, rest_api_id: &str) -> Result<()>;
    /// Replace the invocation policy after the owned endpoint identity is known.
    async fn update_rest_api_policy(
        &self,
        rest_api_id: &str,
        policy: &str,
    ) -> Result<RestApiDetails>;
    async fn get_resources(
        &self,
        rest_api_id: &str,
        position: Option<String>,
    ) -> Result<ResourcesPage>;
    async fn get_method(
        &self,
        rest_api_id: &str,
        resource_id: &str,
        http_method: &str,
    ) -> Result<RestMethod>;
    async fn get_stage(&self, rest_api_id: &str, stage_name: &str) -> Result<RestStage>;

    async fn create_resource(
        &self,
        rest_api_id: &str,
        parent_id: &str,
        request: CreateResourceRequest,
    ) -> Result<Resource>;
    async fn put_method(
        &self,
        rest_api_id: &str,
        resource_id: &str,
        http_method: &str,
        request: PutMethodRequest,
    ) -> Result<()>;
    async fn put_integration(
        &self,
        rest_api_id: &str,
        resource_id: &str,
        http_method: &str,
        request: PutIntegrationRequest,
    ) -> Result<()>;
    async fn create_deployment(
        &self,
        rest_api_id: &str,
        request: CreateDeploymentRequest,
    ) -> Result<Deployment>;

    async fn create_domain_name(&self, request: CreateDomainNameRequest) -> Result<DomainName>;
    async fn get_domain_name(&self, domain_name: &str) -> Result<DomainName>;
    async fn delete_domain_name(&self, domain_name: &str) -> Result<()>;

    /// Private domains require their provider ID even when the hostname is unique.
    async fn create_private_domain_name(
        &self,
        request: CreatePrivateDomainNameRequest,
    ) -> Result<PrivateDomainName>;
    async fn get_private_domain_name(
        &self,
        domain_name: &str,
        domain_name_id: &str,
    ) -> Result<PrivateDomainName>;
    async fn delete_private_domain_name(
        &self,
        domain_name: &str,
        domain_name_id: &str,
    ) -> Result<()>;
    async fn update_private_domain_certificate(
        &self,
        domain_name: &str,
        domain_name_id: &str,
        certificate_arn: &str,
    ) -> Result<PrivateDomainName>;
    async fn create_private_base_path_mapping(
        &self,
        domain_name: &str,
        domain_name_id: &str,
        request: CreateBasePathMappingRequest,
    ) -> Result<BasePathMapping>;
    async fn delete_private_base_path_mapping(
        &self,
        domain_name: &str,
        domain_name_id: &str,
        base_path: &str,
    ) -> Result<()>;

    async fn create_domain_name_access_association(
        &self,
        request: CreateDomainNameAccessAssociationRequest,
    ) -> Result<DomainNameAccessAssociation>;
    /// One account-owned page; callers must persist/consume position for complete discovery.
    async fn get_domain_name_access_associations(
        &self,
        position: Option<String>,
    ) -> Result<DomainNameAccessAssociationsPage>;
    async fn delete_domain_name_access_association(&self, association_arn: &str) -> Result<()>;

    async fn create_base_path_mapping(
        &self,
        domain_name: &str,
        request: CreateBasePathMappingRequest,
    ) -> Result<BasePathMapping>;
    async fn delete_base_path_mapping(&self, domain_name: &str, base_path: &str) -> Result<()>;
    /// Tags any REST API object by ARN. A stage is created as a side effect of
    /// `CreateDeployment`, which takes no tags, so it must be tagged after the
    /// fact to carry the deployment's boundary tags.
    async fn tag_resource(
        &self,
        resource_arn: &str,
        tags: std::collections::HashMap<String, String>,
    ) -> Result<()>;
}

// ---------------------------------------------------------------------------
// API Gateway V1 Client
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ApiGatewayClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl ApiGatewayClient {
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

    fn host(&self) -> String {
        format!("apigateway.{}.amazonaws.com", self.credentials.region())
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self.credentials.get_service_endpoint_option("apigateway") {
            override_url.to_string()
        } else {
            format!("https://{}", self.host())
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
        // A create can commit before a 5xx/transport error. Let the durable
        // controller rediscover its attempt instead of blindly sending again.
        let retry_when: fn(&alien_error::AlienError<ErrorData>) -> bool = if method == Method::POST
        {
            is_json_throttling
        } else {
            is_transient_json_error
        };

        let mut builder = self
            .client
            .request(method, &url)
            .host(&self.host())
            .header("Accept", "application/json")
            .content_type_json();

        if let Some(body) = body {
            builder = builder.content_sha256(&body).body(body.clone());
            let result =
                sign_send_json_retrying_when(builder, &self.sign_config(), retry_when).await;
            return Self::map_result(result, operation, resource, Some(&body));
        }

        builder = builder.content_sha256("");
        let result = sign_send_json_retrying_when(builder, &self.sign_config(), retry_when).await;
        Self::map_result(result, operation, resource, None)
    }

    async fn send_no_response(
        &self,
        method: Method,
        path: &str,
        body: Option<String>,
        operation: &str,
        resource: &str,
    ) -> Result<()> {
        self.credentials.ensure_fresh().await?;
        let base_url = self.get_base_url();
        let url = format!("{}{}", base_url.trim_end_matches('/'), path);

        let mut builder = self
            .client
            .request(method, &url)
            .host(&self.host())
            .header("Accept", "application/json")
            .content_type_json();

        let result = if let Some(body) = body {
            builder = builder.content_sha256(&body).body(body.clone());
            crate::aws::aws_request_utils::sign_send_no_response_retrying_when(
                builder,
                &self.sign_config(),
                crate::aws::aws_request_utils::is_transient_json_error,
            )
            .await
        } else {
            builder = builder.content_sha256("");
            crate::aws::aws_request_utils::sign_send_no_response_retrying_when(
                builder,
                &self.sign_config(),
                crate::aws::aws_request_utils::is_transient_json_error,
            )
            .await
        };
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
        operation: &str,
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
            "ConflictException" => ErrorData::RemoteResourceConflict {
                resource_type: "ApiGateway".into(),
                resource_name: resource.into(),
                message: format!("{operation}: {message}"),
            },
            "TooManyRequestsException" | "ThrottlingException" => ErrorData::RateLimitExceeded {
                message: format!("{operation}: {message}"),
            },
            "BadRequestException" | "ValidationException" => ErrorData::InvalidInput {
                message: format!("{operation}: {message}"),
                field_name: None,
            },
            _ => match status {
                StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
                    resource_type: "ApiGateway".into(),
                    resource_name: resource.into(),
                },
                StatusCode::CONFLICT => ErrorData::RemoteResourceConflict {
                    resource_type: "ApiGateway".into(),
                    resource_name: resource.into(),
                    message: format!("{operation}: {message}"),
                },
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
                    resource_type: "ApiGateway".into(),
                    resource_name: resource.into(),
                },
                StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded {
                    message: format!("{operation}: {message}"),
                },
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => ErrorData::RemoteServiceUnavailable {
                    message: format!("{operation}: {message}"),
                },
                _ => ErrorData::HttpResponseError {
                    message: format!("ApiGateway {operation} failed: {message}"),
                    url: "apigateway.amazonaws.com".to_string(),
                    http_status: status.as_u16(),
                    http_response_text: Some(body.into()),
                    http_request_text: request_body.map(|s| s.to_string()),
                },
            },
        })
    }

    fn serialize<T: Serialize>(request: &T, name: &str) -> Result<String> {
        serde_json::to_string(request)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Failed to serialize {name}"),
            })
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ApiGatewayApi for ApiGatewayClient {
    async fn create_rest_api(&self, request: CreateRestApiRequest) -> Result<RestApi> {
        let body = Self::serialize(&request, "CreateRestApiRequest")?;
        self.send_json(
            Method::POST,
            "/restapis",
            Some(body),
            "CreateRestApi",
            &request.name,
        )
        .await
    }

    async fn create_private_rest_api(
        &self,
        request: CreatePrivateRestApiRequest,
    ) -> Result<RestApi> {
        let mut body = serde_json::json!({
            "name":request.name,
            "endpointConfiguration":{"types":["PRIVATE"],"vpcEndpointIds":request.vpc_endpoint_ids},
            "policy":request.policy,
            "disableExecuteApiEndpoint":request.disable_execute_api_endpoint,
            "tags":request.tags,
        });
        if let Some(media_types) = request.binary_media_types {
            body["binaryMediaTypes"] = serde_json::json!(media_types);
        }
        let body = Self::serialize(&body, "CreatePrivateRestApiRequest")?;
        self.send_json(
            Method::POST,
            "/restapis",
            Some(body),
            "CreateRestApi",
            &request.name,
        )
        .await
    }

    async fn get_rest_api(&self, rest_api_id: &str) -> Result<RestApi> {
        let path = format!("/restapis/{}", rest_api_id);
        self.send_json(Method::GET, &path, None, "GetRestApi", rest_api_id)
            .await
    }

    async fn get_rest_api_details(&self, rest_api_id: &str) -> Result<RestApiDetails> {
        let path = format!("/restapis/{}", urlencoding::encode(rest_api_id));
        self.send_json(Method::GET, &path, None, "GetRestApi", rest_api_id)
            .await
    }

    async fn get_rest_apis(&self, position: Option<String>) -> Result<RestApisPage> {
        let path = collection_path("/restapis?limit=500", position);
        self.send_json(
            Method::GET,
            &path,
            None,
            "GetRestApis",
            "REST API collection",
        )
        .await
    }

    async fn get_domain_names(&self, position: Option<String>) -> Result<DomainNamesPage> {
        let path = collection_path("/domainnames?limit=500&resourceOwner=SELF", position);
        self.send_json(
            Method::GET,
            &path,
            None,
            "GetDomainNames",
            "account-owned domains",
        )
        .await
    }

    async fn update_rest_api_policy(
        &self,
        rest_api_id: &str,
        policy: &str,
    ) -> Result<RestApiDetails> {
        let path = format!("/restapis/{}", urlencoding::encode(rest_api_id));
        let body = Self::serialize(
            &serde_json::json!({"patchOperations": [{"op": "replace", "path": "/policy", "value": policy}]}),
            "UpdateRestApiPolicy",
        )?;
        self.send_json(
            Method::PATCH,
            &path,
            Some(body),
            "UpdateRestApi",
            rest_api_id,
        )
        .await
    }

    async fn get_resources(
        &self,
        rest_api_id: &str,
        position: Option<String>,
    ) -> Result<ResourcesPage> {
        let path = collection_path(
            &format!(
                "/restapis/{}/resources?limit=500",
                urlencoding::encode(rest_api_id)
            ),
            position,
        );
        self.send_json(Method::GET, &path, None, "GetResources", rest_api_id)
            .await
    }

    async fn get_method(
        &self,
        rest_api_id: &str,
        resource_id: &str,
        http_method: &str,
    ) -> Result<RestMethod> {
        let path = format!(
            "/restapis/{}/resources/{}/methods/{}",
            urlencoding::encode(rest_api_id),
            urlencoding::encode(resource_id),
            urlencoding::encode(http_method)
        );
        self.send_json(Method::GET, &path, None, "GetMethod", rest_api_id)
            .await
    }

    async fn get_stage(&self, rest_api_id: &str, stage_name: &str) -> Result<RestStage> {
        let path = format!(
            "/restapis/{}/stages/{}",
            urlencoding::encode(rest_api_id),
            urlencoding::encode(stage_name)
        );
        self.send_json(Method::GET, &path, None, "GetStage", rest_api_id)
            .await
    }

    async fn delete_rest_api(&self, rest_api_id: &str) -> Result<()> {
        let path = format!("/restapis/{}", rest_api_id);
        self.send_no_response(Method::DELETE, &path, None, "DeleteRestApi", rest_api_id)
            .await
    }

    async fn create_resource(
        &self,
        rest_api_id: &str,
        parent_id: &str,
        request: CreateResourceRequest,
    ) -> Result<Resource> {
        let path = format!("/restapis/{}/resources/{}", rest_api_id, parent_id);
        let body = Self::serialize(&request, "CreateResourceRequest")?;
        self.send_json(
            Method::POST,
            &path,
            Some(body),
            "CreateResource",
            rest_api_id,
        )
        .await
    }

    async fn put_method(
        &self,
        rest_api_id: &str,
        resource_id: &str,
        http_method: &str,
        request: PutMethodRequest,
    ) -> Result<()> {
        let path = format!(
            "/restapis/{}/resources/{}/methods/{}",
            rest_api_id, resource_id, http_method
        );
        let body = Self::serialize(&request, "PutMethodRequest")?;
        self.send_no_response(Method::PUT, &path, Some(body), "PutMethod", rest_api_id)
            .await
    }

    async fn put_integration(
        &self,
        rest_api_id: &str,
        resource_id: &str,
        http_method: &str,
        request: PutIntegrationRequest,
    ) -> Result<()> {
        let path = format!(
            "/restapis/{}/resources/{}/methods/{}/integration",
            rest_api_id, resource_id, http_method
        );
        let body = Self::serialize(&request, "PutIntegrationRequest")?;
        self.send_no_response(
            Method::PUT,
            &path,
            Some(body),
            "PutIntegration",
            rest_api_id,
        )
        .await
    }

    async fn create_deployment(
        &self,
        rest_api_id: &str,
        request: CreateDeploymentRequest,
    ) -> Result<Deployment> {
        let path = format!("/restapis/{}/deployments", rest_api_id);
        let body = Self::serialize(&request, "CreateDeploymentRequest")?;
        self.send_json(
            Method::POST,
            &path,
            Some(body),
            "CreateDeployment",
            rest_api_id,
        )
        .await
    }

    async fn create_domain_name(&self, request: CreateDomainNameRequest) -> Result<DomainName> {
        let body = Self::serialize(&request, "CreateDomainNameRequest")?;
        self.send_json(
            Method::POST,
            "/domainnames",
            Some(body),
            "CreateDomainName",
            &request.domain_name,
        )
        .await
    }

    async fn get_domain_name(&self, domain_name: &str) -> Result<DomainName> {
        let path = format!("/domainnames/{}", domain_name);
        self.send_json(Method::GET, &path, None, "GetDomainName", domain_name)
            .await
    }

    async fn delete_domain_name(&self, domain_name: &str) -> Result<()> {
        let path = format!("/domainnames/{}", domain_name);
        self.send_no_response(Method::DELETE, &path, None, "DeleteDomainName", domain_name)
            .await
    }

    async fn create_base_path_mapping(
        &self,
        domain_name: &str,
        request: CreateBasePathMappingRequest,
    ) -> Result<BasePathMapping> {
        let path = format!("/domainnames/{}/basepathmappings", domain_name);
        let body = Self::serialize(&request, "CreateBasePathMappingRequest")?;
        self.send_json(
            Method::POST,
            &path,
            Some(body),
            "CreateBasePathMapping",
            domain_name,
        )
        .await
    }

    async fn create_private_domain_name(
        &self,
        request: CreatePrivateDomainNameRequest,
    ) -> Result<PrivateDomainName> {
        let body = Self::serialize(
            &serde_json::json!({
                "domainName": request.domain_name,
                "certificateArn": request.certificate_arn,
                "endpointConfiguration": { "types": ["PRIVATE"] },
                "securityPolicy": "TLS_1_2",
                "policy": request.policy,
                "tags": request.tags,
            }),
            "CreatePrivateDomainNameRequest",
        )?;
        self.send_json(
            Method::POST,
            "/domainnames",
            Some(body),
            "CreateDomainName",
            &request.domain_name,
        )
        .await
    }

    async fn get_private_domain_name(
        &self,
        domain_name: &str,
        domain_name_id: &str,
    ) -> Result<PrivateDomainName> {
        let path = private_domain_path(domain_name, domain_name_id, "")?;
        self.send_json(Method::GET, &path, None, "GetDomainName", domain_name)
            .await
    }

    async fn delete_private_domain_name(
        &self,
        domain_name: &str,
        domain_name_id: &str,
    ) -> Result<()> {
        let path = private_domain_path(domain_name, domain_name_id, "")?;
        self.send_no_response(Method::DELETE, &path, None, "DeleteDomainName", domain_name)
            .await
    }

    async fn update_private_domain_certificate(
        &self,
        domain_name: &str,
        domain_name_id: &str,
        certificate_arn: &str,
    ) -> Result<PrivateDomainName> {
        let path = private_domain_path(domain_name, domain_name_id, "")?;
        let body = Self::serialize(
            &serde_json::json!({"patchOperations": [{"op": "replace", "path": "/certificateArn", "value": certificate_arn}]}),
            "UpdatePrivateDomainNameRequest",
        )?;
        self.send_json(
            Method::PATCH,
            &path,
            Some(body),
            "UpdateDomainName",
            domain_name,
        )
        .await
    }

    async fn create_private_base_path_mapping(
        &self,
        domain_name: &str,
        domain_name_id: &str,
        request: CreateBasePathMappingRequest,
    ) -> Result<BasePathMapping> {
        let path = private_domain_path(domain_name, domain_name_id, "/basepathmappings")?;
        let body = Self::serialize(&request, "CreateBasePathMappingRequest")?;
        self.send_json(
            Method::POST,
            &path,
            Some(body),
            "CreateBasePathMapping",
            domain_name,
        )
        .await
    }

    async fn delete_private_base_path_mapping(
        &self,
        domain_name: &str,
        domain_name_id: &str,
        base_path: &str,
    ) -> Result<()> {
        let suffix = format!("/basepathmappings/{}", urlencoding::encode(base_path));
        let path = private_domain_path(domain_name, domain_name_id, &suffix)?;
        self.send_no_response(
            Method::DELETE,
            &path,
            None,
            "DeleteBasePathMapping",
            domain_name,
        )
        .await
    }

    async fn create_domain_name_access_association(
        &self,
        request: CreateDomainNameAccessAssociationRequest,
    ) -> Result<DomainNameAccessAssociation> {
        let body = Self::serialize(
            &serde_json::json!({
                "accessAssociationSource":request.vpc_endpoint_id,
                "accessAssociationSourceType":"VPCE",
                "domainNameArn":request.domain_name_arn,
                "tags":request.tags,
            }),
            "CreateDomainNameAccessAssociationRequest",
        )?;
        self.send_json(
            Method::POST,
            "/domainnameaccessassociations",
            Some(body),
            "CreateDomainNameAccessAssociation",
            &request.domain_name_arn,
        )
        .await
    }

    async fn get_domain_name_access_associations(
        &self,
        position: Option<String>,
    ) -> Result<DomainNameAccessAssociationsPage> {
        let path = collection_path(
            "/domainnameaccessassociations?limit=500&resourceOwner=SELF",
            position,
        );
        self.send_json(
            Method::GET,
            &path,
            None,
            "GetDomainNameAccessAssociations",
            "account-owned associations",
        )
        .await
    }

    async fn delete_domain_name_access_association(&self, association_arn: &str) -> Result<()> {
        let path = format!(
            "/domainnameaccessassociations/{}",
            urlencoding::encode(association_arn)
        );
        self.send_no_response(
            Method::DELETE,
            &path,
            None,
            "DeleteDomainNameAccessAssociation",
            association_arn,
        )
        .await
    }

    async fn tag_resource(
        &self,
        resource_arn: &str,
        tags: std::collections::HashMap<String, String>,
    ) -> Result<()> {
        // The ARN is a path segment here, so its colons and slashes must be escaped.
        let path = format!("/tags/{}", urlencoding::encode(resource_arn));
        let body = Self::serialize(&serde_json::json!({ "tags": tags }), "TagResourceRequest")?;
        self.send_no_response(Method::PUT, &path, Some(body), "TagResource", resource_arn)
            .await
    }

    async fn delete_base_path_mapping(&self, domain_name: &str, base_path: &str) -> Result<()> {
        let path = format!(
            "/domainnames/{}/basepathmappings/{}",
            domain_name, base_path
        );
        self.send_no_response(
            Method::DELETE,
            &path,
            None,
            "DeleteBasePathMapping",
            domain_name,
        )
        .await
    }
}

#[derive(Debug, Clone, Builder)]
pub struct CreateDomainNameAccessAssociationRequest {
    pub vpc_endpoint_id: String,
    pub domain_name_arn: String,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainNameAccessAssociation {
    pub access_association_source: Option<String>,
    pub access_association_source_type: Option<String>,
    pub domain_name_access_association_arn: Option<String>,
    pub domain_name_arn: Option<String>,
    pub tags: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainNameAccessAssociationsPage {
    #[serde(rename = "item")]
    pub items: Vec<DomainNameAccessAssociation>,
    pub position: Option<String>,
}

/// Only the JSON collection format is accepted. Never turn an unexpected HAL
/// payload into an empty result: that could hide a committed lost-response create.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestApisPage {
    #[serde(rename = "item")]
    pub items: Vec<RestApiDetails>,
    pub position: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainNamesPage {
    #[serde(rename = "item")]
    pub items: Vec<PrivateDomainName>,
    pub position: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestApiDetails {
    pub id: Option<String>,
    pub binary_media_types: Option<Vec<String>>,
    pub name: Option<String>,
    pub root_resource_id: Option<String>,
    pub endpoint_configuration: Option<RestApiEndpointConfiguration>,
    pub disable_execute_api_endpoint: Option<bool>,
    pub policy: Option<String>,
    pub tags: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestApiEndpointConfiguration {
    pub types: Vec<String>,
    #[serde(default)]
    pub vpc_endpoint_ids: Vec<String>,
    pub ip_address_type: Option<String>,
}

fn collection_path(base: &str, position: Option<String>) -> String {
    match position {
        Some(position) => format!("{base}&position={}", urlencoding::encode(&position)),
        None => base.to_string(),
    }
}

/// Private-domain query IDs are mandatory; an omitted ID can select a public
/// domain with the same hostname. Validate before any request, including delete.
fn private_domain_path(domain_name: &str, domain_name_id: &str, suffix: &str) -> Result<String> {
    if domain_name_id.trim().is_empty() {
        return Err(alien_error::AlienError::new(ErrorData::InvalidInput {
            message: "Private API Gateway domain operations require a domainNameId".to_string(),
            field_name: Some("domainNameId".to_string()),
        }));
    }
    Ok(format!(
        "/domainnames/{}{}?domainNameId={}",
        urlencoding::encode(domain_name),
        suffix,
        urlencoding::encode(domain_name_id)
    ))
}

/// The SDK fixes PRIVATE endpoint type and TLS1.2 for this request. The policy
/// must restrict execute-api invocation to the owned VPC endpoint.
#[derive(Debug, Clone, Builder)]
pub struct CreatePrivateDomainNameRequest {
    pub domain_name: String,
    pub certificate_arn: String,
    pub policy: String,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivateDomainName {
    pub domain_name: Option<String>,
    pub domain_name_id: Option<String>,
    pub domain_name_arn: Option<String>,
    pub domain_name_status: Option<String>,
    pub domain_name_status_message: Option<String>,
    pub certificate_arn: Option<String>,
    pub policy: Option<String>,
    pub tags: Option<HashMap<String, String>>,
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointConfiguration {
    pub types: Vec<String>,
}

/// Private REST API creation carries its invocation policy and VPC endpoint
/// association from the first provider mutation.
#[derive(Debug, Clone, Builder)]
pub struct CreatePrivateRestApiRequest {
    pub name: String,
    /// Binary request MIME types; use */* for transparent proxy payloads.
    pub binary_media_types: Option<Vec<String>>,
    pub vpc_endpoint_ids: Vec<String>,
    pub policy: String,
    pub disable_execute_api_endpoint: bool,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateRestApiRequest {
    pub name: String,
    pub endpoint_configuration: EndpointConfiguration,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestApi {
    pub id: Option<String>,
    pub name: Option<String>,
    pub root_resource_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateResourceRequest {
    pub path_part: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub id: Option<String>,
    pub path_part: Option<String>,
}

/// A resource's full path and parent are needed to recover a lost create response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestResourceDetails {
    pub id: Option<String>,
    pub parent_id: Option<String>,
    pub path_part: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcesPage {
    #[serde(rename = "item")]
    pub items: Vec<RestResourceDetails>,
    pub position: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestMethod {
    pub http_method: Option<String>,
    pub authorization_type: Option<String>,
    pub api_key_required: Option<bool>,
    pub method_integration: Option<PutIntegrationRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestStage {
    pub stage_name: Option<String>,
    pub deployment_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct PutMethodRequest {
    pub authorization_type: String,
}

/// The `type`/`httpMethod` fields carry the wire names REST V1 expects: `type`
/// is the integration type and `httpMethod` is the *backend* method (POST for a
/// Lambda proxy), distinct from the resource method (`ANY`) in the URL path.
/// `/response-streaming-invocations` + `responseTransferMode = STREAM` is what
/// makes the endpoint stream rather than buffer.
#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct PutIntegrationRequest {
    #[serde(rename = "type")]
    pub integration_type: String,
    #[serde(rename = "httpMethod")]
    pub integration_http_method: String,
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_transfer_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_in_millis: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateDeploymentRequest {
    pub stage_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deployment {
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateDomainNameRequest {
    pub domain_name: String,
    pub regional_certificate_arn: String,
    pub endpoint_configuration: EndpointConfiguration,
    pub security_policy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainName {
    pub domain_name: Option<String>,
    pub regional_domain_name: Option<String>,
    pub regional_hosted_zone_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateBasePathMappingRequest {
    pub rest_api_id: String,
    pub stage: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BasePathMapping {
    pub base_path: Option<String>,
    pub rest_api_id: Option<String>,
    pub stage: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn put_integration_request_carries_streaming_fields() {
        let request = PutIntegrationRequest {
            integration_type: "AWS_PROXY".to_string(),
            integration_http_method: "POST".to_string(),
            uri: "arn:aws:apigateway:us-east-2:lambda:path/2021-11-15/functions/fn/response-streaming-invocations"
                .to_string(),
            response_transfer_mode: Some("STREAM".to_string()),
            timeout_in_millis: Some(900_000),
        };

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({
                "type": "AWS_PROXY",
                "httpMethod": "POST",
                "uri": "arn:aws:apigateway:us-east-2:lambda:path/2021-11-15/functions/fn/response-streaming-invocations",
                "responseTransferMode": "STREAM",
                "timeoutInMillis": 900_000
            })
        );
    }

    #[test]
    fn create_rest_api_request_emits_regional_endpoint() {
        let request = CreateRestApiRequest {
            name: "my-app-proxy".to_string(),
            endpoint_configuration: EndpointConfiguration {
                types: vec!["REGIONAL".to_string()],
            },
            tags: None,
        };

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({
                "name": "my-app-proxy",
                "endpointConfiguration": { "types": ["REGIONAL"] }
            })
        );
    }
}
