//! `alien-egress-gateway --relay <listen> <target>`: forwards inbound TCP connections to one
//! fixed target and does nothing else. `alien dev` runs it as a per-session container that sits
//! on the session's internal network and publishes the session's preview ports on the host. It
//! only dials its one target, so it gives the sandbox no route out.

use std::net::SocketAddr;

use tokio::net::{TcpListener, TcpStream};

/// Accepts connections on `listen` and copies each to `target` (a `host:port`) until killed.
pub async fn run(listen: SocketAddr, target: String) -> std::io::Result<()> {
    let listener = TcpListener::bind(listen).await?;
    loop {
        let (mut inbound, _) = listener.accept().await?;
        let target = target.clone();
        tokio::spawn(async move {
            if let Ok(mut outbound) = TcpStream::connect(&target).await {
                let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[tokio::test]
    async fn forwards_bytes_both_ways_to_the_target() {
        let echo = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target = echo.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            let (mut stream, _) = echo.accept().await.unwrap();
            let mut buffer = [0u8; 4];
            stream.read_exact(&mut buffer).await.unwrap();
            stream.write_all(&buffer).await.unwrap();
        });
        let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listen = probe.local_addr().unwrap();
        drop(probe);
        tokio::spawn(run(listen, target));
        let mut client = loop {
            if let Ok(client) = TcpStream::connect(listen).await {
                break client;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        };
        client.write_all(b"ping").await.unwrap();
        let mut reply = [0u8; 4];
        client.read_exact(&mut reply).await.unwrap();
        assert_eq!(&reply, b"ping");
    }
}
