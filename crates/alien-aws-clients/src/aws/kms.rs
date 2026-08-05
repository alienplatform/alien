use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, ContextError, IntoAlienError};
use async_trait::async_trait;
use bon::Builder;
use reqwest::{Client, Method, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::BTreeMap;

#[cfg(feature = "test-utils")]
use mockall::automock;

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait KmsApi: Send + Sync + std::fmt::Debug {
    async fn encrypt(&self, request: EncryptRequest) -> Result<EncryptResponse>;
    async fn decrypt(&self, request: DecryptRequest) -> Result<DecryptResponse>;
    async fn describe_key(&self, key_id: &str) -> Result<DescribeKeyResponse>;
}

#[derive(Debug, Clone)]
pub struct KmsClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl KmsClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
        }
    }

    async fn send<T: DeserializeOwned + Send + 'static>(
        &self,
        target: &str,
        body: String,
        key_id: &str,
    ) -> Result<T> {
        self.credentials.ensure_fresh().await?;
        let region = self.credentials.region();
        let endpoint = self
            .credentials
            .get_service_endpoint_option("kms")
            .map(str::to_string)
            .unwrap_or_else(|| format!("https://kms.{region}.amazonaws.com"));
        let host = format!("kms.{region}.amazonaws.com");
        let request = self
            .client
            .request(Method::POST, endpoint)
            .host(&host)
            .header("X-Amz-Target", format!("TrentService.{target}"))
            .header("Content-Type", "application/x-amz-json-1.1")
            .content_sha256(&body)
            .body(body);
        let result = crate::aws::aws_request_utils::sign_send_json(
            request,
            &AwsSignConfig {
                service_name: "kms".to_string(),
                region: region.to_string(),
                credentials: self.credentials.get_credentials(),
                signing_region: None,
            },
        )
        .await;
        Self::map_result(result, target, key_id)
    }

    fn map_result<T>(result: Result<T>, operation: &str, key_id: &str) -> Result<T> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                let Some(ErrorData::HttpResponseError {
                    http_status,
                    http_response_text: Some(body),
                    ..
                }) = &error.error
                else {
                    return Err(error.context(ErrorData::GenericError {
                        message: format!("AWS KMS {operation} failed for key '{key_id}'"),
                    }));
                };
                let status =
                    StatusCode::from_u16(*http_status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                let mapped = map_kms_error(status, body, key_id).unwrap_or_else(|| {
                    ErrorData::GenericError {
                        message: format!("AWS KMS {operation} failed for key '{key_id}'"),
                    }
                });
                Err(error.context(mapped))
            }
        }
    }
}

#[derive(Deserialize)]
struct KmsErrorResponse {
    #[serde(rename = "__type")]
    type_field: Option<String>,
}

fn map_kms_error(status: StatusCode, body: &str, key_id: &str) -> Option<ErrorData> {
    let parsed: KmsErrorResponse = serde_json::from_str(body).ok()?;
    let raw_code = parsed.type_field?;
    let code = raw_code.rsplit('#').next().unwrap_or(&raw_code);
    Some(match code {
        "AccessDeniedException"
        | "NotAuthorizedException"
        | "UnrecognizedClientException"
        | "ExpiredTokenException" => ErrorData::RemoteAccessDenied {
            resource_type: "KMS Key".to_string(),
            resource_name: key_id.to_string(),
        },
        _ if status == StatusCode::FORBIDDEN || status == StatusCode::UNAUTHORIZED => {
            ErrorData::RemoteAccessDenied {
                resource_type: "KMS Key".to_string(),
                resource_name: key_id.to_string(),
            }
        }
        _ => return None,
    })
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl KmsApi for KmsClient {
    async fn encrypt(&self, request: EncryptRequest) -> Result<EncryptResponse> {
        let key_id = request.key_id.clone();
        let body = serde_json::to_string(&request).into_alien_error().context(
            alien_client_core::ErrorData::SerializationError {
                message: "Failed to serialize AWS KMS Encrypt request".to_string(),
            },
        )?;
        self.send("Encrypt", body, &key_id).await
    }

    async fn decrypt(&self, request: DecryptRequest) -> Result<DecryptResponse> {
        let key_id = request.key_id.clone();
        let body = serde_json::to_string(&request).into_alien_error().context(
            alien_client_core::ErrorData::SerializationError {
                message: "Failed to serialize AWS KMS Decrypt request".to_string(),
            },
        )?;
        self.send("Decrypt", body, &key_id).await
    }

    async fn describe_key(&self, key_id: &str) -> Result<DescribeKeyResponse> {
        let body = serde_json::to_string(&DescribeKeyRequest { key_id })
            .into_alien_error()
            .context(alien_client_core::ErrorData::SerializationError {
                message: "Failed to serialize AWS KMS DescribeKey request".to_string(),
            })?;
        self.send("DescribeKey", body, key_id).await
    }
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct DescribeKeyRequest<'a> {
    key_id: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeKeyResponse {
    pub key_metadata: KeyMetadata,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct KeyMetadata {
    pub arn: String,
    pub key_id: String,
    pub enabled: bool,
    pub key_state: String,
    pub key_usage: String,
    pub key_spec: String,
    #[serde(default)]
    pub deletion_date: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct EncryptRequest {
    #[builder(start_fn)]
    pub key_id: String,
    #[builder(start_fn)]
    pub plaintext: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_context: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct EncryptResponse {
    pub ciphertext_blob: String,
    pub key_id: String,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct DecryptRequest {
    #[builder(start_fn)]
    pub key_id: String,
    #[builder(start_fn)]
    pub ciphertext_blob: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_context: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DecryptResponse {
    pub plaintext: String,
    pub key_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_kms_access_denied_from_json_rpc_error() {
        let mapped = map_kms_error(
            StatusCode::BAD_REQUEST,
            r#"{"__type":"AccessDeniedException","message":"denied"}"#,
            "arn:aws:kms:us-east-1:123:key/example",
        )
        .expect("KMS access denial should be recognized");

        assert!(matches!(mapped, ErrorData::RemoteAccessDenied { .. }));
    }

    #[test]
    fn leaves_disabled_key_distinct_from_iam_denial() {
        assert!(map_kms_error(
            StatusCode::BAD_REQUEST,
            r#"{"__type":"DisabledException","message":"disabled"}"#,
            "arn:aws:kms:us-east-1:123:key/example",
        )
        .is_none());
    }
}
