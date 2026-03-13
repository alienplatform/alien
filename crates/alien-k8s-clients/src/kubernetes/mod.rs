use alien_client_core::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use base64::{engine::general_purpose, Engine as _};
use std::collections::HashMap;

// Re-export types from alien-core
pub use alien_core::KubernetesClientConfig;

pub mod deployments;
pub mod jobs;
pub mod kubernetes_client;
pub mod kubernetes_request_utils;
pub mod pods;
pub mod secrets;
pub mod services;

/// Resolved Kubernetes configuration for making API calls
#[derive(Debug, Clone)]
pub struct ResolvedKubernetesConfig {
    /// The Kubernetes cluster server URL
    pub server_url: String,
    /// Certificate authority data (base64 encoded)
    pub certificate_authority_data: Option<String>,
    /// Client certificate data (base64 encoded)
    pub client_certificate_data: Option<String>,
    /// Client key data (base64 encoded)
    pub client_key_data: Option<String>,
    /// Bearer token for authentication
    pub bearer_token: Option<String>,
    /// Whether to skip TLS verification
    pub insecure_skip_tls_verify: bool,
    /// Additional headers to include in requests
    pub additional_headers: HashMap<String, String>,
}

/// Trait for Kubernetes client configuration operations
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait KubernetesClientConfigExt {
    /// Create a new `KubernetesClientConfig` from environment variables.
    async fn from_env(
        environment_variables: &HashMap<String, String>,
    ) -> Result<KubernetesClientConfig>;

    /// Create a new `KubernetesClientConfig` from standard environment variables.
    async fn from_std_env() -> Result<KubernetesClientConfig>;

    /// Infer configuration automatically by detecting available sources
    async fn infer() -> Result<KubernetesClientConfig>;

    /// Try to create in-cluster configuration
    async fn try_incluster() -> Result<KubernetesClientConfig>;

    /// Try to create kubeconfig configuration
    async fn try_kubeconfig() -> Result<KubernetesClientConfig>;

    /// Resolve configuration to a concrete format for API calls
    async fn resolve(&self) -> Result<ResolvedKubernetesConfig>;

    /// Create a mock KubernetesClientConfig with dummy values for testing
    #[cfg(any(test, feature = "test-utils"))]
    fn mock() -> Self;
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl KubernetesClientConfigExt for KubernetesClientConfig {
    /// Create a new `KubernetesClientConfig` from environment variables.
    /// This function automatically infers the configuration mode based on available indicators.
    async fn from_env(environment_variables: &HashMap<String, String>) -> Result<Self> {
        // Auto-detect the best configuration mode based on available indicators

        // 1. Check for manual configuration (explicit server URL)
        if environment_variables.contains_key("KUBERNETES_SERVER_URL") {
            tracing::debug!("Detected manual Kubernetes configuration via KUBERNETES_SERVER_URL");
            return from_env_manual(environment_variables).await;
        }

        // 2. Check for in-cluster configuration (standard Kubernetes env vars)
        if is_in_cluster_env(environment_variables) {
            tracing::debug!("Detected in-cluster Kubernetes configuration");
            return from_env_incluster(environment_variables).await;
        }

        // 3. Check for kubeconfig environment variable
        if environment_variables.contains_key("KUBECONFIG") {
            tracing::debug!("Detected kubeconfig via KUBECONFIG environment variable");
            return from_env_kubeconfig(environment_variables).await;
        }

        // 4. Fall back to kubeconfig mode (might work with default kubeconfig)
        tracing::debug!(
            "No specific Kubernetes configuration detected, defaulting to kubeconfig mode"
        );
        from_env_kubeconfig(environment_variables).await
    }

    /// Create a new `KubernetesClientConfig` from standard environment variables.
    async fn from_std_env() -> Result<Self> {
        let env_vars: HashMap<String, String> = std::env::vars().collect();
        Self::from_env(&env_vars).await
    }

    /// Infer configuration automatically by detecting available sources
    async fn infer() -> Result<Self> {
        // Check for in-cluster configuration first (most specific)
        if let Ok(config) = Self::try_incluster().await {
            tracing::debug!("Successfully detected in-cluster Kubernetes configuration");
            return Ok(config);
        }

        // Try kubeconfig with default path
        if let Ok(config) = Self::try_kubeconfig().await {
            tracing::debug!("Successfully detected kubeconfig-based Kubernetes configuration");
            return Ok(config);
        }

        // If all else fails, return an error with helpful guidance
        Err(AlienError::new(ErrorData::InvalidClientConfig {
            message: "No Kubernetes configuration found. Please ensure you are either running inside a Kubernetes cluster, have a valid kubeconfig file, or set KUBERNETES_SERVER_URL for manual configuration.".to_string(),
            errors: None,
        }))
    }

    /// Try to create in-cluster configuration
    async fn try_incluster() -> Result<Self> {
        // Check if in-cluster files exist
        if tokio::fs::metadata("/var/run/secrets/kubernetes.io/serviceaccount/token")
            .await
            .is_ok()
        {
            Ok(Self::InCluster {
                namespace: None,
                additional_headers: None,
            })
        } else {
            Err(AlienError::new(ErrorData::InvalidClientConfig {
                message: "Not running in Kubernetes cluster".to_string(),
                errors: None,
            }))
        }
    }

    /// Try to create kubeconfig configuration
    async fn try_kubeconfig() -> Result<Self> {
        Ok(Self::Kubeconfig {
            kubeconfig_path: None, // Will use default path
            context: None,
            cluster: None,
            user: None,
            namespace: None,
            additional_headers: None,
        })
    }

    /// Resolve configuration to a concrete format for API calls
    /// This method now expects kubeconfig resolution to be handled by alien-infra
    async fn resolve(&self) -> Result<ResolvedKubernetesConfig> {
        match self {
            Self::InCluster {
                namespace: _,
                additional_headers,
            } => resolve_incluster(additional_headers.clone()).await,
            Self::Kubeconfig { .. } => Err(AlienError::new(ErrorData::InvalidClientConfig {
                message: "Kubeconfig resolution must be handled by alien-infra".to_string(),
                errors: None,
            })),
            Self::Manual {
                server_url,
                certificate_authority_data,
                client_certificate_data,
                client_key_data,
                token,
                additional_headers,
                ..
            } => {
                Ok(ResolvedKubernetesConfig {
                    server_url: server_url.clone(),
                    certificate_authority_data: certificate_authority_data.clone(),
                    client_certificate_data: client_certificate_data.clone(),
                    client_key_data: client_key_data.clone(),
                    bearer_token: token.clone(),
                    insecure_skip_tls_verify: false, // Manual config defaults to secure
                    additional_headers: additional_headers.clone(),
                })
            }
        }
    }

    /// Create a mock KubernetesClientConfig with dummy values for testing
    #[cfg(any(test, feature = "test-utils"))]
    fn mock() -> Self {
        Self::InCluster {
            namespace: None,
            additional_headers: None,
        }
    }
}

/// Create from environment variables using in-cluster mode
async fn from_env_incluster(
    environment_variables: &HashMap<String, String>,
) -> Result<KubernetesClientConfig> {
    let namespace = environment_variables.get("KUBERNETES_NAMESPACE").cloned();
    let additional_headers = parse_additional_headers(environment_variables)?;

    Ok(KubernetesClientConfig::InCluster {
        namespace,
        additional_headers,
    })
}

/// Create from environment variables using kubeconfig mode
async fn from_env_kubeconfig(
    environment_variables: &HashMap<String, String>,
) -> Result<KubernetesClientConfig> {
    let kubeconfig_path = environment_variables.get("KUBECONFIG").cloned();
    let context = environment_variables.get("KUBERNETES_CONTEXT").cloned();
    let cluster = environment_variables.get("KUBERNETES_CLUSTER").cloned();
    let user = environment_variables.get("KUBERNETES_USER").cloned();
    let namespace = environment_variables.get("KUBERNETES_NAMESPACE").cloned();
    let additional_headers = parse_additional_headers(environment_variables)?;

    Ok(KubernetesClientConfig::Kubeconfig {
        kubeconfig_path,
        context,
        cluster,
        user,
        namespace,
        additional_headers,
    })
}

/// Create from environment variables using manual mode
async fn from_env_manual(
    environment_variables: &HashMap<String, String>,
) -> Result<KubernetesClientConfig> {
    let server_url = environment_variables
        .get("KUBERNETES_SERVER_URL")
        .ok_or_else(|| {
            AlienError::new(ErrorData::InvalidClientConfig {
                message: "KUBERNETES_SERVER_URL is required for manual mode".to_string(),
                errors: None,
            })
        })?
        .clone();

    let certificate_authority_data = environment_variables.get("KUBERNETES_CA_DATA").cloned();
    let client_certificate_data = environment_variables
        .get("KUBERNETES_CLIENT_CERT_DATA")
        .cloned();
    let client_key_data = environment_variables
        .get("KUBERNETES_CLIENT_KEY_DATA")
        .cloned();
    let token = environment_variables
        .get("KUBERNETES_BEARER_TOKEN")
        .cloned();
    let namespace = environment_variables.get("KUBERNETES_NAMESPACE").cloned();
    let additional_headers = parse_additional_headers(environment_variables)?;

    Ok(KubernetesClientConfig::Manual {
        server_url,
        certificate_authority_data,
        insecure_skip_tls_verify: None,
        client_certificate_data,
        client_key_data,
        token,
        username: None,
        password: None,
        namespace,
        additional_headers: additional_headers.unwrap_or_default(),
    })
}

/// Check if environment indicates in-cluster configuration
fn is_in_cluster_env(environment_variables: &HashMap<String, String>) -> bool {
    // Standard Kubernetes environment variables that are present in pods
    let has_k8s_service = environment_variables.contains_key("KUBERNETES_SERVICE_HOST")
        && environment_variables.contains_key("KUBERNETES_SERVICE_PORT");

    // Additional check: service account token file should exist
    let has_service_account =
        std::path::Path::new("/var/run/secrets/kubernetes.io/serviceaccount/token").exists();

    has_k8s_service || has_service_account
}

/// Parse additional headers from environment
fn parse_additional_headers(
    environment_variables: &HashMap<String, String>,
) -> Result<Option<HashMap<String, String>>> {
    if let Some(headers_json) = environment_variables.get("KUBERNETES_ADDITIONAL_HEADERS") {
        let headers: HashMap<String, String> = serde_json::from_str(headers_json)
            .into_alien_error()
            .context(ErrorData::InvalidClientConfig {
                message: "Failed to parse KUBERNETES_ADDITIONAL_HEADERS".to_string(),
                errors: None,
            })?;
        Ok(Some(headers))
    } else {
        Ok(None)
    }
}

/// Resolve in-cluster configuration
async fn resolve_incluster(
    additional_headers: Option<HashMap<String, String>>,
) -> Result<ResolvedKubernetesConfig> {
    // Build server URL from environment variables
    let server_url = if let Ok(host) = std::env::var("KUBERNETES_SERVICE_HOST") {
        if let Ok(port) = std::env::var("KUBERNETES_SERVICE_PORT") {
            format!("https://{}:{}", host, port)
        } else {
            "https://kubernetes.default.svc".to_string()
        }
    } else {
        "https://kubernetes.default.svc".to_string()
    };

    // Read service account token
    let bearer_token =
        tokio::fs::read_to_string("/var/run/secrets/kubernetes.io/serviceaccount/token")
            .await
            .ok()
            .map(|token| token.trim().to_string());

    // Read CA certificate
    let certificate_authority_data =
        tokio::fs::read("/var/run/secrets/kubernetes.io/serviceaccount/ca.crt")
            .await
            .ok()
            .map(|ca_cert| general_purpose::STANDARD.encode(ca_cert));

    Ok(ResolvedKubernetesConfig {
        server_url,
        certificate_authority_data,
        client_certificate_data: None,
        client_key_data: None,
        bearer_token,
        insecure_skip_tls_verify: false, // In-cluster should always verify TLS
        additional_headers: additional_headers.unwrap_or_default(),
    })
}
