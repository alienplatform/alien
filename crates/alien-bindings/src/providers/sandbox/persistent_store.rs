//! The binding's side of persistent storage: the key's store as the customer's environment
//! reaches it (full rights, used to claim a key, read files without a sandbox, erase a key and
//! collect garbage), and the grants it mints for the agent, which reach one key only.

use std::sync::Arc;

use alien_error::AlienError;
use alien_sandbox_storage::{
    ErrorData as StorageError, ObjectStore, ObjectVersion, Precondition, PutOutcome,
    Result as StorageResult, StoredObject,
};
use async_trait::async_trait;
use futures::TryStreamExt;
use object_store::path::Path;
use object_store::{GetOptions, GetRange, PutMode, PutOptions, UpdateVersion};

fn failed(
    operation: &str,
    object: &str,
    error: impl std::fmt::Display,
) -> AlienError<StorageError> {
    AlienError::new(StorageError::StoreFailed {
        operation: operation.to_string(),
        object: object.to_string(),
        reason: error.to_string(),
    })
}

/// One key of a persistent store, over any `object_store` backend (S3, GCS, Azure Blob, local
/// files). Object names are relative to `<prefix><key>/`.
pub struct KeyStore {
    inner: Arc<dyn object_store::ObjectStore>,
    root: String,
}

impl KeyStore {
    /// The key `key` under `prefix` in `inner`.
    pub fn new(
        inner: Arc<dyn object_store::ObjectStore>,
        prefix: &str,
        key: &str,
    ) -> StorageResult<Self> {
        Ok(Self {
            inner,
            root: format!("{prefix}{}", alien_sandbox_storage::key_root(key)?),
        })
    }

    /// The full object key of `object`.
    pub fn object_key(&self, object: &str) -> String {
        format!("{}{object}", self.root)
    }

    fn path(&self, object: &str) -> Path {
        Path::from(self.object_key(object))
    }
}

fn version_of(meta: &object_store::ObjectMeta) -> ObjectVersion {
    ObjectVersion {
        etag: meta.e_tag.clone().unwrap_or_default(),
        version: meta.version.clone(),
    }
}

#[async_trait]
impl ObjectStore for KeyStore {
    async fn get(&self, object: &str) -> StorageResult<Option<StoredObject>> {
        match self.inner.get(&self.path(object)).await {
            Ok(result) => {
                let version = version_of(&result.meta);
                let bytes = result.bytes().await.map_err(|e| failed("get", object, e))?;
                Ok(Some(StoredObject {
                    bytes: bytes.to_vec(),
                    version,
                }))
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(error) => Err(failed("get", object, error)),
        }
    }

    async fn get_range(&self, object: &str, offset: u64, len: u64) -> StorageResult<Vec<u8>> {
        let options = GetOptions {
            range: Some(GetRange::Bounded(offset..offset + len)),
            ..Default::default()
        };
        let result = self
            .inner
            .get_opts(&self.path(object), options)
            .await
            .map_err(|e| failed("get", object, e))?;
        Ok(result
            .bytes()
            .await
            .map_err(|e| failed("get", object, e))?
            .to_vec())
    }

    async fn put_if(
        &self,
        object: &str,
        bytes: Vec<u8>,
        precondition: Precondition,
    ) -> StorageResult<PutOutcome> {
        let mode = match precondition {
            Precondition::Absent => PutMode::Create,
            Precondition::Matches(version) => PutMode::Update(UpdateVersion {
                e_tag: Some(version.etag).filter(|e| !e.is_empty()),
                version: version.version,
            }),
        };
        let options = PutOptions {
            mode,
            ..Default::default()
        };
        match self
            .inner
            .put_opts(&self.path(object), bytes.into(), options)
            .await
        {
            Ok(result) => Ok(PutOutcome::Written {
                version: ObjectVersion {
                    etag: result.e_tag.unwrap_or_default(),
                    version: result.version,
                },
            }),
            Err(object_store::Error::AlreadyExists { .. })
            | Err(object_store::Error::Precondition { .. }) => Ok(PutOutcome::Conflict),
            Err(error) => Err(failed("put", object, error)),
        }
    }

    async fn list(&self, prefix: &str) -> StorageResult<Vec<String>> {
        let root = self.root.clone();
        let listed: Vec<object_store::ObjectMeta> = self
            .inner
            .list(Some(&Path::from(format!("{root}{prefix}"))))
            .try_collect()
            .await
            .map_err(|e| failed("list", prefix, e))?;
        Ok(listed
            .into_iter()
            .filter_map(|meta| {
                meta.location
                    .as_ref()
                    .strip_prefix(root.as_str())
                    .map(str::to_string)
            })
            .filter(|name| name.starts_with(prefix))
            .collect())
    }

    async fn delete(&self, object: &str) -> StorageResult<()> {
        match self.inner.delete(&self.path(object)).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(error) => Err(failed("delete", object, error)),
        }
    }
}

/// S3 grants: a SigV4 POST policy for `<prefix><key>/`, a presigned conditional PUT for HEAD and
/// presigned GETs, all signed with the binding's own credentials and expiring with them.
#[cfg(feature = "aws")]
pub mod s3 {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use alien_sandbox_storage::{S3PostPolicy, StorageGrant, HEAD_OBJECT};
    use chrono::{DateTime, Utc};
    use hmac::{Hmac, Mac};
    use object_store::signer::Signer;
    use sha2::Sha256;

    use super::*;

    /// Signs grants for one persistent store.
    pub struct S3Grants {
        bucket: String,
        region: String,
        prefix: String,
        credentials: alien_aws_clients::AwsCredentialProvider,
        store: Arc<object_store::aws::AmazonS3>,
        /// Largest object a POST may upload.
        max_object_bytes: u64,
    }

    fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
        let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes any key length");
        mac.update(data);
        mac.finalize().into_bytes().to_vec()
    }

    /// The SigV4 signature of a POST policy (the base64 policy is the string to sign).
    pub fn sign_policy(secret: &str, date: &str, region: &str, policy_b64: &str) -> String {
        let key = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes());
        let key = hmac(&key, region.as_bytes());
        let key = hmac(&key, b"s3");
        let key = hmac(&key, b"aws4_request");
        hex::encode(hmac(&key, policy_b64.as_bytes()))
    }

    impl std::fmt::Debug for S3Grants {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            // Never the credentials.
            f.debug_struct("S3Grants")
                .field("bucket", &self.bucket)
                .field("region", &self.region)
                .field("prefix", &self.prefix)
                .finish_non_exhaustive()
        }
    }

    impl S3Grants {
        /// Grants for `bucket` (keys under `prefix`), signed with `credentials`.
        pub fn new(
            bucket: String,
            prefix: String,
            credentials: alien_aws_clients::AwsCredentialProvider,
            max_object_bytes: u64,
        ) -> crate::error::Result<Self> {
            let region = credentials.region().to_string();
            let store = object_store::aws::AmazonS3Builder::new()
                .with_bucket_name(&bucket)
                .with_region(&region)
                .with_credentials(Arc::new(
                    crate::providers::storage::credential_bridge::AwsCredentialBridge::new(
                        credentials.clone(),
                    ),
                ))
                .with_conditional_put(object_store::aws::S3ConditionalPut::ETagMatch)
                .build()
                .map_err(|error| {
                    AlienError::new(crate::error::ErrorData::BindingSetupFailed {
                        binding_type: "sandbox persistent storage".to_string(),
                        reason: format!("cannot build the S3 client for '{bucket}': {error}"),
                    })
                })?;
            Ok(Self {
                bucket,
                region,
                prefix,
                credentials,
                store: Arc::new(store),
                max_object_bytes,
            })
        }

        async fn sign(
            &self,
            method: http::Method,
            keys: &KeyStore,
            object: &str,
            ttl: Duration,
        ) -> StorageResult<String> {
            self.store
                .signed_url(method, &Path::from(keys.object_key(object)), ttl)
                .await
                .map(|url| url.to_string())
                .map_err(|e| failed("sign", object, e))
        }

        /// One key with the binding's full rights.
        pub fn key_store(&self, key: &str) -> StorageResult<KeyStore> {
            KeyStore::new(self.store.clone(), &self.prefix, key)
        }

        /// A grant to write `key` and read `reads` (object names relative to the key), valid for
        /// `ttl` or until the binding's credentials expire, whichever is first.
        pub async fn grant(
            &self,
            key: &str,
            reads: &[String],
            ttl: Duration,
        ) -> StorageResult<StorageGrant> {
            self.credentials
                .ensure_fresh()
                .await
                .map_err(|e| failed("sign", key, e))?;
            let credentials = self.credentials.get_credentials();
            let now = Utc::now();
            let mut expires_at = now + chrono::Duration::from_std(ttl).expect("a short ttl");
            if let Some(expiry) = credentials.expiry() {
                let expiry: DateTime<Utc> = expiry.into();
                expires_at = expires_at.min(expiry);
            }
            let ttl = (expires_at - now).to_std().unwrap_or_default();
            let keys = self.key_store(key)?;
            let key_prefix = keys.object_key("");

            let date = now.format("%Y%m%d").to_string();
            let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
            let credential = format!(
                "{}/{date}/{}/s3/aws4_request",
                credentials.access_key_id(),
                self.region
            );
            let mut conditions = vec![
                serde_json::json!({ "bucket": self.bucket }),
                serde_json::json!(["starts-with", "$key", key_prefix]),
                serde_json::json!(["content-length-range", 0, self.max_object_bytes]),
                serde_json::json!({ "x-amz-algorithm": "AWS4-HMAC-SHA256" }),
                serde_json::json!({ "x-amz-credential": credential }),
                serde_json::json!({ "x-amz-date": amz_date }),
            ];
            let mut fields = BTreeMap::from([
                (
                    "x-amz-algorithm".to_string(),
                    "AWS4-HMAC-SHA256".to_string(),
                ),
                ("x-amz-credential".to_string(), credential.clone()),
                ("x-amz-date".to_string(), amz_date.clone()),
            ]);
            if let Some(token) = credentials.session_token() {
                conditions.push(serde_json::json!({ "x-amz-security-token": token }));
                fields.insert("x-amz-security-token".to_string(), token.to_string());
            }
            let policy = serde_json::json!({
                "expiration": expires_at.format("%Y-%m-%dT%H:%M:%S.000Z").to_string(),
                "conditions": conditions,
            });
            use base64::Engine;
            let policy_b64 =
                base64::engine::general_purpose::STANDARD.encode(policy.to_string().as_bytes());
            let signature = sign_policy(
                credentials.secret_access_key(),
                &date,
                &self.region,
                &policy_b64,
            );
            fields.insert("policy".to_string(), policy_b64);
            fields.insert("x-amz-signature".to_string(), signature);

            let head_put_url = self
                .sign(http::Method::PUT, &keys, HEAD_OBJECT, ttl)
                .await?;
            let mut read_urls = BTreeMap::new();
            for object in reads {
                read_urls.insert(
                    object.clone(),
                    self.sign(http::Method::GET, &keys, object, ttl).await?,
                );
            }
            Ok(StorageGrant::S3 {
                post: S3PostPolicy {
                    url: format!("https://{}.s3.{}.amazonaws.com/", self.bucket, self.region),
                    fields,
                    key_prefix,
                },
                head_put_url,
                reads: read_urls,
                expires_at,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_sandbox_storage::{read_head, takeover};

    /// The binding's view keeps every key under its own prefix, claims with a conditional put
    /// and lists and erases one key without touching another.
    #[tokio::test]
    async fn a_key_store_claims_lists_and_erases_one_key() {
        let inner: Arc<dyn object_store::ObjectStore> =
            Arc::new(object_store::memory::InMemory::new());
        let a = KeyStore::new(inner.clone(), "p/", "engine-42").unwrap();
        let b = KeyStore::new(inner.clone(), "p/", "engine-43").unwrap();
        let first = takeover(&a, "sandbox-1").await.unwrap();
        takeover(&b, "sandbox-9").await.unwrap();
        let second = takeover(&a, "sandbox-2").await.unwrap();
        assert_ne!(first.head_version, second.head_version);
        assert_eq!(read_head(&a).await.unwrap().unwrap().0.writer, "sandbox-2");
        assert_eq!(
            a.put_if(
                "HEAD",
                b"{}".to_vec(),
                Precondition::Matches(first.head_version)
            )
            .await
            .unwrap(),
            PutOutcome::Conflict,
            "a stale version cannot move HEAD"
        );
        assert_eq!(
            a.put_if("packs/1-x", b"x".to_vec(), Precondition::Absent)
                .await
                .unwrap(),
            PutOutcome::Written {
                version: a.get("packs/1-x").await.unwrap().unwrap().version
            }
        );
        assert_eq!(a.get_range("packs/1-x", 0, 1).await.unwrap(), b"x");
        assert_eq!(a.list("").await.unwrap(), vec!["HEAD", "packs/1-x"]);
        for object in a.list("").await.unwrap() {
            a.delete(&object).await.unwrap();
        }
        assert!(a.list("").await.unwrap().is_empty());
        assert_eq!(b.list("").await.unwrap(), vec!["HEAD"]);
        assert!(KeyStore::new(inner, "p/", "../x").is_err());
    }
}
