//! A loopback forwarder that keeps a VNet sandbox's egress up while one gateway replica is down.
//!
//! Azure sandboxes reach the egress gateway through proxy environment variables, and the gateway
//! runs as two VMs at fixed addresses (no load balancer). A proxy variable names one address, so
//! the agent listens on loopback and, for each new connection, dials the replicas in turn: the
//! last one that worked first, then the other, with a connect timeout, so a stopped, rebooting or
//! blackholed replica costs at most one timeout and the next connection goes straight to the
//! healthy one. A CONNECT that a replica accepts and then closes or resets before any answer
//! byte is replayed once on the other replica (see `relay_connect`). Every other byte is relayed
//! unchanged; the forwarder adds no credentials (the proxy token is in the command's own
//! environment).

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{copy_bidirectional, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// The gateway replicas' proxy addresses, comma separated (`ip:port,ip:port`).
pub const ENV_PROXY_REPLICAS: &str = "ALIEN_SANDBOX_PROXY_REPLICAS";
/// The loopback address commands' proxy variables point at.
pub const ENV_PROXY_LISTEN: &str = "ALIEN_SANDBOX_PROXY_LISTEN";

/// The gateway's first connect can take 7-20 s, but a replica that is stopped or blackholed never
/// answers: give each replica a few seconds, then try the other, for two passes.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const PASSES: usize = 2;

/// Failover is by TCP connect, plus one replay of a CONNECT that got no answer byte (see
/// `relay_connect`). It does not retry refusals, on purpose. A replica that is up but has not
/// synchronized a grant yet (a restarted one, or one a new sandbox's grant has not reached)
/// answers `407`/`403` and refuses, which is the fail-closed answer, never a wider one: both
/// replicas enforce the same signed policy and a missing grant only ever denies. An answer byte
/// means the replica spoke, so that answer stands. The sticky preference keeps traffic on the
/// replica that last worked, and the startup gate (which passes when either replica connects)
/// then does not hold the workload back for a replica that is still syncing.
///
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
        self.dial_index(None).await.map(|(stream, _)| stream)
    }

    /// Like `dial`, with the winner's index so a caller can demote it. `avoid` (the replica a
    /// CONNECT just failed on) is tried only after every other replica, in each pass: the shared
    /// preference can point back at it (another connection that reached it set that), and a
    /// replay must go to the peer whenever the peer accepts a connection.
    async fn dial_index(&self, avoid: Option<usize>) -> Option<(TcpStream, usize)> {
        let count = self.addresses.len();
        if count == 0 {
            return None;
        }
        let first = self.preferred.load(Ordering::Relaxed) % count;
        let mut order: Vec<usize> = (0..count).map(|offset| (first + offset) % count).collect();
        if let Some(avoid) = avoid {
            order.retain(|index| *index != avoid);
            order.push(avoid);
        }
        for _ in 0..PASSES {
            for &index in &order {
                let attempt = tokio::time::timeout(
                    self.timeout,
                    TcpStream::connect(self.addresses[index].as_str()),
                )
                .await;
                if let Ok(Ok(stream)) = attempt {
                    self.preferred.store(index, Ordering::Relaxed);
                    return Some((stream, index));
                }
            }
        }
        None
    }
}

/// A replica that accepted the connection but closed or reset it before one response byte (it is
/// stopping, or its proxy is not ready): the other replica goes first from now on.
fn demote(replicas: &Replicas, index: usize) {
    let count = replicas.addresses.len().max(1);
    replicas
        .preferred
        .store((index + 1) % count, Ordering::Relaxed);
}

/// The most of a CONNECT request kept for the one replay. Past this, a replica that closes
/// without answering is not retried.
const REPLAY_LIMIT: usize = 32 * 1024;

async fn relay(replicas: Arc<Replicas>, mut guest: TcpStream) {
    let mut first = vec![0u8; 8192];
    let read = match guest.read(&mut first).await {
        Ok(read) if read > 0 => read,
        _ => return,
    };
    first.truncate(read);
    let gateway = if first.starts_with(b"CONNECT ") {
        // The buffers of the handshake are gone before the tunnel starts.
        let Some(gateway) = connect_handshake(&replicas, &mut guest, first).await else {
            return;
        };
        gateway
    } else {
        // Anything else (a plain proxied request, or a CONNECT whose first read is shorter than
        // `CONNECT `) is never replayed: it may carry a body.
        let Some(mut gateway) = replicas.dial().await else {
            return;
        };
        if gateway.write_all(&first).await.is_err() {
            return;
        }
        gateway
    };
    let mut gateway = gateway;
    let _ = copy_bidirectional(&mut guest, &mut gateway).await;
}

/// A CONNECT is replayed once on the other replica when the first closes or resets before one
/// answer byte: nothing was tunnelled, so the other replica sees the same request. Guest bytes
/// keep flowing to the replica while waiting for the answer (a request head can arrive in many
/// writes), and a copy of up to `REPLAY_LIMIT` bytes is kept for the replay.
///
/// The replay goes to the other replica whenever it accepts a connection (`dial_index(avoid)`).
/// The limit on connections per identity is per replica, so a sandbox whose connections get
/// replayed can reach up to twice its limit: accepted, because the alternative is a failed
/// connection during every rolling replacement.
///
/// Returns the replica that answered once its first answer bytes are written to the guest;
/// the caller relays the rest. The copy and the read buffers end here, not with the tunnel.
async fn connect_handshake(
    replicas: &Replicas,
    guest: &mut TcpStream,
    first: Vec<u8>,
) -> Option<TcpStream> {
    let mut copy = Some(first);
    let mut guest_eof = false;
    let mut chunk = vec![0u8; 8192];
    let mut answer = vec![0u8; 8192];
    let mut avoid = None;
    for _ in 0..2 {
        let (mut gateway, index) = replicas.dial_index(avoid).await?;
        let mut failed = match &copy {
            Some(bytes) => gateway.write_all(bytes).await.is_err(),
            None => false,
        };
        if !failed && guest_eof {
            let _ = gateway.shutdown().await;
        }
        while !failed {
            tokio::select! {
                read = guest.read(&mut chunk), if !guest_eof => match read {
                    Ok(0) => {
                        guest_eof = true;
                        let _ = gateway.shutdown().await;
                    }
                    Err(_) => return None,
                    Ok(read) => {
                        if let Some(bytes) = copy.as_mut() {
                            if bytes.len() + read > REPLAY_LIMIT {
                                copy = None;
                            } else {
                                bytes.extend_from_slice(&chunk[..read]);
                            }
                        }
                        failed = gateway.write_all(&chunk[..read]).await.is_err();
                    }
                },
                read = gateway.read(&mut answer) => match read {
                    Ok(read) if read > 0 => {
                        return guest.write_all(&answer[..read]).await.ok().map(|_| gateway);
                    }
                    _ => failed = true,
                },
            }
        }
        demote(replicas, index);
        avoid = Some(index);
        copy.as_ref()?;
    }
    None
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
        let got = through(
            vec!["192.0.2.1:3128".into(), good],
            Duration::from_millis(300),
        )
        .await;
        assert_eq!(got.as_deref(), Some(&b"hello"[..]));
    }

    #[tokio::test]
    async fn the_last_good_replica_is_tried_first_next_time() {
        let good = echo_server().await;
        let replicas = Replicas::new(
            vec!["192.0.2.1:3128".into(), good],
            Duration::from_millis(300),
        );
        assert!(replicas.dial().await.is_some());
        let started = std::time::Instant::now();
        assert!(replicas.dial().await.is_some());
        assert!(
            started.elapsed() < Duration::from_millis(250),
            "the second dial skipped the dead replica"
        );
    }

    /// Accepts, reads the request, and closes without a byte back.
    async fn closing_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buffer = [0u8; 64];
                let _ = stream.read(&mut buffer).await;
            }
        });
        address
    }

    async fn send(replicas: Vec<String>, request: &[u8]) -> usize {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local = listener.local_addr().unwrap();
        tokio::spawn(serve(
            listener,
            Arc::new(Replicas::new(replicas, Duration::from_millis(300))),
        ));
        let mut client = TcpStream::connect(local).await.unwrap();
        client.write_all(request).await.unwrap();
        let mut buffer = [0u8; 64];
        tokio::time::timeout(Duration::from_secs(5), client.read(&mut buffer))
            .await
            .unwrap()
            .unwrap_or(0)
    }

    #[tokio::test]
    async fn a_connect_closed_before_any_byte_goes_to_the_other_replica() {
        let good = echo_server().await;
        let got = send(
            vec![closing_server().await, good],
            b"CONNECT a:443 HTTP/1.1\r\n\r\n",
        )
        .await;
        assert!(got > 0);
    }

    #[tokio::test]
    async fn a_non_connect_request_is_not_replayed() {
        let good = echo_server().await;
        let got = send(
            vec![closing_server().await, good],
            b"POST http://a/ HTTP/1.1\r\n\r\n",
        )
        .await;
        assert_eq!(got, 0);
    }

    /// What a scripted replica does once it has the whole request.
    #[derive(Clone, Copy)]
    enum Reply {
        Answer,
        Close,
        OneByteThenClose,
    }

    /// A replica that reads a full request (head, and a body when `Content-Length` says so),
    /// then acts. Returns its address and how many connections it got.
    async fn scripted(reply: Reply) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let mut got = Vec::new();
                    let mut buffer = [0u8; 4096];
                    loop {
                        if let Some(end) = got.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&got[..end]).to_lowercase();
                            let length = head
                                .split("content-length: ")
                                .nth(1)
                                .and_then(|rest| rest.split("\r\n").next())
                                .and_then(|value| value.parse::<usize>().ok())
                                .unwrap_or(0);
                            if got.len() >= end + 4 + length {
                                break;
                            }
                        }
                        match stream.read(&mut buffer).await {
                            Ok(0) | Err(_) => return,
                            Ok(read) => got.extend_from_slice(&buffer[..read]),
                        }
                    }
                    match reply {
                        Reply::Answer => {
                            // Bytes that follow the head at once (a ClientHello) count too.
                            while let Ok(Ok(read)) = tokio::time::timeout(
                                Duration::from_millis(200),
                                stream.read(&mut buffer),
                            )
                            .await
                            {
                                if read == 0 {
                                    break;
                                }
                                got.extend_from_slice(&buffer[..read]);
                            }
                            let _ = stream
                                .write_all(format!("ANSWER {}", got.len()).as_bytes())
                                .await;
                            let mut rest = Vec::new();
                            let _ = stream.read_to_end(&mut rest).await;
                        }
                        Reply::Close => {}
                        Reply::OneByteThenClose => {
                            let _ = stream.write_all(b"X").await;
                        }
                    }
                });
            }
        });
        (address, hits)
    }

    /// Sends the writes one after another (with a pause between) and reads until the forwarder
    /// closes or 3 seconds pass.
    async fn exchange(replicas: Vec<String>, writes: Vec<Vec<u8>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local = listener.local_addr().unwrap();
        tokio::spawn(serve(
            listener,
            Arc::new(Replicas::new(replicas, Duration::from_millis(300))),
        ));
        let mut client = TcpStream::connect(local).await.unwrap();
        for write in writes {
            client.write_all(&write).await.unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let mut got = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(3), client.read_to_end(&mut got)).await;
        String::from_utf8_lossy(&got).to_string()
    }

    #[tokio::test]
    async fn a_connect_head_in_three_writes_reaches_the_replica_whole() {
        let (good, _) = scripted(Reply::Answer).await;
        let got = exchange(
            vec![good],
            vec![
                b"CONNECT a:443 HTTP/1.1\r\n".to_vec(),
                b"Host: a:443\r\n".to_vec(),
                b"\r\n".to_vec(),
            ],
        )
        .await;
        assert!(got.starts_with("ANSWER 39"), "{got}");
    }

    #[tokio::test]
    async fn a_connect_head_of_20_kib_reaches_the_replica_whole() {
        let (good, _) = scripted(Reply::Answer).await;
        let head = format!(
            "CONNECT a:443 HTTP/1.1\r\nX-Pad: {}\r\n\r\n",
            "p".repeat(20 * 1024)
        );
        let want = format!("ANSWER {}", head.len());
        let got = exchange(vec![good], vec![head.into_bytes()]).await;
        assert_eq!(got, want);
    }

    #[tokio::test]
    async fn a_post_with_its_body_in_a_second_write_is_relayed_as_before() {
        let (good, _) = scripted(Reply::Answer).await;
        let head = b"POST http://a/ HTTP/1.1\r\nContent-Length: 5\r\n\r\n".to_vec();
        let want = format!("ANSWER {}", head.len() + 5);
        let got = exchange(vec![good], vec![head, b"hello".to_vec()]).await;
        assert_eq!(got, want);
    }

    #[tokio::test]
    async fn a_split_connect_head_is_replayed_whole_on_the_other_replica() {
        let (bad, bad_hits) = scripted(Reply::Close).await;
        let (good, good_hits) = scripted(Reply::Answer).await;
        let got = exchange(
            vec![bad, good],
            vec![
                b"CONNECT a:443 HTTP/1.1\r\n".to_vec(),
                b"Host: a:443\r\n".to_vec(),
                b"\r\n".to_vec(),
            ],
        )
        .await;
        assert!(got.starts_with("ANSWER 39"), "{got}");
        assert_eq!(bad_hits.load(Ordering::SeqCst), 1);
        assert_eq!(good_hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn one_answer_byte_then_a_close_is_not_replayed() {
        let (first, first_hits) = scripted(Reply::OneByteThenClose).await;
        let (second, second_hits) = scripted(Reply::Answer).await;
        let got = exchange(
            vec![first, second],
            vec![b"CONNECT a:443 HTTP/1.1\r\n\r\n".to_vec()],
        )
        .await;
        assert_eq!(got, "X");
        assert_eq!(first_hits.load(Ordering::SeqCst), 1);
        assert_eq!(second_hits.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn when_both_replicas_close_the_connection_closes_after_one_replay() {
        let (first, first_hits) = scripted(Reply::Close).await;
        let (second, second_hits) = scripted(Reply::Close).await;
        let got = exchange(
            vec![first, second],
            vec![b"CONNECT a:443 HTTP/1.1\r\n\r\n".to_vec()],
        )
        .await;
        assert_eq!(got, "");
        assert_eq!(first_hits.load(Ordering::SeqCst), 1);
        assert_eq!(second_hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_connect_head_past_the_replay_limit_is_not_replayed() {
        let (first, first_hits) = scripted(Reply::Close).await;
        let (second, second_hits) = scripted(Reply::Answer).await;
        let head = format!(
            "CONNECT a:443 HTTP/1.1\r\nX-Pad: {}\r\n\r\n",
            "p".repeat(40 * 1024)
        );
        let got = exchange(vec![first, second], vec![head.into_bytes()]).await;
        assert_eq!(got, "");
        assert_eq!(first_hits.load(Ordering::SeqCst), 1);
        assert_eq!(second_hits.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_first_read_shorter_than_connect_is_relayed_without_replay() {
        let (first, first_hits) = scripted(Reply::Close).await;
        let (second, second_hits) = scripted(Reply::Answer).await;
        let got = exchange(
            vec![first, second],
            vec![b"CON".to_vec(), b"NECT a:443 HTTP/1.1\r\n\r\n".to_vec()],
        )
        .await;
        assert_eq!(got, "");
        assert_eq!(first_hits.load(Ordering::SeqCst), 1);
        assert_eq!(second_hits.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_client_hello_right_after_the_head_is_replayed_with_it() {
        let (bad, _) = scripted(Reply::Close).await;
        let (good, good_hits) = scripted(Reply::Answer).await;
        let head = b"CONNECT a:443 HTTP/1.1\r\n\r\n".to_vec();
        let hello = vec![0x16, 0x03, 0x01, 0x00, 0x05, 1, 2, 3, 4, 5];
        let want = format!("ANSWER {}", head.len() + hello.len());
        let got = exchange(vec![bad, good], vec![head, hello]).await;
        assert_eq!(got, want);
        assert_eq!(good_hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_connect_head_just_under_the_replay_limit_is_replayed() {
        let (first, _) = scripted(Reply::Close).await;
        let (second, second_hits) = scripted(Reply::Answer).await;
        let head = format!(
            "CONNECT a:443 HTTP/1.1\r\nX-Pad: {}\r\n\r\n",
            "p".repeat(REPLAY_LIMIT - 100)
        );
        assert!(head.len() < REPLAY_LIMIT);
        let want = format!("ANSWER {}", head.len());
        let got = exchange(vec![first, second], vec![head.into_bytes()]).await;
        assert_eq!(got, want);
        assert_eq!(second_hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn the_replay_avoids_the_replica_that_just_failed_even_when_the_preference_points_back() {
        let (first, first_hits) = scripted(Reply::Close).await;
        let (second, _) = scripted(Reply::Answer).await;
        let replicas = Replicas::new(vec![first, second], Duration::from_millis(300));
        // Another connection reached replica 0 and set the shared preference back to it.
        replicas.preferred.store(0, Ordering::Relaxed);
        let (_stream, index) = replicas.dial_index(Some(0)).await.unwrap();
        assert_eq!(index, 1);
        assert_eq!(first_hits.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn the_replay_falls_back_to_the_failed_replica_when_the_peer_refuses() {
        let (first, _) = scripted(Reply::Answer).await;
        let replicas = Replicas::new(vec![first, refused()], Duration::from_millis(300));
        let (_stream, index) = replicas.dial_index(Some(0)).await.unwrap();
        assert_eq!(index, 0);
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
