//! Ngrok tunnel for E2E tests.
//!
//! Cloud-deployed functions (Cloud Run, Lambda, Container Apps) need to reach
//! the local manager's commands endpoint over the internet. This module starts
//! an ngrok tunnel that forwards traffic from a public URL to the local manager.
//!
//! ## Requirements
//!
//! - `NGROK_AUTHTOKEN` environment variable must be set.
//! - For CI, reserved domains (e.g., `alien-e2e-tests.ngrok.dev`) are
//!   configured in the ngrok dashboard.

use anyhow::Context;
use ngrok::config::ForwarderBuilder;
use ngrok::forwarder::Forwarder;
use ngrok::tunnel::{EndpointInfo, HttpTunnel};
use tracing::info;

/// A running ngrok tunnel with its public URL.
///
/// The tunnel stays alive as long as this struct is alive. The `_forwarder`
/// field holds the ngrok forwarder which internally spawns a task that
/// accepts connections and forwards them. The `_session` keeps the ngrok
/// session alive (dropping it closes all tunnels).
pub struct NgrokTunnel {
    /// The public URL of the tunnel (e.g., `https://alien-e2e-tests.ngrok.dev`).
    pub url: String,
    /// The ngrok forwarder. Its internal spawned task handles forwarding.
    _forwarder: Forwarder<HttpTunnel>,
    /// The ngrok session. Must be kept alive or all tunnels are closed.
    _session: ngrok::Session,
}

/// Start an ngrok HTTP tunnel forwarding to `localhost:{port}`.
///
/// - `port`: The local port the manager is listening on.
/// - `domain`: An optional reserved ngrok domain. If `None`, ngrok assigns a
///   random subdomain.
///
/// Returns the tunnel's public URL and a handle that keeps the tunnel alive.
///
/// # Errors
///
/// Returns an error if `NGROK_AUTHTOKEN` is not set or if the tunnel fails
/// to start.
pub async fn start_tunnel(port: u16, domain: Option<&str>) -> anyhow::Result<NgrokTunnel> {
    let forward_url = format!("http://localhost:{}", port);
    let forward_url_parsed =
        url::Url::parse(&forward_url).context("Failed to parse forward URL")?;

    let session = ngrok::Session::builder()
        .authtoken_from_env()
        .connect()
        .await
        .context("Failed to connect ngrok session (is NGROK_AUTHTOKEN set?)")?;

    let mut endpoint = session.http_endpoint();

    if let Some(d) = domain {
        endpoint.domain(d);
    }

    let forwarder = endpoint
        .listen_and_forward(forward_url_parsed)
        .await
        .context("Failed to start ngrok listener")?;

    let tunnel_url = forwarder.url().to_string();
    info!(%tunnel_url, %forward_url, "Ngrok tunnel started");

    Ok(NgrokTunnel {
        url: tunnel_url,
        _forwarder: forwarder,
        _session: session,
    })
}
