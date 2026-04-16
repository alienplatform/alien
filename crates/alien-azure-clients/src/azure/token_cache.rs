use std::collections::HashMap;
use std::time::{Duration, Instant};

use alien_client_core::Result;
use alien_core::AzureClientConfig;

use crate::azure::AzureClientConfigExt;

const TOKEN_CACHE_TTL: Duration = Duration::from_secs(45 * 60); // 45 minutes

#[derive(Debug)]
struct CachedToken {
    token: String,
    expires_at: Instant,
}

#[derive(Debug)]
pub struct AzureTokenCache {
    config: AzureClientConfig,
    cache: tokio::sync::Mutex<HashMap<String, CachedToken>>,
}

impl AzureTokenCache {
    pub fn new(config: AzureClientConfig) -> Self {
        Self {
            config,
            cache: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    pub async fn get_bearer_token_with_scope(&self, scope: &str) -> Result<String> {
        // Skip caching for AccessToken credential type (static/opaque)
        if matches!(
            &self.config.credentials,
            alien_core::AzureCredentials::AccessToken { .. }
        ) {
            return self.config.get_bearer_token_with_scope(scope).await;
        }

        // Check cache first
        {
            let cache = self.cache.lock().await;
            if let Some(cached) = cache.get(scope) {
                if Instant::now() < cached.expires_at {
                    return Ok(cached.token.clone());
                }
            }
        }

        // Cache miss or expired - fetch a new token
        let token = self.config.get_bearer_token_with_scope(scope).await?;

        // Store in cache
        {
            let mut cache = self.cache.lock().await;
            cache.insert(
                scope.to_string(),
                CachedToken {
                    token: token.clone(),
                    expires_at: Instant::now() + TOKEN_CACHE_TTL,
                },
            );
        }

        Ok(token)
    }

    pub fn config(&self) -> &AzureClientConfig {
        &self.config
    }

    pub fn management_endpoint(&self) -> &str {
        self.config.management_endpoint()
    }

    pub fn get_service_endpoint(&self, service_name: &str) -> Option<&str> {
        self.config.get_service_endpoint(service_name)
    }
}
