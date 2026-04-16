use alien_client_core::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use aws_credential_types::Credentials;
use serde::Deserialize;
use std::collections::HashMap;

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
pub mod codebuild;
pub mod credential_provider;
pub mod dynamodb;
pub mod ec2;
pub mod ecr;
pub mod eventbridge;
pub mod elbv2;
pub mod iam;
pub mod lambda;
pub mod s3;
pub mod secrets_manager;
pub mod sqs;
pub mod ssm;
pub mod sts;

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl AwsClientConfigExt for AwsClientConfig {
    /// Create a new `AwsClientConfig` from environment variables.
    async fn from_env(environment_variables: &HashMap<String, String>) -> Result<Self> {
        let region = resolve_region(environment_variables)?;
        let credentials = resolve_credentials(environment_variables)?;
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

        // If using WebIdentity (IRSA), first exchange the token for real temporary credentials
        // before calling AssumeRole, which requires valid signed credentials.
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
            credentials: AwsCredentials::AccessKeys {
                access_key_id: credentials.access_key_id,
                secret_access_key: credentials.secret_access_key,
                session_token: Some(credentials.session_token),
            },
            service_overrides: self.service_overrides.clone(),
        })
    }

    /// Get AWS credentials from this config
    /// For web identity tokens, this will return placeholder credentials
    /// Call get_web_identity_credentials() to get actual credentials
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
            AwsCredentials::WebIdentity { .. } => {
                // For web identity, we need to assume the role first
                // This method returns placeholder credentials
                Credentials::new(
                    "PLACEHOLDER_ACCESS_KEY".to_string(),
                    "PLACEHOLDER_SECRET_KEY".to_string(),
                    None,
                    None,
                    "WebIdentityPlaceholder",
                )
            }
        }
    }

    /// Get credentials for web identity token authentication
    /// This method reads the token file and assumes the role
    async fn get_web_identity_credentials(&self) -> Result<AwsClientConfig> {
        match &self.credentials {
            AwsCredentials::WebIdentity { config } => {
                use crate::aws::sts::{AssumeRoleWithWebIdentityRequest, StsApi, StsClient};
                use reqwest::Client;
                use uuid::Uuid;

                // Read the web identity token from file
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

                // Create a temporary config with placeholder credentials to call STS
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
                    credentials: AwsCredentials::AccessKeys {
                        access_key_id: credentials.access_key_id,
                        secret_access_key: credentials.secret_access_key,
                        session_token: Some(credentials.session_token),
                    },
                    service_overrides: self.service_overrides.clone(),
                })
            }
            AwsCredentials::AccessKeys { .. } => {
                // Already have access keys, return self
                Ok(self.clone())
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

fn resolve_region(environment_variables: &HashMap<String, String>) -> Result<String> {
    if let Some(region) = environment_variables.get("AWS_REGION") {
        return Ok(region.clone());
    }

    if let Some(region) = environment_variables.get("AWS_DEFAULT_REGION") {
        return Ok(region.clone());
    }

    let profile = profile_name(environment_variables);
    if let Some(region) = load_profile_region(&profile)? {
        return Ok(region);
    }

    Err(AlienError::new(ErrorData::InvalidClientConfig {
        message: "Missing AWS region. Set AWS_REGION, AWS_DEFAULT_REGION, or configure a default region in your AWS profile.".to_string(),
        errors: None,
    }))
}

fn resolve_credentials(environment_variables: &HashMap<String, String>) -> Result<AwsCredentials> {
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
            session_token: environment_variables.get("AWS_SESSION_TOKEN").cloned(),
        });
    }

    let profile = profile_name(environment_variables);
    load_profile_credentials(&profile)
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

    if matches!(probe_config.credentials, AwsCredentials::WebIdentity { .. }) {
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
fn load_profile_credentials(profile: &str) -> Result<AwsCredentials> {
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

    Ok(AwsCredentials::AccessKeys {
        access_key_id: exported.access_key_id,
        secret_access_key: exported.secret_access_key,
        session_token: exported.session_token,
    })
}

#[cfg(target_arch = "wasm32")]
fn load_profile_credentials(profile: &str) -> Result<AwsCredentials> {
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

    #[test]
    fn test_resolve_region_uses_default_region_fallback() {
        let mut env = HashMap::new();
        env.insert("AWS_DEFAULT_REGION".to_string(), "us-west-2".to_string());

        assert_eq!(resolve_region(&env).unwrap(), "us-west-2");
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

    #[test]
    fn test_resolve_credentials_prefers_explicit_keys() {
        let mut env = HashMap::new();
        env.insert("AWS_ACCESS_KEY_ID".to_string(), "AKIA123".to_string());
        env.insert("AWS_SECRET_ACCESS_KEY".to_string(), "secret".to_string());
        env.insert("AWS_SESSION_TOKEN".to_string(), "token".to_string());
        env.insert("AWS_PROFILE".to_string(), "should-not-be-used".to_string());

        let credentials = resolve_credentials(&env).unwrap();
        assert_eq!(
            credentials,
            AwsCredentials::AccessKeys {
                access_key_id: "AKIA123".to_string(),
                secret_access_key: "secret".to_string(),
                session_token: Some("token".to_string()),
            }
        );
    }
}
