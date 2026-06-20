//! Extension trait for ClientConfig to provide environment-based configuration and impersonation

use alien_client_core::{ErrorData, Result};
#[cfg(feature = "azure")]
use alien_core::{AzureClientConfig, AzureCredentials, AzureServiceOverrides};
use alien_core::{ClientConfig, ImpersonationConfig, Platform};
#[cfg(feature = "gcp")]
use alien_core::{GcpClientConfig, GcpCredentials, GcpServiceOverrides};
use alien_error::AlienError;
#[cfg(any(
    feature = "aws",
    feature = "gcp",
    feature = "azure",
    feature = "kubernetes"
))]
use alien_error::Context;
#[cfg(any(
    feature = "aws",
    feature = "gcp",
    feature = "azure",
    feature = "kubernetes"
))]
use alien_error::IntoAlienError;
use async_trait::async_trait;
use std::collections::HashMap;

/// Extension trait for ClientConfig providing environment-based configuration and cloud-agnostic impersonation
#[async_trait]
pub trait ClientConfigExt {
    /// Create a platform configuration from environment variables based on the specified platform.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use alien_client_config::{ClientConfigExt};
    /// use alien_core::{ClientConfig, Platform};
    /// use std::collections::HashMap;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let env_vars: HashMap<String, String> = std::env::vars().collect();
    /// let aws_config = ClientConfig::from_env(Platform::Aws, &env_vars).await?;
    /// let gcp_config = ClientConfig::from_env(Platform::Gcp, &env_vars).await?;
    /// let azure_config = ClientConfig::from_env(Platform::Azure, &env_vars).await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn from_env(
        platform: Platform,
        environment_variables: &HashMap<String, String>,
    ) -> Result<Self>
    where
        Self: Sized;

    /// Create a platform configuration from standard environment variables based on the specified platform.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use alien_client_config::{ClientConfigExt};
    /// use alien_core::{ClientConfig, Platform};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let aws_config = ClientConfig::from_std_env(Platform::Aws).await?;
    /// let gcp_config = ClientConfig::from_std_env(Platform::Gcp).await?;
    /// let azure_config = ClientConfig::from_std_env(Platform::Azure).await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn from_std_env(platform: Platform) -> Result<Self>
    where
        Self: Sized;

    /// Returns the platform enum for this configuration.
    fn platform(&self) -> Platform;

    /// Cloud-agnostic impersonation method
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use alien_client_config::{ClientConfigExt};
    /// use alien_core::{ClientConfig, ImpersonationConfig, AwsImpersonationConfig};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let aws_config = ClientConfig::Test; // placeholder
    /// let impersonation = ImpersonationConfig::Aws(AwsImpersonationConfig {
    ///     role_arn: "arn:aws:iam::123456789012:role/MyRole".to_string(),
    ///     session_name: None,
    ///     duration_seconds: Some(3600),
    ///     external_id: None,
    ///     target_region: None,
    /// });
    ///
    /// let impersonated_config = aws_config.impersonate(impersonation).await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn impersonate(&self, config: ImpersonationConfig) -> Result<ClientConfig>;
}

#[async_trait]
impl ClientConfigExt for ClientConfig {
    async fn from_env(
        platform: Platform,
        environment_variables: &HashMap<String, String>,
    ) -> Result<Self> {
        match platform {
            #[cfg(feature = "aws")]
            Platform::Aws => {
                let config =
                    aws_config_from_env(environment_variables)
                        .await
                        .context(ErrorData::InvalidClientConfig {
                        message:
                            "Failed to create AWS client configuration from environment variables"
                                .to_string(),
                        errors: None,
                    })?;
                Ok(ClientConfig::Aws(Box::new(config)))
            }
            #[cfg(not(feature = "aws"))]
            Platform::Aws => Err(AlienError::new(ErrorData::InvalidClientConfig {
                message: "AWS support is not enabled in this build".to_string(),
                errors: None,
            })),
            #[cfg(feature = "gcp")]
            Platform::Gcp => {
                let config =
                    gcp_config_from_env(environment_variables)
                        .await
                        .context(ErrorData::InvalidClientConfig {
                        message:
                            "Failed to create GCP client configuration from environment variables"
                                .to_string(),
                        errors: None,
                    })?;
                Ok(ClientConfig::Gcp(Box::new(config)))
            }
            #[cfg(not(feature = "gcp"))]
            Platform::Gcp => Err(AlienError::new(ErrorData::InvalidClientConfig {
                message: "GCP support is not enabled in this build".to_string(),
                errors: None,
            })),
            #[cfg(feature = "azure")]
            Platform::Azure => {
                let config =
                    azure_config_from_env(environment_variables)
                        .context(ErrorData::InvalidClientConfig {
                        message:
                            "Failed to create Azure client configuration from environment variables"
                                .to_string(),
                        errors: None,
                    })?;
                Ok(ClientConfig::Azure(Box::new(config)))
            }
            #[cfg(not(feature = "azure"))]
            Platform::Azure => Err(AlienError::new(ErrorData::InvalidClientConfig {
                message: "Azure support is not enabled in this build".to_string(),
                errors: None,
            })),
            #[cfg(feature = "kubernetes")]
            Platform::Kubernetes => {
                let config = kubernetes_config_from_env(environment_variables)?;
                Ok(ClientConfig::Kubernetes(Box::new(config)))
            }
            #[cfg(not(feature = "kubernetes"))]
            Platform::Kubernetes => Err(AlienError::new(ErrorData::InvalidClientConfig {
                message: "Kubernetes support is not enabled in this build".to_string(),
                errors: None,
            })),
            Platform::Test => Ok(ClientConfig::Test),
            Platform::Local => {
                // Local platform reads state directory from ALIEN_LOCAL_STATE_DIRECTORY
                let state_directory = environment_variables
                    .get("ALIEN_LOCAL_STATE_DIRECTORY")
                    .cloned()
                    .unwrap_or_else(|| "/tmp/alien-local".to_string());

                Ok(ClientConfig::Local { state_directory })
            }
        }
    }

    async fn from_std_env(platform: Platform) -> Result<Self> {
        let env_vars: HashMap<String, String> = std::env::vars().collect();
        Self::from_env(platform, &env_vars).await
    }

    fn platform(&self) -> Platform {
        match self {
            #[cfg(feature = "aws")]
            ClientConfig::Aws(_) => Platform::Aws,
            #[cfg(feature = "gcp")]
            ClientConfig::Gcp(_) => Platform::Gcp,
            #[cfg(feature = "azure")]
            ClientConfig::Azure(_) => Platform::Azure,
            #[cfg(feature = "kubernetes")]
            ClientConfig::Kubernetes(_) => Platform::Kubernetes,
            #[cfg(feature = "kubernetes")]
            ClientConfig::KubernetesCloud { .. } => Platform::Kubernetes,
            ClientConfig::Test => Platform::Test,
            ClientConfig::Local { .. } => Platform::Local,
            // This should never be reached when no features are enabled,
            // as the enum would be uninhabitable
            #[allow(unreachable_patterns)]
            _ => unreachable!("ClientConfig requires at least one platform feature to be enabled"),
        }
    }

    async fn impersonate(&self, config: ImpersonationConfig) -> Result<ClientConfig> {
        match (self, config) {
            #[cfg(feature = "aws")]
            (ClientConfig::Aws(aws_config), ImpersonationConfig::Aws(imp_config)) => {
                let new_config = assume_aws_role_config(aws_config, imp_config)
                    .await
                    .map_err(|e| {
                        AlienError::new(ErrorData::AuthenticationError {
                            message: format!("AWS role impersonation failed: {}", e),
                        })
                    })?;
                Ok(ClientConfig::Aws(Box::new(new_config)))
            }
            #[cfg(feature = "gcp")]
            (ClientConfig::Gcp(gcp_config), ImpersonationConfig::Gcp(imp_config)) => {
                let new_config = impersonated_gcp_config(gcp_config, imp_config);
                Ok(ClientConfig::Gcp(Box::new(new_config)))
            }
            #[cfg(feature = "azure")]
            (ClientConfig::Azure(azure_config), ImpersonationConfig::Azure(imp_config)) => {
                let new_config = impersonated_azure_config(azure_config, imp_config)?;
                Ok(ClientConfig::Azure(Box::new(new_config)))
            }
            // Kubernetes doesn't support impersonation
            #[cfg(feature = "kubernetes")]
            (ClientConfig::Kubernetes(_), _) => Err(AlienError::new(ErrorData::InvalidInput {
                message: "Kubernetes platform does not support impersonation".to_string(),
                field_name: Some("impersonation_config".to_string()),
            })),
            _ => Err(AlienError::new(ErrorData::InvalidInput {
                message: "Platform config and impersonation config types must match".to_string(),
                field_name: Some("impersonation_config".to_string()),
            })),
        }
    }
}

#[cfg(feature = "gcp")]
async fn gcp_config_from_env(
    environment_variables: &HashMap<String, String>,
) -> Result<GcpClientConfig> {
    let (credentials, project_id, region) = if let Some(token) =
        environment_variables.get("GCP_ACCESS_TOKEN")
    {
        let project_id = required_env(
            environment_variables,
            "GCP_PROJECT_ID",
            "Missing GCP_PROJECT_ID environment variable when using GCP_ACCESS_TOKEN",
        )?;
        let region = required_env(
            environment_variables,
            "GCP_REGION",
            "Missing GCP_REGION environment variable when using GCP_ACCESS_TOKEN",
        )?;
        (
            GcpCredentials::AccessToken {
                token: token.clone(),
            },
            project_id,
            region,
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
        let region = match environment_variables.get("GCP_REGION") {
            Some(region) => region.clone(),
            None => fetch_gcp_metadata_region().await?,
        };
        (
            GcpCredentials::ServiceAccountKey { json: json.clone() },
            project_id,
            region,
        )
    } else if let Some(key_path) = environment_variables.get("GOOGLE_APPLICATION_CREDENTIALS") {
        if key_path.contains("/var/run/secrets/")
            && (key_path.ends_with("token") || key_path.contains("credentials.json"))
        {
            let project_id = environment_variables
                    .get("GCP_PROJECT_ID")
                    .or_else(|| environment_variables.get("GOOGLE_CLOUD_PROJECT"))
                    .cloned()
                    .ok_or_else(|| AlienError::new(ErrorData::InvalidClientConfig {
                        message: "Missing GCP_PROJECT_ID or GOOGLE_CLOUD_PROJECT environment variable for projected service account".to_string(),
                        errors: None,
                    }))?;
            let service_account_email = required_env(
                    environment_variables,
                    "GCP_SERVICE_ACCOUNT_EMAIL",
                    "Missing GCP_SERVICE_ACCOUNT_EMAIL environment variable for projected service account",
                )?;
            let region = environment_variables
                .get("GCP_REGION")
                .cloned()
                .unwrap_or_else(|| "us-central1".to_string());
            (
                GcpCredentials::ProjectedServiceAccount {
                    token_file: key_path.clone(),
                    service_account_email,
                },
                project_id,
                region,
            )
        } else {
            let json = std::fs::read_to_string(key_path)
                .into_alien_error()
                .context(ErrorData::InvalidClientConfig {
                    message: format!("Failed to read credentials file from path: {key_path}"),
                    errors: None,
                })?;
            let credential_data: serde_json::Value = serde_json::from_str(&json)
                .into_alien_error()
                .context(ErrorData::InvalidClientConfig {
                    message: "Failed to parse JSON from GOOGLE_APPLICATION_CREDENTIALS file"
                        .to_string(),
                    errors: None,
                })?;
            parse_gcp_credentials_json(&credential_data, &json, environment_variables).await?
        }
    } else if let Some((json, credential_data)) = read_well_known_gcp_adc_file() {
        parse_gcp_credentials_json(&credential_data, &json, environment_variables).await?
    } else {
        let project_id = match environment_variables
            .get("GCP_PROJECT_ID")
            .or_else(|| environment_variables.get("GOOGLE_CLOUD_PROJECT"))
        {
            Some(project_id) => project_id.clone(),
            None => fetch_gcp_metadata_project_id().await?,
        };
        let region = match environment_variables.get("GCP_REGION") {
            Some(region) => region.clone(),
            None => fetch_gcp_metadata_region().await?,
        };
        (GcpCredentials::ServiceMetadata, project_id, region)
    };

    Ok(GcpClientConfig {
        project_id,
        region,
        credentials,
        service_overrides: parse_gcp_service_overrides(environment_variables)?,
        project_number: None,
    })
}

#[cfg(feature = "gcp")]
fn parse_gcp_service_overrides(
    environment_variables: &HashMap<String, String>,
) -> Result<Option<GcpServiceOverrides>> {
    let Some(endpoints_json) = environment_variables.get("GCP_SERVICE_OVERRIDES_ENDPOINTS") else {
        return Ok(None);
    };

    let endpoints: HashMap<String, String> = serde_json::from_str(endpoints_json)
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "Failed to parse GCP_SERVICE_OVERRIDES_ENDPOINTS".to_string(),
            errors: None,
        })?;
    Ok(Some(GcpServiceOverrides { endpoints }))
}

#[cfg(feature = "gcp")]
async fn parse_gcp_credentials_json(
    credential_data: &serde_json::Value,
    raw_json: &str,
    environment_variables: &HashMap<String, String>,
) -> Result<(GcpCredentials, String, String)> {
    match credential_data["type"]
        .as_str()
        .unwrap_or("service_account")
    {
        "external_account" => {
            parse_gcp_external_account_credentials(credential_data, environment_variables)
        }
        "authorized_user" => {
            parse_gcp_authorized_user_credentials(credential_data, environment_variables)
        }
        _ => {
            let project_id = credential_data["project_id"]
                .as_str()
                .ok_or_else(|| {
                    AlienError::new(ErrorData::InvalidClientConfig {
                        message: "project_id not found in credentials file".to_string(),
                        errors: None,
                    })
                })?
                .to_string();
            let region = match environment_variables.get("GCP_REGION") {
                Some(region) => region.clone(),
                None => fetch_gcp_metadata_region().await?,
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
}

#[cfg(feature = "gcp")]
fn parse_gcp_external_account_credentials(
    credential_data: &serde_json::Value,
    environment_variables: &HashMap<String, String>,
) -> Result<(GcpCredentials, String, String)> {
    let audience = credential_data["audience"]
        .as_str()
        .ok_or_else(|| {
            AlienError::new(ErrorData::InvalidClientConfig {
                message: "audience not found in external_account credentials".to_string(),
                errors: None,
            })
        })?
        .to_string();
    let subject_token_type = credential_data["subject_token_type"]
        .as_str()
        .unwrap_or("urn:ietf:params:oauth:token-type:jwt")
        .to_string();
    let token_url = credential_data["token_url"]
        .as_str()
        .unwrap_or("https://sts.googleapis.com/v1/token")
        .to_string();
    let credential_source_file = credential_data["credential_source"]["file"]
        .as_str()
        .ok_or_else(|| {
            AlienError::new(ErrorData::InvalidClientConfig {
                message: "credential_source.file not found in external_account credentials"
                    .to_string(),
                errors: None,
            })
        })?
        .to_string();
    let service_account_impersonation_url = credential_data["service_account_impersonation_url"]
        .as_str()
        .map(ToString::to_string);
    let project_id = environment_variables
        .get("GCP_PROJECT_ID")
        .or_else(|| environment_variables.get("GOOGLE_CLOUD_PROJECT"))
        .cloned()
        .or_else(|| {
            credential_data["quota_project_id"]
                .as_str()
                .map(ToString::to_string)
        })
        .ok_or_else(|| AlienError::new(ErrorData::InvalidClientConfig {
            message: "Missing GCP_PROJECT_ID or GOOGLE_CLOUD_PROJECT environment variable for external_account credentials".to_string(),
            errors: None,
        }))?;
    let region = required_env(
        environment_variables,
        "GCP_REGION",
        "Missing GCP_REGION environment variable for external_account credentials",
    )?;

    Ok((
        GcpCredentials::ExternalAccount {
            audience,
            subject_token_type,
            token_url,
            credential_source_file,
            service_account_impersonation_url,
        },
        project_id,
        region,
    ))
}

#[cfg(feature = "gcp")]
fn parse_gcp_authorized_user_credentials(
    credential_data: &serde_json::Value,
    environment_variables: &HashMap<String, String>,
) -> Result<(GcpCredentials, String, String)> {
    let client_id = required_json_string(
        credential_data,
        "client_id",
        "client_id not found in authorized_user credentials",
    )?;
    let client_secret = required_json_string(
        credential_data,
        "client_secret",
        "client_secret not found in authorized_user credentials",
    )?;
    let refresh_token = required_json_string(
        credential_data,
        "refresh_token",
        "refresh_token not found in authorized_user credentials",
    )?;
    let project_id = environment_variables
        .get("GCP_PROJECT_ID")
        .cloned()
        .or_else(|| {
            credential_data["quota_project_id"]
                .as_str()
                .map(ToString::to_string)
        })
        .ok_or_else(|| AlienError::new(ErrorData::InvalidClientConfig {
            message: "Missing GCP_PROJECT_ID environment variable for authorized_user credentials (quota_project_id not found in credentials file either)".to_string(),
            errors: None,
        }))?;
    let region = required_env(
        environment_variables,
        "GCP_REGION",
        "Missing GCP_REGION environment variable for authorized_user credentials",
    )?;

    Ok((
        GcpCredentials::AuthorizedUser {
            client_id,
            client_secret,
            refresh_token,
        },
        project_id,
        region,
    ))
}

#[cfg(feature = "gcp")]
fn read_well_known_gcp_adc_file() -> Option<(String, serde_json::Value)> {
    let home = std::env::var("HOME").ok()?;
    let adc_path = std::path::Path::new(&home)
        .join(".config")
        .join("gcloud")
        .join("application_default_credentials.json");
    let json = std::fs::read_to_string(adc_path).ok()?;
    let value = serde_json::from_str(&json).ok()?;
    Some((json, value))
}

#[cfg(feature = "gcp")]
async fn fetch_gcp_metadata_project_id() -> Result<String> {
    fetch_gcp_metadata_text(
        "http://metadata.google.internal/computeMetadata/v1/project/project-id",
        "Failed to fetch project ID from GCP metadata server",
        "Failed to parse project ID response from GCP metadata server",
    )
    .await
}

#[cfg(feature = "gcp")]
async fn fetch_gcp_metadata_region() -> Result<String> {
    let region_url = "http://metadata.google.internal/computeMetadata/v1/instance/region";
    let client = reqwest::Client::new();
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
        let zone = fetch_gcp_metadata_text(
            zone_url,
            "Failed to fetch zone from GCP metadata server",
            "Failed to parse zone response from GCP metadata server",
        )
        .await?;
        let zone = zone.split('/').last().ok_or_else(|| {
            AlienError::new(ErrorData::InvalidClientConfig {
                message: format!("Invalid zone format from metadata server: {zone}"),
                errors: None,
            })
        })?;
        gcp_region_from_zone(zone).ok_or_else(|| {
            AlienError::new(ErrorData::InvalidClientConfig {
                message: format!("Invalid zone format from metadata server: {zone}"),
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
            message: format!("Metadata server returned error {status}: {error_text}"),
            url: region_url.to_string(),
            http_status: status.as_u16(),
            http_request_text: None,
            http_response_text: Some(error_text),
        }));
    };

    let region_full_path = region_response.trim();
    let region = region_full_path.split('/').last().ok_or_else(|| {
        AlienError::new(ErrorData::InvalidClientConfig {
            message: format!("Invalid region format from metadata server: {region_full_path}"),
            errors: None,
        })
    })?;
    Ok(region.to_string())
}

#[cfg(feature = "gcp")]
async fn fetch_gcp_metadata_text(
    url: &str,
    request_message: &str,
    parse_message: &str,
) -> Result<String> {
    let response = reqwest::Client::new()
        .get(url)
        .header("Metadata-Flavor", "Google")
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: request_message.to_string(),
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(AlienError::new(ErrorData::HttpResponseError {
            message: format!("Metadata server returned error {status}: {error_text}"),
            url: url.to_string(),
            http_status: status.as_u16(),
            http_request_text: None,
            http_response_text: Some(error_text),
        }));
    }

    response
        .text()
        .await
        .map(|text| text.trim().to_string())
        .into_alien_error()
        .context(ErrorData::SerializationError {
            message: parse_message.to_string(),
        })
}

#[cfg(feature = "gcp")]
fn gcp_region_from_zone(zone: &str) -> Option<String> {
    let mut parts: Vec<&str> = zone.split('-').collect();
    (parts.len() >= 3).then(|| {
        parts.pop();
        parts.join("-")
    })
}

#[cfg(feature = "gcp")]
fn impersonated_gcp_config(
    config: &GcpClientConfig,
    impersonation: alien_core::GcpImpersonationConfig,
) -> GcpClientConfig {
    let has_target_project = impersonation.target_project_id.is_some();
    let project_id = impersonation
        .target_project_id
        .clone()
        .unwrap_or_else(|| config.project_id.clone());
    let region = impersonation
        .target_region
        .clone()
        .unwrap_or_else(|| config.region.clone());

    GcpClientConfig {
        project_id,
        region,
        credentials: GcpCredentials::ImpersonatedServiceAccount {
            source: Box::new(config.clone()),
            config: impersonation,
        },
        service_overrides: config.service_overrides.clone(),
        project_number: if has_target_project {
            None
        } else {
            config.project_number.clone()
        },
    }
}

#[cfg(feature = "azure")]
fn azure_config_from_env(
    environment_variables: &HashMap<String, String>,
) -> Result<AzureClientConfig> {
    let credentials = if let Some(token) = environment_variables.get("AZURE_ACCESS_TOKEN") {
        AzureCredentials::AccessToken {
            token: token.clone(),
        }
    } else if let (Some(client_id), Some(federated_token_file)) = (
        environment_variables.get("AZURE_CLIENT_ID"),
        environment_variables.get("AZURE_FEDERATED_TOKEN_FILE"),
    ) {
        AzureCredentials::WorkloadIdentity {
            client_id: client_id.clone(),
            tenant_id: required_env(
                environment_variables,
                "AZURE_TENANT_ID",
                "Missing AZURE_TENANT_ID environment variable for workload identity",
            )?,
            federated_token_file: federated_token_file.clone(),
            authority_host: environment_variables
                .get("AZURE_AUTHORITY_HOST")
                .cloned()
                .unwrap_or_else(|| "https://login.microsoftonline.com/".to_string()),
        }
    } else if let (Some(client_id), Some(client_secret)) = (
        environment_variables.get("AZURE_CLIENT_ID"),
        environment_variables.get("AZURE_CLIENT_SECRET"),
    ) {
        AzureCredentials::ServicePrincipal {
            client_id: client_id.clone(),
            client_secret: client_secret.clone(),
        }
    } else if let (Some(client_id), Some(identity_endpoint), Some(identity_header)) = (
        environment_variables.get("AZURE_CLIENT_ID"),
        environment_variables.get("IDENTITY_ENDPOINT"),
        environment_variables.get("IDENTITY_HEADER"),
    ) {
        AzureCredentials::ManagedIdentity {
            client_id: client_id.clone(),
            identity_endpoint: identity_endpoint.clone(),
            identity_header: identity_header.clone(),
        }
    } else if let Some(client_id) = environment_variables.get("AZURE_CLIENT_ID") {
        AzureCredentials::VmManagedIdentity {
            client_id: client_id.clone(),
            identity_endpoint: environment_variables.get("AZURE_IMDS_ENDPOINT").cloned(),
        }
    } else {
        return Err(AlienError::new(ErrorData::InvalidClientConfig {
            message: "Missing Azure credentials environment variables. Provide one of: AZURE_ACCESS_TOKEN, AZURE_CLIENT_ID+AZURE_CLIENT_SECRET, AZURE_CLIENT_ID+AZURE_FEDERATED_TOKEN_FILE, or AZURE_CLIENT_ID+IDENTITY_ENDPOINT+IDENTITY_HEADER".to_string(),
            errors: None,
        }));
    };

    Ok(AzureClientConfig {
        subscription_id: required_env(
            environment_variables,
            "AZURE_SUBSCRIPTION_ID",
            "Missing AZURE_SUBSCRIPTION_ID environment variable",
        )?,
        tenant_id: required_env(
            environment_variables,
            "AZURE_TENANT_ID",
            "Missing AZURE_TENANT_ID environment variable",
        )?,
        region: environment_variables.get("AZURE_REGION").cloned(),
        credentials,
        service_overrides: parse_azure_service_overrides(environment_variables)?,
    })
}

#[cfg(feature = "azure")]
fn parse_azure_service_overrides(
    environment_variables: &HashMap<String, String>,
) -> Result<Option<AzureServiceOverrides>> {
    let Some(endpoints_json) = environment_variables.get("AZURE_SERVICE_OVERRIDES_ENDPOINTS")
    else {
        return Ok(None);
    };
    let endpoints: HashMap<String, String> = serde_json::from_str(endpoints_json)
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "Failed to parse AZURE_SERVICE_OVERRIDES_ENDPOINTS".to_string(),
            errors: None,
        })?;
    Ok(Some(AzureServiceOverrides { endpoints }))
}

#[cfg(feature = "azure")]
fn impersonated_azure_config(
    config: &AzureClientConfig,
    impersonation: alien_core::AzureImpersonationConfig,
) -> Result<AzureClientConfig> {
    let credentials = match &config.credentials {
        AzureCredentials::WorkloadIdentity {
            federated_token_file,
            authority_host,
            ..
        } => AzureCredentials::WorkloadIdentity {
            client_id: impersonation.client_id.clone(),
            tenant_id: impersonation
                .tenant_id
                .clone()
                .unwrap_or_else(|| config.tenant_id.clone()),
            federated_token_file: federated_token_file.clone(),
            authority_host: authority_host.clone(),
        },
        AzureCredentials::ManagedIdentity {
            identity_endpoint,
            identity_header,
            ..
        } => AzureCredentials::ManagedIdentity {
            client_id: impersonation.client_id.clone(),
            identity_endpoint: identity_endpoint.clone(),
            identity_header: identity_header.clone(),
        },
        AzureCredentials::VmManagedIdentity {
            identity_endpoint, ..
        } => AzureCredentials::VmManagedIdentity {
            client_id: impersonation.client_id.clone(),
            identity_endpoint: identity_endpoint.clone(),
        },
        AzureCredentials::AccessToken { .. } => {
            return Err(AlienError::new(ErrorData::InvalidInput {
                message: "Cannot impersonate Azure using an existing access token".to_string(),
                field_name: Some("credentials".to_string()),
            }));
        }
        AzureCredentials::ServicePrincipal { .. } => {
            return Err(AlienError::new(ErrorData::InvalidInput {
                message: "Azure service principal token exchange for impersonation is not supported in client configuration; use workload identity or managed identity credentials".to_string(),
                field_name: Some("credentials".to_string()),
            }));
        }
    };

    Ok(AzureClientConfig {
        subscription_id: impersonation
            .target_subscription_id
            .unwrap_or_else(|| config.subscription_id.clone()),
        tenant_id: impersonation
            .tenant_id
            .unwrap_or_else(|| config.tenant_id.clone()),
        region: impersonation
            .target_region
            .or_else(|| config.region.clone()),
        credentials,
        service_overrides: config.service_overrides.clone(),
    })
}

#[cfg(any(feature = "gcp", feature = "azure"))]
fn required_env(
    environment_variables: &HashMap<String, String>,
    name: &str,
    message: &str,
) -> Result<String> {
    environment_variables.get(name).cloned().ok_or_else(|| {
        AlienError::new(ErrorData::InvalidClientConfig {
            message: message.to_string(),
            errors: None,
        })
    })
}

#[cfg(feature = "gcp")]
fn required_json_string(value: &serde_json::Value, field: &str, message: &str) -> Result<String> {
    value[field]
        .as_str()
        .map(ToString::to_string)
        .ok_or_else(|| {
            AlienError::new(ErrorData::InvalidClientConfig {
                message: message.to_string(),
                errors: None,
            })
        })
}

#[cfg(feature = "aws")]
async fn aws_config_from_env(
    environment_variables: &HashMap<String, String>,
) -> Result<alien_core::AwsClientConfig> {
    let region = resolve_aws_region(environment_variables).await?;
    let credentials = resolve_aws_credentials(environment_variables)?;
    let service_overrides =
        parse_aws_service_overrides(environment_variables.get("AWS_SERVICE_OVERRIDES_ENDPOINTS"))?;
    let account_id = infer_aws_account_id(
        environment_variables,
        &region,
        &credentials,
        service_overrides.as_ref(),
    )
    .await?;

    Ok(alien_core::AwsClientConfig {
        account_id,
        region,
        credentials,
        service_overrides,
    })
}

#[cfg(feature = "aws")]
async fn resolve_aws_region(environment_variables: &HashMap<String, String>) -> Result<String> {
    if let Some(region) = environment_variables.get("AWS_REGION") {
        return Ok(region.clone());
    }

    if let Some(region) = environment_variables.get("AWS_DEFAULT_REGION") {
        return Ok(region.clone());
    }

    if let Ok(Some(region)) = load_aws_profile_region(&aws_profile_name(environment_variables)) {
        return Ok(region);
    }

    if !aws_metadata_disabled(environment_variables) {
        if let Ok(region) = load_aws_imds_region(environment_variables).await {
            return Ok(region);
        }
    }

    Err(AlienError::new(ErrorData::InvalidClientConfig {
        message: "Missing AWS region. Set AWS_REGION, AWS_DEFAULT_REGION, or configure a default region in your AWS profile.".to_string(),
        errors: None,
    }))
}

#[cfg(feature = "aws")]
fn resolve_aws_credentials(
    environment_variables: &HashMap<String, String>,
) -> Result<alien_core::AwsCredentials> {
    if let (Some(role_arn), Some(token_file)) = (
        environment_variables.get("AWS_ROLE_ARN"),
        environment_variables.get("AWS_WEB_IDENTITY_TOKEN_FILE"),
    ) {
        return Ok(alien_core::AwsCredentials::WebIdentity {
            config: alien_core::AwsWebIdentityConfig {
                role_arn: role_arn.clone(),
                session_name: environment_variables.get("AWS_ROLE_SESSION_NAME").cloned(),
                web_identity_token_file: token_file.clone(),
                duration_seconds: environment_variables
                    .get("AWS_ROLE_DURATION_SECONDS")
                    .and_then(|duration| duration.parse().ok()),
            },
        });
    }

    if let (Some(access_key_id), Some(secret_access_key)) = (
        environment_variables.get("AWS_ACCESS_KEY_ID"),
        environment_variables.get("AWS_SECRET_ACCESS_KEY"),
    ) {
        return Ok(alien_core::AwsCredentials::AccessKeys {
            access_key_id: access_key_id.clone(),
            secret_access_key: secret_access_key.clone(),
            session_token: environment_variables
                .get("AWS_SESSION_TOKEN")
                .filter(|token| !token.trim().is_empty())
                .cloned(),
        });
    }

    if aws_profile_is_explicit(environment_variables) {
        return Ok(alien_core::AwsCredentials::Profile {
            name: aws_profile_name(environment_variables),
        });
    }

    if !aws_metadata_disabled(environment_variables) {
        return Ok(alien_core::AwsCredentials::Imds {
            endpoint: environment_variables
                .get("AWS_EC2_METADATA_SERVICE_ENDPOINT")
                .cloned(),
        });
    }

    Err(AlienError::new(ErrorData::InvalidClientConfig {
        message: "Missing AWS credentials. Set AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY, AWS_PROFILE, AWS_ROLE_ARN/AWS_WEB_IDENTITY_TOKEN_FILE, or enable EC2 metadata credentials.".to_string(),
        errors: None,
    }))
}

#[cfg(feature = "aws")]
fn parse_aws_service_overrides(
    endpoints_json: Option<&String>,
) -> Result<Option<alien_core::AwsServiceOverrides>> {
    endpoints_json
        .map(|endpoints_json| {
            serde_json::from_str(endpoints_json)
                .into_alien_error()
                .context(ErrorData::InvalidClientConfig {
                    message: "Failed to parse AWS_SERVICE_OVERRIDES_ENDPOINTS".to_string(),
                    errors: None,
                })
                .map(|endpoints| alien_core::AwsServiceOverrides { endpoints })
        })
        .transpose()
}

#[cfg(feature = "aws")]
async fn infer_aws_account_id(
    environment_variables: &HashMap<String, String>,
    region: &str,
    credentials: &alien_core::AwsCredentials,
    service_overrides: Option<&alien_core::AwsServiceOverrides>,
) -> Result<String> {
    if let Some(account_id) = environment_variables.get("AWS_ACCOUNT_ID") {
        return Ok(account_id.clone());
    }

    if let Some(role_arn) = environment_variables.get("AWS_ROLE_ARN") {
        if let Some(account_id) = extract_aws_account_id_from_role_arn(role_arn) {
            return Ok(account_id);
        }
    }

    if let alien_core::AwsCredentials::WebIdentity { config } = credentials {
        if let Some(account_id) = extract_aws_account_id_from_role_arn(&config.role_arn) {
            return Ok(account_id);
        }
    }

    let probe_config = alien_core::AwsClientConfig {
        account_id: String::new(),
        region: region.to_string(),
        credentials: credentials.clone(),
        service_overrides: service_overrides.cloned(),
    };

    let caller_identity = sts_client_from_aws_config(&probe_config)
        .await?
        .get_caller_identity()
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "Failed to infer AWS account ID from STS caller identity".to_string(),
            errors: None,
        })?;

    caller_identity
        .account()
        .map(ToString::to_string)
        .ok_or_else(|| {
            AlienError::new(ErrorData::InvalidClientConfig {
                message: "STS caller identity response did not include an account ID".to_string(),
                errors: None,
            })
        })
}

#[cfg(feature = "aws")]
async fn assume_aws_role_config(
    config: &alien_core::AwsClientConfig,
    impersonation: alien_core::AwsImpersonationConfig,
) -> Result<alien_core::AwsClientConfig> {
    let target_account_id = extract_aws_account_id_from_role_arn(&impersonation.role_arn)
        .unwrap_or_else(|| config.account_id.clone());
    let target_region = impersonation
        .target_region
        .clone()
        .unwrap_or_else(|| config.region.clone());
    let session_name = impersonation
        .session_name
        .as_deref()
        .unwrap_or("alien-impersonation");

    let mut request = sts_client_from_aws_config(config)
        .await?
        .assume_role()
        .role_arn(&impersonation.role_arn)
        .role_session_name(session_name);

    if let Some(duration_seconds) = impersonation.duration_seconds {
        request = request.duration_seconds(duration_seconds);
    }
    if let Some(external_id) = &impersonation.external_id {
        request = request.external_id(external_id);
    }

    let response =
        request
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::AuthenticationError {
                message: format!("Failed to assume AWS role '{}'", impersonation.role_arn),
            })?;

    let credentials = response.credentials().ok_or_else(|| {
        AlienError::new(ErrorData::AuthenticationError {
            message: format!(
                "AssumeRole for '{}' returned no credentials",
                impersonation.role_arn
            ),
        })
    })?;

    let expires_at = credentials
        .expiration()
        .fmt(aws_sdk_sts::primitives::DateTimeFormat::DateTime)
        .into_alien_error()
        .context(ErrorData::AuthenticationError {
            message: format!(
                "Failed to format AssumeRole credential expiration for '{}'",
                impersonation.role_arn
            ),
        })?;

    Ok(alien_core::AwsClientConfig {
        account_id: target_account_id,
        region: target_region,
        credentials: alien_core::AwsCredentials::SessionCredentials {
            access_key_id: credentials.access_key_id().to_string(),
            secret_access_key: credentials.secret_access_key().to_string(),
            session_token: credentials.session_token().to_string(),
            expires_at,
        },
        service_overrides: config.service_overrides.clone(),
    })
}

#[cfg(feature = "aws")]
async fn sdk_config_from_aws_config(
    config: &alien_core::AwsClientConfig,
) -> Result<aws_config::SdkConfig> {
    let region = aws_types::region::Region::new(config.region.clone());
    let loader = aws_config::defaults(aws_config::BehaviorVersion::latest()).region(region.clone());

    let loader = match &config.credentials {
        alien_core::AwsCredentials::AccessKeys {
            access_key_id,
            secret_access_key,
            session_token,
        } => loader.credentials_provider(aws_credential_types::Credentials::new(
            access_key_id,
            secret_access_key,
            session_token.clone(),
            None,
            "AlienAccessKeys",
        )),
        alien_core::AwsCredentials::SessionCredentials {
            access_key_id,
            secret_access_key,
            session_token,
            expires_at,
        } => {
            let expires_after = chrono::DateTime::parse_from_rfc3339(expires_at)
                .map(|expires_at| expires_at.to_utc().into())
                .into_alien_error()
                .context(ErrorData::InvalidClientConfig {
                    message: format!("Invalid AWS credential expiration timestamp: {expires_at}"),
                    errors: None,
                })?;
            loader.credentials_provider(aws_credential_types::Credentials::new(
                access_key_id,
                secret_access_key,
                Some(session_token.clone()),
                Some(expires_after),
                "AlienSessionCredentials",
            ))
        }
        alien_core::AwsCredentials::Profile { name } => loader.profile_name(name),
        alien_core::AwsCredentials::WebIdentity { config } => {
            let provider_config = aws_config::provider_config::ProviderConfig::without_region()
                .with_region(Some(region));
            let provider =
                aws_config::web_identity_token::WebIdentityTokenCredentialsProvider::builder()
                    .configure(&provider_config)
                    .static_configuration(aws_config::web_identity_token::StaticConfiguration {
                        web_identity_token_file: config.web_identity_token_file.clone().into(),
                        role_arn: config.role_arn.clone(),
                        session_name: config
                            .session_name
                            .clone()
                            .unwrap_or_else(|| "alien-web-identity".to_string()),
                    })
                    .build();
            loader.credentials_provider(provider)
        }
        alien_core::AwsCredentials::Imds { endpoint } => {
            let provider_config = aws_config::provider_config::ProviderConfig::without_region()
                .with_region(Some(region));
            let mut client_builder =
                aws_config::imds::Client::builder().configure(&provider_config);
            if let Some(endpoint) = endpoint {
                client_builder = client_builder.endpoint(endpoint).map_err(|err| {
                    AlienError::new(ErrorData::InvalidClientConfig {
                        message: format!("Invalid AWS IMDS endpoint override '{endpoint}': {err}"),
                        errors: None,
                    })
                })?;
            }
            let imds_client = client_builder.build();
            let provider = aws_config::imds::credentials::ImdsCredentialsProvider::builder()
                .configure(&provider_config)
                .imds_client(imds_client)
                .build();
            loader.credentials_provider(provider)
        }
    };

    Ok(loader.load().await)
}

#[cfg(feature = "aws")]
async fn sts_client_from_aws_config(
    config: &alien_core::AwsClientConfig,
) -> Result<aws_sdk_sts::Client> {
    let sdk_config = sdk_config_from_aws_config(config).await?;
    let mut sts_config = aws_sdk_sts::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("sts"))
    {
        sts_config = sts_config.endpoint_url(endpoint);
    }

    Ok(aws_sdk_sts::Client::from_conf(sts_config.build()))
}

#[cfg(feature = "aws")]
fn aws_profile_is_explicit(environment_variables: &HashMap<String, String>) -> bool {
    environment_variables.contains_key("AWS_PROFILE")
        || environment_variables.contains_key("AWS_DEFAULT_PROFILE")
}

#[cfg(feature = "aws")]
fn aws_metadata_disabled(environment_variables: &HashMap<String, String>) -> bool {
    environment_variables
        .get("AWS_EC2_METADATA_DISABLED")
        .map(|value| value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[cfg(feature = "aws")]
fn aws_profile_name(environment_variables: &HashMap<String, String>) -> String {
    environment_variables
        .get("AWS_PROFILE")
        .or_else(|| environment_variables.get("AWS_DEFAULT_PROFILE"))
        .cloned()
        .unwrap_or_else(|| "default".to_string())
}

#[cfg(all(feature = "aws", not(target_arch = "wasm32")))]
fn load_aws_profile_region(profile: &str) -> Result<Option<String>> {
    let output = std::process::Command::new("aws")
        .args(["configure", "get", "region", "--profile", profile])
        .output()
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: format!("Failed to invoke AWS CLI for profile '{}'", profile),
            errors: None,
        })?;

    if !output.status.success() {
        return Ok(None);
    }

    let region = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if region.is_empty() {
        Ok(None)
    } else {
        Ok(Some(region))
    }
}

#[cfg(all(feature = "aws", target_arch = "wasm32"))]
fn load_aws_profile_region(_profile: &str) -> Result<Option<String>> {
    Ok(None)
}

#[cfg(feature = "aws")]
async fn load_aws_imds_region(environment_variables: &HashMap<String, String>) -> Result<String> {
    let endpoint = environment_variables
        .get("AWS_EC2_METADATA_SERVICE_ENDPOINT")
        .map(String::as_str)
        .unwrap_or("http://169.254.169.254")
        .trim_end_matches('/');

    let client = reqwest::Client::builder()
        .build()
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "Failed to create AWS IMDS HTTP client".to_string(),
            errors: None,
        })?;

    let token = client
        .put(format!("{endpoint}/latest/api/token"))
        .timeout(std::time::Duration::from_millis(500))
        .header("X-aws-ec2-metadata-token-ttl-seconds", "21600")
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "Failed to request AWS IMDSv2 token".to_string(),
            errors: None,
        })?
        .error_for_status()
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "AWS IMDSv2 token request failed".to_string(),
            errors: None,
        })?
        .text()
        .await
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "Failed to read AWS IMDSv2 token".to_string(),
            errors: None,
        })?;

    let region = client
        .get(format!("{endpoint}/latest/meta-data/placement/region"))
        .timeout(std::time::Duration::from_millis(500))
        .header("X-aws-ec2-metadata-token", token.trim())
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "Failed to request AWS IMDS region".to_string(),
            errors: None,
        })?
        .error_for_status()
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "AWS IMDS region request failed".to_string(),
            errors: None,
        })?
        .text()
        .await
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "Failed to read AWS IMDS region".to_string(),
            errors: None,
        })?;

    let region = region.trim();
    if region.is_empty() {
        return Err(AlienError::new(ErrorData::InvalidClientConfig {
            message: "AWS IMDS did not return a region".to_string(),
            errors: None,
        }));
    }

    Ok(region.to_string())
}

#[cfg(feature = "aws")]
fn extract_aws_account_id_from_role_arn(role_arn: &str) -> Option<String> {
    let parts: Vec<&str> = role_arn.split(':').collect();
    (parts.len() >= 6 && parts[2] == "iam")
        .then(|| parts[4].to_string())
        .filter(|account_id| !account_id.is_empty())
}

#[cfg(feature = "kubernetes")]
fn kubernetes_config_from_env(
    environment_variables: &HashMap<String, String>,
) -> Result<alien_core::KubernetesClientConfig> {
    if environment_variables.contains_key("KUBERNETES_SERVER_URL") {
        return kubernetes_config_from_manual_env(environment_variables);
    }

    let additional_headers = parse_kubernetes_additional_headers(environment_variables)?;

    if is_in_cluster_env(environment_variables) {
        return Ok(alien_core::KubernetesClientConfig::InCluster {
            namespace: environment_variables.get("KUBERNETES_NAMESPACE").cloned(),
            additional_headers,
        });
    }

    Ok(alien_core::KubernetesClientConfig::Kubeconfig {
        kubeconfig_path: environment_variables.get("KUBECONFIG").cloned(),
        context: environment_variables.get("KUBERNETES_CONTEXT").cloned(),
        cluster: environment_variables.get("KUBERNETES_CLUSTER").cloned(),
        user: environment_variables.get("KUBERNETES_USER").cloned(),
        namespace: environment_variables.get("KUBERNETES_NAMESPACE").cloned(),
        additional_headers,
    })
}

#[cfg(feature = "kubernetes")]
fn kubernetes_config_from_manual_env(
    environment_variables: &HashMap<String, String>,
) -> Result<alien_core::KubernetesClientConfig> {
    let server_url = environment_variables
        .get("KUBERNETES_SERVER_URL")
        .ok_or_else(|| {
            AlienError::new(ErrorData::InvalidClientConfig {
                message: "KUBERNETES_SERVER_URL is required for manual mode".to_string(),
                errors: None,
            })
        })?
        .clone();

    Ok(alien_core::KubernetesClientConfig::Manual {
        server_url,
        certificate_authority_data: environment_variables.get("KUBERNETES_CA_DATA").cloned(),
        insecure_skip_tls_verify: environment_variables
            .get("KUBERNETES_INSECURE_SKIP_TLS_VERIFY")
            .map(|value| value == "true" || value == "1"),
        client_certificate_data: environment_variables
            .get("KUBERNETES_CLIENT_CERT_DATA")
            .cloned(),
        client_key_data: environment_variables
            .get("KUBERNETES_CLIENT_KEY_DATA")
            .cloned(),
        token: environment_variables
            .get("KUBERNETES_BEARER_TOKEN")
            .cloned(),
        username: environment_variables.get("KUBERNETES_USERNAME").cloned(),
        password: environment_variables.get("KUBERNETES_PASSWORD").cloned(),
        namespace: environment_variables.get("KUBERNETES_NAMESPACE").cloned(),
        additional_headers: parse_kubernetes_additional_headers(environment_variables)?
            .unwrap_or_default(),
    })
}

#[cfg(feature = "kubernetes")]
fn is_in_cluster_env(environment_variables: &HashMap<String, String>) -> bool {
    let has_kubernetes_service = environment_variables.contains_key("KUBERNETES_SERVICE_HOST")
        && environment_variables.contains_key("KUBERNETES_SERVICE_PORT");
    let has_service_account =
        std::path::Path::new("/var/run/secrets/kubernetes.io/serviceaccount/token").exists();

    has_kubernetes_service || has_service_account
}

#[cfg(feature = "kubernetes")]
fn parse_kubernetes_additional_headers(
    environment_variables: &HashMap<String, String>,
) -> Result<Option<HashMap<String, String>>> {
    let Some(headers_json) = environment_variables.get("KUBERNETES_ADDITIONAL_HEADERS") else {
        return Ok(None);
    };

    serde_json::from_str(headers_json)
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "Failed to parse KUBERNETES_ADDITIONAL_HEADERS".to_string(),
            errors: None,
        })
}
