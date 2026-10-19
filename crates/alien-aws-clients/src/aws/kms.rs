use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::Result;
use alien_error::{Context, IntoAlienError};
use async_trait::async_trait;
use bon::Builder;
use reqwest::{Client, Method};
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
        crate::aws::aws_request_utils::sign_send_json(
            request,
            &AwsSignConfig {
                service_name: "kms".to_string(),
                region: region.to_string(),
                credentials: self.credentials.get_credentials(),
                signing_region: None,
            },
        )
        .await
        .context(alien_client_core::ErrorData::GenericError {
            message: format!("AWS KMS {target} failed for key '{key_id}'"),
        })
    }
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
    /// Key material origin, including EXTERNAL_KEY_STORE.
    #[serde(default)]
    pub origin: Option<String>,
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

/// Checks external key access using public plaintext and an isolated encryption context.
/// An unavailable external key is an observation; IAM, timeout (`DependencyTimeoutException`)
/// and malformed-request failures stay errors.
pub async fn probe_external_key(client: &dyn KmsApi, key_id: &str) -> Result<bool> {
    let result = client
        .encrypt(
            EncryptRequest::builder(key_id.to_string(), "YWxpZW4ta2V5LWhlYWx0aA==".to_string())
                .encryption_context(BTreeMap::from([(
                    "deployment-purpose".to_string(),
                    "key-health".to_string(),
                )]))
                .build(),
        )
        .await;
    match result {
        Ok(_) => Ok(true),
        Err(error) if external_key_is_unavailable(&error) => Ok(false),
        Err(error) => Err(error),
    }
}

fn external_key_is_unavailable(
    error: &alien_error::AlienError<alien_client_core::ErrorData>,
) -> bool {
    let contexts = std::iter::once(error.context.as_ref()).chain(
        std::iter::successors(error.source.as_deref(), |source| source.source.as_deref())
            .map(|source| source.context.as_ref()),
    );
    contexts.flatten().any(|context| {
        let Some(body) = context
            .get("http_response_text")
            .and_then(serde_json::Value::as_str)
        else {
            return false;
        };
        let Ok(body) = serde_json::from_str::<serde_json::Value>(body) else {
            return false;
        };
        let code = body
            .get("__type")
            .or_else(|| body.get("code"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        matches!(
            code.rsplit('#').next().unwrap_or(code),
            "DisabledException"
                | "KMSInvalidStateException"
                | "KeyUnavailableException"
                | "XksProxyInvalidResponseException"
                | "CustomKeyStoreInvalidStateException"
        )
    })
}
