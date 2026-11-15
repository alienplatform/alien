//! A loopback forwarder that keeps a VNet sandbox's egress up while one gateway replica is down.
//!
//! Azure sandboxes reach the egress gateway through proxy environment variables, and the gateway
//! runs as two VMs at fixed addresses (no load balancer). A proxy variable names one address, so
//! the agent listens on loopback and, for each new connection, dials the replicas in turn: the
//! last one that worked first, then the other, with a connect timeout, so a stopped, rebooting or
//! blackholed replica costs at most one timeout and the next connection goes straight to the
//! healthy one. Bytes are relayed unchanged, so the proxy credentials stay between the command
//! and the gateway.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};

/// The gateway replicas' proxy addresses, comma separated (`ip:port,ip:port`).
pub const ENV_PROXY_REPLICAS: &str = "ALIEN_SANDBOX_PROXY_REPLICAS";
/// The loopback address commands' proxy variables point at.
pub const ENV_PROXY_LISTEN: &str = "ALIEN_SANDBOX_PROXY_LISTEN";

/// The gateway's first connect can take 7-20 s, but a replica that is stopped or blackholed never
/// answers: give each replica a few seconds, then try the other, for two passes.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const PASSES: usize = 2;

/// Replicas in dial order with a sticky preference for the last good one.
pub struct Replicas {
    addresses: Vec<String>,
    preferred: AtomicUsize,
    timeout: Duration,
}

impl Replicas {
    pub fn new(addresses: Vec<String>, timeout: Duration) -> Self {
        Self {
            addresses,
            preferred: AtomicUsize::new(0),
            timeout,
        }
    }

    /// Dials the preferred replica, then the others, for `PASSES` passes. Remembers the winner.
    pub async fn dial(&self) -> Option<TcpStream> {
        let count = self.addresses.len();
        if count == 0 {
            return None;
        }
        let first = self.preferred.load(Ordering::Relaxed) % count;
        for _ in 0..PASSES {
            for offset in 0..count {
                let index = (first + offset) % count;
                let attempt = tokio::time::timeout(
                    self.timeout,
                    TcpStream::connect(self.addresses[index].as_str()),
                )
                .await;
                if let Ok(Ok(stream)) = attempt {
                    self.preferred.store(index, Ordering::Relaxed);
                    return Some(stream);
                }
            }
        }
        None
    }
}

async fn relay(replicas: Arc<Replicas>, mut guest: TcpStream) {
    let Some(mut gateway) = replicas.dial().await else {
        return;
    };
    let _ = copy_bidirectional(&mut guest, &mut gateway).await;
}

/// Accept loop. Returns only if the listener fails.
pub async fn serve(listener: TcpListener, replicas: Arc<Replicas>) {
    loop {
        let Ok((guest, _)) = listener.accept().await else {
            return;
        };
        tokio::spawn(relay(replicas.clone(), guest));
    }
}

/// Starts the forwarder when the Azure backend asked for it. The listener is bound before this
/// returns, so the proxy address works as soon as the workload starts.
pub fn spawn_from_env() -> Result<(), String> {
    let Some(list) = std::env::var(ENV_PROXY_REPLICAS)
        .ok()
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(());
    };
    let listen: SocketAddr = std::env::var(ENV_PROXY_LISTEN)
        .map_err(|_| format!("{ENV_PROXY_LISTEN} is not set"))?
        .parse()
        .map_err(|_| format!("{ENV_PROXY_LISTEN} is not an address"))?;
    if !listen.ip().is_loopback() {
        return Err(format!("{ENV_PROXY_LISTEN} must be a loopback address"));
    }
    let addresses: Vec<String> = list
        .split(',')
        .map(str::trim)
        .filter(|address| !address.is_empty())
        .map(str::to_string)
        .collect();
    let std_listener = std::net::TcpListener::bind(listen)
        .map_err(|error| format!("cannot bind the proxy failover listener on {listen}: {error}"))?;
    std_listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let replicas = Arc::new(Replicas::new(addresses, CONNECT_TIMEOUT));
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
        else {
            return;
        };
        runtime.block_on(async move {
            if let Ok(listener) = TcpListener::from_std(std_listener) {
                serve(listener, replicas).await;
            }
        });
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn echo_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let mut buffer = [0u8; 64];
                    while let Ok(read) = stream.read(&mut buffer).await {
                        if read == 0 || stream.write_all(&buffer[..read]).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        address
    }

    /// A port nothing listens on: connect is refused at once.
    fn refused() -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().to_string()
    }

    /// A listener whose backlog is full never completes a connect: stands in for a blackhole.
    async fn through(replicas: Vec<String>, timeout: Duration) -> Option<Vec<u8>> {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local = listener.local_addr().unwrap();
        let replicas = Arc::new(Replicas::new(replicas, timeout));
        tokio::spawn(serve(listener, replicas));
        let mut client = TcpStream::connect(local).await.unwrap();
        client.write_all(b"hello").await.unwrap();
        let mut buffer = [0u8; 5];
        tokio::time::timeout(Duration::from_secs(10), client.read_exact(&mut buffer))
            .await
            .ok()?
            .ok()?;
        Some(buffer.to_vec())
    }

    #[tokio::test]
    async fn bytes_flow_through_the_second_replica_when_the_first_refuses() {
        let good = echo_server().await;
        let got = through(vec![refused(), good], Duration::from_millis(300)).await;
        assert_eq!(got.as_deref(), Some(&b"hello"[..]));
    }

    #[tokio::test]
    async fn bytes_flow_through_the_second_replica_when_the_first_is_blackholed() {
        let good = echo_server().await;
        // 192.0.2.0/24 (TEST-NET-1) is never routed: the connect neither completes nor fails
        // quickly on a host with a default route, and the timeout moves on.
        let got = through(vec!["192.0.2.1:3128".into(), good], Duration::from_millis(300)).await;
        assert_eq!(got.as_deref(), Some(&b"hello"[..]));
    }

    #[tokio::test]
    async fn the_last_good_replica_is_tried_first_next_time() {
        let good = echo_server().await;
        let replicas = Replicas::new(vec!["192.0.2.1:3128".into(), good], Duration::from_millis(300));
        assert!(replicas.dial().await.is_some());
        let started = std::time::Instant::now();
        assert!(replicas.dial().await.is_some());
        assert!(
            started.elapsed() < Duration::from_millis(250),
            "the second dial skipped the dead replica"
        );
    }

    #[tokio::test]
    async fn no_replica_up_closes_the_connection() {
        let got = through(vec![refused(), refused()], Duration::from_millis(100)).await;
        assert!(got.is_none());
    }

    #[test]
    fn the_listener_must_be_loopback() {
        std::env::set_var(ENV_PROXY_REPLICAS, "10.0.6.4:3128");
        std::env::set_var(ENV_PROXY_LISTEN, "0.0.0.0:3128");
        let error = spawn_from_env().unwrap_err();
        std::env::remove_var(ENV_PROXY_REPLICAS);
        std::env::remove_var(ENV_PROXY_LISTEN);
        assert!(error.contains("loopback"), "{error}");
    }
}
