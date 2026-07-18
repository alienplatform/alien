use std::{env, process, sync::Arc};

use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    let identity = Arc::new(format!(
        "{}-{}",
        env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string()),
        process::id()
    ));
    let version = Arc::new(env::var("E2E_TCP_VERSION").unwrap_or_else(|_| "v1".to_string()));
    let tcp = TcpListener::bind("0.0.0.0:7000").await?;
    let health = TcpListener::bind("0.0.0.0:8080").await?;
    info!(%identity, %version, "TCP transaction fixture ready");

    let health_task = tokio::spawn(serve_health(health));
    loop {
        let (stream, _) = tcp.accept().await?;
        let identity = identity.clone();
        let version = version.clone();
        tokio::spawn(async move {
            if let Err(error) = serve_transactions(stream, &identity, &version).await {
                warn!(%error, "TCP transaction connection failed");
            }
        });

        if health_task.is_finished() {
            return Err("health server exited unexpectedly".into());
        }
    }
}

async fn serve_transactions(
    stream: TcpStream,
    identity: &str,
    version: &str,
) -> std::io::Result<()> {
    let mut stream = BufReader::new(stream);
    let mut request = String::new();
    while stream.read_line(&mut request).await? != 0 {
        let fields: Vec<&str> = request.trim_end().splitn(3, ' ').collect();
        let response = match fields.as_slice() {
            ["TX", sequence, payload] => {
                format!("ACK {identity} {version} {sequence} {payload}\n")
            }
            _ => "ERR expected: TX <sequence> <payload>\n".to_string(),
        };
        stream.get_mut().write_all(response.as_bytes()).await?;
        request.clear();
    }
    Ok(())
}

async fn serve_health(listener: TcpListener) -> std::io::Result<()> {
    loop {
        let (mut stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            let mut request = [0_u8; 1024];
            let result = async {
                let read = stream.read(&mut request).await?;
                let healthy = request[..read].starts_with(b"GET /health ");
                let (status, body) = if healthy {
                    ("200 OK", "ok")
                } else {
                    ("404 Not Found", "not found")
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await
            }
            .await;
            if let Err(error) = result {
                warn!(%error, "health connection failed");
            }
        });
    }
}
