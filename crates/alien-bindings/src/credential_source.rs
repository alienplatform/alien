//! Minting-backed client credential resolution.
//!
//! This is the *only* client-side credential resolver: every language SDK
//! inherits it through the napi addon, so it must stay runtime-agnostic (no
//! background timers, no spawned refresh loops — refresh happens lazily, on the
//! access path).
//!
//! When a deployed app process cannot resolve native/projected cloud
//! credentials from its environment (see the selection order in
//! [`crate::provider::LazyEnvBindingsProvider`]), it falls back to *minting*:
//! it POSTs to `{ALIEN_MANAGER_URL}/v1/credentials/mint` with its deployment
//! token and receives a short-lived [`ClientConfig`] plus a server-computed
//! `expiresAt` refresh hint. The minted config is cached and re-minted on
//! access once it passes the refresh threshold, with a single-flight guard so a
//! burst of concurrent binding loads triggers at most one mint.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use alien_core::{
    ClientConfig, ENV_ALIEN_DEPLOYMENT_ID, ENV_ALIEN_DEPLOYMENT_SERVICE_ACCOUNT,
    ENV_ALIEN_DEPLOYMENT_TOKEN, ENV_ALIEN_MANAGER_URL,
};
use alien_error::{AlienError, Context, IntoAlienError};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::Deserialize;
use tokio::sync::{Mutex, RwLock};
use tracing::debug;

use crate::error::{ErrorData, Result};
use crate::provider::BindingsProvider;

/// Re-mint credentials once they are within this many seconds of their
/// server-declared expiry. Gives in-flight work a safety margin so it never
/// races a hard expiry, and absorbs modest client/server clock skew.
const REFRESH_SKEW_SECONDS: i64 = 300;

/// Timeout for a single mint HTTP request. The mint endpoint impersonates a
/// service account / calls STS, so it is not instant; this is generous enough
/// for that round-trip while still bounding a hung manager.
const MINT_TIMEOUT: Duration = Duration::from_secs(30);

/// The mint request inputs read from the process environment, plus the HTTP
/// client used to reach the manager.
///
/// The manager injects these vars for deployed app processes (controller
/// injection is an ALIEN-218 task-10/16 follow-up).
pub(crate) struct MintingCredentialSource {
    /// Base URL of the deployment's manager (`ALIEN_MANAGER_URL`).
    manager_url: String,
    /// Deployment bearer token (`ALIEN_DEPLOYMENT_TOKEN`). Secret material —
    /// see the manual [`fmt::Debug`] impl, which must never print it.
    token: String,
    /// Deployment id (`ALIEN_DEPLOYMENT_ID`).
    deployment_id: String,
    /// Service-account binding to mint credentials for
    /// (`ALIEN_DEPLOYMENT_SERVICE_ACCOUNT`).
    binding_name: String,
    http: reqwest::Client,
}

/// Manual `Debug`: `token` is a live bearer credential. Never let a `{:?}` of
/// this source (log line, panic message, test failure output) print it.
impl fmt::Debug for MintingCredentialSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MintingCredentialSource")
            .field("manager_url", &self.manager_url)
            .field("token", &"<redacted>")
            .field("deployment_id", &self.deployment_id)
            .field("binding_name", &self.binding_name)
            .finish()
    }
}

impl MintingCredentialSource {
    /// Builds a source from the environment when the mint contract is present.
    ///
    /// Returns `Ok(None)` when the gate (`ALIEN_MANAGER_URL` +
    /// `ALIEN_DEPLOYMENT_TOKEN`) is absent — the caller then keeps the existing
    /// (non-minting) behaviour. When the gate *is* present but the rest of the
    /// request contract (`ALIEN_DEPLOYMENT_ID`, `ALIEN_DEPLOYMENT_SERVICE_ACCOUNT`)
    /// is missing, this fails fast: a half-injected mint environment is a
    /// manager bug, not a reason to silently fall back to static resolution.
    pub(crate) fn from_env(env: &HashMap<String, String>) -> Result<Option<Self>> {
        let (Some(manager_url), Some(token)) = (
            env.get(ENV_ALIEN_MANAGER_URL),
            env.get(ENV_ALIEN_DEPLOYMENT_TOKEN),
        ) else {
            return Ok(None);
        };

        let deployment_id = env.get(ENV_ALIEN_DEPLOYMENT_ID).ok_or_else(|| {
            AlienError::new(ErrorData::EnvironmentVariableMissing {
                variable_name: ENV_ALIEN_DEPLOYMENT_ID.to_string(),
            })
        })?;
        let binding_name = env
            .get(ENV_ALIEN_DEPLOYMENT_SERVICE_ACCOUNT)
            .ok_or_else(|| {
                AlienError::new(ErrorData::EnvironmentVariableMissing {
                    variable_name: ENV_ALIEN_DEPLOYMENT_SERVICE_ACCOUNT.to_string(),
                })
            })?;

        let http = reqwest::Client::builder()
            .timeout(MINT_TIMEOUT)
            .build()
            .into_alien_error()
            .context(ErrorData::RemoteAccessFailed {
                operation: "build minting HTTP client".to_string(),
            })?;

        Ok(Some(Self {
            manager_url: manager_url.clone(),
            token: token.clone(),
            deployment_id: deployment_id.clone(),
            binding_name: binding_name.clone(),
            http,
        }))
    }

    /// POST the mint request and parse the response. Errors surface typed via
    /// `alien-error` with context; there are no panic paths.
    async fn mint(&self) -> Result<MintedConfig> {
        let url = format!(
            "{}/v1/credentials/mint",
            self.manager_url.trim_end_matches('/')
        );

        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.token)
            .json(&serde_json::json!({
                "deploymentId": self.deployment_id,
                "bindingName": self.binding_name,
            }))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::RemoteAccessFailed {
                operation: "mint credentials from manager".to_string(),
            })?;

        let response = response.error_for_status().into_alien_error().context(
            ErrorData::RemoteAccessFailed {
                operation: "mint credentials from manager (non-success status)".to_string(),
            },
        )?;

        let minted: MintResponse =
            response
                .json()
                .await
                .into_alien_error()
                .context(ErrorData::RemoteAccessFailed {
                    operation: "parse mint response".to_string(),
                })?;

        // Audit trail: never logs the client_config (credential material) — only
        // the non-secret principal and expiry.
        debug!(
            deployment_id = %self.deployment_id,
            binding_name = %self.binding_name,
            principal = %minted.principal,
            expires_at = %minted.expires_at.to_rfc3339(),
            "Minted client credentials"
        );

        Ok(MintedConfig {
            client_config: minted.client_config,
            expires_at: minted.expires_at,
        })
    }
}

/// Deserialised mint response. Mirrors the manager's `MintCredentialsResponse`
/// (`crates/alien-manager/src/routes/credentials.rs`).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MintResponse {
    client_config: ClientConfig,
    /// Server-computed refresh hint (RFC3339). Treated as "re-mint at or before
    /// this instant", not as proof the credential is valid to the last second.
    expires_at: DateTime<Utc>,
    /// Human-readable identity the credentials act as. Non-secret; logged only.
    principal: String,
}

/// A minted config together with the server's expiry hint.
struct MintedConfig {
    client_config: ClientConfig,
    expires_at: DateTime<Utc>,
}

/// A cached [`BindingsProvider`] built from minted credentials, plus the expiry
/// that decides when it must be rebuilt.
struct Cached {
    provider: Arc<BindingsProvider>,
    expires_at: DateTime<Utc>,
}

/// Resolves a [`BindingsProvider`] from minted credentials, caching it until it
/// passes the refresh threshold and re-minting on access under a single-flight
/// guard.
pub(crate) struct MintingResolver {
    source: MintingCredentialSource,
    /// Binding JSON (`ALIEN_*_BINDING`), reused verbatim across re-mints — only
    /// the credentials change, not the binding topology.
    bindings: HashMap<String, serde_json::Value>,
    /// Cached provider + expiry. `None` until the first mint.
    cache: RwLock<Option<Cached>>,
    /// Single-flight guard: a burst of concurrent stale/first loads collapses to
    /// one mint. An async `Mutex` (not a timer) keeps this napi-runtime-agnostic.
    refresh_lock: Mutex<()>,
    /// Re-mint once within this many seconds of expiry. Field (not the bare
    /// const) so tests can pin it.
    refresh_skew_seconds: i64,
}

/// Manual `Debug`: the cached provider holds a live [`ClientConfig`] and the
/// source holds a bearer token. Redact both; delegating to the source's own
/// (already-redacting) `Debug` is safe.
impl fmt::Debug for MintingResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MintingResolver")
            .field("source", &self.source)
            .field("bindings", &self.bindings.keys().collect::<Vec<_>>())
            .field("cache", &"<redacted>")
            .field("refresh_skew_seconds", &self.refresh_skew_seconds)
            .finish()
    }
}

impl MintingResolver {
    pub(crate) fn new(
        source: MintingCredentialSource,
        bindings: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            source,
            bindings,
            cache: RwLock::new(None),
            refresh_lock: Mutex::new(()),
            refresh_skew_seconds: REFRESH_SKEW_SECONDS,
        }
    }

    /// Returns a provider backed by fresh-enough minted credentials, minting (or
    /// re-minting) only when the cache is empty or stale.
    pub(crate) async fn provider(&self) -> Result<Arc<BindingsProvider>> {
        // Fast path: a fresh cached provider needs no lock contention and no mint.
        if let Some(provider) = self.fresh_cached().await {
            return Ok(provider);
        }

        // Slow path: single-flight the mint. Only one task refreshes per
        // staleness window; the rest wait here and then observe the fresh cache.
        let _flight = self.refresh_lock.lock().await;

        // Double-check: a racing task may have refreshed while we waited for the
        // lock. This is what makes concurrent first-loads collapse to one mint.
        if let Some(provider) = self.fresh_cached().await {
            return Ok(provider);
        }

        let minted = self.source.mint().await?;
        let provider = Arc::new(BindingsProvider::new(
            minted.client_config,
            self.bindings.clone(),
        )?);

        let mut cache = self.cache.write().await;
        *cache = Some(Cached {
            provider: provider.clone(),
            expires_at: minted.expires_at,
        });
        Ok(provider)
    }

    /// The cached provider if present and not yet within the refresh window.
    async fn fresh_cached(&self) -> Option<Arc<BindingsProvider>> {
        let cache = self.cache.read().await;
        cache.as_ref().and_then(|cached| {
            if self.is_stale(cached.expires_at) {
                None
            } else {
                Some(cached.provider.clone())
            }
        })
    }

    fn is_stale(&self, expires_at: DateTime<Utc>) -> bool {
        Utc::now() >= expires_at - ChronoDuration::seconds(self.refresh_skew_seconds)
    }

    /// Test-only: simulate elapsed time by pushing the cached expiry into the
    /// past, so the next access observes the entry as stale and re-mints. Keeps
    /// the re-mint test deterministic without a real sleep.
    #[cfg(test)]
    async fn force_stale(&self) {
        if let Some(cached) = self.cache.write().await.as_mut() {
            cached.expires_at = Utc::now() - ChronoDuration::seconds(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::{extract::State, routing::post, Json, Router};
    use serde_json::json;

    /// Shared state for the fake mint endpoint: counts requests and controls the
    /// expiry it hands back.
    #[derive(Clone)]
    struct MintServerState {
        calls: Arc<AtomicUsize>,
        /// Seconds from now used for each response's `expiresAt`.
        expiry_secs: i64,
        /// Optional artificial delay to widen the single-flight race window.
        delay: Option<Duration>,
    }

    async fn mint_handler(State(state): State<MintServerState>) -> Json<serde_json::Value> {
        state.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(delay) = state.delay {
            tokio::time::sleep(delay).await;
        }
        let expires_at = (Utc::now() + ChronoDuration::seconds(state.expiry_secs)).to_rfc3339();
        // A `Local` client config deserialises without needing real cloud
        // credentials, so binding loads against the minted provider stay offline.
        Json(json!({
            "clientConfig": { "platform": "local", "state_directory": "/tmp/alien-mint-test" },
            "expiresAt": expires_at,
            "principal": "local:mint-test",
        }))
    }

    /// Spawn a fake mint server; returns its base URL and the call counter.
    async fn spawn_mint_server(
        expiry_secs: i64,
        delay: Option<Duration>,
    ) -> (String, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let state = MintServerState {
            calls: calls.clone(),
            expiry_secs,
            delay,
        };
        let app = Router::new()
            .route("/v1/credentials/mint", post(mint_handler))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .expect("bind fake mint server");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        (format!("http://{addr}"), calls)
    }

    fn source(manager_url: &str) -> MintingCredentialSource {
        let env = HashMap::from([
            (ENV_ALIEN_MANAGER_URL.to_string(), manager_url.to_string()),
            (
                ENV_ALIEN_DEPLOYMENT_TOKEN.to_string(),
                "ax_deploy_SECRET_TOKEN".to_string(),
            ),
            (ENV_ALIEN_DEPLOYMENT_ID.to_string(), "dep_123".to_string()),
            (
                ENV_ALIEN_DEPLOYMENT_SERVICE_ACCOUNT.to_string(),
                "management".to_string(),
            ),
        ]);
        MintingCredentialSource::from_env(&env)
            .expect("source builds")
            .expect("mint contract present")
    }

    #[tokio::test]
    async fn from_env_returns_none_without_gate() {
        // No manager URL / token -> not a minting environment.
        let env = HashMap::from([(ENV_ALIEN_DEPLOYMENT_ID.to_string(), "dep_1".to_string())]);
        assert!(MintingCredentialSource::from_env(&env)
            .expect("no error")
            .is_none());
    }

    #[tokio::test]
    async fn from_env_fails_fast_when_gate_present_but_contract_incomplete() {
        // Manager URL + token present, but deployment id / SA binding missing:
        // a half-injected mint environment must error, not silently fall back.
        let env = HashMap::from([
            (
                ENV_ALIEN_MANAGER_URL.to_string(),
                "http://localhost".to_string(),
            ),
            (ENV_ALIEN_DEPLOYMENT_TOKEN.to_string(), "tok".to_string()),
        ]);
        let error = MintingCredentialSource::from_env(&env)
            .expect_err("incomplete contract should fail fast");
        assert_eq!(error.code, "ENVIRONMENT_VARIABLE_MISSING");
    }

    #[tokio::test]
    async fn first_use_mints_then_caches_then_re_mints_when_stale() {
        let (base_url, calls) = spawn_mint_server(3600, None).await;
        let resolver = MintingResolver::new(source(&base_url), HashMap::new());

        // First access mints.
        resolver.provider().await.expect("first mint");
        assert_eq!(calls.load(Ordering::SeqCst), 1, "first access mints once");

        // Second access is served from the fresh cache: no new mint.
        resolver.provider().await.expect("cached");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "fresh cache must not re-hit the manager"
        );

        // Once stale, the next access re-mints.
        resolver.force_stale().await;
        resolver.provider().await.expect("re-mint");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "stale credentials must trigger a re-mint on access"
        );
    }

    #[tokio::test]
    async fn near_expiry_config_is_treated_as_stale() {
        // Server hands back an expiry inside the refresh skew window, so the
        // very next access re-mints — the credential is never handed out cutting
        // it close to the hard expiry.
        let (base_url, calls) = spawn_mint_server(60, None).await;
        let resolver = MintingResolver::new(source(&base_url), HashMap::new());

        resolver.provider().await.expect("first mint");
        resolver.provider().await.expect("second mint");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "an expiry within the 300s skew window is stale on the next access"
        );
    }

    #[tokio::test]
    async fn concurrent_first_loads_mint_exactly_once() {
        // Two tasks race the empty cache; the single-flight guard must collapse
        // them to one mint. A server delay widens the window to make the race real.
        let (base_url, calls) = spawn_mint_server(3600, Some(Duration::from_millis(100))).await;
        let resolver = Arc::new(MintingResolver::new(source(&base_url), HashMap::new()));

        let a = {
            let resolver = resolver.clone();
            tokio::spawn(async move { resolver.provider().await.map(|_| ()) })
        };
        let b = {
            let resolver = resolver.clone();
            tokio::spawn(async move { resolver.provider().await.map(|_| ()) })
        };
        a.await.expect("join a").expect("mint a");
        b.await.expect("join b").expect("mint b");

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "single-flight must collapse concurrent first-loads to one mint"
        );
    }

    #[tokio::test]
    async fn debug_never_leaks_token_or_credentials() {
        let (base_url, _calls) = spawn_mint_server(3600, None).await;
        let resolver = MintingResolver::new(source(&base_url), HashMap::new());
        resolver.provider().await.expect("mint");

        let rendered = format!("{resolver:?}");
        assert!(
            !rendered.contains("ax_deploy_SECRET_TOKEN"),
            "resolver Debug leaked the deployment token: {rendered}"
        );
        assert!(
            rendered.contains("<redacted>"),
            "resolver Debug should mark redacted fields: {rendered}"
        );

        // The source alone must also redact its token.
        let source_rendered = format!("{:?}", source(&base_url));
        assert!(!source_rendered.contains("ax_deploy_SECRET_TOKEN"));
    }
}
