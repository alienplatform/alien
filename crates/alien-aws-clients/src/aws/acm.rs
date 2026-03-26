//! AWS Certificate Manager (ACM) Client
//!
//! Provides minimal ACM operations needed for importing and managing certificates.

use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, ContextError, IntoAlienError};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use bon::Builder;
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[cfg(feature = "test-utils")]
use mockall::automock;

// ---------------------------------------------------------------------------
// ACM Error Response Parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct AcmErrorResponse {
    #[serde(rename = "__type")]
    pub type_field: Option<String>,
    pub code: Option<String>,
    pub message: Option<String>,
    #[serde(rename = "Message")]
    pub message_capital: Option<String>,
}

// ---------------------------------------------------------------------------
// ACM API Trait
// ---------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait AcmApi: Send + Sync + std::fmt::Debug {
    async fn import_certificate(
        &self,
        request: ImportCertificateRequest,
    ) -> Result<ImportCertificateResponse>;
    async fn reimport_certificate(
        &self,
        request: ReimportCertificateRequest,
    ) -> Result<ImportCertificateResponse>;
    async fn describe_certificate(
        &self,
        certificate_arn: &str,
    ) -> Result<DescribeCertificateResponse>;
    async fn delete_certificate(&self, certificate_arn: &str) -> Result<()>;
}

// ---------------------------------------------------------------------------
// ACM Client
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AcmClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl AcmClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self { client, credentials }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "acm".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self.credentials.get_service_endpoint_option("acm") {
            override_url.to_string()
        } else {
            format!("https://acm.{}.amazonaws.com", self.credentials.region())
        }
    }

    async fn send_json<T: DeserializeOwned + Send + 'static>(
        &self,
        target: &str,
        body: String,
        operation: &str,
        resource: &str,
    ) -> Result<T> {
        self.credentials.ensure_fresh().await?;
        let base_url = self.get_base_url();
        let url = format!("{}/", base_url.trim_end_matches('/'));

        let builder = self
            .client
            .request(Method::POST, &url)
            .host(&format!("acm.{}.amazonaws.com", self.credentials.region()))
            .header("X-Amz-Target", format!("CertificateManager.{}", target))
            .content_type_amz_json()
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
        let base_url = self.get_base_url();
        let url = format!("{}/", base_url.trim_end_matches('/'));

        let builder = self
            .client
            .request(Method::POST, &url)
            .host(&format!("acm.{}.amazonaws.com", self.credentials.region()))
            .header("X-Amz-Target", format!("CertificateManager.{}", target))
            .content_type_amz_json()
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
                        Self::map_acm_error(status, text, operation, resource, request_body)
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

    fn map_acm_error(
        status: StatusCode,
        body: &str,
        _operation: &str,
        resource: &str,
        request_body: Option<&str>,
    ) -> Option<ErrorData> {
        let parsed: std::result::Result<AcmErrorResponse, _> = serde_json::from_str(body);
        let (code, message) = match parsed {
            Ok(e) => {
                let code = e
                    .type_field
                    .or(e.code)
                    .unwrap_or_else(|| "UnknownError".into());
                let message = e
                    .message
                    .or(e.message_capital)
                    .unwrap_or_else(|| "Unknown error".into());
                (code, message)
            }
            Err(_) => return None,
        };

        Some(match code.as_str() {
            "AccessDeniedException" | "UnrecognizedClientException" | "ExpiredTokenException" => {
                ErrorData::RemoteAccessDenied {
                    resource_type: "Certificate".into(),
                    resource_name: resource.into(),
                }
            }
            "ResourceNotFoundException" => ErrorData::RemoteResourceNotFound {
                resource_type: "Certificate".into(),
                resource_name: resource.into(),
            },
            "LimitExceededException" | "ThrottlingException" | "TooManyRequestsException" => {
                ErrorData::RateLimitExceeded { message }
            }
            "ValidationException" | "InvalidArnException" | "InvalidParameterException" => {
                ErrorData::InvalidInput {
                    message,
                    field_name: None,
                }
            }
            _ => match status {
                StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
                    resource_type: "Certificate".into(),
                    resource_name: resource.into(),
                },
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
                    resource_type: "Certificate".into(),
                    resource_name: resource.into(),
                },
                StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded { message },
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => ErrorData::RemoteServiceUnavailable { message },
                _ => ErrorData::HttpResponseError {
                    message: format!("ACM operation failed: {}", message),
                    url: format!("acm.amazonaws.com"),
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
impl AcmApi for AcmClient {
    async fn import_certificate(
        &self,
        request: ImportCertificateRequest,
    ) -> Result<ImportCertificateResponse> {
        let body = serde_json::to_string(&ImportCertificateWireRequest::from(request))
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: "Failed to serialize ImportCertificateRequest".to_string(),
            })?;
        self.send_json(
            "ImportCertificate",
            body,
            "ImportCertificate",
            "certificate",
        )
        .await
    }

    async fn reimport_certificate(
        &self,
        request: ReimportCertificateRequest,
    ) -> Result<ImportCertificateResponse> {
        let resource = request.certificate_arn.clone();
        let body = serde_json::to_string(&ReimportCertificateWireRequest::from(request))
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: "Failed to serialize ReimportCertificateRequest".to_string(),
            })?;
        self.send_json("ImportCertificate", body, "ReimportCertificate", &resource)
            .await
    }

    async fn describe_certificate(
        &self,
        certificate_arn: &str,
    ) -> Result<DescribeCertificateResponse> {
        let body = serde_json::to_string(&DescribeCertificateRequest {
            certificate_arn: certificate_arn.to_string(),
        })
        .into_alien_error()
        .context(ErrorData::SerializationError {
            message: "Failed to serialize DescribeCertificateRequest".to_string(),
        })?;
        self.send_json(
            "DescribeCertificate",
            body,
            "DescribeCertificate",
            certificate_arn,
        )
        .await
    }

    async fn delete_certificate(&self, certificate_arn: &str) -> Result<()> {
        let body = serde_json::to_string(&DeleteCertificateRequest {
            certificate_arn: certificate_arn.to_string(),
        })
        .into_alien_error()
        .context(ErrorData::SerializationError {
            message: "Failed to serialize DeleteCertificateRequest".to_string(),
        })?;
        self.send_no_response(
            "DeleteCertificate",
            body,
            "DeleteCertificate",
            certificate_arn,
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// ACM Request/Response Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct ImportCertificateRequest {
    pub certificate: String,
    pub private_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate_chain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<Tag>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct ImportCertificateWireRequest {
    pub certificate: String,
    pub private_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate_chain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<Tag>>,
}

impl From<ImportCertificateRequest> for ImportCertificateWireRequest {
    fn from(request: ImportCertificateRequest) -> Self {
        Self {
            certificate: STANDARD.encode(request.certificate.as_bytes()),
            private_key: STANDARD.encode(request.private_key.as_bytes()),
            certificate_chain: request
                .certificate_chain
                .map(|chain| STANDARD.encode(chain.as_bytes())),
            tags: request.tags,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct ReimportCertificateRequest {
    pub certificate_arn: String,
    pub certificate: String,
    pub private_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate_chain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<Tag>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct ReimportCertificateWireRequest {
    pub certificate_arn: String,
    pub certificate: String,
    pub private_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate_chain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<Tag>>,
}

impl From<ReimportCertificateRequest> for ReimportCertificateWireRequest {
    fn from(request: ReimportCertificateRequest) -> Self {
        Self {
            certificate_arn: request.certificate_arn,
            certificate: STANDARD.encode(request.certificate.as_bytes()),
            private_key: STANDARD.encode(request.private_key.as_bytes()),
            certificate_chain: request
                .certificate_chain
                .map(|chain| STANDARD.encode(chain.as_bytes())),
            tags: request.tags,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ImportCertificateResponse {
    pub certificate_arn: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeCertificateRequest {
    pub certificate_arn: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeCertificateResponse {
    pub certificate: Option<CertificateDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DeleteCertificateRequest {
    pub certificate_arn: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CertificateDetail {
    pub certificate_arn: Option<String>,
    pub domain_name: Option<String>,
    pub status: Option<String>,
    pub not_after: Option<f64>,
    pub not_before: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Tag {
    pub key: String,
    pub value: String,
}
