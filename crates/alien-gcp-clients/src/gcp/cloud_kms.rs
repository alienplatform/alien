use crate::gcp::api_client::{GcpClientBase, GcpServiceConfig};
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

    async fn encrypt(
        &self,
        crypto_key_name: &str,
        request: EncryptRequest,
    ) -> Result<EncryptResponse> {
        self.base
            .execute_request(
                Method::POST,
                &format!("{crypto_key_name}:encrypt"),
                None,
                Some(request),
                crypto_key_name,
            )
            .await
    }

    async fn decrypt(
        &self,
        crypto_key_name: &str,
        request: DecryptRequest,
    ) -> Result<DecryptResponse> {
        self.base
            .execute_request(
                Method::POST,
                &format!("{crypto_key_name}:decrypt"),
                None,
                Some(request),
                crypto_key_name,
            )
            .await
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CryptoKey {
    pub name: String,
    pub purpose: String,
    pub primary: Option<CryptoKeyVersion>,
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
