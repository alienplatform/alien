//! Node-API addon for the alien AI gateway.
//!
//! The gateway itself is the Rust `alien-gateway` crate — the single, shared
//! implementation. This addon is a thin wrapper: it starts the gateway's loopback
//! HTTP server inside napi's tokio runtime and returns the server's URL to JS.
//! The application then points a plain OpenAI-compatible client at that URL, and
//! every request/response (including SSE token streams) flows over the loopback
//! HTTP socket — never across the napi boundary.

use alien_gateway::{bindings_from_env, start_gateway, GatewayHandle};
use napi_derive::napi;

mod error;
use error::map_gateway_error;

#[napi]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// A running gateway: its loopback base URL and the underlying handle, which owns
/// the server task. JS must hold this object for as long as it uses the gateway —
/// dropping it aborts the server.
#[napi]
pub struct AiGatewayHandle {
    url: String,
    _handle: GatewayHandle,
}

#[napi]
impl AiGatewayHandle {
    /// The gateway's loopback base URL (`http://127.0.0.1:<port>`). Callers append
    /// `/<binding>/v1` and point an OpenAI-compatible client at it.
    #[napi(getter)]
    pub fn url(&self) -> String {
        self.url.clone()
    }
}

/// Start the embedded AI gateway from the ambient `ALIEN_<NAME>_BINDING` env vars
/// in the current process, and return a handle whose `url` is the loopback base.
/// A no-op-cost call when the process links no ambient AI resource (the gateway
/// serves no routes). Hold the returned handle for the process lifetime.
#[napi]
pub async fn start_ai_gateway() -> napi::Result<AiGatewayHandle> {
    let bindings = bindings_from_env().map_err(map_gateway_error)?;
    let handle = start_gateway(bindings).await.map_err(map_gateway_error)?;
    Ok(AiGatewayHandle {
        url: handle.url.clone(),
        _handle: handle,
    })
}
