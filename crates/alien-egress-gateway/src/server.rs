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
    sync::{watch, Semaphore},
    task::JoinSet,
    time::timeout,
};

use crate::{
    config::Config,
    connections::{Connections, IdentityPermit, SourcePermit},
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
    pub policies: Arc<PolicyStore>,
    /// Gateway-only CA and vault access.
    pub credentials: Arc<crate::credentials::Credentials>,
    /// Total accepted proxy connections.
    pub accepted: Arc<std::sync::atomic::AtomicU64>,
    /// Total denied or failed proxy connections.
    pub denied: Arc<std::sync::atomic::AtomicU64>,
    pub connections: Arc<Connections>,
    pub resolver: hickory_resolver::TokioResolver,
    pub replication: Arc<crate::replication::Replication>,
}

impl Gateway {
    /// Authorize the authenticated identity against its currently stored policy.
    pub async fn authorize(&self, claims: &Claims, target: &Target) -> Result<()> {
        if !self.config.audiences.contains(&claims.aud) {
            return Err(denied("unknown audience"));
        }
        let policy = self.policies.resolve(&claims.aud, &claims.sub).await?;
        if !policy.permits(target) {
            if matches!(target.port, 25 | 465 | 587) {
                return Err(denied(
                    "mail port requires an explicit destination:port grant",
                ));
            }
            return Err(alien_error::AlienError::new(
                crate::error::ErrorData::PolicyDenied,
            ));
        }
        Ok(())
    }

    /// Resolve once, validate every returned address, then dial a validated socket address.
    /// The connect operation cannot resolve the name a second time.
    pub async fn connect(&self, target: &Target, identity: &IdentityPermit) -> Result<TcpStream> {
        let _dns = identity
            .dns
            .try_acquire()
            .map_err(|_| crate::connections::capacity())?;
        let addresses: Vec<_> = if let Ok(ip) = target.host.parse::<std::net::Ipv4Addr>() {
            vec![SocketAddr::new(ip.into(), target.port)]
        } else {
            // An absolute DNS name never applies deployment search domains.
            timeout(
                HEADER_TIMEOUT,
                self.resolver.ipv4_lookup(format!("{}.", target.host)),
            )
            .await
            .into_alien_error()
            .context(unavailable("DNS timeout"))?
            .into_alien_error()
            .context(unavailable("resolve destination"))?
            .answers()
            .iter()
            .filter_map(|record| record.data.ip_addr())
            .filter(|ip| ip.is_ipv4())
            .map(|ip| SocketAddr::new(ip, target.port))
            .collect()
        };
        drop(_dns);
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
        identity.record_peer(
            stream
                .peer_addr()
                .into_alien_error()
                .context(unavailable("read connected destination"))?
                .ip(),
        )?;
        stream
            .set_nodelay(true)
            .into_alien_error()
            .context(unavailable("configure destination socket"))?;
        Ok(stream)
    }
}

/// Serve proxy connections with bounded concurrency and supervised connection tasks.
pub async fn serve(
    listener: TcpListener,
    live: Arc<crate::runtime::LiveGateway>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let max_connections = live
        .snapshot()
        .ok_or_else(|| denied("config unavailable"))?
        .config
        .max_connections;
    let limit = Arc::new(Semaphore::new(max_connections));
    let registry = live
        .snapshot()
        .ok_or_else(|| denied("config unavailable"))?
        .connections
        .clone();
    let mut tasks = JoinSet::new();
    let mut sweep = tokio::time::interval(Duration::from_secs(1));
    let revalidate = async {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            registry.revalidate(&live).await;
        }
    };
    tokio::pin!(revalidate);
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = sweep.tick() => registry.expire_idle(),
            _ = &mut revalidate => break,
            accepted = listener.accept() => {
                let (stream, peer) = match accepted {
                    Ok(connection) => connection,
                    Err(error) => { tracing::warn!(%error, "proxy accept failed; retrying"); tokio::time::sleep(Duration::from_millis(100)).await; continue; }
                };
                let Some(gateway) = live.snapshot() else { drop(stream); continue; };
                gateway.accepted.fetch_add(1, Ordering::Relaxed);
                let admission = registry.source(peer.ip()).and_then(|source| {
                    limit.clone().try_acquire_owned().map(|permit| (source, permit)).map_err(|_| crate::connections::capacity())
                });
                let (source, permit) = match admission {
                    Ok(permits) => permits,
                    Err(_) => {
                        gateway.denied.fetch_add(1, Ordering::Relaxed);
                        if registry.record_denial(0) { tracing::info!(event = "close", decision = "deny", reason = if limit.available_permits() == 0 { "global connection capacity" } else { "source connection capacity" }, max_connections, "egress connection"); }
                        continue;
                    }
                };
                tasks.spawn(async move { let _permit = permit; connection(stream, gateway, source).await; });
            }
            result = tasks.join_next(), if !tasks.is_empty() => {
                if let Some(Err(error)) = result { tracing::error!(%error, "proxy task failed"); }
            }
        }
    }
    drop(listener);
    // Stop accepting immediately; existing requests get a bounded completion window.
    let _ = timeout(Duration::from_secs(15), async {
        while tasks.join_next().await.is_some() {}
    })
    .await;
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    Ok(())
}

#[derive(Default)]
struct Audit {
    aud: String,
    sub: String,
    host: String,
    port: u16,
    allowed: bool,
}

async fn connection(stream: TcpStream, gateway: Arc<Gateway>, source: SourcePermit) {
    let counters = Arc::new(Counters::default());
    let mut stream = Metered {
        inner: stream,
        counters: counters.clone(),
    };
    let started = Instant::now();
    let mut audit = Audit::default();
    let result = timeout(
        CONNECTION_TIMEOUT,
        proxy(&mut stream, gateway.clone(), &mut audit, source),
    )
    .await;
    let (reason, category, protocol_denial) = match result {
        Ok(Ok(())) => ("completed".to_string(), 4, false),
        Ok(Err(error)) => {
            let category = match error.code.as_str() {
                "EGRESS_CAPACITY" => 0,
                "EGRESS_INVALID" => 3,
                _ if audit.aud.is_empty() => 1,
                "EGRESS_DENIED" | "EGRESS_POLICY_DENIED" => 2,
                _ => 4,
            };
            let protocol_denial = matches!(
                error.code.as_str(),
                "EGRESS_INVALID" | "EGRESS_DENIED" | "EGRESS_POLICY_DENIED"
            );
            (error.message, category, protocol_denial)
        }
        Err(_) => ("connection lifetime exceeded".into(), 4, false),
    };
    let allowed = audit.allowed && !protocol_denial;
    let log = if allowed {
        true
    } else {
        gateway.denied.fetch_add(1, Ordering::Relaxed);
        gateway.connections.record_denial(category)
    };
    if log {
        tracing::info!(event = "close", ts = %chrono::Utc::now().to_rfc3339(), aud = %audit.aud, sub = %audit.sub,
            host = %audit.host, port = audit.port, decision = if allowed { "allow" } else { "deny" },
            closeReason = %reason, reason = %reason, bytesUp = counters.up.load(Ordering::Relaxed),
            bytesDown = counters.down.load(Ordering::Relaxed), durationMs = started.elapsed().as_millis() as u64,
            "egress connection");
    }
}

async fn proxy(
    stream: &mut Metered<TcpStream>,
    gateway: Arc<Gateway>,
    audit: &mut Audit,
    source: SourcePermit,
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
    if !gateway.policies.ready() {
        refuse(stream, false).await?;
        return Err(alien_error::AlienError::new(unavailable(
            "replica snapshot pending",
        )));
    }
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
    let (identity, mut cancel) =
        gateway
            .connections
            .register(claims.clone(), target.clone(), stream.counters.clone())?;
    drop(source);
    tokio::select! {
        result = authorized_proxy(stream, &gateway, audit, &claims, target, connect, input, head_len, &identity) => result,
        _ = cancel.changed() => Err(alien_error::AlienError::new(unavailable(cancel.borrow().unwrap_or("connection cancelled")))),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "parsed proxy request and authenticated connection context"
)]
async fn authorized_proxy(
    stream: &mut Metered<TcpStream>,
    gateway: &Gateway,
    audit: &mut Audit,
    claims: &Claims,
    target: Option<Target>,
    connect: bool,
    input: Vec<u8>,
    head_len: usize,
    identity: &IdentityPermit,
) -> Result<()> {
    let Some(target) = target else {
        audit.allowed = timeout(
            HEADER_TIMEOUT,
            crate::recovery::handle(stream, gateway, claims, &input, head_len),
        )
        .await
        .into_alien_error()
        .context(unavailable("policy recovery timeout"))??;
        if audit.allowed {
            log_open(audit);
        }
        return Ok(());
    };
    if let Err(error) = gateway.authorize(claims, &target).await {
        refuse(stream, error.code == "EGRESS_POLICY_DENIED").await?;
        return Err(error);
    }
    if !connect {
        let upstream = gateway.connect(&target, identity).await?;
        audit.allowed = true;
        log_open(audit);
        return crate::http_proxy::forward(Prefixed::new(input, stream), upstream, target, None)
            .await;
    }
    // Resolve before acknowledging, so denied ranges receive an explicit gateway denial.
    let mut upstream = match gateway.connect(&target, identity).await {
        Ok(stream) => stream,
        Err(error) => {
            refuse(stream, error.code == "EGRESS_POLICY_DENIED").await?;
            return Err(error);
        }
    };
    audit.allowed = true;
    log_open(audit);
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

fn log_open(audit: &Audit) {
    tracing::info!(event = "open", ts = %chrono::Utc::now().to_rfc3339(), aud = %audit.aud, sub = %audit.sub,
        host = %audit.host, port = audit.port, decision = "allow", "egress connection");
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
    let recovery = request.method == Some("PUT") && request.path == Some("/v1/policies/register");
    let target = if health || recovery {
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
