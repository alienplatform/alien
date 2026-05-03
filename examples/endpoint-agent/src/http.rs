/// Minimal HTTP health server
///
/// TODO: Remove this entire module once Worker resource is implemented.
/// Worker resources don't require HTTP endpoints - they signal ready through gRPC.
/// This is a temporary workaround because Function resources require HTTP registration
/// to signal ready state to the runtime.
use alien_sdk::AlienContext;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use std::net::SocketAddr;
use tokio::net::TcpListener;

/// Start a minimal HTTP health server and register it with the runtime
pub async fn start_health_server(
    ctx: &AlienContext,
) -> std::result::Result<u16, Box<dyn std::error::Error>> {
    // Bind to random port
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let port = addr.port();

    // Register with runtime
    ctx.register_http_server(port).await?;

    // Start HTTP server in background
    tokio::spawn(async move {
        if let Err(e) = run_server(listener, addr).await {
            tracing::error!("HTTP server error: {}", e);
        }
    });

    Ok(port)
}

async fn run_server(
    listener: TcpListener,
    addr: SocketAddr,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    tracing::debug!("Health server listening on {}", addr);

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);

        tokio::spawn(async move {
            if let Err(e) = http1::Builder::new()
                .serve_connection(io, service_fn(handle_request))
                .await
            {
                tracing::debug!("Error serving connection: {:?}", e);
            }
        });
    }
}

async fn handle_request(
    _req: Request<hyper::body::Incoming>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Simple 200 OK for all requests (health check)
    Ok(Response::new(Full::new(Bytes::from("ok"))))
}
