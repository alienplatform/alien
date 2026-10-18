use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};

use alien_error::{Context, IntoAlienError};
use ed25519_compact::PublicKey;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    task::JoinSet,
    time::timeout,
};

use crate::{
    config::Config,
    error::{denied, invalid, unavailable, Result},
    generation::GenerationFence,
    io::{Counters, Metered, Prefixed},
    policy::{check_addresses, PolicyExt, Target},
    protocol::{read_client_hello, request_target, MAX_HEADER_BYTES},
    token::{self, Claims},
};

/// Maximum time to receive initial request headers or a TLS ClientHello.
pub const HEADER_TIMEOUT: Duration = Duration::from_secs(5);
/// Maximum connection lifetime; limits resource retention by idle peers.
pub const CONNECTION_TIMEOUT: Duration = Duration::from_secs(3600);

/// Shared immutable configuration and generation store.
pub struct Gateway {
    /// Deployment configuration.
    pub config: Config,
    /// Parsed public keys.
    pub keys: HashMap<String, PublicKey>,
    /// Shared monotonic generation fence.
    pub generations: GenerationFence,
    /// Gateway-only CA and vault access.
    pub credentials: crate::credentials::Credentials,
    /// Total accepted proxy connections.
    pub accepted: std::sync::atomic::AtomicU64,
    /// Total denied or failed proxy connections.
    pub denied: std::sync::atomic::AtomicU64,
}

impl Gateway {
    /// Authorize against the independently configured bound and the signed grant.
    pub async fn authorize(&self, claims: &Claims, target: &Target) -> Result<()> {
        let bound = self
            .config
            .bounds
            .get(&claims.aud)
            .ok_or_else(|| denied("unknown audience"))?;
        let policy = self
            .generations
            .resolve(&claims.aud, &claims.sub, claims.gen, &claims.policy)
            .await?;
        if !bound.intersection_permits(&policy, target) {
            return Err(alien_error::AlienError::new(
                crate::error::ErrorData::PolicyDenied,
            ));
        }
        Ok(())
    }

    /// Resolve once, validate every returned address, then dial a validated socket address.
    /// The connect operation cannot resolve the name a second time.
    pub async fn connect(&self, target: &Target) -> Result<TcpStream> {
        let addresses: Vec<_> = timeout(
            HEADER_TIMEOUT,
            tokio::net::lookup_host((target.host.as_str(), target.port)),
        )
        .await
        .into_alien_error()
        .context(unavailable("DNS timeout"))?
        .into_alien_error()
        .context(unavailable("resolve destination"))?
        .filter(|address| address.is_ipv4())
        .collect();
        check_addresses(
            &addresses.iter().map(SocketAddr::ip).collect::<Vec<_>>(),
            &self.config.denied_ranges,
        )?;
        // Address failover is delegated to TcpStream, with only the validated addresses supplied.
        timeout(HEADER_TIMEOUT, TcpStream::connect(addresses.as_slice()))
            .await
            .into_alien_error()
            .context(unavailable("destination timeout"))?
            .into_alien_error()
            .context(unavailable("connect destination"))
    }
}

/// Serve proxy connections with bounded concurrency and supervised connection tasks.
pub async fn serve(listener: TcpListener, gateway: Arc<Gateway>) -> Result<()> {
    let limit = Arc::new(Semaphore::new(1024));
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted.into_alien_error().context(unavailable("accept proxy connection"))?;
                let permit = match limit.clone().try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => {
                        gateway.accepted.fetch_add(1, Ordering::Relaxed);
                        gateway.denied.fetch_add(1, Ordering::Relaxed);
                        tracing::info!(ts = %chrono::Utc::now().to_rfc3339(), aud = "", sub = "",
                            host = "", port = 0, decision = "deny", reason = "connection capacity",
                            bytesUp = 0, bytesDown = 0, durationMs = 0, "egress connection");
                        drop(stream);
                        continue;
                    }
                };
                let gateway = gateway.clone();
                tasks.spawn(async move { let _permit = permit; connection(stream, gateway).await; });
            }
            result = tasks.join_next(), if !tasks.is_empty() => {
                if let Some(Err(error)) = result { tracing::error!(%error, "proxy task failed"); }
            }
        }
    }
}

#[derive(Default)]
struct Audit {
    aud: String,
    sub: String,
    host: String,
    port: u16,
}

async fn connection(stream: TcpStream, gateway: Arc<Gateway>) {
    gateway.accepted.fetch_add(1, Ordering::Relaxed);
    let counters = Arc::new(Counters::default());
    let mut stream = Metered {
        inner: stream,
        counters: counters.clone(),
    };
    let started = Instant::now();
    let mut audit = Audit::default();
    let result = timeout(
        CONNECTION_TIMEOUT,
        proxy(&mut stream, gateway.clone(), &mut audit),
    )
    .await;
    let reason = match result {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error.message),
        Err(_) => Some("connection timeout".into()),
    };
    if reason.is_some() {
        gateway.denied.fetch_add(1, Ordering::Relaxed);
    }
    // Fixed fields only: never include tokens, headers, URLs with queries, or secret values.
    tracing::info!(ts = %chrono::Utc::now().to_rfc3339(), aud = %audit.aud, sub = %audit.sub,
        host = %audit.host, port = audit.port, decision = if reason.is_none() { "allow" } else { "deny" },
        reason = reason.as_deref().unwrap_or("permitted"), bytesUp = counters.up.load(Ordering::Relaxed),
        bytesDown = counters.down.load(Ordering::Relaxed), durationMs = started.elapsed().as_millis() as u64,
        "egress connection");
}

async fn proxy(
    stream: &mut Metered<TcpStream>,
    gateway: Arc<Gateway>,
    audit: &mut Audit,
) -> Result<()> {
    let input = timeout(HEADER_TIMEOUT, read_head(stream))
        .await
        .into_alien_error()
        .context(unavailable("request header timeout"))??;
    let checked = inspect_head(&input);
    let (target, bearer, connect, head_len) = match checked {
        Ok(head) => head,
        Err(error) => {
            refuse(stream, error.code == "EGRESS_POLICY_DENIED").await?;
            return Err(error);
        }
    };
    audit.host = target.host.clone();
    audit.port = target.port;
    let claims = match token::verify(
        &bearer,
        &gateway.keys,
        chrono::Utc::now().timestamp().max(0) as u64,
    ) {
        Ok(claims) => claims,
        Err(error) => {
            refuse(stream, error.code == "EGRESS_POLICY_DENIED").await?;
            return Err(error);
        }
    };
    audit.aud = claims.aud.clone();
    audit.sub = claims.sub.clone();
    if let Err(error) = gateway.authorize(&claims, &target).await {
        refuse(stream, error.code == "EGRESS_POLICY_DENIED").await?;
        return Err(error);
    }
    if !connect {
        let upstream = gateway.connect(&target).await?;
        return crate::http_proxy::forward(Prefixed::new(input, stream), upstream, target, None)
            .await;
    }
    // Resolve before acknowledging, so denied ranges receive an explicit gateway denial.
    let mut upstream = match gateway.connect(&target).await {
        Ok(stream) => stream,
        Err(error) => {
            refuse(stream, error.code == "EGRESS_POLICY_DENIED").await?;
            return Err(error);
        }
    };
    stream
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await
        .into_alien_error()
        .context(unavailable("acknowledge CONNECT"))?;
    let mut guest = Prefixed::new(input[head_len..].to_vec(), stream);
    let rules: Vec<_> = gateway
        .config
        .credentials
        .iter()
        .filter(|rule| {
            rule.aud == claims.aud && rule.host == target.host && rule.port == target.port
        })
        .cloned()
        .collect();
    if target.port == 443 || !rules.is_empty() {
        let hello = timeout(HEADER_TIMEOUT, read_client_hello(&mut guest, &target))
            .await
            .into_alien_error()
            .context(unavailable("ClientHello timeout"))??;
        if !rules.is_empty() {
            return gateway
                .credentials
                .forward(
                    Prefixed::new(hello, guest),
                    upstream,
                    (&claims.aud, &claims.sub),
                    target,
                    &rules,
                )
                .await;
        }
        upstream
            .write_all(&hello)
            .await
            .into_alien_error()
            .context(unavailable("forward ClientHello"))?;
    } else if target.port == 80 {
        // CONNECT is not an escape hatch around plain-HTTP Host checks.
        return crate::http_proxy::forward(guest, upstream, target, None).await;
    } else {
        return relay_explicit_port(guest, upstream, &target).await;
    }
    tokio::io::copy_bidirectional(&mut guest, &mut upstream)
        .await
        .into_alien_error()
        .context(unavailable("relay connection"))?;
    Ok(())
}

/// Preserve server-first protocols such as SSH while checking TLS ClientHello on explicit ports.
/// The upstream reader may deliver a protocol greeting, but no guest bytes reach upstream before
/// the first payload is classified. Protocol upgrades inside an opaque stream are not inspected.
async fn relay_explicit_port<G>(guest: G, upstream: TcpStream, target: &Target) -> Result<()>
where
    G: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (mut guest_read, mut guest_write) = tokio::io::split(guest);
    let (mut upstream_read, mut upstream_write) = upstream.into_split();
    let send = async {
        let mut first = [0u8; 4096];
        let count = guest_read
            .read(&mut first)
            .await
            .into_alien_error()
            .context(unavailable("read explicit-port protocol"))?;
        let mut input = Prefixed::new(first[..count].to_vec(), guest_read);
        if count > 0 && first[0] == 22 {
            let hello = timeout(HEADER_TIMEOUT, read_client_hello(&mut input, target))
                .await
                .into_alien_error()
                .context(unavailable("ClientHello timeout"))??;
            upstream_write
                .write_all(&hello)
                .await
                .into_alien_error()
                .context(unavailable("forward ClientHello"))?;
        }
        tokio::io::copy(&mut input, &mut upstream_write)
            .await
            .into_alien_error()
            .context(unavailable("relay explicit-port request"))?;
        upstream_write
            .shutdown()
            .await
            .into_alien_error()
            .context(unavailable("finish upstream request"))
    };
    let receive = async {
        tokio::io::copy(&mut upstream_read, &mut guest_write)
            .await
            .into_alien_error()
            .context(unavailable("relay explicit-port response"))?;
        guest_write
            .shutdown()
            .await
            .into_alien_error()
            .context(unavailable("finish guest response"))
    };
    tokio::try_join!(send, receive)?;
    Ok(())
}

async fn refuse(stream: &mut (impl tokio::io::AsyncWrite + Unpin), policy: bool) -> Result<()> {
    let response = if policy {
        b"HTTP/1.1 403 Forbidden\r\nX-Alien-Egress-Reason: policy\r\nConnection: close\r\nContent-Length: 0\r\n\r\n".as_slice()
    } else {
        b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\nContent-Length: 0\r\n\r\n".as_slice()
    };
    stream
        .write_all(response)
        .await
        .into_alien_error()
        .context(unavailable("send denial"))
}

async fn read_head(stream: &mut (impl tokio::io::AsyncRead + Unpin)) -> Result<Vec<u8>> {
    let mut input = Vec::new();
    loop {
        let mut chunk = [0; 2048];
        let n = stream
            .read(&mut chunk)
            .await
            .into_alien_error()
            .context(unavailable("read request"))?;
        if n == 0 {
            return Err(invalid("incomplete request"));
        }
        input.extend_from_slice(&chunk[..n]);
        if input.len() > MAX_HEADER_BYTES {
            return Err(invalid("request too large"));
        }
        if input.windows(4).any(|v| v == b"\r\n\r\n") {
            return Ok(input);
        }
    }
}

fn inspect_head(input: &[u8]) -> Result<(Target, String, bool, usize)> {
    let target = request_target(input)?.ok_or_else(|| invalid("incomplete request"))?;
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut request = httparse::Request::new(&mut headers);
    let head_len = request
        .parse(input)
        .map_err(|_| invalid("invalid request"))?;
    let httparse::Status::Complete(head_len) = head_len else {
        return Err(invalid("incomplete request"));
    };
    let connect = request.method == Some("CONNECT");
    let mut bearer = None;
    for header in request.headers {
        if header.name.eq_ignore_ascii_case("proxy-authorization") {
            if bearer.is_some() {
                return Err(denied("duplicate proxy authorization"));
            }
            bearer = Some(
                std::str::from_utf8(header.value)
                    .map_err(|_| denied("invalid proxy authorization"))?
                    .strip_prefix("Bearer ")
                    .ok_or_else(|| denied("Bearer token required"))?
                    .to_string(),
            );
        }
        if connect
            && (header.name.eq_ignore_ascii_case("transfer-encoding")
                || header.name.eq_ignore_ascii_case("content-length"))
        {
            return Err(invalid("CONNECT cannot carry body framing"));
        }
    }
    Ok((
        target,
        bearer.ok_or_else(|| denied("proxy authorization required"))?,
        connect,
        head_len,
    ))
}
