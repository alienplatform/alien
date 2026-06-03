use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

use alien_client_core::{ErrorData, Result};
#[cfg(any(test, feature = "test-utils"))]
use alien_core::AwsServiceOverrides;
use alien_core::{AwsClientConfig, AwsCredentials};
use alien_error::{AlienError, Context, IntoAlienError};
use aws_credential_types::Credentials;

/// How many seconds before expiry to proactively refresh credentials.
const REFRESH_BUFFER_SECS: u64 = 300; // 5 minutes

/// Thread-safe credential provider that auto-refreshes WebIdentity/IRSA credentials.
///
/// For static `AccessKeys`, this is a zero-cost wrapper — `get_credentials()` returns
/// the same credentials every time and `ensure_fresh()` is a no-op.
///
/// For `WebIdentity` (IRSA), credentials are resolved via STS on first use and
/// automatically refreshed before expiry. The refresh is coordinated via a
/// `tokio::sync::Mutex` to prevent thundering herd, while reads use `std::sync::RwLock`
/// for lock-free access in `sign_config()`.
#[derive(Debug, Clone)]
pub struct AwsCredentialProvider {
    inner: std::sync::Arc<CredentialProviderInner>,
}

struct CredentialProviderInner {
    /// Original config — preserves refreshable source variants for re-exchange.
    original_config: AwsClientConfig,
    /// Cached resolved credentials.
    cached: RwLock<CachedCredentials>,
    /// Prevents concurrent refresh attempts.
    refresh_lock: tokio::sync::Mutex<()>,
}

impl std::fmt::Debug for CredentialProviderInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialProviderInner")
            .field("region", &self.original_config.region)
            .field("account_id", &self.original_config.account_id)
            .finish()
    }
}

#[derive(Clone)]
struct CachedCredentials {
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
    /// Unix timestamp (seconds). 0 means static credentials that never expire.
    expires_at: u64,
}

impl AwsCredentialProvider {
    /// Create a credential provider from an `AwsClientConfig`.
    ///
    /// For `AccessKeys`, credentials are used directly (no STS call).
    /// For `WebIdentity`, an initial STS exchange is performed to resolve credentials.
    pub async fn from_config(config: AwsClientConfig) -> Result<Self> {
        let (cached, original_config) = match &config.credentials {
            AwsCredentials::AccessKeys {
                access_key_id,
                secret_access_key,
                session_token,
            } => {
                let cached = CachedCredentials {
                    access_key_id: access_key_id.clone(),
                    secret_access_key: secret_access_key.clone(),
                    session_token: session_token.clone(),
                    expires_at: 0, // Never expires
                };
                (cached, config)
            }
            AwsCredentials::SessionCredentials {
                access_key_id,
                secret_access_key,
                session_token,
                expires_at,
            } => {
                let cached = CachedCredentials {
                    access_key_id: access_key_id.clone(),
                    secret_access_key: secret_access_key.clone(),
                    session_token: Some(session_token.clone()),
                    expires_at: parse_expires_at(expires_at)?,
                };
                (cached, config)
            }
            AwsCredentials::WebIdentity { .. }
            | AwsCredentials::Imds { .. }
            | AwsCredentials::Profile { .. } => {
                use crate::aws::AwsClientConfigExt;
                let resolved = config.get_web_identity_credentials().await?;
                let cached = cached_credentials_from_resolved(&resolved.credentials)?;
                (cached, config)
            }
        };

        Ok(Self {
            inner: std::sync::Arc::new(CredentialProviderInner {
                original_config,
                cached: RwLock::new(cached),
                refresh_lock: tokio::sync::Mutex::new(()),
            }),
        })
    }

    /// Create a credential provider for testing with mock/static credentials.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn from_config_sync(config: AwsClientConfig) -> Self {
        let cached = match &config.credentials {
            AwsCredentials::AccessKeys {
                access_key_id,
                secret_access_key,
                session_token,
            } => CachedCredentials {
                access_key_id: access_key_id.clone(),
                secret_access_key: secret_access_key.clone(),
                session_token: session_token.clone(),
                expires_at: 0,
            },
            AwsCredentials::SessionCredentials {
                access_key_id,
                secret_access_key,
                session_token,
                expires_at,
            } => CachedCredentials {
                access_key_id: access_key_id.clone(),
                secret_access_key: secret_access_key.clone(),
                session_token: Some(session_token.clone()),
                expires_at: parse_expires_at(expires_at).expect("valid AWS expiration"),
            },
            AwsCredentials::WebIdentity { .. }
            | AwsCredentials::Imds { .. }
            | AwsCredentials::Profile { .. } => {
                panic!("Cannot create sync credential provider from refreshable source config")
            }
        };

        Self {
            inner: std::sync::Arc::new(CredentialProviderInner {
                original_config: config,
                cached: RwLock::new(cached),
                refresh_lock: tokio::sync::Mutex::new(()),
            }),
        }
    }

    /// Get current credentials for request signing. This is a cheap, synchronous operation.
    pub fn get_credentials(&self) -> Credentials {
        let cached = self.inner.cached.read().unwrap();
        Credentials::new(
            cached.access_key_id.clone(),
            cached.secret_access_key.clone(),
            cached.session_token.clone(),
            None,
            "AwsCredentialProvider",
        )
    }

    /// Ensure credentials are fresh. Call this at the top of each async request method.
    ///
    /// For static credentials, this is a no-op. For WebIdentity, it checks expiry and
    /// refreshes via STS if needed. Only one refresh happens at a time (coordinated via
    /// `tokio::Mutex`).
    pub async fn ensure_fresh(&self) -> Result<()> {
        let expires_at = {
            let cached = self.inner.cached.read().unwrap();
            cached.expires_at
        };

        // Static credentials never expire.
        if expires_at == 0 {
            return Ok(());
        }

        let now = now_secs();
        if now + REFRESH_BUFFER_SECS < expires_at {
            return Ok(()); // Still valid
        }

        // Need refresh — acquire the refresh lock to prevent thundering herd
        let _guard = self.inner.refresh_lock.lock().await;

        // Double-check after acquiring lock (another task may have refreshed)
        {
            let cached = self.inner.cached.read().unwrap();
            if now_secs() + REFRESH_BUFFER_SECS < cached.expires_at {
                return Ok(());
            }
        }

        if matches!(
            self.inner.original_config.credentials,
            AwsCredentials::SessionCredentials { .. }
        ) {
            return Err(AlienError::new(ErrorData::AuthenticationError {
                message:
                    "AWS session credentials have expired and cannot be refreshed from their source"
                        .to_string(),
            }));
        }

        tracing::info!("Refreshing AWS credentials");

        use crate::aws::AwsClientConfigExt;
        let resolved = self
            .inner
            .original_config
            .get_web_identity_credentials()
            .await?;

        let fresh = cached_credentials_from_resolved(&resolved.credentials)?;
        let mut cached = self.inner.cached.write().unwrap();
        *cached = fresh;

        tracing::info!("AWS credentials refreshed successfully");
        Ok(())
    }

    /// Get the AWS region from the underlying config.
    pub fn region(&self) -> &str {
        &self.inner.original_config.region
    }

    /// Get the AWS account ID from the underlying config.
    pub fn account_id(&self) -> &str {
        &self.inner.original_config.account_id
    }

    /// Get a service endpoint override, if configured.
    pub fn get_service_endpoint_option(&self, service_name: &str) -> Option<&str> {
        self.inner
            .original_config
            .service_overrides
            .as_ref()
            .and_then(|overrides| overrides.endpoints.get(service_name))
            .map(|s| s.as_str())
    }

    /// Get service endpoint, checking for overrides first.
    pub fn get_service_endpoint(&self, service_name: &str, default_endpoint: &str) -> String {
        self.get_service_endpoint_option(service_name)
            .map(|s| s.to_string())
            .unwrap_or_else(|| default_endpoint.to_string())
    }

    /// Get the underlying config (needed for STS operations and impersonation).
    pub fn config(&self) -> &AwsClientConfig {
        &self.inner.original_config
    }

    /// Creates a new credential provider targeting a different region but
    /// sharing the same credentials. Useful for cross-region ECR operations
    /// (e.g., setting repo policies on replicated repos in target regions).
    pub async fn with_region(&self, region: &str) -> Result<Self> {
        let mut config = self.inner.original_config.clone();
        config.region = region.to_string();
        Self::from_config(config).await
    }

    /// Create a provider with service endpoint overrides for testing.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn with_service_overrides(self, overrides: AwsServiceOverrides) -> Self {
        let mut config = self.inner.original_config.clone();
        config.service_overrides = Some(overrides);
        let cached = self.inner.cached.read().unwrap().clone();
        Self {
            inner: std::sync::Arc::new(CredentialProviderInner {
                original_config: config,
                cached: RwLock::new(cached),
                refresh_lock: tokio::sync::Mutex::new(()),
            }),
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn parse_expires_at(expires_at: &str) -> Result<u64> {
    chrono::DateTime::parse_from_rfc3339(expires_at)
        .map(|dt| dt.timestamp().max(0) as u64)
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: format!("Invalid AWS credential expiration timestamp: {expires_at}"),
            errors: None,
        })
}

fn cached_credentials_from_resolved(credentials: &AwsCredentials) -> Result<CachedCredentials> {
    match credentials {
        AwsCredentials::AccessKeys {
            access_key_id,
            secret_access_key,
            session_token,
        } => Ok(CachedCredentials {
            access_key_id: access_key_id.clone(),
            secret_access_key: secret_access_key.clone(),
            session_token: session_token.clone(),
            expires_at: 0,
        }),
        AwsCredentials::SessionCredentials {
            access_key_id,
            secret_access_key,
            session_token,
            expires_at,
        } => Ok(CachedCredentials {
            access_key_id: access_key_id.clone(),
            secret_access_key: secret_access_key.clone(),
            session_token: Some(session_token.clone()),
            expires_at: parse_expires_at(expires_at)?,
        }),
        AwsCredentials::WebIdentity { .. }
        | AwsCredentials::Imds { .. }
        | AwsCredentials::Profile { .. } => Err(AlienError::new(ErrorData::InvalidClientConfig {
            message: "AWS credential source did not resolve to concrete credentials".to_string(),
            errors: None,
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with_credentials(credentials: AwsCredentials) -> AwsClientConfig {
        AwsClientConfig {
            account_id: "123456789012".to_string(),
            region: "us-east-1".to_string(),
            credentials,
            service_overrides: None,
        }
    }

    #[test]
    fn session_credentials_keep_session_token() {
        let provider = AwsCredentialProvider::from_config_sync(config_with_credentials(
            AwsCredentials::SessionCredentials {
                access_key_id: "access-key".to_string(),
                secret_access_key: "secret-key".to_string(),
                session_token: "session-token".to_string(),
                expires_at: "2099-01-01T00:00:00Z".to_string(),
            },
        ));

        let credentials = provider.get_credentials();
        assert_eq!(credentials.access_key_id(), "access-key");
        assert_eq!(credentials.secret_access_key(), "secret-key");
        assert_eq!(credentials.session_token(), Some("session-token"));
    }

    #[tokio::test]
    async fn expired_session_credentials_fail_instead_of_reusing_stale_credentials() {
        let provider = AwsCredentialProvider::from_config(config_with_credentials(
            AwsCredentials::SessionCredentials {
                access_key_id: "access-key".to_string(),
                secret_access_key: "secret-key".to_string(),
                session_token: "session-token".to_string(),
                expires_at: "1970-01-01T00:00:01Z".to_string(),
            },
        ))
        .await
        .expect("provider");

        let error = provider.ensure_fresh().await.expect_err("expired");
        assert!(format!("{error}").contains("cannot be refreshed from their source"));
    }
}
