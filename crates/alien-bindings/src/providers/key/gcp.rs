use super::{encode_context, frame, unframe};
use crate::error::{ErrorData, Result};
use crate::traits::{Binding, Key};
use alien_error::{Context, IntoAlienError};
use alien_gcp_clients::cloud_kms::{CloudKmsApi, DecryptRequest, EncryptRequest};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug)]
pub struct GcpCloudKmsKey {
    client: Arc<dyn CloudKmsApi>,
    crypto_key_name: String,
}

impl GcpCloudKmsKey {
    pub fn new(client: Arc<dyn CloudKmsApi>, crypto_key_name: String) -> Self {
        Self {
            client,
            crypto_key_name,
        }
    }
}

impl Binding for GcpCloudKmsKey {}

#[async_trait]
impl Key for GcpCloudKmsKey {
    async fn encrypt(
        &self,
        plaintext: &[u8],
        context: Option<&BTreeMap<String, String>>,
    ) -> Result<Vec<u8>> {
        let canonical = encode_context(context)?;
        let response = self
            .client
            .encrypt(
                &self.crypto_key_name,
                EncryptRequest {
                    plaintext: STANDARD.encode(frame(plaintext, &canonical)?),
                    additional_authenticated_data: Some(STANDARD.encode(&canonical)),
                },
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "GCP Cloud KMS encrypt failed".to_string(),
                resource_id: None,
            })?;
        STANDARD
            .decode(response.ciphertext)
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "GCP Cloud KMS returned invalid ciphertext encoding".to_string(),
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
                &self.crypto_key_name,
                DecryptRequest {
                    ciphertext: STANDARD.encode(ciphertext),
                    additional_authenticated_data: Some(STANDARD.encode(&canonical)),
                },
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "GCP Cloud KMS decrypt failed".to_string(),
                resource_id: None,
            })?;
        let framed = STANDARD
            .decode(response.plaintext)
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
    use alien_gcp_clients::cloud_kms::{DecryptResponse, EncryptResponse, MockCloudKmsApi};

    #[tokio::test]
    async fn passes_canonical_context_as_aad() {
        let context = BTreeMap::from([("tenant".to_string(), "acme".to_string())]);
        let canonical = encode_context(Some(&context)).unwrap();
        let expected_aad = STANDARD.encode(&canonical);
        let framed = frame(b"root", &canonical).unwrap();
        let encrypt_aad = expected_aad.clone();
        let mut client = MockCloudKmsApi::new();
        client
            .expect_encrypt()
            .withf(move |name, request| {
                name == "key-name"
                    && request.additional_authenticated_data.as_ref() == Some(&encrypt_aad)
            })
            .returning(|_, _| {
                Ok(EncryptResponse {
                    name: "version-name".to_string(),
                    ciphertext: STANDARD.encode(b"ciphertext"),
                })
            });
        client
            .expect_decrypt()
            .withf(move |name, request| {
                name == "key-name"
                    && request.ciphertext == STANDARD.encode(b"ciphertext")
                    && request.additional_authenticated_data.as_ref() == Some(&expected_aad)
            })
            .returning(move |_, _| {
                Ok(DecryptResponse {
                    plaintext: STANDARD.encode(&framed),
                })
            });

        let key = GcpCloudKmsKey::new(Arc::new(client), "key-name".to_string());
        let ciphertext = key.encrypt(b"root", Some(&context)).await.unwrap();
        assert_eq!(
            key.decrypt(&ciphertext, Some(&context)).await.unwrap(),
            b"root"
        );
    }
}
