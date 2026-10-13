//! Shared HTTP client settings for every request the CLI makes.
//!
//! Some networks silently drop the SYN of a new TCP connection. Without a
//! connect timeout the CLI then waits for the operating system's SYN
//! retransmits (on macOS the connect finally succeeds or fails after 7, 19 or
//! 35 seconds). With [`CONNECT_TIMEOUT`] the attempt fails fast instead: when a
//! host resolves to several addresses the timeout is split across them, so the
//! connector moves on to the next address, and a connect that still fails is
//! retried on a fresh connection.
//!
//! Only connect-phase failures are retried. Nothing has reached the server at
//! that point, so retrying is safe for every method, including POST. Failures
//! after the connection is up (timeouts, resets, error statuses) are returned
//! to the caller unchanged.

use std::{error::Error as StdError, time::Duration};

use alien_error::{Context, IntoAlienError};
use reqwest::{retry, Client, ClientBuilder};

use crate::error::{ErrorData, Result};

/// Upper bound for establishing a connection (TCP and TLS), separate from any
/// overall request timeout a caller sets.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Extra attempts after a failed connect, on top of the first attempt.
const MAX_CONNECT_RETRIES: u32 = 2;

/// A `reqwest` client builder with the CLI's connect timeout and connect
/// retry. Every CLI HTTP client should start from this.
pub fn builder() -> ClientBuilder {
    builder_with_connect_timeout(CONNECT_TIMEOUT)
}

/// A client with the CLI defaults and nothing else configured.
pub fn client() -> Result<Client> {
    builder()
        .build()
        .into_alien_error()
        .context(ErrorData::NetworkError {
            message: "Failed to create HTTP client".to_string(),
        })
}

fn builder_with_connect_timeout(connect_timeout: Duration) -> ClientBuilder {
    Client::builder()
        .connect_timeout(connect_timeout)
        .retry(connect_retry_policy())
}

fn connect_retry_policy() -> retry::Builder {
    // The policy applies to every host the client talks to.
    retry::for_host(AnyHost)
        .max_retries_per_request(MAX_CONNECT_RETRIES)
        .classify_fn(|req_rep| {
            let retryable = req_rep
                .error()
                .is_some_and(|error| is_connect_failure(error) || is_http2_refusal(error));
            if retryable {
                req_rep.retryable()
            } else {
                req_rep.success()
            }
        })
}

/// Matches every host, for [`retry::for_host`].
struct AnyHost;

impl PartialEq<&str> for AnyHost {
    fn eq(&self, _host: &&str) -> bool {
        true
    }
}

fn is_connect_failure(error: &(dyn StdError + 'static)) -> bool {
    error
        .downcast_ref::<reqwest::Error>()
        .is_some_and(reqwest::Error::is_connect)
}

/// HTTP/2 signals that the server did not process the request: a graceful
/// GOAWAY or a REFUSED_STREAM reset. `reqwest` retries these by default;
/// installing a custom policy replaces that default, so it is kept here.
fn is_http2_refusal(error: &(dyn StdError + 'static)) -> bool {
    let mut source = error.source();
    while let Some(cause) = source {
        if let Some(h2_error) = cause.downcast_ref::<h2::Error>() {
            let graceful_goaway = h2_error.is_go_away()
                && h2_error.is_remote()
                && h2_error.reason() == Some(h2::Reason::NO_ERROR);
            let refused_stream = h2_error.is_reset()
                && h2_error.is_remote()
                && h2_error.reason() == Some(h2::Reason::REFUSED_STREAM);
            return graceful_goaway || refused_stream;
        }
        source = cause.source();
    }
    false
}

#[cfg(test)]
mod tests {
    use std::{
        net::SocketAddr,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::Instant,
    };

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpSocket, TcpStream},
    };

    use super::*;

    /// A listening socket whose accept queue is full, so the kernel drops
    /// every new SYN: the same symptom as a network that silently drops a
    /// connection. Returns the listener (accepting from it drains the queue)
    /// and the connections that fill the queue.
    async fn syn_dropping_listener() -> (TcpListener, SocketAddr, Vec<TcpStream>) {
        let socket = TcpSocket::new_v4().expect("socket");
        socket
            .bind("127.0.0.1:0".parse().unwrap())
            .expect("bind loopback");
        let listener = socket.listen(1).expect("listen");
        let addr = listener.local_addr().expect("local addr");

        let mut fillers = Vec::new();
        loop {
            match tokio::time::timeout(Duration::from_millis(300), TcpStream::connect(addr)).await {
                Ok(Ok(stream)) => fillers.push(stream),
                Ok(Err(error)) => panic!("filler connect failed: {error}"),
                // A connect that hangs means the queue is full and SYNs are dropped.
                Err(_) => break,
            }
            assert!(fillers.len() < 64, "accept queue never filled up");
        }
        (listener, addr, fillers)
    }

    #[tokio::test]
    async fn the_cli_client_gives_up_on_a_host_that_never_answers_the_connect() {
        let (_listener, addr, _fillers) = syn_dropping_listener().await;
        let client = crate::auth::client_with_header("Bearer test").expect("client");

        let started = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(25),
            client.get(format!("http://{addr}/v1/whoami")).send(),
        )
        .await
        .expect("request must not hang until the OS connect timeout");

        let elapsed = started.elapsed();
        let error = result.expect_err("nothing answers the connect");
        assert!(
            error.is_connect(),
            "expected a connect error, got {error:?}"
        );
        let attempts = MAX_CONNECT_RETRIES + 1;
        assert!(
            elapsed >= CONNECT_TIMEOUT * attempts - Duration::from_millis(500),
            "each attempt should wait the full connect timeout, took {elapsed:?}"
        );
        assert!(
            elapsed < CONNECT_TIMEOUT * attempts + Duration::from_secs(3),
            "should fail after {attempts} connect timeouts, took {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn a_failed_connect_is_retried_and_the_request_body_is_sent_once() {
        let (listener, addr, fillers) = syn_dropping_listener().await;
        let requests = Arc::new(AtomicUsize::new(0));
        let connect_timeout = Duration::from_millis(200);

        // The server starts answering after the client's first connect has
        // already timed out. Without the retry the kernel would only resend
        // the first SYN after its 1 s initial retransmission timeout, so a
        // response well before 1 s shows a new connect attempt got through.
        let server_requests = Arc::clone(&requests);
        let server = tokio::spawn(async move {
            tokio::time::sleep(connect_timeout + Duration::from_millis(50)).await;
            drop(fillers);
            loop {
                let (mut stream, _) = listener.accept().await.expect("accept");
                let requests = Arc::clone(&server_requests);
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let mut received = Vec::new();
                    while !received.ends_with(b"payload") {
                        let n = stream.read(&mut buf).await.expect("read");
                        if n == 0 {
                            // A filler connection closed by the client side.
                            return;
                        }
                        received.extend_from_slice(&buf[..n]);
                    }
                    requests.fetch_add(1, Ordering::SeqCst);
                    stream
                        .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok")
                        .await
                        .expect("write");
                });
            }
        });

        let client = builder_with_connect_timeout(connect_timeout)
            .build()
            .expect("client");
        let started = Instant::now();
        let response = tokio::time::timeout(
            Duration::from_secs(10),
            client
                .post(format!("http://{addr}/v1/things"))
                .body("payload")
                .send(),
        )
        .await
        .expect("request should finish")
        .expect("a retried connect should reach the server");

        let elapsed = started.elapsed();
        assert_eq!(response.status(), 200);
        assert_eq!(response.text().await.expect("body"), "ok");
        assert!(
            elapsed >= connect_timeout,
            "the first connect should have timed out, took {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_millis(900),
            "a retried connect should answer before the kernel's SYN retransmit, took {elapsed:?}"
        );
        assert_eq!(requests.load(Ordering::SeqCst), 1, "POST body sent once");
        server.abort();
    }

    #[tokio::test]
    async fn the_cli_client_moves_to_the_next_address_when_one_never_answers() {
        // A host behind a load balancer resolves to several addresses; on the
        // affected networks a connect to one of them gets no answer.
        let (_dead_listener, dead, _fillers) = syn_dropping_listener().await;
        let live_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let live = live_listener.local_addr().expect("local addr");
        let server = tokio::spawn(async move {
            let (mut stream, _) = live_listener.accept().await.expect("accept");
            let mut buf = vec![0u8; 4096];
            let mut received = Vec::new();
            while !received.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buf).await.expect("read");
                assert!(n > 0, "client closed before sending the request");
                received.extend_from_slice(&buf[..n]);
            }
            stream
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok")
                .await
                .expect("write");
        });

        let client = builder()
            .resolve_to_addrs("api.multi-address.test", &[dead, live])
            .build()
            .expect("client");
        let started = Instant::now();
        let response = tokio::time::timeout(
            Duration::from_secs(25),
            client.get("http://api.multi-address.test/health").send(),
        )
        .await
        .expect("request must not wait for the OS connect timeout on the dead address")
        .expect("the live address should answer");

        let elapsed = started.elapsed();
        assert_eq!(response.status(), 200);
        assert_eq!(response.text().await.expect("body"), "ok");
        // The connect timeout is split across the two addresses.
        assert!(
            elapsed >= CONNECT_TIMEOUT / 2 - Duration::from_millis(500),
            "took {elapsed:?}"
        );
        assert!(elapsed < CONNECT_TIMEOUT, "took {elapsed:?}");
        server.await.expect("server task");
    }

    #[tokio::test]
    async fn a_failure_after_the_connection_is_up_is_not_retried() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let connections = Arc::new(AtomicUsize::new(0));
        let server_connections = Arc::clone(&connections);
        let server = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.expect("accept");
                server_connections.fetch_add(1, Ordering::SeqCst);
                let mut buf = vec![0u8; 4096];
                let _ = stream.read(&mut buf).await;
                // Close without answering: the request may have been processed.
                drop(stream);
            }
        });

        let client = client().expect("client");
        let error = client
            .post(format!("http://{addr}/v1/things"))
            .body("payload")
            .send()
            .await
            .expect_err("the server closed the connection");

        assert!(
            !error.is_connect(),
            "expected a post-connect error, got {error:?}"
        );
        assert_eq!(
            connections.load(Ordering::SeqCst),
            1,
            "no retry after connect"
        );
        server.abort();
    }
}
