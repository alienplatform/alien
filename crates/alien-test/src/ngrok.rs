//! Ngrok tunnel for E2E tests.
//!
//! Cloud-deployed functions (Cloud Run, Lambda, Container Apps) need to reach
//! the local manager's commands endpoint over the internet. This module starts
//! an ngrok tunnel that forwards traffic from a public URL to the local manager.
//!
//! Each test run gets its own ephemeral `ngrok.dev` domain, so multiple test
//! runs can execute concurrently without domain conflicts.
//!
//! ## Requirements
//!
//! - `NGROK_AUTHTOKEN` environment variable must be set (paid plan required).

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
    /// The public URL of the tunnel (e.g., `https://e2e-a1b2c3d4.ngrok.dev`).
    pub url: String,
    /// The ngrok forwarder. Its internal spawned task handles forwarding.
    _forwarder: Forwarder<HttpTunnel>,
    /// The ngrok session. Must be kept alive or all tunnels are closed.
    _session: ngrok::Session,
}

/// Start an ngrok HTTP tunnel forwarding to `localhost:{port}`.
///
/// Generates an ephemeral `e2e-<uuid>.ngrok.dev` domain so each test
/// run gets its own unique public URL (requires a paid ngrok plan).
///
/// # Errors
///
/// Returns an error if `NGROK_AUTHTOKEN` is not set or if the tunnel fails
/// to start.
pub async fn start_tunnel(port: u16) -> anyhow::Result<NgrokTunnel> {
    // rustls 0.23+ requires an explicit CryptoProvider when multiple providers
    // are compiled in. Our workspace has both `ring` (from reqwest) and
    // `aws-lc-rs` (from ngrok). Install `ring` as the process default since
    // that's what the rest of the codebase already uses via reqwest.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let forward_url = format!("http://localhost:{}", port);
    let forward_url_parsed =
        url::Url::parse(&forward_url).context("Failed to parse forward URL")?;

    let session = ngrok::Session::builder()
        .authtoken_from_env()
        .connect()
        .await
        .context("Failed to connect ngrok session (is NGROK_AUTHTOKEN set?)")?;

    let ephemeral_domain = format!(
        "e2e-{}.ngrok.dev",
        uuid::Uuid::new_v4().to_string().replace('-', "")
    );

    let forwarder = session
        .http_endpoint()
        .domain(&ephemeral_domain)
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
