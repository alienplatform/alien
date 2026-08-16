use super::{encode_context, frame, unframe};
use crate::error::{ErrorData, Result};
use crate::traits::{Binding, Key};
use alien_aws_clients::kms::{DecryptRequest, EncryptRequest, KmsApi};
use alien_error::{Context, IntoAlienError};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug)]
pub struct AwsKmsKey {
    client: Arc<dyn KmsApi>,
    key_arn: String,
}

impl AwsKmsKey {
    pub fn new(client: Arc<dyn KmsApi>, key_arn: String) -> Self {
        Self { client, key_arn }
    }
}

impl Binding for AwsKmsKey {}

#[async_trait]
impl Key for AwsKmsKey {
    async fn encrypt(
        &self,
        plaintext: &[u8],
        context: Option<&BTreeMap<String, String>>,
    ) -> Result<Vec<u8>> {
        let canonical = encode_context(context)?;
        let response = self
            .client
            .encrypt(
                EncryptRequest::builder(
                    self.key_arn.clone(),
                    STANDARD.encode(frame(plaintext, &canonical)?),
                )
                .maybe_encryption_context(context.cloned())
                .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "AWS KMS encrypt failed".to_string(),
                resource_id: None,
            })?;
        STANDARD
            .decode(response.ciphertext_blob)
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "AWS KMS returned invalid ciphertext encoding".to_string(),
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
                DecryptRequest::builder(self.key_arn.clone(), STANDARD.encode(ciphertext))
                    .maybe_encryption_context(context.cloned())
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "AWS KMS decrypt failed".to_string(),
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
    use alien_aws_clients::kms::{DecryptResponse, EncryptResponse, MockKmsApi};

    #[tokio::test]
    async fn passes_context_to_kms_and_validates_it_after_decrypt() {
        let context = BTreeMap::from([("tenant".to_string(), "acme".to_string())]);
        let canonical = encode_context(Some(&context)).unwrap();
        let framed = frame(b"root", &canonical).unwrap();
        let expected_context = context.clone();
        let mut client = MockKmsApi::new();
        client
            .expect_encrypt()
            .withf(move |request| {
                request.key_id == "key-arn"
                    && request.encryption_context.as_ref() == Some(&expected_context)
            })
            .returning(|_| {
                Ok(EncryptResponse {
                    ciphertext_blob: STANDARD.encode(b"ciphertext"),
                    key_id: "key-arn".to_string(),
                })
            });
        let decrypt_context = context.clone();
        client
            .expect_decrypt()
            .withf(move |request| {
                request.key_id == "key-arn"
                    && request.ciphertext_blob == STANDARD.encode(b"ciphertext")
                    && request.encryption_context.as_ref() == Some(&decrypt_context)
            })
            .returning(move |_| {
                Ok(DecryptResponse {
                    plaintext: STANDARD.encode(&framed),
                    key_id: "key-arn".to_string(),
                })
            });

        let key = AwsKmsKey::new(Arc::new(client), "key-arn".to_string());
        let ciphertext = key.encrypt(b"root", Some(&context)).await.unwrap();
        assert_eq!(
            key.decrypt(&ciphertext, Some(&context)).await.unwrap(),
            b"root"
        );
    }
}
