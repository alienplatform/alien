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
    use alien_core::{GcpClientConfig, GcpCredentials, GcpImpersonationConfig};
    use alien_error::{AlienError, Context, IntoAlienError};
    use google_cloud_auth::credentials::{
        self, CacheableResource, Credentials, CredentialsProvider, EntityTag,
    };
    use google_cloud_auth::errors::CredentialsError;
    use http::{header::AUTHORIZATION, Extensions, HeaderMap, HeaderValue};
    use object_store::gcp::GcpCredential;
    use serde_json::{json, Value};
    use std::future::Future;

    const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

    /// Bridges `GcpClientConfig` to `object_store::CredentialProvider<Credential = GcpCredential>`.
    #[derive(Debug)]
    pub(crate) struct GcpCredentialBridge {
        credentials: Credentials,
        cache: Mutex<Option<CachedCredential<GcpCredential>>>,
    }

    impl GcpCredentialBridge {
        pub(crate) fn new(config: GcpClientConfig) -> crate::Result<Self> {
            Ok(Self {
                credentials: credentials_from_gcp_config(&config)?,
                cache: Mutex::new(None),
            })
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

            let headers = match self
                .credentials
                .headers(Extensions::new())
                .await
                .map_err(|e| to_object_store_error("GCS", e))?
            {
                CacheableResource::New { data, .. } => data,
                CacheableResource::NotModified => {
                    return Err(object_store::Error::Generic {
                        store: "GCS",
                        source: Box::new(std::io::Error::other(
                            "Google auth returned NotModified without cached headers",
                        )),
                    });
                }
            };

            let token =
                bearer_from_headers(&headers).map_err(|e| to_object_store_error("GCS", e))?;

            let credential = Arc::new(GcpCredential { bearer: token });
            *cache = Some(CachedCredential {
                credential: Arc::clone(&credential),
                expires_at: Instant::now() + CACHE_TTL,
            });
            Ok(credential)
        }
    }

    #[derive(Debug, Clone)]
    struct StaticAccessTokenCredentials {
        token: String,
        entity_tag: EntityTag,
    }

    impl StaticAccessTokenCredentials {
        fn new(token: String) -> Self {
            Self {
                token,
                entity_tag: EntityTag::new(),
            }
        }
    }

    impl CredentialsProvider for StaticAccessTokenCredentials {
        fn headers(
            &self,
            _extensions: Extensions,
        ) -> impl Future<Output = std::result::Result<CacheableResource<HeaderMap>, CredentialsError>>
               + Send {
            let token = self.token.clone();
            let entity_tag = self.entity_tag.clone();
            async move {
                let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
                    .map_err(|error| CredentialsError::from_source(false, error))?;
                value.set_sensitive(true);

                let mut headers = HeaderMap::new();
                headers.insert(AUTHORIZATION, value);

                Ok(CacheableResource::New {
                    entity_tag,
                    data: headers,
                })
            }
        }

        fn universe_domain(&self) -> impl Future<Output = Option<String>> + Send {
            async { None }
        }
    }

    fn credentials_from_gcp_config(config: &GcpClientConfig) -> crate::Result<Credentials> {
        credentials_from_gcp_credentials(&config.credentials)
    }

    fn credentials_from_gcp_credentials(
        credentials: &GcpCredentials,
    ) -> crate::Result<Credentials> {
        match credentials {
            GcpCredentials::AccessToken { token } => {
                Ok(Credentials::from(StaticAccessTokenCredentials::new(token.clone())))
            }
            GcpCredentials::ServiceAccountKey { json } => {
                let key = serde_json::from_str::<Value>(json).into_alien_error().context(
                    crate::ErrorData::BindingSetupFailed {
                        binding_type: "storage.gcs".to_string(),
                        reason: "Failed to parse GCP service account key JSON".to_string(),
                    },
                )?;
                credentials::service_account::Builder::new(key)
                    .with_access_specifier(credentials::service_account::AccessSpecifier::from_scopes(
                        [CLOUD_PLATFORM_SCOPE],
                    ))
                    .build()
                    .into_alien_error()
                    .context(crate::ErrorData::BindingSetupFailed {
                        binding_type: "storage.gcs".to_string(),
                        reason: "Failed to build official GCP service account credentials".to_string(),
                    })
            }
            GcpCredentials::ServiceMetadata => credentials::mds::Builder::default()
                .with_scopes([CLOUD_PLATFORM_SCOPE])
                .build()
                .into_alien_error()
                .context(crate::ErrorData::BindingSetupFailed {
                    binding_type: "storage.gcs".to_string(),
                    reason: "Failed to build official GCP metadata credentials".to_string(),
                }),
            GcpCredentials::ExternalAccount {
                audience,
                subject_token_type,
                token_url,
                credential_source_file,
                service_account_impersonation_url,
            } => {
                let external_account = external_account_json(
                    audience,
                    subject_token_type,
                    token_url,
                    credential_source_file,
                    service_account_impersonation_url.as_deref(),
                );
                credentials::external_account::Builder::new(external_account)
                    .build()
                    .into_alien_error()
                    .context(crate::ErrorData::BindingSetupFailed {
                        binding_type: "storage.gcs".to_string(),
                        reason: "Failed to build official GCP external account credentials".to_string(),
                    })
            }
            GcpCredentials::AuthorizedUser {
                client_id,
                client_secret,
                refresh_token,
            } => {
                let authorized_user = json!({
                    "type": "authorized_user",
                    "client_id": client_id,
                    "client_secret": client_secret,
                    "refresh_token": refresh_token,
                });
                credentials::user_account::Builder::new(authorized_user)
                    .with_scopes([CLOUD_PLATFORM_SCOPE])
                    .build()
                    .into_alien_error()
                    .context(crate::ErrorData::BindingSetupFailed {
                        binding_type: "storage.gcs".to_string(),
                        reason: "Failed to build official GCP authorized user credentials".to_string(),
                    })
            }
            GcpCredentials::ImpersonatedServiceAccount { source, config } => {
                impersonated_credentials_from_gcp_config(source, config)
            }
            GcpCredentials::ProjectedServiceAccount { .. } => Err(AlienError::new(
                crate::ErrorData::BindingSetupFailed {
                    binding_type: "storage.gcs".to_string(),
                    reason: "Projected service account token files are not a complete official Google auth credential configuration; use external_account credentials with an audience and credential source instead".to_string(),
                },
            )),
        }
    }

    fn impersonated_credentials_from_gcp_config(
        source: &GcpClientConfig,
        config: &GcpImpersonationConfig,
    ) -> crate::Result<Credentials> {
        let source_credentials = credentials_from_gcp_config(source)?;
        let mut builder =
            credentials::impersonated::Builder::from_source_credentials(source_credentials)
                .with_target_principal(config.service_account_email.clone())
                .with_scopes(config.scopes.clone());

        if let Some(delegates) = &config.delegates {
            builder = builder.with_delegates(delegates.clone());
        }

        if let Some(lifetime) = &config.lifetime {
            builder = builder.with_lifetime(parse_google_duration(lifetime)?);
        }

        builder
            .build()
            .into_alien_error()
            .context(crate::ErrorData::BindingSetupFailed {
                binding_type: "storage.gcs".to_string(),
                reason: "Failed to build official GCP impersonated credentials".to_string(),
            })
    }

    fn external_account_json(
        audience: &str,
        subject_token_type: &str,
        token_url: &str,
        credential_source_file: &str,
        service_account_impersonation_url: Option<&str>,
    ) -> Value {
        let mut value = json!({
            "type": "external_account",
            "audience": audience,
            "subject_token_type": subject_token_type,
            "token_url": token_url,
            "credential_source": {
                "file": credential_source_file,
            },
            "scopes": [CLOUD_PLATFORM_SCOPE],
        });

        if let Some(url) = service_account_impersonation_url {
            value["service_account_impersonation_url"] = Value::String(url.to_string());
        }

        value
    }

    fn parse_google_duration(value: &str) -> crate::Result<Duration> {
        let seconds = value
            .strip_suffix('s')
            .ok_or_else(|| {
                AlienError::new(crate::ErrorData::BindingSetupFailed {
                    binding_type: "storage.gcs".to_string(),
                    reason: format!("Invalid Google duration '{}': missing 's' suffix", value),
                })
            })?
            .parse::<u64>()
            .into_alien_error()
            .context(crate::ErrorData::BindingSetupFailed {
                binding_type: "storage.gcs".to_string(),
                reason: format!("Invalid Google duration '{}'", value),
            })?;

        Ok(Duration::from_secs(seconds))
    }

    fn bearer_from_headers(headers: &HeaderMap) -> Result<String, std::io::Error> {
        let value = headers
            .get(AUTHORIZATION)
            .ok_or_else(|| std::io::Error::other("Google auth headers missing Authorization"))?
            .to_str()
            .map_err(std::io::Error::other)?;
        value
            .strip_prefix("Bearer ")
            .map(str::to_string)
            .ok_or_else(|| {
                std::io::Error::other("Google auth Authorization header is not a bearer token")
            })
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
    use aws_credential_types::provider::ProvideCredentials;
    use aws_types::SdkConfig;
    use object_store::aws::AwsCredential;

    /// Bridges official AWS SDK credentials to `object_store::CredentialProvider`.
    #[derive(Debug)]
    pub(crate) struct AwsCredentialBridge {
        sdk_config: SdkConfig,
    }

    impl AwsCredentialBridge {
        pub(crate) fn new(sdk_config: SdkConfig) -> Self {
            Self { sdk_config }
        }
    }

    #[async_trait]
    impl CredentialProvider for AwsCredentialBridge {
        type Credential = AwsCredential;

        async fn get_credential(&self) -> object_store::Result<Arc<AwsCredential>> {
            let provider = self.sdk_config.credentials_provider().ok_or_else(|| {
                object_store::Error::Generic {
                    store: "S3",
                    source: Box::new(std::io::Error::other(
                        "AWS SDK config does not have a credentials provider",
                    )),
                }
            })?;

            let creds = provider
                .provide_credentials()
                .await
                .map_err(|e| to_object_store_error("S3", e))?;
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
    use alien_core::{AzureClientConfig, AzureCredentials};
    use alien_error::{AlienError, Context, IntoAlienError};
    use azure_core::{
        cloud::{CloudConfiguration, CustomConfiguration},
        credentials::{
            AccessToken as AzureAccessToken, Secret, TokenCredential, TokenRequestOptions,
        },
        http::ClientOptions,
        time::{Duration as AzureDuration, OffsetDateTime},
    };
    use azure_identity::{
        ClientAssertionCredentialOptions, ClientSecretCredential, ClientSecretCredentialOptions,
        ManagedIdentityCredential, ManagedIdentityCredentialOptions, UserAssignedId,
        WorkloadIdentityCredential, WorkloadIdentityCredentialOptions,
    };
    use object_store::azure::AzureCredential;
    use std::path::PathBuf;

    const AZURE_STORAGE_SCOPE: &str = "https://storage.azure.com/.default";

    /// Bridges `AzureClientConfig` to `object_store::CredentialProvider<Credential = AzureCredential>`.
    #[derive(Debug)]
    pub(crate) struct AzureCredentialBridge {
        credential: Arc<dyn TokenCredential>,
        cache: Mutex<Option<CachedCredential<AzureCredential>>>,
    }

    impl AzureCredentialBridge {
        pub(crate) fn new(config: AzureClientConfig) -> crate::Result<Self> {
            Ok(Self {
                credential: azure_credential_from_config(&config)?,
                cache: Mutex::new(None),
            })
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
                .credential
                .get_token(&[AZURE_STORAGE_SCOPE], None)
                .await
                .into_alien_error()
                .map_err(|e| to_object_store_error("AzureBlob", e))?;

            let credential = Arc::new(AzureCredential::BearerToken(
                token.token.secret().to_string(),
            ));
            *cache = Some(CachedCredential {
                credential: Arc::clone(&credential),
                expires_at: Instant::now() + CACHE_TTL,
            });
            Ok(credential)
        }
    }

    #[derive(Debug)]
    struct StaticAzureAccessTokenCredential {
        token: String,
    }

    #[async_trait]
    impl TokenCredential for StaticAzureAccessTokenCredential {
        async fn get_token(
            &self,
            scopes: &[&str],
            _options: Option<TokenRequestOptions<'_>>,
        ) -> azure_core::Result<AzureAccessToken> {
            if scopes.is_empty() {
                return Err(azure_core::Error::with_message(
                    azure_core::error::ErrorKind::Credential,
                    "no scopes specified",
                ));
            }

            Ok(AzureAccessToken::new(
                self.token.clone(),
                OffsetDateTime::now_utc() + AzureDuration::days(365),
            ))
        }
    }

    fn azure_credential_from_config(
        config: &AzureClientConfig,
    ) -> crate::Result<Arc<dyn TokenCredential>> {
        match &config.credentials {
            AzureCredentials::AccessToken { token } => Ok(Arc::new(StaticAzureAccessTokenCredential {
                token: token.clone(),
            })),
            AzureCredentials::ServicePrincipal {
                client_id,
                client_secret,
            } => ClientSecretCredential::new(
                &config.tenant_id,
                client_id.clone(),
                Secret::new(client_secret.clone()),
                Some(ClientSecretCredentialOptions {
                    client_options: azure_client_options(None),
                }),
            )
            .map(|credential| credential as Arc<dyn TokenCredential>)
            .into_alien_error()
            .context(crate::ErrorData::BindingSetupFailed {
                binding_type: "storage.azureBlob".to_string(),
                reason: "Failed to build official Azure service principal credentials".to_string(),
            }),
            AzureCredentials::WorkloadIdentity {
                client_id,
                tenant_id,
                federated_token_file,
                authority_host,
            } => WorkloadIdentityCredential::new(Some(WorkloadIdentityCredentialOptions {
                credential_options: ClientAssertionCredentialOptions {
                    client_options: azure_client_options(Some(authority_host)),
                },
                client_id: Some(client_id.clone()),
                tenant_id: Some(tenant_id.clone()),
                token_file_path: Some(PathBuf::from(federated_token_file)),
            }))
            .map(|credential| credential as Arc<dyn TokenCredential>)
            .into_alien_error()
            .context(crate::ErrorData::BindingSetupFailed {
                binding_type: "storage.azureBlob".to_string(),
                reason: "Failed to build official Azure workload identity credentials".to_string(),
            }),
            AzureCredentials::VmManagedIdentity {
                client_id,
                identity_endpoint,
            } => {
                if let Some(identity_endpoint) = identity_endpoint {
                    return Err(AlienError::new(crate::ErrorData::BindingSetupFailed {
                        binding_type: "storage.azureBlob".to_string(),
                        reason: format!(
                            "Official Azure ManagedIdentityCredential does not support per-config IMDS endpoint override '{}'; use the standard IMDS endpoint or provide an access token",
                            identity_endpoint
                        ),
                    }));
                }

                ManagedIdentityCredential::new(Some(ManagedIdentityCredentialOptions {
                    user_assigned_id: Some(UserAssignedId::ClientId(client_id.clone())),
                    client_options: azure_client_options(None),
                }))
                .map(|credential| credential as Arc<dyn TokenCredential>)
                .into_alien_error()
                .context(crate::ErrorData::BindingSetupFailed {
                    binding_type: "storage.azureBlob".to_string(),
                    reason: "Failed to build official Azure VM managed identity credentials"
                        .to_string(),
                })
            }
            AzureCredentials::ManagedIdentity {
                client_id,
                identity_endpoint,
                ..
            } => Err(AlienError::new(crate::ErrorData::BindingSetupFailed {
                binding_type: "storage.azureBlob".to_string(),
                reason: format!(
                    "Official Azure ManagedIdentityCredential cannot be constructed from explicit App Service identity endpoint '{}' for client '{}'; use workload identity, VM managed identity, or provide an access token",
                    identity_endpoint, client_id
                ),
            })),
        }
    }

    fn azure_client_options(authority_host: Option<&str>) -> ClientOptions {
        let cloud = authority_host.map(|authority_host| {
            let mut custom = CustomConfiguration::default();
            custom.authority_host = authority_host.to_string();
            Arc::new(CloudConfiguration::Custom(custom))
        });

        ClientOptions {
            cloud,
            ..Default::default()
        }
    }
}

#[cfg(feature = "azure")]
pub(crate) use azure::AzureCredentialBridge;
