use crate::kubernetes::{
    kubernetes_request_utils::KubernetesAuthConfig, KubernetesClientConfig,
    KubernetesClientConfigExt, ResolvedKubernetesConfig,
};
use alien_client_core::{ErrorData, Result};
use alien_error::Context;
use alien_error::IntoAlienError;
use base64::{engine::general_purpose, Engine as _};
use reqwest::Client;

/// Main Kubernetes client that provides access to all Kubernetes resources.
///
/// The client resolves configuration (TLS, auth) at creation time and either succeeds or fails.
/// No lazy initialization, no fallbacks - if it's created, it's ready to use.
#[derive(Debug, Clone)]
pub struct KubernetesClient {
    client: Client,
    config: KubernetesClientConfig,
    resolved_config: ResolvedKubernetesConfig,
}

impl KubernetesClient {
    /// Create a new Kubernetes client with proper TLS and authentication configuration.
    ///
    /// This resolves the config (kubeconfig, in-cluster, or manual) and builds a properly
    /// configured HTTP client. If configuration fails, returns an error immediately.
    pub async fn new(config: KubernetesClientConfig) -> Result<Self> {
        // Resolve the config to get TLS and auth details
        let resolved_config = config.resolve().await?;

        // Build a reqwest client with proper TLS configuration
        let mut client_builder = reqwest::Client::builder();

        // Configure TLS verification
        if resolved_config.insecure_skip_tls_verify {
            client_builder = client_builder.danger_accept_invalid_certs(true);
        } else if let Some(ref ca_data) = resolved_config.certificate_authority_data {
            // Configure CA certificate
            let ca_cert_bytes = general_purpose::STANDARD
                .decode(ca_data)
                .into_alien_error()
                .context(ErrorData::KubeconfigError {
                    message: "Failed to decode CA certificate data".to_string(),
                })?;

            let ca_cert = reqwest::Certificate::from_pem(&ca_cert_bytes)
                .into_alien_error()
                .context(ErrorData::KubeconfigError {
                    message: "Failed to parse CA certificate".to_string(),
                })?;

            client_builder = client_builder.add_root_certificate(ca_cert);
        }

        // Configure client certificate and key if provided (mutual TLS)
        if let (Some(ref cert_data), Some(ref key_data)) = (
            &resolved_config.client_certificate_data,
            &resolved_config.client_key_data,
        ) {
            let cert_bytes = general_purpose::STANDARD
                .decode(cert_data)
                .into_alien_error()
                .context(ErrorData::KubeconfigError {
                    message: "Failed to decode client certificate data".to_string(),
                })?;

            let key_bytes = general_purpose::STANDARD
                .decode(key_data)
                .into_alien_error()
                .context(ErrorData::KubeconfigError {
                    message: "Failed to decode client key data".to_string(),
                })?;

            // Combine certificate and key for reqwest
            let mut identity_pem = cert_bytes;
            identity_pem.extend_from_slice(&key_bytes);

            let identity = reqwest::Identity::from_pem(&identity_pem)
                .into_alien_error()
                .context(ErrorData::KubeconfigError {
                    message: "Failed to create client identity".to_string(),
                })?;

            client_builder = client_builder.identity(identity);
        }

        let client =
            client_builder
                .build()
                .into_alien_error()
                .context(ErrorData::KubeconfigError {
                    message: "Failed to build HTTP client".to_string(),
                })?;

        Ok(Self {
            client,
            config,
            resolved_config,
        })
    }

    /// Get the authentication configuration
    pub fn auth_config(&self) -> KubernetesAuthConfig {
        KubernetesAuthConfig::from(&self.resolved_config)
    }

    /// Get the base URL for the Kubernetes API
    pub fn get_base_url(&self) -> &str {
        &self.resolved_config.server_url
    }

    /// Get the HTTP client
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Get the platform configuration
    pub fn config(&self) -> &KubernetesClientConfig {
        &self.config
    }

    /// Get the resolved configuration
    pub fn resolved_config(&self) -> &ResolvedKubernetesConfig {
        &self.resolved_config
    }
}
