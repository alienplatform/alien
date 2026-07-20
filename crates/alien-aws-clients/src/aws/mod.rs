use alien_client_core::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use aws_credential_types::Credentials;
use serde::Deserialize;
use std::{collections::HashMap, time::Duration};

// Re-export types from alien-core
pub use alien_core::{
    AwsClientConfig, AwsCredentials, AwsImpersonationConfig,
    AwsServiceOverrides as ServiceOverrides, AwsWebIdentityConfig,
};

/// Trait for AWS platform configuration operations
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait AwsClientConfigExt {
    /// Create a new `AwsClientConfig` from environment variables.
    async fn from_env(environment_variables: &HashMap<String, String>) -> Result<AwsClientConfig>;

    /// Create a new `AwsClientConfig` from standard environment variables.
    async fn from_std_env() -> Result<AwsClientConfig>;

    /// Assume an AWS IAM role and return a new platform config with the assumed credentials
    async fn impersonate(&self, config: AwsImpersonationConfig) -> Result<AwsClientConfig>;

    /// Get AWS credentials from this config
    fn get_credentials(&self) -> Credentials;

    /// Get credentials for web identity token authentication
    async fn get_web_identity_credentials(&self) -> Result<AwsClientConfig>;

    /// Resolve any refreshable source and exchange static keys for an expiring
    /// STS session, returning only `SessionCredentials`.
    async fn materialize_session_credentials(&self) -> Result<AwsClientConfig>;

    /// Get service endpoint, checking for overrides first
    fn get_service_endpoint(&self, service_name: &str, default_endpoint: &str) -> String;

    /// Get the endpoint for a specific service, with override support (returns Option)
    fn get_service_endpoint_option(&self, service_name: &str) -> Option<&str>;

    /// Create a config with service endpoint overrides for testing
    #[cfg(any(test, feature = "test-utils"))]
    fn with_service_overrides(self, overrides: ServiceOverrides) -> Self;

    /// Create a mock AwsClientConfig with dummy values for testing
    #[cfg(any(test, feature = "test-utils"))]
    fn mock() -> Self;
}

pub mod acm;
pub mod apigatewayv2;
pub mod autoscaling;
pub mod aws_request_utils;
pub mod cloudformation;
pub mod cloudwatch;
pub mod codebuild;
pub mod credential_provider;
pub mod dynamodb;
pub mod ec2;
pub mod ecr;
pub mod eks;
pub mod elbv2;
pub mod eventbridge;
pub mod iam;
pub mod lambda;
pub mod rds;
pub mod resourcegroupstagging;
pub mod s3;
pub mod secrets_manager;
pub mod sqs;
pub mod ssm;
pub mod sts;

const AWS_IMDS_ENDPOINT: &str = "http://169.254.169.254";
const AWS_IMDS_DISCOVERY_TIMEOUT: Duration = Duration::from_millis(500);
const AWS_IMDS_CREDENTIALS_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl AwsClientConfigExt for AwsClientConfig {
    /// Create a new `AwsClientConfig` from environment variables.
    async fn from_env(environment_variables: &HashMap<String, String>) -> Result<Self> {
        let region = resolve_region(environment_variables).await?;
        let credentials = resolve_credentials(environment_variables).await?;
        let service_overrides =
            parse_service_overrides(environment_variables.get("AWS_SERVICE_OVERRIDES_ENDPOINTS"))?;
        let account_id = infer_account_id(
            environment_variables,
            &region,
            &credentials,
            service_overrides.as_ref(),
        )
        .await?;

        let config = Self {
            account_id,
            region,
            credentials,
            service_overrides,
        };

        Ok(config)
    }

    /// Create a new `AwsClientConfig` from standard environment variables.
    async fn from_std_env() -> Result<Self> {
        let env_vars: HashMap<String, String> = std::env::vars().collect();
        Self::from_env(&env_vars).await
    }

    /// Assume an AWS IAM role and return a new platform config with the assumed credentials
    async fn impersonate(&self, config: AwsImpersonationConfig) -> Result<AwsClientConfig> {
        use crate::aws::sts::{AssumeRoleRequest, StsApi, StsClient};
        use reqwest::Client;
        use uuid::Uuid;

        // Extract the target account ID from the role ARN (arn:aws:iam::{account_id}:role/...).
        // This ensures cross-account impersonation produces a config with the correct account.
        let target_account_id = extract_account_id_from_role_arn(&config.role_arn)
            .unwrap_or_else(|| self.account_id.clone());

        let target_region = config.target_region.unwrap_or_else(|| self.region.clone());

        // Resolve the source before calling AssumeRole, which requires signed credentials.
        let base_config = self.get_web_identity_credentials().await?;
        let sts_client = StsClient::new(Client::new(), base_config);

        let session_name = config
            .session_name
            .unwrap_or_else(|| format!("alien-impersonation-{}", Uuid::new_v4().simple()));

        let assume_role_request = AssumeRoleRequest::builder()
            .role_arn(config.role_arn)
            .role_session_name(session_name)
            .maybe_duration_seconds(config.duration_seconds)
            .maybe_external_id(config.external_id)
            .build();

        let response = sts_client.assume_role(assume_role_request).await?;

        let credentials = response.assume_role_result.credentials;

        Ok(AwsClientConfig {
            account_id: target_account_id,
            region: target_region,
            credentials: AwsCredentials::SessionCredentials {
                access_key_id: credentials.access_key_id,
                secret_access_key: credentials.secret_access_key,
                session_token: credentials.session_token,
                expires_at: credentials.expiration,
            },
            service_overrides: self.service_overrides.clone(),
        })
    }

    /// Get AWS credentials from this config.
    ///
    /// Refreshable sources must be resolved before this synchronous method is
    /// called. Callers that sign requests should use `AwsCredentialProvider`.
    fn get_credentials(&self) -> Credentials {
        match &self.credentials {
            AwsCredentials::AccessKeys {
                access_key_id,
                secret_access_key,
                session_token,
            } => Credentials::new(
                access_key_id.clone(),
                secret_access_key.clone(),
                session_token.clone(),
                None,
                "ProvidedCredentials",
            ),
            AwsCredentials::SessionCredentials {
                access_key_id,
                secret_access_key,
                session_token,
                ..
            } => Credentials::new(
                access_key_id.clone(),
                secret_access_key.clone(),
                Some(session_token.clone()),
                None,
                "SessionCredentials",
            ),
            AwsCredentials::Imds { .. }
            | AwsCredentials::Profile { .. }
            | AwsCredentials::WebIdentity { .. } => Credentials::new(
                "PLACEHOLDER_ACCESS_KEY".to_string(),
                "PLACEHOLDER_SECRET_KEY".to_string(),
                None,
                None,
                "UnresolvedCredentialSource",
            ),
        }
    }

    /// Get credentials for refreshable credential sources.
    async fn get_web_identity_credentials(&self) -> Result<AwsClientConfig> {
        match &self.credentials {
            AwsCredentials::WebIdentity { config } => {
                use crate::aws::sts::{AssumeRoleWithWebIdentityRequest, StsApi, StsClient};
                use reqwest::Client;
                use uuid::Uuid;

                let token = std::fs::read_to_string(&config.web_identity_token_file)
                    .into_alien_error()
                    .context(ErrorData::InvalidClientConfig {
                        message: format!(
                            "Failed to read web identity token file: {}",
                            config.web_identity_token_file
                        ),
                        errors: None,
                    })?
                    .trim()
                    .to_string();

                let temp_config = AwsClientConfig {
                    account_id: self.account_id.clone(),
                    region: self.region.clone(),
                    credentials: AwsCredentials::AccessKeys {
                        access_key_id: "TEMP".to_string(),
                        secret_access_key: "TEMP".to_string(),
                        session_token: None,
                    },
                    service_overrides: self.service_overrides.clone(),
                };

                let sts_client = StsClient::new(Client::new(), temp_config);

                let session_name = config
                    .session_name
                    .clone()
                    .unwrap_or_else(|| format!("alien-web-identity-{}", Uuid::new_v4().simple()));

                let assume_role_request = AssumeRoleWithWebIdentityRequest::builder()
                    .role_arn(config.role_arn.clone())
                    .role_session_name(session_name)
                    .web_identity_token(token)
                    .maybe_duration_seconds(config.duration_seconds)
                    .build();

                let response = sts_client
                    .assume_role_with_web_identity(assume_role_request)
                    .await?;
                let credentials = response.assume_role_with_web_identity_result.credentials;

                Ok(AwsClientConfig {
                    account_id: self.account_id.clone(),
                    region: self.region.clone(),
                    credentials: AwsCredentials::SessionCredentials {
                        access_key_id: credentials.access_key_id,
                        secret_access_key: credentials.secret_access_key,
                        session_token: credentials.session_token,
                        expires_at: credentials.expiration,
                    },
                    service_overrides: self.service_overrides.clone(),
                })
            }
            AwsCredentials::Imds { endpoint } => {
                let credentials = load_imds_session_credentials(endpoint.as_deref()).await?;
                Ok(AwsClientConfig {
                    account_id: self.account_id.clone(),
                    region: self.region.clone(),
                    credentials,
                    service_overrides: self.service_overrides.clone(),
                })
            }
            AwsCredentials::Profile { name } => {
                let credentials = load_profile_session_credentials(name)?;
                Ok(AwsClientConfig {
                    account_id: self.account_id.clone(),
                    region: self.region.clone(),
                    credentials,
                    service_overrides: self.service_overrides.clone(),
                })
            }
            AwsCredentials::AccessKeys { .. } | AwsCredentials::SessionCredentials { .. } => {
                Ok(self.clone())
            }
        }
    }

    async fn materialize_session_credentials(&self) -> Result<AwsClientConfig> {
        use crate::aws::sts::{StsApi, StsClient};

        let resolved = self.get_web_identity_credentials().await?;
        match resolved.credentials {
            AwsCredentials::SessionCredentials { .. } => Ok(resolved),
            AwsCredentials::AccessKeys { .. } => {
                let response = StsClient::new(reqwest::Client::new(), resolved.clone())
                    .get_session_token(Some(3600))
                    .await?;
                let credentials = response.get_session_token_result.credentials;
                Ok(AwsClientConfig {
                    account_id: resolved.account_id,
                    region: resolved.region,
                    credentials: AwsCredentials::SessionCredentials {
                        access_key_id: credentials.access_key_id,
                        secret_access_key: credentials.secret_access_key,
                        session_token: credentials.session_token,
                        expires_at: credentials.expiration,
                    },
                    service_overrides: resolved.service_overrides,
                })
            }
            AwsCredentials::Imds { .. }
            | AwsCredentials::Profile { .. }
            | AwsCredentials::WebIdentity { .. } => {
                Err(AlienError::new(ErrorData::InvalidClientConfig {
                    message: "AWS credential source did not resolve to session credentials"
                        .to_string(),
                    errors: None,
                }))
            }
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

    /// Create a mock AwsClientConfig with dummy values for testing
    #[cfg(any(test, feature = "test-utils"))]
    fn mock() -> Self {
        Self {
            account_id: "123456789012".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
                secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string(),
                session_token: None,
            },
            service_overrides: None,
        }
    }
}

async fn resolve_region(environment_variables: &HashMap<String, String>) -> Result<String> {
    if let Some(region) = environment_variables.get("AWS_REGION") {
        return Ok(region.clone());
    }

    if let Some(region) = environment_variables.get("AWS_DEFAULT_REGION") {
        return Ok(region.clone());
    }

    let imds_error = if !metadata_disabled(environment_variables) {
        match load_imds_region(environment_variables).await {
            Ok(region) => return Ok(region),
            Err(error) => Some(error),
        }
    } else {
        None
    };

    let profile = profile_name(environment_variables);
    match load_profile_region(&profile) {
        Ok(Some(region)) => return Ok(region),
        Ok(None) => {}
        Err(profile_error) => {
            if let Some(imds_error) = imds_error {
                return Err(AlienError::new(ErrorData::InvalidClientConfig {
                    message: format!(
                        "Failed to resolve AWS region from IMDS and fallback profile '{}': {}; IMDS error: {}",
                        profile, profile_error, imds_error
                    ),
                    errors: None,
                }));
            }
            return Err(profile_error);
        }
    }

    Err(AlienError::new(ErrorData::InvalidClientConfig {
        message: "Missing AWS region. Set AWS_REGION, AWS_DEFAULT_REGION, or configure a default region in your AWS profile.".to_string(),
        errors: None,
    }))
}

async fn resolve_credentials(
    environment_variables: &HashMap<String, String>,
) -> Result<AwsCredentials> {
    if let (Some(role_arn), Some(token_file)) = (
        environment_variables.get("AWS_ROLE_ARN"),
        environment_variables.get("AWS_WEB_IDENTITY_TOKEN_FILE"),
    ) {
        return Ok(AwsCredentials::WebIdentity {
            config: AwsWebIdentityConfig {
                role_arn: role_arn.clone(),
                session_name: environment_variables.get("AWS_ROLE_SESSION_NAME").cloned(),
                web_identity_token_file: token_file.clone(),
                duration_seconds: environment_variables
                    .get("AWS_ROLE_DURATION_SECONDS")
                    .and_then(|s| s.parse().ok()),
            },
        });
    }

    if let (Some(access_key_id), Some(secret_access_key)) = (
        environment_variables.get("AWS_ACCESS_KEY_ID"),
        environment_variables.get("AWS_SECRET_ACCESS_KEY"),
    ) {
        return Ok(AwsCredentials::AccessKeys {
            access_key_id: access_key_id.clone(),
            secret_access_key: secret_access_key.clone(),
            session_token: environment_variables
                .get("AWS_SESSION_TOKEN")
                .filter(|token| !token.trim().is_empty())
                .cloned(),
        });
    }

    if profile_is_explicit(environment_variables) {
        let profile = profile_name(environment_variables);
        return Ok(AwsCredentials::Profile { name: profile });
    }

    let imds_error = if !metadata_disabled(environment_variables) {
        match discover_imds_credentials(environment_variables).await {
            Ok(()) => {
                return Ok(AwsCredentials::Imds {
                    endpoint: environment_variables
                        .get("AWS_EC2_METADATA_SERVICE_ENDPOINT")
                        .cloned(),
                })
            }
            Err(error) => Some(error),
        }
    } else {
        None
    };

    let profile = profile_name(environment_variables);
    match load_profile_session_credentials(&profile) {
        Ok(_) => Ok(AwsCredentials::Profile { name: profile }),
        Err(profile_error) => {
            if let Some(imds_error) = imds_error {
                return Err(AlienError::new(ErrorData::InvalidClientConfig {
                    message: format!(
                        "Failed to resolve AWS credentials from IMDS and fallback profile '{}': {}; IMDS error: {}",
                        profile, profile_error, imds_error
                    ),
                    errors: None,
                }));
            }
            Err(profile_error)
        }
    }
}

fn profile_is_explicit(environment_variables: &HashMap<String, String>) -> bool {
    environment_variables.contains_key("AWS_PROFILE")
        || environment_variables.contains_key("AWS_DEFAULT_PROFILE")
}

fn metadata_disabled(environment_variables: &HashMap<String, String>) -> bool {
    environment_variables
        .get("AWS_EC2_METADATA_DISABLED")
        .map(|value| value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AwsImdsCredentials {
    access_key_id: String,
    secret_access_key: String,
    token: String,
    expiration: String,
}

async fn discover_imds_credentials(environment_variables: &HashMap<String, String>) -> Result<()> {
    let endpoint = environment_variables
        .get("AWS_EC2_METADATA_SERVICE_ENDPOINT")
        .map(String::as_str);
    load_imds_session_credentials(endpoint).await.map(|_| ())
}

async fn load_imds_session_credentials(endpoint: Option<&str>) -> Result<AwsCredentials> {
    let endpoint = endpoint.unwrap_or(AWS_IMDS_ENDPOINT).trim_end_matches('/');

    let client = reqwest::Client::builder()
        .build()
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "Failed to create AWS IMDS HTTP client".to_string(),
            errors: None,
        })?;

    let token_url = format!("{endpoint}/latest/api/token");
    let token = client
        .put(&token_url)
        .timeout(AWS_IMDS_DISCOVERY_TIMEOUT)
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

    let role_url = format!("{endpoint}/latest/meta-data/iam/security-credentials/");
    let role_name = client
        .get(&role_url)
        .timeout(AWS_IMDS_DISCOVERY_TIMEOUT)
        .header("X-aws-ec2-metadata-token", &token)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "Failed to request AWS IMDS role name".to_string(),
            errors: None,
        })?
        .error_for_status()
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "AWS IMDS role name request failed".to_string(),
            errors: None,
        })?
        .text()
        .await
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "Failed to read AWS IMDS role name".to_string(),
            errors: None,
        })?;

    let role_name = role_name
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .ok_or_else(|| {
            AlienError::new(ErrorData::InvalidClientConfig {
                message: "AWS IMDS did not return an IAM role name".to_string(),
                errors: None,
            })
        })?;

    let credentials_url = format!("{role_url}{role_name}");
    let credentials: AwsImdsCredentials = client
        .get(&credentials_url)
        .timeout(AWS_IMDS_CREDENTIALS_TIMEOUT)
        .header("X-aws-ec2-metadata-token", &token)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "Failed to request AWS IMDS credentials".to_string(),
            errors: None,
        })?
        .error_for_status()
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "AWS IMDS credentials request failed".to_string(),
            errors: None,
        })?
        .json()
        .await
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "Failed to parse AWS IMDS credentials".to_string(),
            errors: None,
        })?;

    Ok(AwsCredentials::SessionCredentials {
        access_key_id: credentials.access_key_id,
        secret_access_key: credentials.secret_access_key,
        session_token: credentials.token,
        expires_at: credentials.expiration,
    })
}

async fn load_imds_region(environment_variables: &HashMap<String, String>) -> Result<String> {
    let endpoint = environment_variables
        .get("AWS_EC2_METADATA_SERVICE_ENDPOINT")
        .map(String::as_str)
        .unwrap_or(AWS_IMDS_ENDPOINT)
        .trim_end_matches('/');

    let client = reqwest::Client::builder()
        .build()
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "Failed to create AWS IMDS HTTP client".to_string(),
            errors: None,
        })?;

    let token_url = format!("{endpoint}/latest/api/token");
    let token = client
        .put(&token_url)
        .timeout(AWS_IMDS_DISCOVERY_TIMEOUT)
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

    let region_url = format!("{endpoint}/latest/meta-data/placement/region");
    let region = client
        .get(&region_url)
        .timeout(AWS_IMDS_DISCOVERY_TIMEOUT)
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

fn parse_service_overrides(endpoints_json: Option<&String>) -> Result<Option<ServiceOverrides>> {
    if let Some(endpoints_json) = endpoints_json {
        let endpoints: HashMap<String, String> = serde_json::from_str(endpoints_json)
            .into_alien_error()
            .context(ErrorData::InvalidClientConfig {
                message: "Failed to parse AWS_SERVICE_OVERRIDES_ENDPOINTS".to_string(),
                errors: None,
            })?;
        Ok(Some(ServiceOverrides { endpoints }))
    } else {
        Ok(None)
    }
}

async fn infer_account_id(
    environment_variables: &HashMap<String, String>,
    region: &str,
    credentials: &AwsCredentials,
    service_overrides: Option<&ServiceOverrides>,
) -> Result<String> {
    if let Some(account_id) = environment_variables.get("AWS_ACCOUNT_ID") {
        return Ok(account_id.clone());
    }

    if let Some(role_arn) = environment_variables.get("AWS_ROLE_ARN") {
        if let Some(account_id) = extract_account_id_from_role_arn(role_arn) {
            return Ok(account_id);
        }
    }

    if let AwsCredentials::WebIdentity { config } = credentials {
        if let Some(account_id) = extract_account_id_from_role_arn(&config.role_arn) {
            return Ok(account_id);
        }
    }

    use crate::aws::sts::{StsApi, StsClient};
    let mut probe_config = AwsClientConfig {
        account_id: String::new(),
        region: region.to_string(),
        credentials: credentials.clone(),
        service_overrides: service_overrides.cloned(),
    };

    if matches!(
        probe_config.credentials,
        AwsCredentials::WebIdentity { .. }
            | AwsCredentials::Imds { .. }
            | AwsCredentials::Profile { .. }
    ) {
        probe_config = probe_config.get_web_identity_credentials().await?;
    }

    let caller_identity = StsClient::new(reqwest::Client::new(), probe_config)
        .get_caller_identity()
        .await
        .context(ErrorData::InvalidClientConfig {
            message: "Failed to infer AWS account ID from credentials".to_string(),
            errors: None,
        })?;

    caller_identity
        .get_caller_identity_result
        .account
        .ok_or_else(|| {
            AlienError::new(ErrorData::InvalidClientConfig {
                message: "Failed to infer AWS account ID from STS caller identity".to_string(),
                errors: None,
            })
        })
}

fn profile_name(environment_variables: &HashMap<String, String>) -> String {
    environment_variables
        .get("AWS_PROFILE")
        .or_else(|| environment_variables.get("AWS_DEFAULT_PROFILE"))
        .cloned()
        .unwrap_or_else(|| "default".to_string())
}

#[cfg(not(target_arch = "wasm32"))]
fn load_profile_session_credentials(profile: &str) -> Result<AwsCredentials> {
    let output = std::process::Command::new("aws")
        .args([
            "configure",
            "export-credentials",
            "--profile",
            profile,
            "--format",
            "process",
        ])
        .output()
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: format!("Failed to invoke AWS CLI for profile '{}'", profile),
            errors: None,
        })?;

    if !output.status.success() {
        return Err(AlienError::new(ErrorData::InvalidClientConfig {
            message: format!(
                "Failed to export AWS credentials for profile '{}': {}",
                profile,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            errors: None,
        }));
    }

    let exported: AwsCliExportCredentials = serde_json::from_slice(&output.stdout)
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: format!(
                "Failed to parse exported AWS credentials for profile '{}'",
                profile
            ),
            errors: None,
        })?;

    if let (Some(session_token), Some(expires_at)) = (exported.session_token, exported.expiration) {
        Ok(AwsCredentials::SessionCredentials {
            access_key_id: exported.access_key_id,
            secret_access_key: exported.secret_access_key,
            session_token,
            expires_at,
        })
    } else {
        Ok(AwsCredentials::AccessKeys {
            access_key_id: exported.access_key_id,
            secret_access_key: exported.secret_access_key,
            session_token: None,
        })
    }
}

#[cfg(target_arch = "wasm32")]
fn load_profile_session_credentials(profile: &str) -> Result<AwsCredentials> {
    Err(AlienError::new(ErrorData::InvalidClientConfig {
        message: format!(
            "AWS_PROFILE ('{}') is not supported in wasm builds; provide explicit credentials",
            profile
        ),
        errors: None,
    }))
}

#[cfg(not(target_arch = "wasm32"))]
fn load_profile_region(profile: &str) -> Result<Option<String>> {
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

#[cfg(target_arch = "wasm32")]
fn load_profile_region(_profile: &str) -> Result<Option<String>> {
    Ok(None)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AwsCliExportCredentials {
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
    expiration: Option<String>,
}

/// Extract the AWS account ID from a role ARN.
///
/// Role ARNs follow the format `arn:aws:iam::{account_id}:role/{role_name}`.
/// Returns `None` if the ARN doesn't match the expected format.
fn extract_account_id_from_role_arn(role_arn: &str) -> Option<String> {
    let parts: Vec<&str> = role_arn.split(':').collect();
    if parts.len() >= 5 && !parts[4].is_empty() {
        Some(parts[4].to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[test]
    fn test_extract_account_id_from_role_arn() {
        assert_eq!(
            extract_account_id_from_role_arn("arn:aws:iam::123456789012:role/MyRole"),
            Some("123456789012".to_string())
        );
        assert_eq!(
            extract_account_id_from_role_arn("arn:aws:iam::987654321098:role/cross-account-role"),
            Some("987654321098".to_string())
        );
        assert_eq!(extract_account_id_from_role_arn("invalid-arn"), None);
        assert_eq!(
            extract_account_id_from_role_arn("arn:aws:iam:::role/NoAccount"),
            None
        );
    }

    #[test]
    fn test_profile_name_prefers_aws_profile() {
        let mut env = HashMap::new();
        env.insert("AWS_PROFILE".to_string(), "primary".to_string());
        env.insert("AWS_DEFAULT_PROFILE".to_string(), "fallback".to_string());

        assert_eq!(profile_name(&env), "primary".to_string());
    }

    #[test]
    fn test_profile_name_falls_back_to_default() {
        let env = HashMap::new();
        assert_eq!(profile_name(&env), "default".to_string());
    }

    #[test]
    fn test_profile_name_uses_aws_default_profile() {
        let mut env = HashMap::new();
        env.insert("AWS_DEFAULT_PROFILE".to_string(), "fallback".to_string());

        assert_eq!(profile_name(&env), "fallback".to_string());
    }

    #[tokio::test]
    async fn test_resolve_region_uses_default_region_fallback() {
        let mut env = HashMap::new();
        env.insert("AWS_DEFAULT_REGION".to_string(), "us-west-2".to_string());

        assert_eq!(resolve_region(&env).await.unwrap(), "us-west-2");
    }

    #[test]
    fn test_parse_service_overrides() {
        let parsed =
            parse_service_overrides(Some(&"{\"sts\":\"http://localhost:4566\"}".to_string()))
                .unwrap()
                .unwrap();

        assert_eq!(
            parsed.endpoints.get("sts"),
            Some(&"http://localhost:4566".to_string())
        );
    }

    #[tokio::test]
    async fn test_resolve_credentials_prefers_explicit_keys() {
        let mut env = HashMap::new();
        env.insert("AWS_ACCESS_KEY_ID".to_string(), "AKIA123".to_string());
        env.insert("AWS_SECRET_ACCESS_KEY".to_string(), "secret".to_string());
        env.insert("AWS_SESSION_TOKEN".to_string(), "token".to_string());
        env.insert("AWS_PROFILE".to_string(), "should-not-be-used".to_string());

        let credentials = resolve_credentials(&env).await.unwrap();
        assert_eq!(
            credentials,
            AwsCredentials::AccessKeys {
                access_key_id: "AKIA123".to_string(),
                secret_access_key: "secret".to_string(),
                session_token: Some("token".to_string()),
            }
        );
    }

    #[tokio::test]
    async fn test_resolve_credentials_ignores_empty_session_token() {
        let mut env = HashMap::new();
        env.insert("AWS_ACCESS_KEY_ID".to_string(), "AKIA123".to_string());
        env.insert("AWS_SECRET_ACCESS_KEY".to_string(), "secret".to_string());
        env.insert("AWS_SESSION_TOKEN".to_string(), "".to_string());

        let credentials = resolve_credentials(&env).await.unwrap();
        assert_eq!(
            credentials,
            AwsCredentials::AccessKeys {
                access_key_id: "AKIA123".to_string(),
                secret_access_key: "secret".to_string(),
                session_token: None,
            }
        );
    }

    #[tokio::test]
    async fn test_from_env_uses_imds_for_region_and_credentials() {
        let endpoint = start_mock_imds().await;
        let mut env = HashMap::new();
        env.insert("AWS_ACCOUNT_ID".to_string(), "123456789012".to_string());
        env.insert(
            "AWS_EC2_METADATA_SERVICE_ENDPOINT".to_string(),
            endpoint.clone(),
        );

        let config = AwsClientConfig::from_env(&env).await.unwrap();

        assert_eq!(config.region, "us-east-1");
        // Discovery validates the IMDS credential document (the mock would
        // reject a parse failure), but the stored credential stays deferred:
        // role credentials expire, so they are resolved at use time.
        assert_eq!(
            config.credentials,
            AwsCredentials::Imds {
                endpoint: Some(endpoint),
            }
        );
    }

    async fn start_mock_imds() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };

                tokio::spawn(async move {
                    let mut buffer = [0u8; 2048];
                    let Ok(n) = stream.read(&mut buffer).await else {
                        return;
                    };
                    let request = String::from_utf8_lossy(&buffer[..n]);
                    let body = if request.starts_with("PUT /latest/api/token ") {
                        "token".to_string()
                    } else if request.starts_with("GET /latest/meta-data/placement/region ") {
                        "us-east-1".to_string()
                    } else if request
                        .starts_with("GET /latest/meta-data/iam/security-credentials/ ")
                    {
                        "test-role".to_string()
                    } else if request
                        .starts_with("GET /latest/meta-data/iam/security-credentials/test-role ")
                    {
                        // Real IMDS role credentials always carry an Expiration.
                        r#"{"AccessKeyId":"AKIAIMDS","SecretAccessKey":"secret","Token":"session","Expiration":"2099-01-01T00:00:00Z"}"#
                            .to_string()
                    } else {
                        let response =
                            "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n".to_string();
                        let _ = stream.write_all(response.as_bytes()).await;
                        return;
                    };

                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                });
            }
        });

        format!("http://{}", addr)
    }
}
