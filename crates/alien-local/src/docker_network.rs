use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use alien_error::{Context, IntoAlienError};
use bollard::{network::InspectNetworkOptions, Docker};
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, info, warn};

use crate::{ErrorData, Result};

/// Returns a Docker bridge gateway that belongs to the private range used by
/// Local services and is bindable by this host.
pub(crate) async fn bindable_docker_bridge_gateway() -> Option<Ipv4Addr> {
    let docker = Docker::connect_with_local_defaults().ok()?;
    let network = docker
        .inspect_network("bridge", None::<InspectNetworkOptions<String>>)
        .await
        .ok()?;
    let gateway = network
        .ipam?
        .config?
        .into_iter()
        .filter_map(|config| config.gateway)
        .find_map(|gateway| docker_bridge_gateway(&gateway))?;
    std::net::TcpListener::bind((gateway, 0)).ok()?;
    Some(gateway)
}

/// Makes a loopback service reachable at the same port on Docker's private
/// host gateway. Hosts where that gateway cannot be bound, such as Docker
/// Desktop, retain their native `host.docker.internal` behavior.
pub async fn start_docker_bridge_proxy(target: SocketAddr) -> Result<Option<SocketAddr>> {
    let Some(gateway) = bindable_docker_bridge_gateway().await else {
        return Ok(None);
    };
    let listen_addr = SocketAddr::new(IpAddr::V4(gateway), target.port());
    let listener = TcpListener::bind(listen_addr)
        .await
        .into_alien_error()
        .context(ErrorData::LocalProcessError {
            process_id: "docker-bridge-proxy".to_string(),
            operation: "bind".to_string(),
            reason: format!("Failed to bind Docker host gateway {listen_addr}"),
        })?;

    tokio::spawn(serve_proxy(listener, target));
    info!(%listen_addr, %target, "Docker bridge proxy started");
    Ok(Some(listen_addr))
}

async fn serve_proxy(listener: TcpListener, target: SocketAddr) {
    loop {
        let (downstream, peer) = match listener.accept().await {
            Ok(connection) => connection,
            Err(error) => {
                warn!(%error, "Docker bridge proxy failed to accept a connection");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };

        tokio::spawn(async move {
            if let Err(error) = proxy_connection(downstream, target).await {
                debug!(%error, %peer, %target, "Docker bridge proxy connection closed with an error");
            }
        });
    }
}

async fn proxy_connection(mut downstream: TcpStream, target: SocketAddr) -> std::io::Result<()> {
    let mut upstream = TcpStream::connect(target).await?;
    copy_bidirectional(&mut downstream, &mut upstream).await?;
    Ok(())
}

fn docker_bridge_gateway(gateway: &str) -> Option<Ipv4Addr> {
    let gateway = gateway.parse::<Ipv4Addr>().ok()?;
    let [first, second, ..] = gateway.octets();
    (first == 172 && (16..=31).contains(&second)).then_some(gateway)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docker_bridge_gateway_accepts_only_the_service_private_range() {
        assert_eq!(
            docker_bridge_gateway("172.17.0.1"),
            Some(Ipv4Addr::new(172, 17, 0, 1))
        );
        assert_eq!(
            docker_bridge_gateway("172.16.0.1"),
            Some(Ipv4Addr::new(172, 16, 0, 1))
        );
        assert_eq!(
            docker_bridge_gateway("172.31.255.254"),
            Some(Ipv4Addr::new(172, 31, 255, 254))
        );
        for rejected in [
            "172.15.0.1",
            "172.32.0.1",
            "10.0.0.1",
            "192.168.0.1",
            "8.8.8.8",
            "fd00::1",
            "not-an-ip",
        ] {
            assert_eq!(docker_bridge_gateway(rejected), None, "{rejected}");
        }
    }

    #[tokio::test]
    async fn proxy_preserves_bidirectional_tcp_streams() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind upstream");
        let upstream_addr = upstream.local_addr().expect("upstream address");
        let upstream_task = tokio::spawn(async move {
            let (mut stream, _) = upstream.accept().await.expect("accept upstream");
            let mut bytes = [0_u8; 4];
            stream.read_exact(&mut bytes).await.expect("read request");
            assert_eq!(&bytes, b"ping");
            stream.write_all(b"pong").await.expect("write response");
        });

        let proxy = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind proxy");
        let proxy_addr = proxy.local_addr().expect("proxy address");
        let proxy_task = tokio::spawn(serve_proxy(proxy, upstream_addr));

        let mut client = TcpStream::connect(proxy_addr).await.expect("connect proxy");
        client.write_all(b"ping").await.expect("write request");
        let mut response = [0_u8; 4];
        client
            .read_exact(&mut response)
            .await
            .expect("read response");
        assert_eq!(&response, b"pong");
        upstream_task.await.expect("upstream task");
        proxy_task.abort();
        proxy_task
            .await
            .expect_err("proxy task should be cancelled");
    }
}
