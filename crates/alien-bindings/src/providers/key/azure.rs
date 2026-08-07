use super::{encode_context, frame, unframe};
use crate::error::{ErrorData, Result};
use crate::traits::{Binding, Key};
use alien_azure_clients::keyvault::{KeyOperationRequest, KeyVaultKeysApi};
use alien_error::{Context, IntoAlienError};
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug)]
pub struct AzureKeyVaultKey {
    client: Arc<dyn KeyVaultKeysApi>,
    key_id: String,
}

impl AzureKeyVaultKey {
    pub fn new(client: Arc<dyn KeyVaultKeysApi>, key_id: String) -> Self {
        Self { client, key_id }
    }
}

impl Binding for AzureKeyVaultKey {}

#[async_trait]
impl Key for AzureKeyVaultKey {
    async fn encrypt(
        &self,
        plaintext: &[u8],
        context: Option<&BTreeMap<String, String>>,
    ) -> Result<Vec<u8>> {
        let canonical = encode_context(context)?;
        let response = self
            .client
            .encrypt(
                &self.key_id,
                KeyOperationRequest {
                    alg: "RSA-OAEP-256".to_string(),
                    value: URL_SAFE_NO_PAD.encode(frame(plaintext, &canonical)?),
                },
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Azure Key Vault encrypt failed".to_string(),
                resource_id: None,
            })?;
        URL_SAFE_NO_PAD
            .decode(response.value)
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Azure Key Vault returned invalid ciphertext encoding".to_string(),
                resource_id: None,
            })
    }

    async fn decrypt(
        &self,
        ciphertext: &[u8],
        context: Option<&BTreeMap<String, String>>,
    ) -> Result<Vec<u8>> {
        let canonical = encode_context(context)?;
        let response = self
            .client
            .decrypt(
                &self.key_id,
                KeyOperationRequest {
                    alg: "RSA-OAEP-256".to_string(),
                    value: URL_SAFE_NO_PAD.encode(ciphertext),
                },
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Azure Key Vault decrypt failed".to_string(),
                resource_id: None,
            })?;
        let framed = URL_SAFE_NO_PAD
            .decode(response.value)
            .into_alien_error()
            .context(ErrorData::KeyCiphertextInvalid {
                reason: "provider plaintext encoding is invalid".to_string(),
            })?;
        unframe(&framed, &canonical)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_azure_clients::keyvault::{KeyOperationResponse, MockKeyVaultKeysApi};

    #[tokio::test]
    async fn binds_context_inside_the_portable_frame() {
        let context = BTreeMap::from([("tenant".to_string(), "acme".to_string())]);
        let canonical = encode_context(Some(&context)).unwrap();
        let framed = frame(b"root", &canonical).unwrap();
        let mut client = MockKeyVaultKeysApi::new();
        client.expect_encrypt().returning(|key_id, request| {
            assert_eq!(key_id, "versioned-key-id");
            assert_eq!(request.alg, "RSA-OAEP-256");
            Ok(KeyOperationResponse {
                kid: key_id.to_string(),
                value: URL_SAFE_NO_PAD.encode(b"ciphertext"),
            })
        });
        client
            .expect_decrypt()
            .times(2)
            .returning(move |key_id, request| {
                assert_eq!(key_id, "versioned-key-id");
                assert_eq!(request.alg, "RSA-OAEP-256");
                Ok(KeyOperationResponse {
                    kid: key_id.to_string(),
                    value: URL_SAFE_NO_PAD.encode(&framed),
                })
            });

        let key = AzureKeyVaultKey::new(Arc::new(client), "versioned-key-id".to_string());
        let ciphertext = key.encrypt(b"root", Some(&context)).await.unwrap();
        assert_eq!(
            key.decrypt(&ciphertext, Some(&context)).await.unwrap(),
            b"root"
        );
        assert!(key.decrypt(&ciphertext, None).await.is_err());
    }
}
