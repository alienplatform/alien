//! Embedded, protocol-agnostic AI gateway: a loopback HTTP server that injects the
//! workload's ambient cloud credential and proxies each request to the model's
//! native upstream endpoint without translating the body.
//!
//! The runtime starts the gateway before the app process (`start_gateway`) and
//! passes its URL to the app; routing lives in `router`, credential injection in
//! `creds`.

mod config;
mod creds;
mod error;
mod router;
pub use config::{bindings_from_env, bindings_from_env_map};
pub use creds::{AmbientCred, AwsSigV4Cred, BearerTokenCred};
pub use error::{ErrorData, Result};
pub use router::{build_router, GatewayRoute};

use std::net::{Ipv4Addr, SocketAddr};

use alien_core::Platform;
use alien_error::{Context, IntoAlienError};
use axum::routing::get;

/// The readiness endpoint the gateway serves and every caller polls. Shared so the
/// path and the poll budget below cannot drift between the runtime and the launcher.
pub const READY_PATH: &str = "/healthz/ready";
/// Readiness poll budget: `READY_POLL_TRIES` attempts, `READY_POLL_INTERVAL_MS` apart.
/// ~1s covers process spawn + tokio init + local config assembly; ambient credential
/// resolution is lazy (deferred to the first request), so no network I/O gates the bind.
pub const READY_POLL_TRIES: usize = 20;
pub const READY_POLL_INTERVAL_MS: u64 = 50;

/// One `ai` resource the gateway serves. An Alien `ai` binding maps to exactly one
/// cloud, so each binding fixes the upstream host and ambient credential; the
/// request's model selects the model and (via the catalog) the protocol path.
#[derive(Debug, Clone)]
pub struct GatewayBinding {
    /// The binding name (the path segment the app calls: `/<name>/...`).
    pub name: String,
    pub cloud: Platform,
    /// AWS region or GCP location.
    pub region: Option<String>,
    /// GCP project id.
    pub project: Option<String>,
    /// Azure account endpoint, e.g. `https://acct.openai.azure.com/`.
    pub azure_endpoint: Option<String>,
}

/// A running gateway: its loopback base URL and the server task that keeps it alive
/// for the process lifetime. Dropping the handle aborts the server.
pub struct GatewayHandle {
    pub url: String,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for GatewayHandle {
    /// Dropping a `JoinHandle` only detaches its task, so abort explicitly — otherwise the
    /// server outlives the handle and keeps the port bound until the process exits.
    fn drop(&mut self) {
        self.server.abort();
    }
}

/// Start the gateway on an ephemeral loopback port and return its base URL once the
/// listener is bound. Each binding's ambient credential is resolved up front and
/// mounted as a proxy route under `/<name>`; `/healthz/ready` answers immediately.
pub async fn start_gateway(bindings: Vec<GatewayBinding>) -> Result<GatewayHandle> {
    start_gateway_on(bindings, SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await
}

/// Like [`start_gateway`] but binds an explicit address. The container launcher uses a
/// fixed loopback port here because it tells the app the URL out of band (an env var)
/// rather than reading back a randomly assigned port.
pub async fn start_gateway_on(
    bindings: Vec<GatewayBinding>,
    addr: SocketAddr,
) -> Result<GatewayHandle> {
    // Built once and shared across routes: on the runtime-less mint path it is the single
    // refresh clock and mint cache every request re-resolves through.
    let managed = config::managed_provider()?;
    let mut routes = Vec::with_capacity(bindings.len());
    for binding in bindings {
        routes.push(config::resolve_route(binding, managed.as_ref()).await?);
    }
    let router = build_router(routes).route(READY_PATH, get(ready));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .into_alien_error()
        .context(ErrorData::BindFailed {
            address: addr.to_string(),
        })?;
    let bound = listener
        .local_addr()
        .into_alien_error()
        .context(ErrorData::Other {
            message: "could not read the gateway's bound address".to_string(),
        })?;
    let url = format!("http://{bound}");

    let server = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, router).await {
            tracing::error!(error = %e, "AI gateway server stopped");
        }
    });

    Ok(GatewayHandle { url, server })
}

async fn ready() -> &'static str {
    "ready"
}

/// Blocks until the gateway at `base_url` answers `READY_PATH`, within the shared poll
/// budget. Returns `false` if it never became ready. The launcher (a non-async process
/// entrypoint) uses this; the runtime uses an async twin sharing the same constants.
pub fn wait_until_ready_blocking(base_url: &str) -> bool {
    let url = format!("{base_url}{READY_PATH}");
    let client = reqwest::blocking::Client::new();
    for _ in 0..READY_POLL_TRIES {
        if let Ok(resp) = client.get(&url).send() {
            if resp.status().is_success() {
                return true;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(READY_POLL_INTERVAL_MS));
    }
    false
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn gateway_starts_and_serves_health() {
        let handle = start_gateway(vec![]).await.expect("gateway should start");
        let body = reqwest::get(format!("{}/healthz/ready", handle.url))
            .await
            .expect("health request should succeed")
            .text()
            .await
            .expect("health body should read");
        assert_eq!(body.trim(), "ready");
    }

    #[tokio::test]
    async fn gateway_binds_the_requested_port() {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let handle = start_gateway_on(vec![], addr)
            .await
            .expect("gateway should start on the given addr");
        // URL reflects a concrete loopback bind, not the placeholder :0
        assert!(handle.url.starts_with("http://127.0.0.1:"));
        assert!(!handle.url.ends_with(":0"));
    }

    /// Dropping the handle must stop the server: a detached task would keep serving (and
    /// keep the port bound) for the rest of the process.
    #[tokio::test]
    async fn dropping_the_handle_stops_the_server() {
        let handle = start_gateway(vec![]).await.expect("gateway should start");
        let url = format!("{}/healthz/ready", handle.url);
        assert!(reqwest::get(&url).await.is_ok(), "the gateway serves while the handle is held");

        drop(handle);
        // The abort lands on the next scheduler pass.
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert!(
            reqwest::get(&url).await.is_err(),
            "the gateway must stop serving once its handle is dropped"
        );
    }
}
