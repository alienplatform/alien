//! AWS Certificate Manager (ACM) Client
//!
//! Provides minimal ACM operations needed for importing and managing certificates.

use crate::aws::aws_request_utils::{
    is_json_throttling, is_transient_json_error, AwsRequestBuilderExt, AwsSignConfig,
};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
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

/// The default policy: retry whatever the HTTP layer marks retryable.
fn retry_any_retryable(error: &AlienError<ErrorData>) -> bool {
    error.retryable
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
    /// One page of certificates. ACM lists only RSA_1024 and RSA_2048 certificates unless
    /// `includes` names other key types; see [`CertificateFilters::all_key_types`].
    async fn list_certificates(
        &self,
        request: ListCertificatesRequest,
    ) -> Result<ListCertificatesResponse>;
    async fn list_tags_for_certificate(&self, certificate_arn: &str) -> Result<Vec<Tag>>;
}

/// Every imported certificate in the client's region that carries the tag `key` = `value`.
///
/// ACM cannot filter by tag, so this pages through ListCertificates over every key type and
/// reads the tags of each imported certificate. A certificate that was deleted meanwhile, or
/// whose tags the caller may not read (a tag-conditioned grant meeting someone else's
/// certificate), is skipped. Any other error, including a denied ListCertificates, is returned.
pub async fn find_imported_certificates_by_tag(
    acm: &dyn AcmApi,
    key: &str,
    value: &str,
) -> Result<Vec<String>> {
    let mut found = Vec::new();
    let mut next_token = None;
    loop {
        let page = acm
            .list_certificates(
                ListCertificatesRequest::builder()
                    .includes(CertificateFilters::all_key_types())
                    .maybe_next_token(next_token)
                    .build(),
            )
            .await?;
        for summary in page.certificate_summary_list {
            if summary.certificate_type.as_deref() != Some("IMPORTED") {
                continue;
            }
            let Some(arn) = summary.certificate_arn else {
                continue;
            };
            match acm.list_tags_for_certificate(&arn).await {
                Ok(tags) => {
                    if tags.iter().any(|tag| tag.key == key && tag.value == value) {
                        found.push(arn);
                    }
                }
                Err(error)
                    if matches!(
                        error.error,
                        Some(ErrorData::RemoteResourceNotFound { .. })
                            | Some(ErrorData::RemoteAccessDenied { .. })
                    ) => {}
                Err(error) => return Err(error),
            }
        }
        next_token = page.next_token;
        if next_token.is_none() {
            return Ok(found);
        }
    }
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
        Self {
            client,
            credentials,
        }
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

    /// Sends a JSON request, retrying the errors `retry_when` accepts.
    async fn send_json<T: DeserializeOwned + Send + 'static>(
        &self,
        target: &str,
        body: String,
        operation: &str,
        resource: &str,
        retry_when: fn(&AlienError<ErrorData>) -> bool,
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

        let result = crate::aws::aws_request_utils::sign_send_json_retrying_when(
            builder,
            &self.sign_config(),
            retry_when,
        )
        .await;

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
            "ValidationException"
            | "InvalidArnException"
            | "InvalidParameterException"
            | "InvalidArgsException" => ErrorData::InvalidInput {
                message,
                field_name: None,
            },
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
        // Every ImportCertificate without an ARN makes a new certificate, so a call that may
        // have been acted on (a lost response or a 5xx) is returned to the caller, not resent.
        self.send_json(
            "ImportCertificate",
            body,
            "ImportCertificate",
            "certificate",
            is_json_throttling,
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
        self.send_json(
            "ImportCertificate",
            body,
            "ReimportCertificate",
            &resource,
            retry_any_retryable,
        )
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
            retry_any_retryable,
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

    async fn list_certificates(
        &self,
        request: ListCertificatesRequest,
    ) -> Result<ListCertificatesResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: "Failed to serialize ListCertificatesRequest".to_string(),
            },
        )?;
        self.send_json(
            "ListCertificates",
            body,
            "ListCertificates",
            "certificates",
            is_transient_json_error,
        )
        .await
    }

    async fn list_tags_for_certificate(&self, certificate_arn: &str) -> Result<Vec<Tag>> {
        let body = serde_json::to_string(&ListTagsForCertificateRequest {
            certificate_arn: certificate_arn.to_string(),
        })
        .into_alien_error()
        .context(ErrorData::SerializationError {
            message: "Failed to serialize ListTagsForCertificateRequest".to_string(),
        })?;
        let response: ListTagsForCertificateResponse = self
            .send_json(
                "ListTagsForCertificate",
                body,
                "ListTagsForCertificate",
                certificate_arn,
                is_transient_json_error,
            )
            .await?;
        Ok(response.tags)
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
    /// ACM rejects tags on a reimport; the certificate keeps the tags it was imported with.
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Tag {
    pub key: String,
    /// ACM tags may have no value.
    #[serde(default)]
    pub value: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct ListCertificatesRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate_statuses: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub includes: Option<CertificateFilters>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_items: Option<i32>,
}

/// ListCertificates `Includes`. ACM spells these members in camelCase.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CertificateFilters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_types: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_usage: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extended_key_usage: Option<Vec<String>>,
}

impl CertificateFilters {
    /// Every key type ACM supports. Without it ListCertificates skips RSA_3072/4096 and EC
    /// certificates.
    pub fn all_key_types() -> Self {
        Self::builder()
            .key_types(
                [
                    "RSA_1024",
                    "RSA_2048",
                    "RSA_3072",
                    "RSA_4096",
                    "EC_prime256v1",
                    "EC_secp384r1",
                    "EC_secp521r1",
                ]
                .map(String::from)
                .to_vec(),
            )
            .build()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ListCertificatesResponse {
    #[serde(default)]
    pub certificate_summary_list: Vec<CertificateSummary>,
    pub next_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CertificateSummary {
    pub certificate_arn: Option<String>,
    pub domain_name: Option<String>,
    pub status: Option<String>,
    /// `IMPORTED`, `AMAZON_ISSUED` or `PRIVATE`.
    #[serde(rename = "Type")]
    pub certificate_type: Option<String>,
    pub key_algorithm: Option<String>,
    pub in_use: Option<bool>,
    pub imported_at: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ListTagsForCertificateRequest {
    certificate_arn: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ListTagsForCertificateResponse {
    #[serde(default)]
    tags: Vec<Tag>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ServiceOverrides;
    use alien_core::{AwsClientConfig, AwsCredentials};
    use httpmock::prelude::*;
    use serde_json::json;
    use std::collections::HashMap;

    const IMPORTED_OURS: &str =
        "arn:aws:acm:us-east-1:111122223333:certificate/12345678-1234-1234-1234-123456789012";
    const AMAZON_ISSUED: &str =
        "arn:aws:acm:us-east-1:111122223333:certificate/22222222-1234-1234-1234-123456789012";
    const IMPORTED_UNREADABLE: &str =
        "arn:aws:acm:us-east-1:111122223333:certificate/33333333-1234-1234-1234-123456789012";
    const IMPORTED_OTHER_TOKEN: &str =
        "arn:aws:acm:us-east-1:111122223333:certificate/44444444-1234-1234-1234-123456789012";

    fn client(server: &MockServer) -> AcmClient {
        let config = AwsClientConfig {
            account_id: "111122223333".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "test-access-key".to_string(),
                secret_access_key: "test-secret-key".to_string(),
                session_token: None,
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("acm".to_string(), server.base_url())]),
            }),
        };
        AcmClient::new(
            reqwest::Client::new(),
            AwsCredentialProvider::from_config_sync(config),
        )
    }

    /// A `CertificateSummary` as the ListCertificates API reference shows it.
    fn summary(arn: &str, certificate_type: &str) -> serde_json::Value {
        json!({
            "CertificateArn": arn,
            "DomainName": "example.com",
            "SubjectAlternativeNameSummaries": ["example.com", "other.example.com"],
            "HasAdditionalSubjectAlternativeNames": false,
            "Status": "ISSUED",
            "Type": certificate_type,
            "KeyAlgorithm": "RSA-2048",
            "KeyUsages": ["DIGITAL_SIGNATURE", "KEY_ENCIPHERMENT"],
            "ExtendedKeyUsages": ["TLS_WEB_SERVER_AUTHENTICATION"],
            "InUse": false,
            "RenewalEligibility": "INELIGIBLE",
            "NotBefore": 1634169600.0,
            "NotAfter": 1665792000.0,
            "CreatedAt": 1634249960.0,
            "ImportedAt": 1634249960.0
        })
    }

    fn all_key_types_body(next_token: Option<&str>) -> serde_json::Value {
        let mut body = json!({
            "Includes": {
                "keyTypes": [
                    "RSA_1024", "RSA_2048", "RSA_3072", "RSA_4096",
                    "EC_prime256v1", "EC_secp384r1", "EC_secp521r1"
                ]
            }
        });
        if let Some(token) = next_token {
            body["NextToken"] = json!(token);
        }
        body
    }

    #[tokio::test]
    async fn list_certificates_sends_the_documented_request_and_parses_summaries() {
        let server = MockServer::start_async().await;
        let list = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/")
                    .header("x-amz-target", "CertificateManager.ListCertificates")
                    .header("content-type", "application/x-amz-json-1.1")
                    .header_exists("authorization")
                    .json_body(all_key_types_body(None));
                then.status(200).json_body(json!({
                    "CertificateSummaryList": [summary(IMPORTED_OURS, "IMPORTED")],
                    "NextToken": "page-2"
                }));
            })
            .await;

        let page = client(&server)
            .list_certificates(
                ListCertificatesRequest::builder()
                    .includes(CertificateFilters::all_key_types())
                    .build(),
            )
            .await
            .expect("list succeeds");

        list.assert_async().await;
        assert_eq!(page.next_token.as_deref(), Some("page-2"));
        assert_eq!(page.certificate_summary_list.len(), 1);
        let certificate = &page.certificate_summary_list[0];
        assert_eq!(certificate.certificate_arn.as_deref(), Some(IMPORTED_OURS));
        assert_eq!(certificate.certificate_type.as_deref(), Some("IMPORTED"));
        assert_eq!(certificate.domain_name.as_deref(), Some("example.com"));
        assert_eq!(certificate.in_use, Some(false));
        assert_eq!(certificate.imported_at, Some(1634249960.0));
    }

    #[tokio::test]
    async fn list_tags_for_certificate_returns_every_tag() {
        let server = MockServer::start_async().await;
        let tags = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/")
                    .header("x-amz-target", "CertificateManager.ListTagsForCertificate")
                    .json_body(json!({ "CertificateArn": IMPORTED_OURS }));
                then.status(200).json_body(json!({
                    "Tags": [
                        { "Key": "Admin", "Value": "Alice" },
                        { "Key": "Purpose" }
                    ]
                }));
            })
            .await;

        let result = client(&server)
            .list_tags_for_certificate(IMPORTED_OURS)
            .await
            .expect("list tags succeeds");

        tags.assert_async().await;
        assert_eq!(
            result,
            vec![
                Tag {
                    key: "Admin".to_string(),
                    value: "Alice".to_string()
                },
                Tag {
                    key: "Purpose".to_string(),
                    value: String::new()
                },
            ]
        );
    }

    #[tokio::test]
    async fn list_tags_of_a_deleted_certificate_is_not_found() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("x-amz-target", "CertificateManager.ListTagsForCertificate");
                then.status(400).json_body(json!({
                    "__type": "ResourceNotFoundException",
                    "message": format!("Could not find certificate {IMPORTED_OURS}.")
                }));
            })
            .await;

        let error = client(&server)
            .list_tags_for_certificate(IMPORTED_OURS)
            .await
            .expect_err("a deleted certificate has no tags");
        assert!(
            matches!(error.error, Some(ErrorData::RemoteResourceNotFound { .. })),
            "{error:?}"
        );
    }

    async fn tags_mock<'a>(
        server: &'a MockServer,
        arn: &str,
        status: u16,
        body: serde_json::Value,
    ) -> httpmock::Mock<'a> {
        server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("x-amz-target", "CertificateManager.ListTagsForCertificate")
                    .json_body(json!({ "CertificateArn": arn }));
                then.status(status).json_body(body);
            })
            .await
    }

    /// Walks every page, reads tags only of imported certificates, skips a certificate whose
    /// tags are denied, and returns only the certificate carrying the token.
    #[tokio::test]
    async fn find_imported_certificates_by_tag_walks_pages_and_skips_unreadable_certificates() {
        let server = MockServer::start_async().await;
        let first_page = server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("x-amz-target", "CertificateManager.ListCertificates")
                    .json_body(all_key_types_body(None));
                then.status(200).json_body(json!({
                    "CertificateSummaryList": [
                        summary(IMPORTED_OURS, "IMPORTED"),
                        summary(AMAZON_ISSUED, "AMAZON_ISSUED"),
                        summary(IMPORTED_UNREADABLE, "IMPORTED"),
                    ],
                    "NextToken": "page-2"
                }));
            })
            .await;
        let second_page = server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("x-amz-target", "CertificateManager.ListCertificates")
                    .json_body(all_key_types_body(Some("page-2")));
                then.status(200).json_body(json!({
                    "CertificateSummaryList": [summary(IMPORTED_OTHER_TOKEN, "IMPORTED")]
                }));
            })
            .await;
        let ours = tags_mock(
            &server,
            IMPORTED_OURS,
            200,
            json!({ "Tags": [
                { "Key": "alien-stack", "Value": "stack" },
                { "Key": "CreateAttempt", "Value": "token-1" }
            ] }),
        )
        .await;
        let amazon_issued = tags_mock(&server, AMAZON_ISSUED, 200, json!({ "Tags": [] })).await;
        let unreadable = tags_mock(
            &server,
            IMPORTED_UNREADABLE,
            400,
            json!({
                "__type": "AccessDeniedException",
                "Message": "User is not authorized to perform: acm:ListTagsForCertificate"
            }),
        )
        .await;
        let other_token = tags_mock(
            &server,
            IMPORTED_OTHER_TOKEN,
            200,
            json!({ "Tags": [{ "Key": "CreateAttempt", "Value": "token-2" }] }),
        )
        .await;

        let found = find_imported_certificates_by_tag(&client(&server), "CreateAttempt", "token-1")
            .await
            .expect("lookup succeeds");

        assert_eq!(found, vec![IMPORTED_OURS.to_string()]);
        first_page.assert_hits_async(1).await;
        second_page.assert_hits_async(1).await;
        ours.assert_hits_async(1).await;
        unreadable.assert_hits_async(1).await;
        other_token.assert_hits_async(1).await;
        amazon_issued.assert_hits_async(0).await;
    }

    #[tokio::test]
    async fn find_imported_certificates_by_tag_returns_a_denied_list() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("x-amz-target", "CertificateManager.ListCertificates");
                then.status(400).json_body(json!({
                    "__type": "AccessDeniedException",
                    "Message": "User is not authorized to perform: acm:ListCertificates"
                }));
            })
            .await;

        let error = find_imported_certificates_by_tag(&client(&server), "CreateAttempt", "token-1")
            .await
            .expect_err("a denied list must not read as no certificates");
        assert!(
            matches!(error.error, Some(ErrorData::RemoteAccessDenied { .. })),
            "{error:?}"
        );
    }
    /// ImportCertificate without an ARN makes a new certificate on every call that ACM acts on,
    /// so a server error (which may follow an import that happened) is not sent again.
    #[tokio::test]
    async fn import_certificate_is_not_resent_after_a_server_error() {
        let server = MockServer::start_async().await;
        let import = server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("x-amz-target", "CertificateManager.ImportCertificate");
                then.status(500).json_body(json!({
                    "__type": "InternalFailure",
                    "message": "We encountered an internal error. Please try again."
                }));
            })
            .await;

        client(&server)
            .import_certificate(
                ImportCertificateRequest::builder()
                    .certificate("leaf".to_string())
                    .private_key("key".to_string())
                    .build(),
            )
            .await
            .expect_err("the server error is returned");

        import.assert_hits_async(1).await;
    }

    #[test]
    fn json_throttling_is_retried_and_a_denial_is_not() {
        let response = |status: u16, body: &str| {
            AlienError::new(ErrorData::HttpResponseError {
                message: "ACM call failed".to_string(),
                url: "https://acm.us-east-1.amazonaws.com/".to_string(),
                http_status: status,
                http_response_text: Some(body.to_string()),
                http_request_text: None,
            })
        };
        let throttled = response(
            400,
            r#"{"__type":"ThrottlingException","message":"Rate exceeded"}"#,
        );
        let denied = response(
            400,
            r#"{"__type":"AccessDeniedException","Message":"not authorized"}"#,
        );
        let unavailable = response(503, r#"{"__type":"ServiceUnavailable"}"#);

        assert!(is_json_throttling(&throttled));
        assert!(!is_json_throttling(&denied));
        assert!(!is_json_throttling(&unavailable));
        assert!(is_transient_json_error(&throttled));
        assert!(is_transient_json_error(&unavailable));
        assert!(!is_transient_json_error(&denied));
    }
}
