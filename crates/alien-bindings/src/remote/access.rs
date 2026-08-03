use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use alien_error::{AlienError, Context, ContextError, GenericError, IntoAlienError};
use alien_manager_api::SdkResultExtReadingBody;
use alien_platform_api::SdkResultExt;
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use tokio::sync::{Mutex, RwLock};

use super::{Clock, ResolvedRemoteBinding};
use crate::error::{ErrorData, Result};

const DEFAULT_PLATFORM_API_URL: &str = "https://api.alien.dev";
const MANAGER_ACCESS_MAX_AGE_SECONDS: i64 = 300;
const MANAGER_TOKEN_REFRESH_SKEW_SECONDS: i64 = 30;
const REMOTE_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) struct RemoteBindingSource {
    pub(super) deployment_id: String,
    platform: alien_platform_api::Client,
    manager: RwLock<DiscoveredManager>,
    manager_refresh_lock: Mutex<()>,
    allow_insecure_manager_url: bool,
    manager_resolver: Arc<dyn ManagerBindingResolver>,
    clock: Arc<dyn Clock>,
}

pub(super) enum ManagerResolverKind {
    Generated,
    #[cfg(test)]
    LocalFixture,
}

#[async_trait]
pub(super) trait ManagerBindingResolver: Send + Sync + fmt::Debug {
    async fn resolve(
        &self,
        manager: &DiscoveredManager,
        deployment_id: &str,
        resource_id: &str,
    ) -> Result<ResolvedRemoteBinding>;
}

#[derive(Debug)]
pub(super) struct GeneratedManagerBindingResolver;

#[derive(Clone)]
pub(super) struct DiscoveredManager {
    pub(super) url: reqwest::Url,
    pub(super) http: reqwest::Client,
    pub(super) refresh_at: DateTime<Utc>,
    pub(super) generation: u64,
}

impl fmt::Debug for DiscoveredManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiscoveredManager")
            .field("url", &"<redacted>")
            .field("credentials", &"<redacted>")
            .field("refresh_at", &self.refresh_at)
            .field("generation", &self.generation)
            .finish()
    }
}

impl fmt::Debug for RemoteBindingSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteBindingSource")
            .field("deployment_id", &self.deployment_id)
            .field("manager", &"<redacted>")
            .finish()
    }
}

impl RemoteBindingSource {
    pub(super) async fn discover(
        deployment_id: &str,
        platform_token: &str,
        api_base_url: Option<&str>,
        resolver_kind: ManagerResolverKind,
        clock: Arc<dyn Clock>,
    ) -> Result<Self> {
        let base_url = api_base_url.unwrap_or(DEFAULT_PLATFORM_API_URL);
        let allow_insecure_manager_url = match api_base_url {
            Some(base_url) => validate_platform_base_url(base_url)?,
            None => false,
        };
        let platform_http = authenticated_http_client(platform_token, "Platform API")?;
        let platform = alien_platform_api::Client::new_with_client(base_url, platform_http);
        let manager_resolver: Arc<dyn ManagerBindingResolver> = match resolver_kind {
            ManagerResolverKind::Generated => Arc::new(GeneratedManagerBindingResolver),
            #[cfg(test)]
            ManagerResolverKind::LocalFixture => Arc::new(LocalFixtureManagerBindingResolver),
        };
        let manager = discover_manager_access(
            &platform,
            deployment_id,
            allow_insecure_manager_url,
            clock.as_ref(),
            0,
        )
        .await?;

        Ok(Self {
            deployment_id: deployment_id.to_string(),
            platform,
            manager: RwLock::new(manager),
            manager_refresh_lock: Mutex::new(()),
            allow_insecure_manager_url,
            manager_resolver,
            clock,
        })
    }

    async fn manager_access(&self) -> Result<DiscoveredManager> {
        let now = self.clock.now();
        {
            let manager = self.manager.read().await;
            if now < manager.refresh_at {
                return Ok(manager.clone());
            }
        }

        let _refresh = self.manager_refresh_lock.lock().await;
        let now = self.clock.now();
        {
            let manager = self.manager.read().await;
            if now < manager.refresh_at {
                return Ok(manager.clone());
            }
        }

        self.refresh_manager_access_locked().await
    }

    async fn refresh_manager_access_locked(&self) -> Result<DiscoveredManager> {
        let next_generation = self.manager.read().await.generation.wrapping_add(1);
        let manager = discover_manager_access(
            &self.platform,
            &self.deployment_id,
            self.allow_insecure_manager_url,
            self.clock.as_ref(),
            next_generation,
        )
        .await?;
        *self.manager.write().await = manager.clone();
        Ok(manager)
    }

    async fn refresh_after_rejection(&self, observed_generation: u64) -> Result<DiscoveredManager> {
        let _refresh = self.manager_refresh_lock.lock().await;
        {
            let manager = self.manager.read().await;
            if manager.generation != observed_generation {
                return Ok(manager.clone());
            }
        }
        self.refresh_manager_access_locked().await
    }

    pub(super) async fn resolve(&self, resource_id: &str) -> Result<ResolvedRemoteBinding> {
        let manager = self.manager_access().await?;
        match self
            .manager_resolver
            .resolve(&manager, &self.deployment_id, resource_id)
            .await
        {
            Ok(binding) => Ok(binding),
            Err(error) if is_auth_or_assignment_rejection(&error) => {
                let manager = self.refresh_after_rejection(manager.generation).await?;
                self.manager_resolver
                    .resolve(&manager, &self.deployment_id, resource_id)
                    .await
            }
            Err(error) => Err(error),
        }
    }
}

#[async_trait]
impl ManagerBindingResolver for GeneratedManagerBindingResolver {
    async fn resolve(
        &self,
        manager: &DiscoveredManager,
        deployment_id: &str,
        resource_id: &str,
    ) -> Result<ResolvedRemoteBinding> {
        let manager_client = alien_manager_api::Client::new_with_client(
            manager.url.as_str().trim_end_matches('/'),
            manager.http.clone(),
        );
        let response = manager_client
            .resolve_binding()
            .body(alien_manager_api::types::ResolveBindingRequest {
                deployment_id: deployment_id.to_string(),
                resource_id: resource_id.to_string(),
            })
            .send()
            .await
            .into_sdk_error_reading_body()
            .await
            .map_err(into_remote_error)?
            .into_inner();

        ResolvedRemoteBinding::from_manager_response(response, resource_id)
    }
}

/// Test-only adapter for typed local leases. Local is deliberately absent from
/// the hosted API contract; cache tests inject this adapter explicitly instead
/// of changing the production generated-client path.
#[cfg(test)]
#[derive(Debug)]
struct LocalFixtureManagerBindingResolver;

#[cfg(test)]
#[async_trait]
impl ManagerBindingResolver for LocalFixtureManagerBindingResolver {
    async fn resolve(
        &self,
        manager: &DiscoveredManager,
        deployment_id: &str,
        resource_id: &str,
    ) -> Result<ResolvedRemoteBinding> {
        let url = manager
            .url
            .join("v1/bindings/resolve")
            .into_alien_error()
            .context(ErrorData::RemoteAccessFailed {
                operation: "build remote binding fixture URL".to_string(),
            })?;
        let response = manager
            .http
            .post(url)
            .json(&serde_json::json!({
                "deploymentId": deployment_id,
                "resourceId": resource_id,
            }))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::RemoteAccessFailed {
                operation: format!("resolve remote Storage binding '{resource_id}'"),
            })?;
        if !response.status().is_success() {
            return Err(test_fixture_http_error(response, resource_id).await);
        }
        response
            .json()
            .await
            .into_alien_error()
            .context(ErrorData::RemoteAccessFailed {
                operation: format!("parse remote Storage binding '{resource_id}'"),
            })
    }
}

async fn discover_manager_access(
    platform: &alien_platform_api::Client,
    deployment_id: &str,
    allow_insecure: bool,
    clock: &dyn Clock,
    generation: u64,
) -> Result<DiscoveredManager> {
    let deployment = platform
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
        .map_err(into_remote_error)?
        .into_inner();
    let requested_at = clock.now();
    let response = platform
        .generate_manager_binding_token()
        .id(deployment.manager_id.to_string())
        .body(
            alien_platform_api::types::GenerateManagerBindingTokenRequest::builder()
                .deployment_id(deployment_id),
        )
        .send()
        .await
        .into_sdk_error()
        .map_err(into_remote_error)?
        .into_inner();

    let expires_in = response.expires_in.ok_or_else(|| {
        invalid_manager_access_response("binding token response omitted its expiry")
    })?;
    let lifetime_millis = positive_duration_millis(expires_in).ok_or_else(|| {
        invalid_manager_access_response("binding token response has an invalid expiry")
    })?;
    if response.access_token.trim().is_empty() {
        return Err(invalid_manager_access_response(
            "binding token response returned an empty token",
        ));
    }
    let manager_url = validate_manager_url(&response.manager_url, allow_insecure)?;
    let http = authenticated_http_client(&response.access_token, "manager binding API")?;
    let skew_millis = lifetime_millis
        .saturating_div(5)
        .min(MANAGER_TOKEN_REFRESH_SKEW_SECONDS * 1_000)
        .max(1);
    let refresh_delay_millis = lifetime_millis
        .saturating_sub(skew_millis)
        .min(MANAGER_ACCESS_MAX_AGE_SECONDS * 1_000);

    Ok(DiscoveredManager {
        url: manager_url,
        http,
        refresh_at: requested_at + ChronoDuration::milliseconds(refresh_delay_millis),
        generation,
    })
}

fn positive_duration_millis(seconds: f64) -> Option<i64> {
    let milliseconds = seconds * 1_000.0;
    (seconds.is_finite() && seconds > 0.0 && milliseconds >= 1.0 && milliseconds <= i64::MAX as f64)
        .then_some(milliseconds.floor() as i64)
}

fn is_auth_or_assignment_rejection(error: &AlienError<ErrorData>) -> bool {
    matches!(error.http_status_code, Some(401 | 403 | 404))
}

pub(super) fn authenticated_http_client(token: &str, destination: &str) -> Result<reqwest::Client> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
            .into_alien_error()
            .context(ErrorData::RemoteAccessFailed {
                operation: format!("build {destination} client with token"),
            })?,
    );
    reqwest::Client::builder()
        .default_headers(headers)
        .timeout(REMOTE_REQUEST_TIMEOUT)
        .build()
        .into_alien_error()
        .context(ErrorData::RemoteAccessFailed {
            operation: format!("build {destination} HTTP client"),
        })
}

#[cfg(test)]
async fn test_fixture_http_error(
    response: reqwest::Response,
    resource_id: &str,
) -> AlienError<ErrorData> {
    let status = response.status();
    match response.json::<AlienError<GenericError>>().await {
        Ok(error) => into_remote_error(error),
        Err(_) => {
            let mut error = AlienError::new(ErrorData::RemoteAccessFailed {
                operation: format!(
                    "resolve remote Storage binding '{resource_id}' (HTTP {status})"
                ),
            });
            error.retryable = alien_manager_api::is_retryable_http_status(status.as_u16());
            error.http_status_code = Some(status.as_u16());
            error
        }
    }
}

pub(super) fn validate_manager_url(raw: &str, allow_insecure: bool) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(raw)
        .into_alien_error()
        .map_err(|error| remote_configuration_source_error(error, "parse assigned manager URL"))?;
    let valid_scheme =
        url.scheme() == "https" || (allow_insecure && url.scheme() == "http" && is_loopback(&url));
    if !valid_scheme
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(remote_configuration_error("validate assigned manager URL"));
    }
    Ok(url)
}

/// Returns whether a caller-supplied Platform base URL may discover a local
/// HTTP manager. Production discovery is HTTPS-only; loopback HTTP exists for
/// local development and deterministic tests.
pub(super) fn validate_platform_base_url(raw: &str) -> Result<bool> {
    let url = reqwest::Url::parse(raw)
        .into_alien_error()
        .map_err(|error| remote_configuration_source_error(error, "parse Platform API base URL"))?;
    let allow_insecure = url.scheme() == "http" && is_loopback(&url);
    let valid_scheme = url.scheme() == "https" || allow_insecure;
    if !valid_scheme
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(remote_configuration_error("validate Platform API base URL"));
    }
    Ok(allow_insecure)
}

fn invalid_manager_access_response(operation: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::RemoteAccessFailed {
        operation: operation.to_string(),
    })
}

fn remote_configuration_error(operation: &str) -> AlienError<ErrorData> {
    let mut error = AlienError::new(ErrorData::RemoteAccessFailed {
        operation: operation.to_string(),
    });
    error.retryable = false;
    error.http_status_code = Some(400);
    error
}

fn remote_configuration_source_error(
    source: AlienError<GenericError>,
    operation: &str,
) -> AlienError<ErrorData> {
    let mut error = source.context(ErrorData::RemoteAccessFailed {
        operation: operation.to_string(),
    });
    error.retryable = false;
    error.http_status_code = Some(400);
    error
}

fn is_loopback(url: &reqwest::Url) -> bool {
    url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    })
}

fn into_remote_error(error: AlienError<GenericError>) -> AlienError<ErrorData> {
    AlienError {
        code: error.code,
        message: error.message,
        context: error.context,
        hint: error.hint,
        retryable: error.retryable,
        internal: error.internal,
        http_status_code: error.http_status_code,
        source: error.source,
        human_layer_presentation: error.human_layer_presentation,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::positive_duration_millis;

    #[test]
    fn binding_token_expiry_must_be_positive_finite_and_representable() {
        assert_eq!(positive_duration_millis(300.0), Some(300_000));
        assert_eq!(positive_duration_millis(0.001), Some(1));
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0] {
            assert_eq!(positive_duration_millis(invalid), None);
        }
        assert_eq!(positive_duration_millis(0.0001), None);
        assert_eq!(positive_duration_millis(f64::MAX), None);
    }
}
