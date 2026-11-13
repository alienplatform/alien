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
    /// Creates a symmetric key. Not idempotent: every call makes a new key.
    async fn create_key(&self, request: CreateKeyRequest) -> Result<DescribeKeyResponse>;
    async fn get_public_key(&self, key_id: &str) -> Result<GetPublicKeyResponse>;
    async fn sign(&self, request: SignRequest) -> Result<SignResponse>;
    async fn enable_key_rotation(&self, key_id: &str) -> Result<()>;
    async fn get_key_rotation_status(&self, key_id: &str) -> Result<bool>;
    async fn put_key_policy(&self, key_id: &str, policy: &str) -> Result<()>;
    /// The `default` key policy document, as a JSON string.
    async fn get_key_policy(&self, key_id: &str) -> Result<String>;
    async fn list_resource_tags(&self, key_id: &str) -> Result<BTreeMap<String, String>>;
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

    async fn request(
        &self,
        target: &str,
        body: String,
    ) -> Result<(reqwest::RequestBuilder, AwsSignConfig)> {
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
        let config = AwsSignConfig {
            service_name: "kms".to_string(),
            region: region.to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        };
        Ok((request, config))
    }

    async fn send<T: DeserializeOwned + Send + 'static>(
        &self,
        target: &str,
        body: String,
        key_id: &str,
    ) -> Result<T> {
        let (request, config) = self.request(target, body).await?;
        crate::aws::aws_request_utils::sign_send_json(request, &config)
            .await
            .context(alien_client_core::ErrorData::GenericError {
                message: format!("AWS KMS {target} failed for key '{key_id}'"),
            })
    }

    /// For operations such as `EnableKeyRotation` and `PutKeyPolicy`, which answer 200 with an
    /// empty body.
    async fn send_no_response(&self, target: &str, body: String, key_id: &str) -> Result<()> {
        let (request, config) = self.request(target, body).await?;
        crate::aws::aws_request_utils::sign_send_no_response(request, &config)
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
        alien_client_core::redact_request_body(self.send("Encrypt", body, &key_id).await)
    }

    async fn decrypt(&self, request: DecryptRequest) -> Result<DecryptResponse> {
        let key_id = request.key_id.clone();
        let body = serde_json::to_string(&request).into_alien_error().context(
            alien_client_core::ErrorData::SerializationError {
                message: "Failed to serialize AWS KMS Decrypt request".to_string(),
            },
        )?;
        alien_client_core::redact_secret_bodies(self.send("Decrypt", body, &key_id).await)
    }

    async fn describe_key(&self, key_id: &str) -> Result<DescribeKeyResponse> {
        let body = serde_json::to_string(&DescribeKeyRequest { key_id })
            .into_alien_error()
            .context(alien_client_core::ErrorData::SerializationError {
                message: "Failed to serialize AWS KMS DescribeKey request".to_string(),
            })?;
        self.send("DescribeKey", body, key_id).await
    }

    async fn get_public_key(&self, key_id: &str) -> Result<GetPublicKeyResponse> {
        let body = serde_json::to_string(&DescribeKeyRequest { key_id })
            .into_alien_error()
            .context(alien_client_core::ErrorData::SerializationError {
                message: "Failed to serialize AWS KMS GetPublicKey request".to_string(),
            })?;
        self.send("GetPublicKey", body, key_id).await
    }

    async fn sign(&self, request: SignRequest) -> Result<SignResponse> {
        let key_id = request.key_id.clone();
        let body = serde_json::to_string(&request).into_alien_error().context(
            alien_client_core::ErrorData::SerializationError {
                message: "Failed to serialize AWS KMS Sign request".to_string(),
            },
        )?;
        self.send("Sign", body, &key_id).await
    }

    async fn create_key(&self, request: CreateKeyRequest) -> Result<DescribeKeyResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            alien_client_core::ErrorData::SerializationError {
                message: "Failed to serialize AWS KMS CreateKey request".to_string(),
            },
        )?;
        self.send("CreateKey", body, "new key").await
    }

    async fn enable_key_rotation(&self, key_id: &str) -> Result<()> {
        let body = key_id_body("EnableKeyRotation", key_id)?;
        self.send_no_response("EnableKeyRotation", body, key_id)
            .await
    }

    async fn get_key_rotation_status(&self, key_id: &str) -> Result<bool> {
        let body = key_id_body("GetKeyRotationStatus", key_id)?;
        let response: KeyRotationStatusResponse =
            self.send("GetKeyRotationStatus", body, key_id).await?;
        Ok(response.key_rotation_enabled)
    }

    async fn put_key_policy(&self, key_id: &str, policy: &str) -> Result<()> {
        let body = serde_json::to_string(&PutKeyPolicyRequest {
            key_id,
            policy_name: "default",
            policy,
        })
        .into_alien_error()
        .context(alien_client_core::ErrorData::SerializationError {
            message: "Failed to serialize AWS KMS PutKeyPolicy request".to_string(),
        })?;
        self.send_no_response("PutKeyPolicy", body, key_id).await
    }

    async fn get_key_policy(&self, key_id: &str) -> Result<String> {
        let body = serde_json::to_string(&serde_json::json!({
            "KeyId": key_id,
            "PolicyName": "default",
        }))
        .into_alien_error()
        .context(alien_client_core::ErrorData::SerializationError {
            message: "Failed to serialize AWS KMS GetKeyPolicy request".to_string(),
        })?;
        let response: GetKeyPolicyResponse = self.send("GetKeyPolicy", body, key_id).await?;
        Ok(response.policy)
    }

    async fn list_resource_tags(&self, key_id: &str) -> Result<BTreeMap<String, String>> {
        let body = key_id_body("ListResourceTags", key_id)?;
        let response: ListResourceTagsResponse =
            self.send("ListResourceTags", body, key_id).await?;
        Ok(response
            .tags
            .into_iter()
            .map(|tag| (tag.tag_key, tag.tag_value))
            .collect())
    }
}

fn key_id_body(operation: &str, key_id: &str) -> Result<String> {
    serde_json::to_string(&DescribeKeyRequest { key_id })
        .into_alien_error()
        .context(alien_client_core::ErrorData::SerializationError {
            message: format!("Failed to serialize AWS KMS {operation} request"),
        })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateKeyRequest {
    pub description: String,
    /// `SYMMETRIC_DEFAULT`
    pub key_spec: String,
    /// `ENCRYPT_DECRYPT`
    pub key_usage: String,
    /// The key policy as a JSON string.
    pub policy: String,
    pub tags: Vec<KmsTag>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct KmsTag {
    pub tag_key: String,
    pub tag_value: String,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct PutKeyPolicyRequest<'a> {
    key_id: &'a str,
    policy_name: &'a str,
    policy: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct KeyRotationStatusResponse {
    key_rotation_enabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct GetKeyPolicyResponse {
    policy: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ListResourceTagsResponse {
    #[serde(default)]
    tags: Vec<ListedTag>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ListedTag {
    tag_key: String,
    tag_value: String,
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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetPublicKeyResponse {
    pub key_id: String,
    /// Base64 DER SubjectPublicKeyInfo.
    pub public_key: String,
    pub key_spec: String,
    pub key_usage: String,
}

/// `Sign` over a message or a precomputed digest.
#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct SignRequest {
    #[builder(start_fn)]
    pub key_id: String,
    /// Base64 of the message, or of its digest when `message_type` is `DIGEST`.
    #[builder(start_fn)]
    pub message: String,
    /// `RAW` or `DIGEST`.
    #[builder(start_fn)]
    pub message_type: String,
    /// For example `ECDSA_SHA_256`.
    #[builder(start_fn)]
    pub signing_algorithm: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SignResponse {
    pub key_id: String,
    /// Base64 DER ECDSA signature.
    pub signature: String,
    pub signing_algorithm: String,
}
