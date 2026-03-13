//! Extension trait for ClientConfig to provide environment-based configuration and impersonation

use alien_client_core::{ErrorData, Result};
use alien_core::{ClientConfig, ImpersonationConfig, Platform};
use alien_error::{AlienError, Context};
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
                use alien_aws_clients::AwsClientConfigExt;
                let config = alien_aws_clients::AwsClientConfig::from_env(environment_variables).await
                    .map_err(|e| AlienError::new(ErrorData::InvalidClientConfig {
                        message: format!("Failed to create AWS client configuration from environment variables: {}", e),
                        errors: None,
                    }))?;
                Ok(ClientConfig::Aws(Box::new(config)))
            }
            #[cfg(not(feature = "aws"))]
            Platform::Aws => Err(AlienError::new(ErrorData::InvalidClientConfig {
                message: "AWS support is not enabled in this build".to_string(),
                errors: None,
            })),
            #[cfg(feature = "gcp")]
            Platform::Gcp => {
                use alien_gcp_clients::GcpClientConfigExt;
                let config = alien_gcp_clients::GcpClientConfig::from_env(environment_variables).await
                    .map_err(|e| AlienError::new(ErrorData::InvalidClientConfig {
                        message: format!("Failed to create GCP client configuration from environment variables: {}", e),
                        errors: None,
                    }))?;
                Ok(ClientConfig::Gcp(Box::new(config)))
            }
            #[cfg(not(feature = "gcp"))]
            Platform::Gcp => Err(AlienError::new(ErrorData::InvalidClientConfig {
                message: "GCP support is not enabled in this build".to_string(),
                errors: None,
            })),
            #[cfg(feature = "azure")]
            Platform::Azure => {
                use alien_azure_clients::AzureClientConfigExt;
                let config = alien_azure_clients::AzureClientConfig::from_env(environment_variables).await
                    .map_err(|e| AlienError::new(ErrorData::InvalidClientConfig {
                        message: format!("Failed to create Azure client configuration from environment variables: {}", e),
                        errors: None,
                    }))?;
                Ok(ClientConfig::Azure(Box::new(config)))
            }
            #[cfg(not(feature = "azure"))]
            Platform::Azure => Err(AlienError::new(ErrorData::InvalidClientConfig {
                message: "Azure support is not enabled in this build".to_string(),
                errors: None,
            })),
            #[cfg(feature = "kubernetes")]
            Platform::Kubernetes => {
                use alien_k8s_clients::KubernetesClientConfigExt;
                let config = alien_k8s_clients::KubernetesClientConfig::from_env(environment_variables).await
                    .map_err(|e| AlienError::new(ErrorData::InvalidClientConfig {
                        message: format!("Failed to create Kubernetes client configuration from environment variables: {}", e),
                        errors: None,
                    }))?;
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

                Ok(ClientConfig::Local {
                    state_directory,
                    artifact_registry_config: None,
                })
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
                use alien_aws_clients::AwsClientConfigExt;
                let new_config = aws_config.impersonate(imp_config).await.map_err(|e| {
                    AlienError::new(ErrorData::AuthenticationError {
                        message: format!("AWS role impersonation failed: {}", e),
                    })
                })?;
                Ok(ClientConfig::Aws(Box::new(new_config)))
            }
            #[cfg(feature = "gcp")]
            (ClientConfig::Gcp(gcp_config), ImpersonationConfig::Gcp(imp_config)) => {
                use alien_gcp_clients::GcpClientConfigExt;
                let new_config = gcp_config.impersonate(imp_config).await.map_err(|e| {
                    AlienError::new(ErrorData::AuthenticationError {
                        message: format!("GCP service account impersonation failed: {}", e),
                    })
                })?;
                Ok(ClientConfig::Gcp(Box::new(new_config)))
            }
            #[cfg(feature = "azure")]
            (ClientConfig::Azure(azure_config), ImpersonationConfig::Azure(imp_config)) => {
                use alien_azure_clients::AzureClientConfigExt;
                let new_config = azure_config.impersonate(imp_config).await.map_err(|e| {
                    AlienError::new(ErrorData::AuthenticationError {
                        message: format!("Azure managed identity impersonation failed: {}", e),
                    })
                })?;
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
