//! CI keys in AWS KMS. The key never leaves KMS: CI asks KMS to sign each
//! statement's digest, so a compromised CI runner can request signatures but
//! can't take the key away.

use std::collections::HashMap;

use alien_aws_clients::kms::{CreateKeyRequest, KmsApi, KmsClient, KmsTag, SignRequest};
use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt, AwsCredentialProvider};
use alien_error::{AlienError, Context};
use alien_signing::PublicKey;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use sha2::{Digest, Sha256};

use crate::error::{ErrorData, Result};

/// A KMS signing key and its public half.
pub struct KmsKey {
    client: KmsClient,
    key_id: String,
    public_key: PublicKey,
}

impl KmsKey {
    /// Open `key_id` with the AWS credentials in the environment and check it
    /// is an ECC_NIST_P256 signing key.
    pub async fn open(key_id: &str) -> Result<Self> {
        let client = client().await?;
        let public = client
            .get_public_key(key_id)
            .await
            .context(kms_failed(&format!("read the public key of '{key_id}'")))?;
        if public.key_spec != "ECC_NIST_P256" || public.key_usage != "SIGN_VERIFY" {
            return Err(AlienError::new(ErrorData::SigningFailed {
                message: format!(
                    "KMS key '{key_id}' is {} for {}; signing needs ECC_NIST_P256 for SIGN_VERIFY",
                    public.key_spec, public.key_usage
                ),
            }));
        }
        let public_key = PublicKey::parse(&format!("ecdsa-p256:{}", public.public_key))
            .context(kms_failed("parse the KMS public key"))?;
        Ok(Self {
            client,
            key_id: public.key_id,
            public_key,
        })
    }

    /// Create a new non-exportable P-256 signing key and open it.
    pub async fn create(description: &str) -> Result<Self> {
        let client = client().await?;
        let created = client
            .create_key(
                CreateKeyRequest::builder("ECC_NIST_P256".to_string(), "SIGN_VERIFY".to_string())
                    .description(description.to_string())
                    .tags(vec![KmsTag {
                        tag_key: "purpose".to_string(),
                        tag_value: "release-signing".to_string(),
                    }])
                    .build(),
            )
            .await
            .context(kms_failed("create the signing key"))?;
        Self::open(&created.key_metadata.arn).await
    }

    /// The key's ARN or id, as KMS reported it.
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    /// The public key.
    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    /// Sign `message`. KMS signs the SHA-256 digest computed here, so a
    /// payload of any size can be signed; the result is the same ECDSA
    /// signature over the message that every enforcer verifies.
    pub async fn sign(&self, message: &[u8]) -> Result<String> {
        let digest = Sha256::digest(message);
        let response = self
            .client
            .sign(
                SignRequest::builder(
                    self.key_id.clone(),
                    STANDARD.encode(digest),
                    "DIGEST".to_string(),
                    "ECDSA_SHA_256".to_string(),
                )
                .build(),
            )
            .await
            .context(kms_failed("sign"))?;
        self.public_key
            .verify(message, &response.signature)
            .context(kms_failed("check the KMS signature"))?;
        Ok(response.signature)
    }
}

async fn client() -> Result<KmsClient> {
    let env: HashMap<String, String> = std::env::vars().collect();
    let config = AwsClientConfig::from_env(&env)
        .await
        .context(kms_failed("read AWS credentials from the environment"))?;
    let credentials = AwsCredentialProvider::from_config(config)
        .await
        .context(kms_failed("load AWS credentials"))?;
    Ok(KmsClient::new(crate::http_client::client()?, credentials))
}

fn kms_failed(action: &str) -> ErrorData {
    ErrorData::SigningFailed {
        message: format!("AWS KMS: could not {action}"),
    }
}
