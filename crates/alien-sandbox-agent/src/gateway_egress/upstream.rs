//! Opening a forwarded connection to the gateway.
//!
//! Gateway replicas sit behind a load balancer that keeps sending some new connections to a
//! replica that died until its health check removes it. An attempt that no gateway answered is
//! therefore repeated on a new connection, which the balancer routes independently. A repeated
//! attempt never carries request bodies or tunnel bytes, and any gateway answer is final: a
//! gateway authorizes every connection, so a repeated attempt can only reach the same decision.
use std::{future::Future, net::SocketAddr, time::Duration};

use alien_error::{AlienError, Context, IntoAlienError};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    time::timeout,
};

use super::{failed, invalid, read_header, redirect, ErrorData, GatewayEgress, Result};

/// The gateway is an internal endpoint of the sandbox's own network and completes a handshake
/// within milliseconds. Behind a pass-through load balancer, a connection routed to a dead
/// replica never completes it. This is far below the kernel's first SYN retransmission (1 s).
const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
/// The balancer routes each new connection independently, so with one of two replicas dead,
/// every attempt fails for 1 forwarded connection in 4096. With `CONNECT_TIMEOUT`, a forwarded
/// connection still waits less for the gateway than the single 5 s connect it used before.
const ATTEMPTS: u32 = 12;
/// The gateway answers a CONNECT after dialing the destination within its own 5 s budget. This
/// is longer, so a slow destination gets the gateway's answer rather than an agent timeout. An
/// expired answer wait is not repeated: the replica may be alive and still dialing.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);

/// Why one attempt failed.
enum Failure {
    /// No gateway answered: the connect failed or timed out, or the connection ended before any
    /// answer byte. The attempt sent at most a CONNECT head, so it can be repeated.
    Unanswered(AlienError<ErrorData>),
    /// A gateway answered, or the failure is local. Repeating cannot help.
    Final(AlienError<ErrorData>),
}

impl GatewayEgress {
    /// Connect for a request whose client bytes are written once the connection exists. Only
    /// the connect is repeated; after the first write, a failure ends the forwarded connection.
    pub(super) async fn connect_forwarded(&self) -> Result<TcpStream> {
        let address = self.gateway_address()?;
        attempts(|| self.connect_attempt(address)).await
    }

    /// Send a CONNECT request head and wait for the gateway's answer. The head carries no body
    /// and the caller holds tunnel bytes until a gateway answers, so an unanswered attempt is
    /// repeated with the same head. Returns the answer head and the connection positioned
    /// after it.
    pub(super) async fn open_tunnel(
        &self,
        request: &[u8],
    ) -> Result<(Vec<u8>, BufReader<TcpStream>)> {
        let address = self.gateway_address()?;
        attempts(|| self.tunnel_attempt(address, request)).await
    }

    async fn connect_attempt(
        &self,
        address: SocketAddr,
    ) -> std::result::Result<TcpStream, Failure> {
        let socket = redirect::socket(self.privileged)
            .into_alien_error()
            .context(failed("create agent control socket"))
            .map_err(Failure::Final)?;
        timeout(CONNECT_TIMEOUT, socket.connect(address))
            .await
            .into_alien_error()
            .context(failed("gateway connect timeout"))
            .map_err(Failure::Unanswered)?
            .into_alien_error()
            .context(failed("connect gateway"))
            .map_err(Failure::Unanswered)
    }

    async fn tunnel_attempt(
        &self,
        address: SocketAddr,
        request: &[u8],
    ) -> std::result::Result<(Vec<u8>, BufReader<TcpStream>), Failure> {
        let mut gateway = BufReader::new(self.connect_attempt(address).await?);
        let answer = timeout(ANSWER_TIMEOUT, async {
            gateway
                .get_mut()
                .write_all(request)
                .await
                .into_alien_error()
                .context(failed("send gateway CONNECT"))
                .map_err(Failure::Unanswered)?;
            // A replica that dies, or a balancer that gives up on it, ends the connection
            // before the first answer byte.
            let received = gateway
                .fill_buf()
                .await
                .into_alien_error()
                .context(failed("read gateway answer"))
                .map_err(Failure::Unanswered)?;
            if received.is_empty() {
                return Err(Failure::Unanswered(invalid(
                    "gateway closed before answering",
                )));
            }
            read_header(&mut gateway).await.map_err(Failure::Final)
        })
        .await
        .into_alien_error()
        .context(failed("CONNECT response timeout"))
        .map_err(Failure::Final)??;
        Ok((answer, gateway))
    }
}

/// Run attempts until one succeeds, one fails finally, or `ATTEMPTS` went unanswered.
async fn attempts<T, Fut>(mut attempt: impl FnMut() -> Fut) -> Result<T>
where
    Fut: Future<Output = std::result::Result<T, Failure>>,
{
    let mut number = 1;
    loop {
        match attempt().await {
            Ok(value) => return Ok(value),
            Err(Failure::Unanswered(error)) if number < ATTEMPTS => {
                tracing::warn!(attempt = number, code = %error.code, "gateway connection unanswered; retrying on a new connection");
                number += 1;
            }
            Err(Failure::Unanswered(error) | Failure::Final(error)) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    use tokio::{io::AsyncReadExt, net::TcpListener};

    const FORWARDED_CONNECT: &[u8] = b"CONNECT allowed.test:443 HTTP/1.1\r\nHost: allowed.test:443\r\nProxy-Authorization: Bearer token\r\n\r\n";

    struct Fixture {
        gateway: TcpListener,
        client: TcpStream,
        forwarder: tokio::task::JoinHandle<Result<()>>,
    }

    /// A ready agent forwarding one explicit-proxy guest connection to a fake gateway.
    async fn fixture() -> Fixture {
        fixture_with(TcpListener::bind("127.0.0.1:0").await.unwrap()).await
    }

    async fn fixture_with(gateway: TcpListener) -> Fixture {
        let agent = GatewayEgress::for_test(gateway.local_addr().unwrap());
        agent.ready.store(true, Ordering::Release);
        let guests = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = TcpStream::connect(guests.local_addr().unwrap())
            .await
            .unwrap();
        let (guest, _) = guests.accept().await.unwrap();
        let forwarder = tokio::spawn(async move { agent.forward(guest, false).await });
        Fixture {
            gateway,
            client,
            forwarder,
        }
    }

    async fn next_gateway_connection(gateway: &TcpListener) -> BufReader<TcpStream> {
        let (socket, _) = timeout(Duration::from_secs(3), gateway.accept())
            .await
            .expect("the forwarder opened no new gateway connection")
            .unwrap();
        BufReader::new(socket)
    }

    /// End a connection with a reset and no answer, like a replica that died.
    fn die(connection: BufReader<TcpStream>) {
        let socket = connection.into_inner();
        socket.set_zero_linger().unwrap();
        drop(socket);
    }

    async fn assert_no_more_connections(gateway: &TcpListener) {
        assert!(
            timeout(Duration::from_millis(500), gateway.accept())
                .await
                .is_err(),
            "the forwarder opened another gateway connection"
        );
    }

    async fn read_until_closed(client: &mut TcpStream) -> Vec<u8> {
        let mut received = Vec::new();
        // A reset after an unanswered request is as final for the client as a clean close.
        let _closed = timeout(Duration::from_secs(15), client.read_to_end(&mut received))
            .await
            .expect("the forwarder kept the client connection open");
        received
    }

    #[tokio::test]
    async fn connect_that_never_completes_is_reopened_on_a_new_connection() {
        // A listener whose accept queue is full drops new SYNs, like a dead target behind a
        // pass-through balancer. The kernel would retransmit the SYN only after 1 s.
        let socket = tokio::net::TcpSocket::new_v4().unwrap();
        socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let gateway = socket.listen(0).unwrap();
        let _queued = TcpStream::connect(gateway.local_addr().unwrap())
            .await
            .unwrap();
        let Fixture {
            gateway,
            mut client,
            forwarder,
        } = fixture_with(gateway).await;
        let started = std::time::Instant::now();
        client
            .write_all(b"POST http://allowed.test/orders HTTP/1.1\r\nHost: allowed.test\r\nContent-Length: 4\r\n\r\nbody")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        // The replica recovers: the queue drains, so only a new SYN can complete now.
        drop(gateway.accept().await.unwrap());

        let mut live = next_gateway_connection(&gateway).await;
        assert!(
            started.elapsed() < Duration::from_millis(800),
            "the forwarder waited {:?} for a connect that never completed",
            started.elapsed()
        );
        assert_eq!(
            read_header(&mut live).await.unwrap(),
            b"POST http://allowed.test/orders HTTP/1.1\r\nHost: allowed.test\r\nContent-Length: 4\r\nProxy-Authorization: Bearer token\r\n\r\n"
        );
        let mut body = [0; 4];
        live.read_exact(&mut body).await.unwrap();
        assert_eq!(&body, b"body");
        live.write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
        drop(live);

        assert_eq!(
            read_until_closed(&mut client).await,
            b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n"
        );
        drop(client);
        forwarder.await.unwrap().unwrap();
        assert_no_more_connections(&gateway).await;
    }

    #[tokio::test]
    async fn tunnel_unanswered_by_a_dead_replica_is_reopened_on_a_new_connection() {
        let Fixture {
            gateway,
            mut client,
            forwarder,
        } = fixture().await;
        client
            .write_all(b"CONNECT allowed.test:443 HTTP/1.1\r\nHost: allowed.test:443\r\nProxy-Authorization: Bearer guest\r\n\r\nhello")
            .await
            .unwrap();

        let mut dead = next_gateway_connection(&gateway).await;
        assert_eq!(read_header(&mut dead).await.unwrap(), FORWARDED_CONNECT);
        assert!(
            timeout(Duration::from_millis(300), dead.read_u8())
                .await
                .is_err(),
            "client bytes reached a gateway before it answered"
        );
        die(dead);

        let mut live = next_gateway_connection(&gateway).await;
        assert_eq!(read_header(&mut live).await.unwrap(), FORWARDED_CONNECT);
        live.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await
            .unwrap();
        let mut early = [0; 5];
        live.read_exact(&mut early).await.unwrap();
        assert_eq!(&early, b"hello");
        live.write_all(b"world").await.unwrap();
        drop(live);

        assert_eq!(
            read_until_closed(&mut client).await,
            b"HTTP/1.1 200 Connection Established\r\n\r\nworld"
        );
        drop(client);
        forwarder.await.unwrap().unwrap();
        assert_no_more_connections(&gateway).await;
    }

    #[tokio::test]
    async fn gateway_answer_is_final() {
        for answer in [
            b"HTTP/1.1 403 Forbidden\r\nX-Alien-Egress-Reason: policy\r\nConnection: close\r\nContent-Length: 0\r\n\r\n".as_slice(),
            b"HTTP/1.1 407 Proxy Authentication Required\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
            b"HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        ] {
            let Fixture {
                gateway,
                mut client,
                forwarder,
            } = fixture().await;
            client
                .write_all(b"CONNECT allowed.test:443 HTTP/1.1\r\nHost: allowed.test:443\r\n\r\n")
                .await
                .unwrap();
            let mut refusing = next_gateway_connection(&gateway).await;
            assert_eq!(read_header(&mut refusing).await.unwrap(), FORWARDED_CONNECT);
            refusing.write_all(answer).await.unwrap();
            drop(refusing);

            assert_eq!(read_until_closed(&mut client).await, answer);
            drop(client);
            forwarder.await.unwrap().unwrap();
            assert_no_more_connections(&gateway).await;
        }
    }

    #[tokio::test]
    async fn client_bytes_a_gateway_received_are_never_replayed() {
        let Fixture {
            gateway,
            mut client,
            forwarder,
        } = fixture().await;
        client
            .write_all(b"POST http://allowed.test/orders HTTP/1.1\r\nHost: allowed.test\r\nContent-Length: 4\r\n\r\nbody")
            .await
            .unwrap();

        let mut dead = next_gateway_connection(&gateway).await;
        assert_eq!(
            read_header(&mut dead).await.unwrap(),
            b"POST http://allowed.test/orders HTTP/1.1\r\nHost: allowed.test\r\nContent-Length: 4\r\nProxy-Authorization: Bearer token\r\n\r\n"
        );
        let mut body = [0; 4];
        dead.read_exact(&mut body).await.unwrap();
        assert_eq!(&body, b"body");
        die(dead);

        assert_eq!(read_until_closed(&mut client).await, b"");
        forwarder
            .await
            .unwrap()
            .expect_err("a request a gateway received and never answered must fail");
        assert_no_more_connections(&gateway).await;
    }

    #[tokio::test]
    async fn unanswered_attempts_are_bounded() {
        let Fixture {
            gateway,
            mut client,
            forwarder,
        } = fixture().await;
        client
            .write_all(b"CONNECT allowed.test:443 HTTP/1.1\r\nHost: allowed.test:443\r\n\r\n")
            .await
            .unwrap();
        for _ in 0..ATTEMPTS {
            let mut dead = next_gateway_connection(&gateway).await;
            assert_eq!(read_header(&mut dead).await.unwrap(), FORWARDED_CONNECT);
            die(dead);
        }

        assert_eq!(read_until_closed(&mut client).await, b"");
        forwarder.await.unwrap().expect_err("no gateway answered");
        assert_no_more_connections(&gateway).await;
    }
}
