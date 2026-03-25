use std::collections::HashMap;
use std::sync::Arc;

use alien_bindings::BindingsProviderApi;
use alien_core::Platform;
use alien_error::{Context, IntoAlienError};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use reqwest::Client as HttpClient;

use super::error::{ErrorData, Result};
use crate::config::{DeepStoreConfig, GcpOAuthConfig};

/// Shared state for platform-mode route handlers and the self-heartbeat loop.
///
/// Built once during server startup and injected into axum routes via `Extension<Arc<PlatformState>>`.
#[derive(Clone)]
pub struct PlatformState {
    /// Platform API base URL (for forwarding caller tokens via ad-hoc clients).
    pub api_url: String,
    /// This manager's identity (resolved at startup via whoami).
    pub manager_id: String,
    /// Public URL of this manager instance.
    pub base_url: String,
    /// Authenticated Platform API client (manager's credentials in default headers).
    pub client: alien_platform_api::Client,
    /// Primary bindings provider (KV, Storage, ServiceAccounts).
    pub bindings: Arc<dyn BindingsProviderApi>,
    /// Per-platform providers for cross-account operations (SA impersonation, artifact registry).
    pub target_bindings: HashMap<Platform, Arc<dyn BindingsProviderApi>>,
    /// Self-heartbeat interval in seconds.
    pub heartbeat_interval_secs: u64,
    /// DeepStore config for telemetry query proxy.
    pub deepstore: DeepStoreConfig,
    /// GCP OAuth config for onboarding flow.
    pub gcp_oauth: GcpOAuthConfig,
}

impl PlatformState {
    /// Returns the target provider for the given platform, falling back to the primary provider.
    pub fn provider_for_target(&self, platform: Platform) -> &Arc<dyn BindingsProviderApi> {
        self.target_bindings
            .get(&platform)
            .unwrap_or(&self.bindings)
    }
}

pub fn build_platform_client(
    api_url: &str,
    api_key: &str,
) -> Result<alien_platform_api::Client> {
    let auth_value = format!("Bearer {}", api_key);
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&auth_value)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Invalid API key format".to_string(),
            })?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("alien-manager"));

    let reqwest_client = HttpClient::builder()
        .default_headers(headers)
        .build()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to build HTTP client".to_string(),
        })?;

    Ok(alien_platform_api::Client::new_with_client(
        api_url,
        reqwest_client,
    ))
}

pub async fn resolve_base_url(
    base_url: &Option<String>,
    port: u16,
    bindings_provider: &Arc<dyn BindingsProviderApi>,
) -> Result<String> {
    if let Some(ref url) = base_url {
        return Ok(url.clone());
    }

    let container_binding_name =
        std::env::var("ALIEN_CURRENT_CONTAINER_BINDING_NAME")
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "BASE_URL not set and ALIEN_CURRENT_CONTAINER_BINDING_NAME not found."
                    .to_string(),
            });

    match container_binding_name {
        Ok(name) => {
            let container = bindings_provider
                .load_container(&name)
                .await
                .context(ErrorData::ConfigurationError {
                    message: format!(
                        "Failed to load container binding '{}'",
                        name
                    ),
                })?;

            container
                .get_public_url()
                .ok_or_else(|| {
                    alien_error::AlienError::new(ErrorData::ConfigurationError {
                        message: "Container has no public URL.".to_string(),
                    })
                })
                .map(|u| u.to_string())
        }
        Err(_) => {
            // Fall back to localhost
            Ok(format!("http://localhost:{}", port))
        }
    }
}
