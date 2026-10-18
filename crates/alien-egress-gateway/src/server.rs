use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};

use alien_error::{Context, IntoAlienError};
use base64::{engine::general_purpose::STANDARD, Engine};
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
    io::{Counters, Metered, Prefixed},
    policy::{check_addresses, PolicyExt, Target},
    protocol::{read_client_hello, request_target, MAX_HEADER_BYTES},
    store::PolicyStore,
    token::{self, Claims},
};

/// Maximum time to receive initial request headers or a TLS ClientHello.
pub const HEADER_TIMEOUT: Duration = Duration::from_secs(5);
/// Maximum connection lifetime; limits resource retention by idle peers.
pub const CONNECTION_TIMEOUT: Duration = Duration::from_secs(3600);

/// Shared configuration and policy store.
pub struct Gateway {
    /// Deployment configuration.
    pub config: Config,
    /// Parsed public keys.
    pub keys: HashMap<String, PublicKey>,
    /// Current policies shared by all replicas.
    pub policies: PolicyStore,
    /// Gateway-only CA and vault access.
    pub credentials: crate::credentials::Credentials,
    /// Total accepted proxy connections.
    pub accepted: std::sync::atomic::AtomicU64,
    /// Total denied or failed proxy connections.
    pub denied: std::sync::atomic::AtomicU64,
}

impl Gateway {
    /// Authorize the authenticated identity against its currently stored policy.
    pub async fn authorize(&self, claims: &Claims, target: &Target) -> Result<()> {
        if !self.config.audiences.contains(&claims.aud) {
            return Err(denied("unknown audience"));
        }
        let policy = self.policies.resolve(&claims.aud, &claims.sub).await?;
        if !policy.permits(target) {
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
        let stream = timeout(HEADER_TIMEOUT, TcpStream::connect(addresses.as_slice()))
            .await
            .into_alien_error()
            .context(unavailable("destination timeout"))?
            .into_alien_error()
            .context(unavailable("connect destination"))?;
        stream
            .set_nodelay(true)
            .into_alien_error()
            .context(unavailable("configure destination socket"))?;
        Ok(stream)
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
    stream
        .inner
        .set_nodelay(true)
        .into_alien_error()
        .context(unavailable("configure guest socket"))?;
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
    if let Some(target) = &target {
        audit.host = target.host.clone();
        audit.port = target.port;
    }
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
    let Some(target) = target else {
        if !gateway.config.audiences.contains(&claims.aud)
            || gateway
                .policies
                .resolve(&claims.aud, &claims.sub)
                .await
                .is_err()
        {
            refuse(stream, false).await?;
            return Err(denied("health identity or policy unavailable"));
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nok\n")
            .await
            .into_alien_error()
            .context(unavailable("reply to authenticated health"))?;
        return Ok(());
    };
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
    if target.port == 80 {
        return crate::http_proxy::forward(guest, upstream, target, None).await;
    }
    if target.host.parse::<std::net::Ipv4Addr>().is_ok() {
        // Explicit IP/CIDR rules are the only grants for opaque, including server-first, protocols.
        tokio::io::copy_bidirectional(&mut guest, &mut upstream)
            .await
            .into_alien_error()
            .context(unavailable("relay IP connection"))?;
        return Ok(());
    }
    let first = timeout(HEADER_TIMEOUT, guest.read_u8())
        .await
        .into_alien_error()
        .context(unavailable("protocol timeout"))?
        .into_alien_error()
        .context(unavailable("read protocol"))?;
    let mut guest = Prefixed::new(vec![first], guest);
    if first != 22 {
        // HTTP parsing enforces Host on every port; arbitrary protocols need an IP/CIDR rule.
        if !rules.is_empty() {
            return Err(denied("credential destination requires TLS"));
        }
        return crate::http_proxy::forward(guest, upstream, target, None).await;
    }
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
    tokio::io::copy_bidirectional(&mut guest, &mut upstream)
        .await
        .into_alien_error()
        .context(unavailable("relay connection"))?;
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

fn inspect_head(input: &[u8]) -> Result<(Option<Target>, String, bool, usize)> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut request = httparse::Request::new(&mut headers);
    let head_len = request
        .parse(input)
        .map_err(|_| invalid("invalid request"))?;
    let httparse::Status::Complete(head_len) = head_len else {
        return Err(invalid("incomplete request"));
    };
    let health = request.method == Some("GET") && request.path == Some("/health");
    let target = if health {
        if request
            .headers
            .iter()
            .filter(|header| header.name.eq_ignore_ascii_case("host"))
            .count()
            != 1
        {
            return Err(invalid("exactly one Host required"));
        }
        None
    } else {
        Some(request_target(input)?.ok_or_else(|| invalid("incomplete request"))?)
    };
    let connect = request.method == Some("CONNECT");
    let mut bearer = None;
    for header in request.headers {
        if header.name.eq_ignore_ascii_case("proxy-authorization") {
            if bearer.is_some() {
                return Err(denied("duplicate proxy authorization"));
            }
            let authorization = std::str::from_utf8(header.value)
                .map_err(|_| denied("invalid proxy authorization"))?;
            bearer = Some(if let Some(token) = authorization.strip_prefix("Bearer ") {
                token.to_string()
            } else if let Some(encoded) = authorization.strip_prefix("Basic ") {
                let bytes = STANDARD
                    .decode(encoded)
                    .map_err(|_| denied("invalid proxy credentials"))?;
                let credentials =
                    std::str::from_utf8(&bytes).map_err(|_| denied("invalid proxy credentials"))?;
                credentials
                    .strip_prefix("alien:")
                    .ok_or_else(|| denied("invalid proxy username"))?
                    .to_string()
            } else {
                return Err(denied("proxy token required"));
            });
        }
        if (connect || health)
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
