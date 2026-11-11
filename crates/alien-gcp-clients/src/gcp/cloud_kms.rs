use crate::gcp::api_client::{GcpClientBase, GcpServiceConfig};
use crate::gcp::iam::{IamPolicy, SetIamPolicyRequest};
use crate::gcp::GcpClientConfig;
use alien_client_core::Result;
use async_trait::async_trait;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};

#[cfg(feature = "test-utils")]
use mockall::automock;

#[derive(Debug)]
pub struct CloudKmsServiceConfig;

impl GcpServiceConfig for CloudKmsServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://cloudkms.googleapis.com/v1"
    }

    fn default_audience(&self) -> &'static str {
        "https://cloudkms.googleapis.com/"
    }

    fn service_name(&self) -> &'static str {
        "Cloud KMS"
    }

    fn service_key(&self) -> &'static str {
        "cloudkms"
    }
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait CloudKmsApi: Send + Sync + std::fmt::Debug {
    async fn get_crypto_key(&self, crypto_key_name: &str) -> Result<CryptoKey>;
    /// Creates a key ring in `location` (`projects/<p>/locations/<l>`). A ring cannot be deleted.
    async fn create_key_ring(&self, location: &str, key_ring_id: &str) -> Result<KeyRing>;
    async fn get_key_ring(&self, key_ring_name: &str) -> Result<KeyRing>;
    /// Creates a crypto key in `key_ring_name`. Cloud KMS makes the first version.
    async fn create_crypto_key(
        &self,
        key_ring_name: &str,
        crypto_key_id: &str,
        request: CreateCryptoKeyRequest,
    ) -> Result<CryptoKey>;
    async fn get_crypto_key_iam_policy(&self, crypto_key_name: &str) -> Result<IamPolicy>;
    /// Replaces the policy; the policy's `etag` makes a concurrent change fail.
    async fn set_crypto_key_iam_policy(
        &self,
        crypto_key_name: &str,
        policy: IamPolicy,
    ) -> Result<IamPolicy>;
    async fn encrypt(
        &self,
        crypto_key_name: &str,
        request: EncryptRequest,
    ) -> Result<EncryptResponse>;
    async fn decrypt(
        &self,
        crypto_key_name: &str,
        request: DecryptRequest,
    ) -> Result<DecryptResponse>;
}

#[derive(Debug)]
pub struct CloudKmsClient {
    base: GcpClientBase,
}

impl CloudKmsClient {
    pub fn new(client: Client, config: GcpClientConfig) -> Self {
        Self {
            base: GcpClientBase::new(client, config, Box::new(CloudKmsServiceConfig)),
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl CloudKmsApi for CloudKmsClient {
    async fn get_crypto_key(&self, crypto_key_name: &str) -> Result<CryptoKey> {
        self.base
            .execute_request::<CryptoKey, ()>(
                Method::GET,
                crypto_key_name,
                None,
                None,
                crypto_key_name,
            )
            .await
    }

    async fn create_key_ring(&self, location: &str, key_ring_id: &str) -> Result<KeyRing> {
        self.base
            .execute_request::<KeyRing, ()>(
                Method::POST,
                &format!("{location}/keyRings"),
                Some(vec![("keyRingId", key_ring_id.to_string())]),
                None,
                key_ring_id,
            )
            .await
    }

    async fn get_key_ring(&self, key_ring_name: &str) -> Result<KeyRing> {
        self.base
            .execute_request::<KeyRing, ()>(Method::GET, key_ring_name, None, None, key_ring_name)
            .await
    }

    async fn create_crypto_key(
        &self,
        key_ring_name: &str,
        crypto_key_id: &str,
        request: CreateCryptoKeyRequest,
    ) -> Result<CryptoKey> {
        self.base
            .execute_request(
                Method::POST,
                &format!("{key_ring_name}/cryptoKeys"),
                Some(vec![("cryptoKeyId", crypto_key_id.to_string())]),
                Some(request),
                crypto_key_id,
            )
            .await
    }

    async fn get_crypto_key_iam_policy(&self, crypto_key_name: &str) -> Result<IamPolicy> {
        self.base
            .execute_request::<IamPolicy, ()>(
                Method::GET,
                &format!("{crypto_key_name}:getIamPolicy"),
                Some(vec![("options.requestedPolicyVersion", "3".to_string())]),
                None,
                crypto_key_name,
            )
            .await
    }

    async fn set_crypto_key_iam_policy(
        &self,
        crypto_key_name: &str,
        policy: IamPolicy,
    ) -> Result<IamPolicy> {
        self.base
            .execute_request(
                Method::POST,
                &format!("{crypto_key_name}:setIamPolicy"),
                None,
                Some(SetIamPolicyRequest { policy }),
                crypto_key_name,
            )
            .await
    }

    async fn encrypt(
        &self,
        crypto_key_name: &str,
        request: EncryptRequest,
    ) -> Result<EncryptResponse> {
        // The body carries the plaintext; redact it from any error.
        alien_client_core::redact_request_body(
            self.base
                .execute_request(
                    Method::POST,
                    &format!("{crypto_key_name}:encrypt"),
                    None,
                    Some(request),
                    crypto_key_name,
                )
                .await,
        )
    }

    async fn decrypt(
        &self,
        crypto_key_name: &str,
        request: DecryptRequest,
    ) -> Result<DecryptResponse> {
        // The success response is the plaintext.
        alien_client_core::redact_response_body(
            self.base
                .execute_request(
                    Method::POST,
                    &format!("{crypto_key_name}:decrypt"),
                    None,
                    Some(request),
                    crypto_key_name,
                )
                .await,
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CryptoKey {
    pub name: String,
    pub purpose: String,
    pub primary: Option<CryptoKeyVersion>,
    #[serde(default)]
    pub labels: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub version_template: Option<VersionTemplate>,
    #[serde(default)]
    pub rotation_period: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionTemplate {
    pub algorithm: String,
    /// `SOFTWARE`, `HSM` or `EXTERNAL`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protection_level: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyRing {
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCryptoKeyRequest {
    pub purpose: String,
    pub version_template: VersionTemplate,
    pub labels: std::collections::BTreeMap<String, String>,
    /// Seconds with an `s` suffix, for example `7776000s`.
    pub rotation_period: String,
    /// RFC 3339 time of the first rotation; the API requires it with a period.
    pub next_rotation_time: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CryptoKeyVersion {
    pub name: String,
    pub state: String,
    pub algorithm: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptRequest {
    pub plaintext: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_authenticated_data: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptResponse {
    pub name: String,
    pub ciphertext: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecryptRequest {
    pub ciphertext: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_authenticated_data: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecryptResponse {
    pub plaintext: String,
}
