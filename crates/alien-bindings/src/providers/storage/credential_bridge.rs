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
    use alien_aws_clients::AwsCredentialProvider;
    use object_store::aws::AwsCredential;

    /// Bridges `AwsCredentialProvider` to `object_store::CredentialProvider<Credential = AwsCredential>`.
    ///
    /// Delegates credential refresh to `AwsCredentialProvider` which handles WebIdentity/IRSA
    /// token exchange and caching. This bridge only converts the credential format for `object_store`.
    #[derive(Debug)]
    pub(crate) struct AwsCredentialBridge {
        provider: AwsCredentialProvider,
    }

    impl AwsCredentialBridge {
        pub(crate) fn new(provider: AwsCredentialProvider) -> Self {
            Self { provider }
        }
    }

    #[async_trait]
    impl CredentialProvider for AwsCredentialBridge {
        type Credential = AwsCredential;

        async fn get_credential(&self) -> object_store::Result<Arc<AwsCredential>> {
            self.provider
                .ensure_fresh()
                .await
                .map_err(|e| to_object_store_error("S3", e))?;

            let creds = self.provider.get_credentials();
            let credential = Arc::new(AwsCredential {
                key_id: creds.access_key_id().to_string(),
                secret_key: creds.secret_access_key().to_string(),
                token: creds.session_token().map(|s| s.to_string()),
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
