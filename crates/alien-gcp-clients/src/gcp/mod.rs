pub mod api_client;
pub mod artifactregistry;
pub mod cloud_sql;
pub mod cloudasset;
pub mod cloudbuild;
pub mod cloudrun;
pub mod cloudscheduler;
pub mod compute;
pub mod container;
mod credential_config;
mod credential_exchange;
pub mod firestore;
pub mod gcp_request_utils;
pub mod gcs;
pub mod iam;
pub mod longrunning;
pub mod monitoring;
pub mod pubsub;
mod remote_storage_credentials;
pub mod resource_manager;
pub mod secret_manager;
pub mod service_usage;

use alien_client_core::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use chrono::{DateTime, Utc};
use reqwest::Client;
use std::collections::HashMap;

// Re-export types from alien-core
pub use alien_core::{
    GcpClientConfig, GcpCredentials, GcpImpersonationConfig,
    GcpServiceOverrides as ServiceOverrides,
};

/// A GCP access token paired with IAMCredentials' authoritative expiry.
pub struct ExpiringAccessToken {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

impl std::fmt::Debug for ExpiringAccessToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExpiringAccessToken")
            .field("token", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

fn expires_at_from_expires_in(provider: &str, expires_in: i64) -> Result<DateTime<Utc>> {
    if expires_in <= 0 {
        return Err(AlienError::new(ErrorData::InvalidInput {
            message: format!("{provider} returned a non-positive access-token lifetime"),
            field_name: Some("expires_in".to_string()),
        }));
    }
    Utc::now()
        .checked_add_signed(chrono::Duration::seconds(expires_in))
        .ok_or_else(|| {
            AlienError::new(ErrorData::InvalidInput {
                message: format!("{provider} returned an unsupported access-token lifetime"),
                field_name: Some("expires_in".to_string()),
            })
        })
}

/// Trait for GCP platform configuration operations
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait GcpClientConfigExt {
    /// Create a new `GcpClientConfig` from environment variables.
    async fn from_env(environment_variables: &HashMap<String, String>) -> Result<GcpClientConfig>;

    /// Create a new `GcpClientConfig` from standard environment variables.
    async fn from_std_env() -> Result<GcpClientConfig>;

    /// Impersonate a GCP service account and return a new platform config with impersonated credentials
    async fn impersonate(&self, config: GcpImpersonationConfig) -> Result<GcpClientConfig>;

    /// Get service endpoint, checking for overrides first
    fn get_service_endpoint(&self, service_name: &str, default_endpoint: &str) -> String;

    /// Get the endpoint for a specific service, with override support (returns Option)
    fn get_service_endpoint_option(&self, service_name: &str) -> Option<&str>;

    /// Get bearer token for the given audience
    async fn get_bearer_token(&self, audience: &str) -> Result<String>;

    /// Materialize an access token and the provider-reported expiry.
    async fn get_access_token_with_expiry(&self, audience: &str) -> Result<ExpiringAccessToken>;

    /// Materialize an impersonated service-account token and authoritative expiry.
    async fn get_impersonated_access_token_with_expiry(&self) -> Result<ExpiringAccessToken>;

    /// Exchanges an access token for a Credential Access Boundary token that
    /// is confined to one Cloud Storage bucket.
    async fn downscope_access_token_for_bucket(
        &self,
        bucket_name: &str,
        available_role: &str,
    ) -> Result<ExpiringAccessToken>;

    /// Generate an OAuth2 access token from service account credentials
    async fn generate_jwt_token(&self, service_account_json: &str) -> Result<String>;

    /// Generate an OAuth2 access token and retain its provider-reported expiry.
    async fn generate_jwt_token_with_expiry(
        &self,
        service_account_json: &str,
    ) -> Result<ExpiringAccessToken>;

    /// Build SDK configuration
    async fn build_sdk_config(&self) -> Result<String>;

    /// Get service token for the given service URL
    async fn get_service_token(&self, service_url: &str) -> Result<String>;

    /// Fetch project ID from metadata server
    async fn fetch_metadata_project_id() -> Result<String>;

    /// Fetch region from metadata server
    async fn fetch_metadata_region() -> Result<String>;

    /// Fetch token from metadata server
    async fn fetch_metadata_token(&self) -> Result<String>;

    /// Fetch token and expiry from metadata server.
    async fn fetch_metadata_token_with_expiry(&self) -> Result<ExpiringAccessToken>;

    /// Get projected token from file
    async fn get_projected_token(&self, token_file: &str) -> Result<String>;

    /// Exchange a refresh token for an access token via Google's OAuth2 endpoint
    async fn exchange_refresh_token(
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
    ) -> Result<String>;

    /// Exchange a refresh token and retain the returned access-token expiry.
    async fn exchange_refresh_token_with_expiry(
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
    ) -> Result<ExpiringAccessToken>;

    /// Exchange an external account subject token for a Google access token.
    async fn exchange_external_account_token(
        audience: &str,
        subject_token_type: &str,
        token_url: &str,
        credential_source_file: &str,
        service_account_impersonation_url: Option<&str>,
    ) -> Result<String>;

    /// Exchange an external account token and retain the final token expiry.
    async fn exchange_external_account_token_with_expiry(
        audience: &str,
        subject_token_type: &str,
        token_url: &str,
        credential_source_file: &str,
        service_account_impersonation_url: Option<&str>,
    ) -> Result<ExpiringAccessToken>;

    /// Parse a credentials JSON value and return (credentials, project_id, region)
    async fn parse_credentials_json(
        credential_data: &serde_json::Value,
        raw_json: &str,
        environment_variables: &HashMap<String, String>,
    ) -> Result<(GcpCredentials, String, String)>;

    /// Try to read the well-known gcloud ADC file (~/.config/gcloud/application_default_credentials.json)
    fn read_well_known_adc_file() -> Option<(String, serde_json::Value)>;

    /// Create a config with service endpoint overrides for testing
    #[cfg(any(test, feature = "test-utils"))]
    fn with_service_overrides(self, overrides: ServiceOverrides) -> Self;

    /// Create a mock GcpClientConfig with dummy values for testing
    #[cfg(any(test, feature = "test-utils"))]
    fn mock() -> Self;
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl GcpClientConfigExt for GcpClientConfig {
    /// Create a new `GcpClientConfig` from environment variables.
    async fn from_env(environment_variables: &HashMap<String, String>) -> Result<Self> {
        let (credentials, project_id, region) = if let Some(token) =
            environment_variables.get("GCP_ACCESS_TOKEN")
        {
            // For access tokens, we still need the project ID and region separately
            let project_id = environment_variables.get("GCP_PROJECT_ID").ok_or_else(|| {
                AlienError::new(ErrorData::InvalidClientConfig {
                    message:
                        "Missing GCP_PROJECT_ID environment variable when using GCP_ACCESS_TOKEN"
                            .to_string(),
                    errors: None,
                })
            })?;
            let region = environment_variables.get("GCP_REGION").ok_or_else(|| {
                AlienError::new(ErrorData::InvalidClientConfig {
                    message: "Missing GCP_REGION environment variable when using GCP_ACCESS_TOKEN"
                        .to_string(),
                    errors: None,
                })
            })?;
            (
                GcpCredentials::AccessToken {
                    token: token.clone(),
                },
                project_id.clone(),
                region.clone(),
            )
        } else if let Some(json) = environment_variables.get("GOOGLE_SERVICE_ACCOUNT_KEY") {
            let credential_data: serde_json::Value = serde_json::from_str(json)
                .into_alien_error()
                .context(ErrorData::InvalidClientConfig {
                    message: "Failed to parse GOOGLE_SERVICE_ACCOUNT_KEY JSON".to_string(),
                    errors: None,
                })?;
            let project_id = credential_data["project_id"]
                .as_str()
                .ok_or_else(|| {
                    AlienError::new(ErrorData::InvalidClientConfig {
                        message: "project_id not found in GOOGLE_SERVICE_ACCOUNT_KEY".to_string(),
                        errors: None,
                    })
                })?
                .to_string();

            // For service account keys, allow region to be provided via env var or fetch from metadata
            let region = if let Some(region) = environment_variables.get("GCP_REGION") {
                region.clone()
            } else {
                Self::fetch_metadata_region().await?
            };

            (
                GcpCredentials::ServiceAccountKey { json: json.clone() },
                project_id,
                region,
            )
        } else if let Some(key_path) = environment_variables.get("GOOGLE_APPLICATION_CREDENTIALS") {
            // Check if this looks like a projected token file (Kubernetes workload identity)
            if key_path.contains("/var/run/secrets/")
                && (key_path.ends_with("token") || key_path.contains("credentials.json"))
            {
                // This is likely a projected service account token or workload identity setup
                let project_id = environment_variables.get("GCP_PROJECT_ID")
                    .or_else(|| environment_variables.get("GOOGLE_CLOUD_PROJECT"))
                    .ok_or_else(|| AlienError::new(ErrorData::InvalidClientConfig {
                        message: "Missing GCP_PROJECT_ID or GOOGLE_CLOUD_PROJECT environment variable for projected service account".to_string(),
                        errors: None,
                    }))?
                    .clone();

                let service_account_email = environment_variables.get("GCP_SERVICE_ACCOUNT_EMAIL")
                    .ok_or_else(|| AlienError::new(ErrorData::InvalidClientConfig {
                        message: "Missing GCP_SERVICE_ACCOUNT_EMAIL environment variable for projected service account".to_string(),
                        errors: None,
                    }))?
                    .clone();

                let region = environment_variables
                    .get("GCP_REGION")
                    .cloned()
                    .unwrap_or_else(|| "us-central1".to_string()); // Default region for K8s workload identity

                (
                    GcpCredentials::ProjectedServiceAccount {
                        token_file: key_path.clone(),
                        service_account_email: service_account_email.clone(),
                    },
                    project_id,
                    region,
                )
            } else {
                // Read and parse the credentials file
                let json = std::fs::read_to_string(key_path)
                    .into_alien_error()
                    .context(ErrorData::InvalidClientConfig {
                        message: format!("Failed to read credentials file from path: {}", key_path),
                        errors: None,
                    })?;

                let credential_data: serde_json::Value = serde_json::from_str(&json)
                    .into_alien_error()
                    .context(ErrorData::InvalidClientConfig {
                        message: "Failed to parse JSON from GOOGLE_APPLICATION_CREDENTIALS file"
                            .to_string(),
                        errors: None,
                    })?;

                Self::parse_credentials_json(&credential_data, &json, environment_variables).await?
            }
        } else if let Some((json, credential_data)) = Self::read_well_known_adc_file() {
            // Auto-detect gcloud Application Default Credentials from well-known path
            Self::parse_credentials_json(&credential_data, &json, environment_variables).await?
        } else {
            // Fallback to metadata server authentication for GCP instances
            let project_id = if let Some(project_id) = environment_variables
                .get("GCP_PROJECT_ID")
                .or_else(|| environment_variables.get("GOOGLE_CLOUD_PROJECT"))
            {
                project_id.clone()
            } else {
                Self::fetch_metadata_project_id().await?
            };
            let region = if let Some(region) = environment_variables.get("GCP_REGION") {
                region.clone()
            } else {
                Self::fetch_metadata_region().await?
            };
            (GcpCredentials::ServiceMetadata, project_id, region)
        };

        Ok(Self {
            project_id,
            region,
            credentials,
            service_overrides: if let Some(endpoints_json) =
                environment_variables.get("GCP_SERVICE_OVERRIDES_ENDPOINTS")
            {
                let endpoints: HashMap<String, String> = serde_json::from_str(endpoints_json)
                    .into_alien_error()
                    .context(ErrorData::InvalidClientConfig {
                        message: "Failed to parse GCP_SERVICE_OVERRIDES_ENDPOINTS".to_string(),
                        errors: None,
                    })?;
                Some(ServiceOverrides { endpoints })
            } else {
                None
            },
            project_number: None,
        })
    }

    /// Create a new `GcpClientConfig` from standard environment variables.
    async fn from_std_env() -> Result<Self> {
        let env_vars: HashMap<String, String> = std::env::vars().collect();
        Self::from_env(&env_vars).await
    }

    /// Impersonate a GCP service account and return a refreshable platform config.
    async fn impersonate(&self, config: GcpImpersonationConfig) -> Result<GcpClientConfig> {
        let has_target_project = config.target_project_id.is_some();
        let target_project_id = config
            .target_project_id
            .clone()
            .unwrap_or_else(|| self.project_id.clone());
        let target_region = config
            .target_region
            .clone()
            .unwrap_or_else(|| self.region.clone());

        Ok(GcpClientConfig {
            project_id: target_project_id,
            region: target_region,
            credentials: GcpCredentials::ImpersonatedServiceAccount {
                source: Box::new(self.clone()),
                config,
            },
            service_overrides: self.service_overrides.clone(),
            project_number: if has_target_project {
                None
            } else {
                self.project_number.clone()
            },
        })
    }

    /// Generates a bearer token for GCP API authentication
    async fn get_bearer_token(&self, _audience: &str) -> Result<String> {
        match &self.credentials {
            GcpCredentials::AccessToken { token } => Ok(token.clone()),
            GcpCredentials::ImpersonatedServiceAccount { source, config } => {
                generate_impersonated_access_token(source, config)
                    .await
                    .map(|response| response.access_token)
            }
            GcpCredentials::ServiceAccountKey { json } => self.generate_jwt_token(json).await,
            GcpCredentials::ServiceMetadata => self.fetch_metadata_token().await,
            GcpCredentials::ProjectedServiceAccount { .. } => Err(AlienError::new(
                ErrorData::InvalidClientConfig {
                    message: "Projected GCP workload-identity JWTs must be exchanged through an explicit external-account STS configuration before use as OAuth access tokens".to_string(),
                    errors: None,
                },
            )),
            GcpCredentials::ExternalAccount {
                audience,
                subject_token_type,
                token_url,
                credential_source_file,
                service_account_impersonation_url,
            } => {
                Self::exchange_external_account_token(
                    audience,
                    subject_token_type,
                    token_url,
                    credential_source_file,
                    service_account_impersonation_url.as_deref(),
                )
                .await
            }
            GcpCredentials::AuthorizedUser {
                client_id,
                client_secret,
                refresh_token,
            } => Self::exchange_refresh_token(client_id, client_secret, refresh_token).await,
        }
    }

    async fn get_access_token_with_expiry(&self, _audience: &str) -> Result<ExpiringAccessToken> {
        match &self.credentials {
            GcpCredentials::AccessToken { .. } => {
                Err(AlienError::new(ErrorData::InvalidClientConfig {
                    message: "An opaque GCP access token has no authoritative expiry".to_string(),
                    errors: None,
                }))
            }
            GcpCredentials::ImpersonatedServiceAccount { .. } => {
                self.get_impersonated_access_token_with_expiry().await
            }
            GcpCredentials::ServiceAccountKey { json } => {
                self.generate_jwt_token_with_expiry(json).await
            }
            GcpCredentials::ServiceMetadata => self.fetch_metadata_token_with_expiry().await,
            GcpCredentials::ProjectedServiceAccount { .. } => Err(AlienError::new(
                ErrorData::InvalidClientConfig {
                    message: "Projected GCP workload-identity JWTs must be exchanged through an explicit external-account STS configuration before use as OAuth access tokens".to_string(),
                    errors: None,
                },
            )),
            GcpCredentials::ExternalAccount {
                audience,
                subject_token_type,
                token_url,
                credential_source_file,
                service_account_impersonation_url,
            } => {
                Self::exchange_external_account_token_with_expiry(
                    audience,
                    subject_token_type,
                    token_url,
                    credential_source_file,
                    service_account_impersonation_url.as_deref(),
                )
                .await
            }
            GcpCredentials::AuthorizedUser {
                client_id,
                client_secret,
                refresh_token,
            } => {
                Self::exchange_refresh_token_with_expiry(client_id, client_secret, refresh_token)
                    .await
            }
        }
    }

    async fn get_impersonated_access_token_with_expiry(&self) -> Result<ExpiringAccessToken> {
        let GcpCredentials::ImpersonatedServiceAccount { source, config } = &self.credentials
        else {
            return Err(AlienError::new(ErrorData::InvalidClientConfig {
                message: "An impersonated service-account credential source is required"
                    .to_string(),
                errors: None,
            }));
        };
        let response = generate_impersonated_access_token(source, config).await?;
        let expires_at = DateTime::parse_from_rfc3339(&response.expire_time)
            .into_alien_error()
            .context(ErrorData::InvalidInput {
                message: "GCP returned an invalid access-token expiry".to_string(),
                field_name: None,
            })?
            .with_timezone(&Utc);
        Ok(ExpiringAccessToken {
            token: response.access_token,
            expires_at,
        })
    }

    async fn downscope_access_token_for_bucket(
        &self,
        bucket_name: &str,
        available_role: &str,
    ) -> Result<ExpiringAccessToken> {
        remote_storage_credentials::downscope_access_token_for_bucket(
            self,
            bucket_name,
            available_role,
        )
        .await
    }

    /// Get service endpoint, checking for overrides first
    fn get_service_endpoint(&self, service_name: &str, default_endpoint: &str) -> String {
        self.service_overrides
            .as_ref()
            .and_then(|overrides| overrides.endpoints.get(service_name))
            .map(|s| s.clone())
            .unwrap_or_else(|| default_endpoint.to_string())
    }

    /// Get the endpoint for a specific service, with override support (returns Option)
    fn get_service_endpoint_option(&self, service_name: &str) -> Option<&str> {
        self.service_overrides
            .as_ref()
            .and_then(|overrides| overrides.endpoints.get(service_name))
            .map(|s| s.as_str())
    }

    /// Create a config with service endpoint overrides for testing
    #[cfg(any(test, feature = "test-utils"))]
    fn with_service_overrides(mut self, overrides: ServiceOverrides) -> Self {
        self.service_overrides = Some(overrides);
        self
    }

    /// Generates an OAuth2 access token from service account credentials.
    ///
    /// Creates a JWT assertion signed with the SA private key, then exchanges it
    /// at Google's OAuth2 token endpoint for an access token with
    /// `cloud-platform` scope.
    async fn generate_jwt_token(&self, service_account_json: &str) -> Result<String> {
        self.generate_jwt_token_with_expiry(service_account_json)
            .await
            .map(|token| token.token)
    }

    async fn generate_jwt_token_with_expiry(
        &self,
        service_account_json: &str,
    ) -> Result<ExpiringAccessToken> {
        credential_exchange::generate_jwt_token_with_expiry(service_account_json).await
    }

    /// Builds a GCP SDK config from the stored configuration.
    /// For now, returns the bearer token for API calls.
    async fn build_sdk_config(&self) -> Result<String> {
        // Default audience for most GCP APIs
        let default_audience = "https://www.googleapis.com/";
        self.get_bearer_token(default_audience).await
    }

    /// Gets a bearer token for a specific GCP service
    async fn get_service_token(&self, service_url: &str) -> Result<String> {
        self.get_bearer_token(service_url).await
    }

    /// Fetches the project ID from the GCP metadata server
    async fn fetch_metadata_project_id() -> Result<String> {
        use reqwest::Client;

        let client = Client::new();
        let response = client
            .get("http://metadata.google.internal/computeMetadata/v1/project/project-id")
            .header("Metadata-Flavor", "Google")
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: "Failed to fetch project ID from GCP metadata server".to_string(),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::HttpResponseError {
                message: format!("Metadata server returned error {}: {}", status, error_text),
                url: "http://metadata.google.internal/computeMetadata/v1/project/project-id"
                    .to_string(),
                http_status: status.as_u16(),
                http_request_text: None,
                http_response_text: Some(error_text),
            }));
        }

        let project_id =
            response
                .text()
                .await
                .into_alien_error()
                .context(ErrorData::SerializationError {
                    message: "Failed to parse project ID response from GCP metadata server"
                        .to_string(),
                })?;

        Ok(project_id.trim().to_string())
    }

    /// Fetches the region from the GCP metadata server.
    ///
    /// Compute Engine exposes `/instance/region`; GKE's metadata server may only
    /// expose `/instance/zone`, so fall back to deriving the region from zone.
    async fn fetch_metadata_region() -> Result<String> {
        use reqwest::Client;

        let client = Client::new();
        let region_url = "http://metadata.google.internal/computeMetadata/v1/instance/region";
        let response = client
            .get(region_url)
            .header("Metadata-Flavor", "Google")
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: "Failed to fetch region from GCP metadata server".to_string(),
            })?;

        let region_response = if response.status().is_success() {
            response
                .text()
                .await
                .into_alien_error()
                .context(ErrorData::SerializationError {
                    message: "Failed to parse region response from GCP metadata server".to_string(),
                })?
        } else if response.status() == reqwest::StatusCode::NOT_FOUND {
            let zone_url = "http://metadata.google.internal/computeMetadata/v1/instance/zone";
            let zone_response = client
                .get(zone_url)
                .header("Metadata-Flavor", "Google")
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Failed to fetch zone from GCP metadata server".to_string(),
                })?;

            if !zone_response.status().is_success() {
                let status = zone_response.status();
                let error_text = zone_response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string());
                return Err(AlienError::new(ErrorData::HttpResponseError {
                    message: format!("Metadata server returned error {}: {}", status, error_text),
                    url: zone_url.to_string(),
                    http_status: status.as_u16(),
                    http_request_text: None,
                    http_response_text: Some(error_text),
                }));
            }

            let zone_response = zone_response.text().await.into_alien_error().context(
                ErrorData::SerializationError {
                    message: "Failed to parse zone response from GCP metadata server".to_string(),
                },
            )?;
            let zone_full_path = zone_response.trim();
            let zone = zone_full_path.split('/').last().ok_or_else(|| {
                AlienError::new(ErrorData::InvalidClientConfig {
                    message: format!("Invalid zone format from metadata server: {zone_full_path}"),
                    errors: None,
                })
            })?;

            gcp_region_from_zone(zone).ok_or_else(|| {
                AlienError::new(ErrorData::InvalidClientConfig {
                    message: format!("Invalid zone format from metadata server: {zone_full_path}"),
                    errors: None,
                })
            })?
        } else {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::HttpResponseError {
                message: format!("Metadata server returned error {}: {}", status, error_text),
                url: region_url.to_string(),
                http_status: status.as_u16(),
                http_request_text: None,
                http_response_text: Some(error_text),
            }));
        };

        // Region response format is: "projects/123456789012/regions/us-central1"
        // We need to extract just the region name
        let region_full_path = region_response.trim();
        let region = region_full_path.split('/').last().ok_or_else(|| {
            AlienError::new(ErrorData::InvalidClientConfig {
                message: format!(
                    "Invalid region format from metadata server: {}",
                    region_full_path
                ),
                errors: None,
            })
        })?;

        Ok(region.to_string())
    }

    /// Fetches an access token from the GCP metadata server
    async fn fetch_metadata_token(&self) -> Result<String> {
        self.fetch_metadata_token_with_expiry()
            .await
            .map(|token| token.token)
    }

    async fn fetch_metadata_token_with_expiry(&self) -> Result<ExpiringAccessToken> {
        credential_exchange::fetch_metadata_token_with_expiry().await
    }

    /// Gets a projected service account token from the file system
    /// This is used for Kubernetes workload identity
    async fn get_projected_token(&self, _token_file: &str) -> Result<String> {
        Err(AlienError::new(ErrorData::InvalidClientConfig {
            message: "Projected GCP workload-identity JWTs require an explicit external-account STS configuration".to_string(),
            errors: None,
        }))
    }

    /// Exchanges a refresh token for an access token via Google's OAuth2 token endpoint.
    async fn exchange_refresh_token(
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
    ) -> Result<String> {
        Self::exchange_refresh_token_with_expiry(client_id, client_secret, refresh_token)
            .await
            .map(|token| token.token)
    }

    async fn exchange_refresh_token_with_expiry(
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
    ) -> Result<ExpiringAccessToken> {
        credential_exchange::exchange_refresh_token_with_expiry(
            client_id,
            client_secret,
            refresh_token,
        )
        .await
    }

    /// Exchanges an external account subject token through Google's Security Token Service.
    async fn exchange_external_account_token(
        audience: &str,
        subject_token_type: &str,
        token_url: &str,
        credential_source_file: &str,
        service_account_impersonation_url: Option<&str>,
    ) -> Result<String> {
        Self::exchange_external_account_token_with_expiry(
            audience,
            subject_token_type,
            token_url,
            credential_source_file,
            service_account_impersonation_url,
        )
        .await
        .map(|token| token.token)
    }

    async fn exchange_external_account_token_with_expiry(
        audience: &str,
        subject_token_type: &str,
        token_url: &str,
        credential_source_file: &str,
        service_account_impersonation_url: Option<&str>,
    ) -> Result<ExpiringAccessToken> {
        credential_exchange::exchange_external_account_token_with_expiry(
            audience,
            subject_token_type,
            token_url,
            credential_source_file,
            service_account_impersonation_url,
        )
        .await
    }

    /// Parse a credentials JSON value and return (credentials, project_id, region).
    /// Supports `service_account`, `authorized_user`, and `external_account` credential types.
    async fn parse_credentials_json(
        credential_data: &serde_json::Value,
        raw_json: &str,
        environment_variables: &HashMap<String, String>,
    ) -> Result<(GcpCredentials, String, String)> {
        credential_config::parse_credentials_json(credential_data, raw_json, environment_variables)
            .await
    }

    /// Try to read the well-known gcloud ADC file.
    /// Returns `Some((raw_json, parsed_value))` if the file exists and is valid JSON.
    fn read_well_known_adc_file() -> Option<(String, serde_json::Value)> {
        credential_config::read_well_known_adc_file()
    }

    /// Creates a mock GCP platform config with dummy values for testing
    #[cfg(any(test, feature = "test-utils"))]
    fn mock() -> Self {
        Self {
            project_id: "test-project-123".to_string(),
            region: "us-central1".to_string(),
            credentials: GcpCredentials::AccessToken {
                token: "mock-access-token-12345".to_string(),
            },
            service_overrides: None,
            project_number: None,
        }
    }
}

/// Mint an impersonated service-account token together with Google's authoritative expiry.
async fn generate_impersonated_access_token(
    source: &GcpClientConfig,
    config: &GcpImpersonationConfig,
) -> Result<crate::gcp::iam::GenerateAccessTokenResponse> {
    use crate::gcp::iam::{GenerateAccessTokenRequest, IamApi, IamClient};

    let iam_client = IamClient::new(Client::new(), source.clone());
    let token_request = GenerateAccessTokenRequest::builder()
        .scope(config.scopes.clone())
        .maybe_delegates(config.delegates.clone())
        .maybe_lifetime(config.lifetime.clone())
        .build();

    iam_client
        .generate_access_token(config.service_account_email.clone(), token_request)
        .await
}

fn gcp_region_from_zone(zone: &str) -> Option<String> {
    let (region, zone_suffix) = zone.rsplit_once('-')?;
    if zone_suffix.len() != 1 || !zone_suffix.as_bytes()[0].is_ascii_lowercase() {
        return None;
    }
    if region.is_empty() {
        None
    } else {
        Some(region.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::gcp_region_from_zone;

    #[test]
    fn derives_region_from_zone() {
        assert_eq!(
            gcp_region_from_zone("us-east4-a").as_deref(),
            Some("us-east4")
        );
        assert_eq!(
            gcp_region_from_zone("europe-west1-b").as_deref(),
            Some("europe-west1")
        );
        assert_eq!(gcp_region_from_zone("us-east4"), None);
        assert_eq!(gcp_region_from_zone(""), None);
    }
}
