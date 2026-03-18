//! Credential bridges from `*ClientConfig` to `object_store::CredentialProvider`.
//!
//! Each bridge implements `CredentialProvider` for the corresponding cloud's credential type,
//! delegating to the existing `*ClientConfigExt` methods for token acquisition and caching
//! the result with a TTL.

use async_trait::async_trait;
use object_store::CredentialProvider;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// How long to cache credentials before refreshing.
/// Tokens typically last 1 hour; we refresh after 45 minutes to avoid edge-case expiry.
const CACHE_TTL: Duration = Duration::from_secs(45 * 60);

/// A cached credential with an expiry timestamp.
#[derive(Debug)]
struct CachedCredential<T> {
    credential: Arc<T>,
    expires_at: Instant,
}

/// Map an `AlienError` to an `object_store::Error::Generic`.
fn to_object_store_error(
    store: &'static str,
    err: impl std::error::Error + Send + Sync + 'static,
) -> object_store::Error {
    object_store::Error::Generic {
        store,
        source: Box::new(err),
    }
}

// ---------------------------------------------------------------------------
// GCP
// ---------------------------------------------------------------------------

#[cfg(feature = "gcp")]
mod gcp {
    use super::*;
    use alien_gcp_clients::GcpClientConfigExt;
    use object_store::gcp::GcpCredential;

    /// Bridges `GcpClientConfig` to `object_store::CredentialProvider<Credential = GcpCredential>`.
    #[derive(Debug)]
    pub(crate) struct GcpCredentialBridge {
        config: alien_core::GcpClientConfig,
        cache: Mutex<Option<CachedCredential<GcpCredential>>>,
    }

    impl GcpCredentialBridge {
        pub(crate) fn new(config: alien_core::GcpClientConfig) -> Self {
            Self {
                config,
                cache: Mutex::new(None),
            }
        }
    }

    #[async_trait]
    impl CredentialProvider for GcpCredentialBridge {
        type Credential = GcpCredential;

        async fn get_credential(&self) -> object_store::Result<Arc<GcpCredential>> {
            let mut cache = self.cache.lock().await;
            if let Some(ref cached) = *cache {
                if Instant::now() < cached.expires_at {
                    return Ok(Arc::clone(&cached.credential));
                }
            }

            let token = self
                .config
                // For service-account JWT credentials, GCP expects the JWT audience
                // to match the target service endpoint.
                .get_bearer_token("https://storage.googleapis.com/")
                .await
                .map_err(|e| to_object_store_error("GCS", e))?;

            let credential = Arc::new(GcpCredential { bearer: token });
            *cache = Some(CachedCredential {
                credential: Arc::clone(&credential),
                expires_at: Instant::now() + CACHE_TTL,
            });
            Ok(credential)
        }
    }
}

#[cfg(feature = "gcp")]
pub(crate) use gcp::GcpCredentialBridge;

// ---------------------------------------------------------------------------
// AWS
// ---------------------------------------------------------------------------

#[cfg(feature = "aws")]
mod aws {
    use super::*;
    use alien_aws_clients::AwsClientConfigExt;
    use alien_core::AwsCredentials;
    use object_store::aws::AwsCredential;

    /// Bridges `AwsClientConfig` to `object_store::CredentialProvider<Credential = AwsCredential>`.
    ///
    /// The original config is kept immutable so that WebIdentity credentials can always
    /// perform a fresh STS exchange when the cache expires. Temporary STS credentials
    /// have their own expiry (~1 hour), so we must re-exchange rather than reuse stale keys.
    #[derive(Debug)]
    pub(crate) struct AwsCredentialBridge {
        config: alien_core::AwsClientConfig,
        cache: Mutex<Option<CachedCredential<AwsCredential>>>,
    }

    impl AwsCredentialBridge {
        pub(crate) fn new(config: alien_core::AwsClientConfig) -> Self {
            Self {
                config,
                cache: Mutex::new(None),
            }
        }
    }

    #[async_trait]
    impl CredentialProvider for AwsCredentialBridge {
        type Credential = AwsCredential;

        async fn get_credential(&self) -> object_store::Result<Arc<AwsCredential>> {
            let mut cache = self.cache.lock().await;
            if let Some(ref cached) = *cache {
                if Instant::now() < cached.expires_at {
                    return Ok(Arc::clone(&cached.credential));
                }
            }

            let credential = match &self.config.credentials {
                AwsCredentials::AccessKeys {
                    access_key_id,
                    secret_access_key,
                    session_token,
                } => Arc::new(AwsCredential {
                    key_id: access_key_id.clone(),
                    secret_key: secret_access_key.clone(),
                    token: session_token.clone(),
                }),
                AwsCredentials::WebIdentity { .. } => {
                    let resolved = self
                        .config
                        .get_web_identity_credentials()
                        .await
                        .map_err(|e| to_object_store_error("S3", e))?;

                    match resolved.credentials {
                        AwsCredentials::AccessKeys {
                            access_key_id,
                            secret_access_key,
                            session_token,
                        } => Arc::new(AwsCredential {
                            key_id: access_key_id,
                            secret_key: secret_access_key,
                            token: session_token,
                        }),
                        _ => unreachable!("get_web_identity_credentials always returns AccessKeys"),
                    }
                }
            };

            *cache = Some(CachedCredential {
                credential: Arc::clone(&credential),
                expires_at: Instant::now() + CACHE_TTL,
            });
            Ok(credential)
        }
    }
}

#[cfg(feature = "aws")]
pub(crate) use aws::AwsCredentialBridge;

// ---------------------------------------------------------------------------
// Azure
// ---------------------------------------------------------------------------

#[cfg(feature = "azure")]
mod azure {
    use super::*;
    use alien_azure_clients::AzureClientConfigExt;
    use object_store::azure::AzureCredential;

    /// Bridges `AzureClientConfig` to `object_store::CredentialProvider<Credential = AzureCredential>`.
    #[derive(Debug)]
    pub(crate) struct AzureCredentialBridge {
        config: alien_core::AzureClientConfig,
        cache: Mutex<Option<CachedCredential<AzureCredential>>>,
    }

    impl AzureCredentialBridge {
        pub(crate) fn new(config: alien_core::AzureClientConfig) -> Self {
            Self {
                config,
                cache: Mutex::new(None),
            }
        }
    }

    #[async_trait]
    impl CredentialProvider for AzureCredentialBridge {
        type Credential = AzureCredential;

        async fn get_credential(&self) -> object_store::Result<Arc<AzureCredential>> {
            let mut cache = self.cache.lock().await;
            if let Some(ref cached) = *cache {
                if Instant::now() < cached.expires_at {
                    return Ok(Arc::clone(&cached.credential));
                }
            }

            let token = self
                .config
                .get_bearer_token_with_scope("https://storage.azure.com/.default")
                .await
                .map_err(|e| to_object_store_error("AzureBlob", e))?;

            let credential = Arc::new(AzureCredential::BearerToken(token));
            *cache = Some(CachedCredential {
                credential: Arc::clone(&credential),
                expires_at: Instant::now() + CACHE_TTL,
            });
            Ok(credential)
        }
    }
}

#[cfg(feature = "azure")]
pub(crate) use azure::AzureCredentialBridge;
