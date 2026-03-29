pub mod api_client;
pub mod artifactregistry;
pub mod certificatemanager;
pub mod cloudbuild;
pub mod cloudrun;
pub mod compute;
pub mod firestore;
pub mod gcp_request_utils;
pub mod gcs;
pub mod iam;
pub mod longrunning;
pub mod pubsub;
pub mod resource_manager;
pub mod secret_manager;
pub mod service_usage;

use alien_client_core::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;

// Re-export types from alien-core
pub use alien_core::{
    GcpClientConfig, GcpCredentials, GcpImpersonationConfig,
    GcpServiceOverrides as ServiceOverrides,
};

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

    /// Generate JWT token from service account JSON
    async fn generate_jwt_token(
        &self,
        service_account_json: &str,
        audience: &str,
    ) -> Result<String>;

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

    /// Get projected token from file
    async fn get_projected_token(&self, token_file: &str) -> Result<String>;

    /// Exchange a refresh token for an access token via Google's OAuth2 endpoint
    async fn exchange_refresh_token(
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
    ) -> Result<String>;

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
            let project_id = Self::fetch_metadata_project_id().await?;
            let region = Self::fetch_metadata_region().await?;
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

    /// Impersonate a GCP service account and return a new platform config with impersonated credentials
    async fn impersonate(&self, config: GcpImpersonationConfig) -> Result<GcpClientConfig> {
        use crate::gcp::iam::{GenerateAccessTokenRequest, IamApi, IamClient};

        let iam_client = IamClient::new(Client::new(), self.clone());

        let token_request = GenerateAccessTokenRequest::builder()
            .scope(config.scopes)
            .maybe_delegates(config.delegates)
            .maybe_lifetime(config.lifetime)
            .build();

        let token_response = iam_client
            .generate_access_token(config.service_account_email.clone(), token_request)
            .await?;

        // Create new platform config with impersonated access token
        Ok(GcpClientConfig {
            project_id: self.project_id.clone(),
            region: self.region.clone(),
            credentials: GcpCredentials::AccessToken {
                token: token_response.access_token,
            },
            service_overrides: self.service_overrides.clone(),
            project_number: self.project_number.clone(),
        })
    }

    /// Generates a bearer token for GCP API authentication
    async fn get_bearer_token(&self, audience: &str) -> Result<String> {
        match &self.credentials {
            GcpCredentials::AccessToken { token } => Ok(token.clone()),
            GcpCredentials::ServiceAccountKey { json } => {
                self.generate_jwt_token(json, audience).await
            }
            GcpCredentials::ServiceMetadata => self.fetch_metadata_token().await,
            GcpCredentials::ProjectedServiceAccount { token_file, .. } => {
                self.get_projected_token(token_file).await
            }
            GcpCredentials::AuthorizedUser {
                client_id,
                client_secret,
                refresh_token,
            } => Self::exchange_refresh_token(client_id, client_secret, refresh_token).await,
        }
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

    /// Generates a JWT token from service account credentials
    async fn generate_jwt_token(
        &self,
        service_account_json: &str,
        audience: &str,
    ) -> Result<String> {
        use jwt_simple::prelude::*;

        #[derive(serde::Deserialize)]
        struct ServiceAccountKey {
            client_email: String,
            private_key_id: String,
            private_key: String,
        }

        // Parse the service account JSON to extract only the fields we need
        let service_account: ServiceAccountKey = serde_json::from_str(service_account_json)
            .into_alien_error()
            .context(ErrorData::InvalidClientConfig {
                message: "Failed to parse service account JSON".to_string(),
                errors: None,
            })?;

        // Create JWT claims
        let claims = Claims::create(Duration::from_secs(3600))
            .with_issuer(&service_account.client_email)
            .with_subject(&service_account.client_email)
            .with_audience(audience);

        // Parse the private key and set the key_id
        let key_pair = RS256KeyPair::from_pem(&service_account.private_key)
            .map_err(|e| {
                AlienError::new(ErrorData::InvalidClientConfig {
                    message: format!(
                        "Failed to parse private key from service account. Internal error: {}",
                        e.to_string()
                    ),
                    errors: None,
                })
            })?
            .with_key_id(&service_account.private_key_id);

        // Sign the JWT (key_id will be automatically included in the header)
        let token = key_pair.sign(claims).map_err(|e| {
            AlienError::new(ErrorData::RequestSignError {
                message: format!("Failed to sign JWT token: {}", e),
            })
        })?;

        Ok(token)
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

    /// Fetches the region from the GCP metadata server
    async fn fetch_metadata_region() -> Result<String> {
        use reqwest::Client;

        let client = Client::new();
        let response = client
            .get("http://metadata.google.internal/computeMetadata/v1/instance/region")
            .header("Metadata-Flavor", "Google")
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: "Failed to fetch region from GCP metadata server".to_string(),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::HttpResponseError {
                message: format!("Metadata server returned error {}: {}", status, error_text),
                url: "http://metadata.google.internal/computeMetadata/v1/instance/region"
                    .to_string(),
                http_status: status.as_u16(),
                http_request_text: None,
                http_response_text: Some(error_text),
            }));
        }

        let region_response =
            response
                .text()
                .await
                .into_alien_error()
                .context(ErrorData::SerializationError {
                    message: "Failed to parse region response from GCP metadata server".to_string(),
                })?;

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
        use reqwest::Client;

        #[derive(serde::Deserialize)]
        struct TokenResponse {
            access_token: String,
        }

        let client = Client::new();
        let response = client
            .get("http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token")
            .header("Metadata-Flavor", "Google")
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: "Failed to fetch token from GCP metadata server".to_string(),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::HttpResponseError {
                message: format!("Metadata server returned error {}: {}", status, error_text),
                url: "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token".to_string(),
                http_status: status.as_u16(),
                http_request_text: None,
                http_response_text: Some(error_text),
            }));
        }

        let token_response: TokenResponse =
            response
                .json()
                .await
                .into_alien_error()
                .context(ErrorData::SerializationError {
                    message: "Failed to parse token response from GCP metadata server".to_string(),
                })?;

        Ok(token_response.access_token)
    }

    /// Gets a projected service account token from the file system
    /// This is used for Kubernetes workload identity
    async fn get_projected_token(&self, token_file: &str) -> Result<String> {
        let token = std::fs::read_to_string(token_file)
            .into_alien_error()
            .context(ErrorData::InvalidClientConfig {
                message: format!(
                    "Failed to read projected service account token from: {}",
                    token_file
                ),
                errors: None,
            })?
            .trim()
            .to_string();

        // For projected tokens, we need to use the token as-is for most operations
        // However, if it's an OIDC token, we might need to exchange it for a Google access token
        // For now, we'll return the token as-is, but this could be enhanced to do token exchange
        Ok(token)
    }

    /// Exchanges a refresh token for an access token via Google's OAuth2 token endpoint.
    async fn exchange_refresh_token(
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
    ) -> Result<String> {
        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: String,
        }

        let client = Client::new();
        let response = client
            .post("https://oauth2.googleapis.com/token")
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("refresh_token", refresh_token),
            ])
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: "Failed to exchange refresh token for access token".to_string(),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::HttpResponseError {
                message: format!(
                    "OAuth2 token exchange failed with status {}: {}",
                    status, error_text
                ),
                url: "https://oauth2.googleapis.com/token".to_string(),
                http_status: status.as_u16(),
                http_request_text: None,
                http_response_text: Some(error_text),
            }));
        }

        let token_response: TokenResponse =
            response
                .json()
                .await
                .into_alien_error()
                .context(ErrorData::SerializationError {
                    message: "Failed to parse OAuth2 token exchange response".to_string(),
                })?;

        Ok(token_response.access_token)
    }

    /// Parse a credentials JSON value and return (credentials, project_id, region).
    /// Supports both `service_account` and `authorized_user` credential types.
    async fn parse_credentials_json(
        credential_data: &serde_json::Value,
        raw_json: &str,
        environment_variables: &HashMap<String, String>,
    ) -> Result<(GcpCredentials, String, String)> {
        let cred_type = credential_data["type"]
            .as_str()
            .unwrap_or("service_account");

        if cred_type == "authorized_user" {
            let client_id = credential_data["client_id"]
                .as_str()
                .ok_or_else(|| {
                    AlienError::new(ErrorData::InvalidClientConfig {
                        message: "client_id not found in authorized_user credentials".to_string(),
                        errors: None,
                    })
                })?
                .to_string();

            let client_secret = credential_data["client_secret"]
                .as_str()
                .ok_or_else(|| {
                    AlienError::new(ErrorData::InvalidClientConfig {
                        message: "client_secret not found in authorized_user credentials"
                            .to_string(),
                        errors: None,
                    })
                })?
                .to_string();

            let refresh_token = credential_data["refresh_token"]
                .as_str()
                .ok_or_else(|| {
                    AlienError::new(ErrorData::InvalidClientConfig {
                        message: "refresh_token not found in authorized_user credentials"
                            .to_string(),
                        errors: None,
                    })
                })?
                .to_string();

            // authorized_user credentials don't contain project_id, so we need it from
            // the environment or from quota_project_id in the file
            let project_id = environment_variables.get("GCP_PROJECT_ID")
                .cloned()
                .or_else(|| credential_data["quota_project_id"].as_str().map(|s| s.to_string()))
                .ok_or_else(|| AlienError::new(ErrorData::InvalidClientConfig {
                    message: "Missing GCP_PROJECT_ID environment variable for authorized_user credentials \
                              (quota_project_id not found in credentials file either)".to_string(),
                    errors: None,
                }))?;

            let region = environment_variables.get("GCP_REGION")
                .ok_or_else(|| AlienError::new(ErrorData::InvalidClientConfig {
                    message: "Missing GCP_REGION environment variable for authorized_user credentials".to_string(),
                    errors: None,
                }))?
                .clone();

            Ok((
                GcpCredentials::AuthorizedUser {
                    client_id,
                    client_secret,
                    refresh_token,
                },
                project_id,
                region,
            ))
        } else {
            // service_account or other types — treat as service account key
            let project_id = credential_data["project_id"]
                .as_str()
                .ok_or_else(|| {
                    AlienError::new(ErrorData::InvalidClientConfig {
                        message: "project_id not found in credentials file".to_string(),
                        errors: None,
                    })
                })?
                .to_string();

            let region = if let Some(region) = environment_variables.get("GCP_REGION") {
                region.clone()
            } else {
                Self::fetch_metadata_region().await?
            };

            Ok((
                GcpCredentials::ServiceAccountKey {
                    json: raw_json.to_string(),
                },
                project_id,
                region,
            ))
        }
    }

    /// Try to read the well-known gcloud ADC file.
    /// Returns `Some((raw_json, parsed_value))` if the file exists and is valid JSON.
    fn read_well_known_adc_file() -> Option<(String, serde_json::Value)> {
        let home = std::env::var("HOME").ok()?;
        let adc_path = std::path::Path::new(&home)
            .join(".config")
            .join("gcloud")
            .join("application_default_credentials.json");

        let json = std::fs::read_to_string(&adc_path).ok()?;
        let value: serde_json::Value = serde_json::from_str(&json).ok()?;
        Some((json, value))
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
