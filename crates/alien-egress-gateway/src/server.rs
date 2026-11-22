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
    config::{CredentialRule, GatewayLink},
    connections::{Connections, IdentityPermit, SourcePermit},
    error::{denied, invalid, unavailable, Result},
    io::{Counters, Metered, Prefixed},
    policy::{check_addresses, check_link_addresses, PolicyExt, Target},
    protocol::{read_client_hello, request_target, MAX_HEADERS, MAX_HEADER_BYTES},
    store::PolicyStore,
    token::{self, Claims},
};

/// Maximum time to receive initial request headers or a TLS ClientHello.
pub const HEADER_TIMEOUT: Duration = Duration::from_secs(5);
/// Lifetime of a connection that is not a declared link; the sweep enforces it.
pub use crate::connections::CONNECTION_TIMEOUT;

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
    /// Authorize the authenticated identity against its currently stored policy. Returns the
    /// declared link the target names, which the connection may then reach inside the
    /// deployment.
    pub async fn authorize(&self, claims: &Claims, target: &Target) -> Result<Option<GatewayLink>> {
        if !self.config.audiences.contains(&claims.aud) {
            return Err(denied("unknown audience"));
        }
        let policy = self.policies.resolve(&claims.aud, &claims.sub).await?;
        if let Some(link) = self.config.link(&claims.aud, &target.host, target.port) {
            // The stack's links are the ceiling; the sandbox's policy can only narrow them.
            if !policy.allows_link(&link.id) {
                return Err(alien_error::AlienError::new(
                    crate::error::ErrorData::PolicyDenied,
                ));
            }
            return Ok(Some(link.clone()));
        }
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
        Ok(None)
    }

    /// Resolve once, validate every returned address, then dial a validated socket address.
    /// The connect operation cannot resolve the name a second time. A linked target's answers
    /// must all fall in the link's ranges; every other target's must all be public. A link with
    /// fixed addresses is dialed at those addresses without a lookup.
    pub async fn connect(
        &self,
        target: &Target,
        identity: &IdentityPermit,
        link: Option<&GatewayLink>,
    ) -> Result<TcpStream> {
        let _dns = identity
            .dns
            .try_acquire()
            .map_err(|_| crate::connections::capacity())?;
        let addresses: Vec<_> = if let Ok(ip) = target.host.parse::<std::net::Ipv4Addr>() {
            vec![SocketAddr::new(ip.into(), target.port)]
        } else if let Some(addresses) = fixed_link_addresses(link, target.port) {
            addresses
        } else {
            let _timer = self.connections.dns_latency.start();
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
        let resolved: Vec<_> = addresses.iter().map(SocketAddr::ip).collect();
        match link {
            Some(link) => check_link_addresses(&resolved, link)?,
            None => check_addresses(&resolved, &self.config.denied_ranges)?,
        }
        let timer = self.connections.connect_latency.start();
        let stream = crate::dial::connect(&addresses)
            .await
            .into_alien_error()
            .context(unavailable("connect destination"))?;
        drop(timer);
        identity.record_peer(
            stream
                .peer_addr()
                .into_alien_error()
                .context(unavailable("read connected destination"))?
                .ip(),
            link.map(|link| link.id.clone()),
        )?;
        stream
            .set_nodelay(true)
            .into_alien_error()
            .context(unavailable("configure destination socket"))?;
        if link.is_some() {
            crate::tunnel::keep_alive(&stream);
        }
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
                        if registry.record_denial(0) { log_capacity_denial(if limit.available_permits() == 0 { "global connection capacity" } else { "source connection capacity" }, max_connections); }
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
    /// The declared link the connection reached, if any.
    link: Option<String>,
}

/// The addresses the deployment owns for a linked name, dialed without a lookup; `None` when
/// the target is not a link or its link resolves by name.
fn fixed_link_addresses(link: Option<&GatewayLink>, port: u16) -> Option<Vec<SocketAddr>> {
    let link = link.filter(|link| !link.addresses.is_empty())?;
    Some(
        link.addresses
            .iter()
            .map(|ip| SocketAddr::new((*ip).into(), port))
            .collect(),
    )
}

async fn connection(stream: TcpStream, gateway: Arc<Gateway>, source: SourcePermit) {
    let counters = Arc::new(Counters::default());
    let mut stream = Metered {
        inner: stream,
        counters: counters.clone(),
    };
    let started = Instant::now();
    let mut audit = Audit::default();
    // Idle and lifetime limits are the registry sweep's (see `Connections::expire_idle`): a
    // declared link's connection has a long idle limit and no lifetime limit.
    let result = proxy(&mut stream, gateway.clone(), &mut audit, source).await;
    let (reason, category, protocol_denial) = match result {
        Ok(()) => ("completed".to_string(), 4, false),
        Err(error) => {
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
    };
    let allowed = audit.allowed && !protocol_denial;
    let log = if allowed {
        true
    } else {
        gateway.denied.fetch_add(1, Ordering::Relaxed);
        gateway.connections.record_denial(category)
    };
    if log {
        log_close(&audit, allowed, &reason, &counters, started.elapsed());
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
    let link = match gateway.authorize(claims, &target).await {
        Ok(link) => link,
        Err(error) => {
            refuse(stream, error.code == "EGRESS_POLICY_DENIED").await?;
            return Err(error);
        }
    };
    audit.link = link.as_ref().map(|link| link.id.clone());
    let rules = credential_rules(&gateway.config, claims, &target);
    let raw = raw_link_relay(link.as_ref(), &rules);
    if raw {
        crate::tunnel::keep_alive(&stream.inner);
    }
    if !connect {
        // A WebSocket handshake to a declared link carries the WebSocket once the service
        // accepts it; every other plain-proxy request is forwarded as exactly one request.
        let websocket = if raw {
            match crate::protocol::websocket_origin_head(&input[..head_len]) {
                Ok(head) => head,
                Err(error) => {
                    refuse(stream, false).await?;
                    return Err(error);
                }
            }
        } else {
            None
        };
        let mut upstream = gateway.connect(&target, identity, link.as_ref()).await?;
        audit.allowed = true;
        log_open(audit);
        let Some(head) = websocket else {
            return crate::http_proxy::forward(
                Prefixed::new(input, stream),
                upstream,
                target,
                None,
            )
            .await;
        };
        upstream
            .write_all(&head)
            .await
            .into_alien_error()
            .context(unavailable("forward WebSocket handshake"))?;
        let guest = Prefixed::new(input[head_len..].to_vec(), stream)
            .flush_prefix(&mut upstream)
            .await
            .into_alien_error()
            .context(unavailable("forward buffered WebSocket payload"))?;
        crate::tunnel::relay(guest, &mut upstream)
            .await
            .into_alien_error()
            .context(unavailable("relay WebSocket"))?;
        return Ok(());
    }
    // Resolve before acknowledging, so denied ranges receive an explicit gateway denial.
    let upstream = match gateway.connect(&target, identity, link.as_ref()).await {
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
    let guest = Prefixed::new(input[head_len..].to_vec(), stream);
    drop(input);
    tunnel(
        guest,
        upstream,
        target,
        raw,
        &rules,
        &gateway.credentials,
        (&claims.aud, &claims.sub),
    )
    .await
}

/// The credential rules for this identity and destination.
fn credential_rules(config: &Config, claims: &Claims, target: &Target) -> Vec<CredentialRule> {
    config
        .credentials
        .iter()
        .filter(|rule| {
            rule.aud == claims.aud && rule.host == target.host && rule.port == target.port
        })
        .cloned()
        .collect()
}

/// Whether a connection may carry any protocol to a declared link. The link is already pinned
/// to the linked service: its exact name, a declared port and addresses inside the link's
/// ranges, rechecked by the sweep. A destination with credential rules stays on the inspected
/// TLS path, so the credential is only ever added by the gateway to a matching request.
fn raw_link_relay(link: Option<&GatewayLink>, rules: &[CredentialRule]) -> bool {
    link.is_some() && rules.is_empty()
}

/// What may flow through an acknowledged CONNECT tunnel. `raw` (a declared link without
/// credential rules) and an explicit IP destination relay any protocol; any other destination
/// must speak TLS or HTTP, and credential destinations TLS only.
async fn tunnel(
    mut guest: Prefixed<&mut Metered<TcpStream>>,
    mut upstream: TcpStream,
    target: Target,
    raw: bool,
    rules: &[CredentialRule],
    credentials: &crate::credentials::Credentials,
    identity: (&str, &str),
) -> Result<()> {
    if raw || target.host.parse::<std::net::Ipv4Addr>().is_ok() {
        // Explicit IP/CIDR rules and declared links are the only grants for opaque, including
        // server-first, protocols.
        let guest = guest
            .flush_prefix(&mut upstream)
            .await
            .into_alien_error()
            .context(unavailable("forward buffered tunnel payload"))?;
        crate::tunnel::relay(guest, &mut upstream)
            .await
            .into_alien_error()
            .context(unavailable("relay tunnel"))?;
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
        return credentials
            .forward(
                Prefixed::new(hello, guest),
                upstream,
                identity,
                target,
                rules,
            )
            .await;
    }
    upstream
        .write_all(&hello)
        .await
        .into_alien_error()
        .context(unavailable("forward ClientHello"))?;
    drop(hello);
    let guest = guest
        .flush_prefix(&mut upstream)
        .await
        .into_alien_error()
        .context(unavailable("forward protocol prefix"))?
        .flush_prefix(&mut upstream)
        .await
        .into_alien_error()
        .context(unavailable("forward buffered TLS payload"))?;
    crate::tunnel::relay(guest, &mut upstream)
        .await
        .into_alien_error()
        .context(unavailable("relay connection"))?;
    Ok(())
}

// Audit records. The JSON layer writes each record's `message` and `timestamp`, so these
// fields must not repeat either one: a log collector drops a timestamp that two fields
// disagree on. `connectionEvent` avoids `event`, which structlog uses as its message.

fn log_open(audit: &Audit) {
    tracing::info!(connectionEvent = "open", aud = %audit.aud, sub = %audit.sub,
        host = %audit.host, port = audit.port, link = audit.link.as_deref(), decision = "allow",
        "egress connection");
}

fn log_close(audit: &Audit, allowed: bool, reason: &str, counters: &Counters, elapsed: Duration) {
    tracing::info!(connectionEvent = "close", aud = %audit.aud, sub = %audit.sub,
        host = %audit.host, port = audit.port, link = audit.link.as_deref(),
        decision = if allowed { "allow" } else { "deny" },
        closeReason = %reason, reason = %reason, bytesUp = counters.up.load(Ordering::Relaxed),
        bytesDown = counters.down.load(Ordering::Relaxed), durationMs = elapsed.as_millis() as u64,
        "egress connection");
}

fn log_capacity_denial(reason: &str, max_connections: usize) {
    tracing::info!(
        connectionEvent = "close",
        decision = "deny",
        reason,
        max_connections,
        "egress connection"
    );
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
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
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

#[cfg(test)]
mod audit_log_tests {
    use std::{
        io::Write,
        sync::{atomic::Ordering, Arc, Mutex},
        time::Duration,
    };

    use tracing_subscriber::{fmt::MakeWriter, layer::SubscriberExt};

    use super::{log_capacity_denial, log_close, log_open, Audit};
    use crate::{io::Counters, logging::json_layer};

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Captured {
        type Writer = Captured;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    /// Log collectors read these records with lognorm. Each one must give back its
    /// message and the exact time the gateway wrote it, not the raw line and no time.
    #[test]
    fn audit_records_keep_their_message_and_timestamp_in_log_collectors() {
        let output = Captured::default();
        let subscriber = tracing_subscriber::registry().with(json_layer(output.clone()));
        let audit = Audit {
            aud: "deployment/resource".into(),
            sub: "sandbox".into(),
            host: "allowed.test".into(),
            port: 443,
            allowed: true,
            link: None,
        };
        let counters = Counters::default();
        counters.up.store(10, Ordering::Relaxed);
        counters.down.store(20, Ordering::Relaxed);

        tracing::subscriber::with_default(subscriber, || {
            log_open(&audit);
            log_close(
                &audit,
                false,
                "Invalid egress input: incomplete request",
                &counters,
                Duration::from_millis(7),
            );
            log_capacity_denial("global connection capacity", 64);
        });

        let text = String::from_utf8(output.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3, "{text}");
        for (line, connection_event) in lines.iter().zip(["open", "close", "close"]) {
            let log = lognorm::parse(line);
            assert_eq!(log.outcome(), lognorm::ParseOutcome::Structured, "{line}");
            assert_eq!(log.message(), "egress connection", "{line}");
            assert_eq!(log.severity(), Some(lognorm::Severity::Info), "{line}");
            let written =
                chrono::DateTime::parse_from_rfc3339(log.fields()["timestamp"].as_str().unwrap())
                    .unwrap();
            let timestamp = log
                .timestamp()
                .unwrap_or_else(|| panic!("no timestamp: {line}"));
            assert_eq!(
                (timestamp.unix_seconds(), timestamp.nanoseconds()),
                (written.timestamp(), written.timestamp_subsec_nanos()),
                "{line}"
            );
            assert_eq!(log.fields()["connectionEvent"], connection_event, "{line}");
        }

        let open = lognorm::parse(lines[0]);
        assert_eq!(open.fields()["decision"], "allow");
        assert_eq!(open.fields()["sub"], "sandbox");
        assert_eq!(open.fields()["host"], "allowed.test");
        assert_eq!(open.fields()["port"], 443);

        let close = lognorm::parse(lines[1]);
        assert_eq!(close.fields()["decision"], "deny");
        assert_eq!(
            close.fields()["closeReason"],
            "Invalid egress input: incomplete request"
        );
        assert_eq!(close.fields()["bytesUp"], 10);
        assert_eq!(close.fields()["bytesDown"], 20);
        assert_eq!(close.fields()["durationMs"], 7);

        let capacity = lognorm::parse(lines[2]);
        assert_eq!(capacity.fields()["reason"], "global connection capacity");
        assert_eq!(capacity.fields()["max_connections"], 64);
    }
}

#[cfg(test)]
mod fixed_link_address_tests {
    use super::{fixed_link_addresses, GatewayLink};

    fn link(addresses: &[&str]) -> GatewayLink {
        GatewayLink {
            id: "api".into(),
            host: "api.svc".into(),
            ports: vec![8080],
            ranges: vec!["10.1.0.0/24".parse().unwrap()],
            addresses: addresses.iter().map(|a| a.parse().unwrap()).collect(),
        }
    }

    #[test]
    fn a_link_with_fixed_addresses_is_dialed_there_and_others_resolve() {
        assert_eq!(
            fixed_link_addresses(Some(&link(&["10.1.0.4", "10.1.0.5"])), 8080).unwrap(),
            vec![
                "10.1.0.4:8080".parse().unwrap(),
                "10.1.0.5:8080".parse().unwrap()
            ]
        );
        assert!(fixed_link_addresses(Some(&link(&[])), 8080).is_none());
        assert!(fixed_link_addresses(None, 8080).is_none());
    }
}

#[cfg(test)]
mod link_relay_tests {
    use std::{collections::HashMap, sync::Arc, time::Duration};

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    };

    use super::{raw_link_relay, tunnel, CredentialRule, GatewayLink};
    use crate::{
        credentials::Credentials,
        io::{Counters, Metered, Prefixed},
        policy::Target,
    };

    fn link() -> GatewayLink {
        GatewayLink {
            id: "api".into(),
            host: "api.svc".into(),
            ports: vec![6379],
            ranges: vec!["10.1.0.0/24".parse().unwrap()],
            addresses: vec![],
        }
    }

    fn rule() -> CredentialRule {
        serde_json::from_value(serde_json::json!({"aud":"p/r","host":"api.svc","port":6379,
            "method":"GET","path":"/","header":"Authorization","vaultBinding":"test-vault","secret":"key"}))
        .unwrap()
    }

    async fn pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        (client, listener.accept().await.unwrap().0)
    }

    /// Runs an acknowledged tunnel to `host:6379` with `payload` from the sandbox; returns
    /// whether the tunnel ended in an error and the bytes the service received.
    async fn run(
        host: &str,
        raw: bool,
        rules: &[CredentialRule],
        payload: &[u8],
    ) -> (bool, Vec<u8>) {
        let bindings = Arc::new(
            alien_bindings::Bindings::from_env_map(HashMap::from([(
                "ALIEN_DEPLOYMENT_TYPE".into(),
                "local".into(),
            )]))
            .unwrap(),
        );
        let credentials = Credentials::new(bindings, &[], rules, Some([7; 32])).unwrap();
        let (mut sandbox, guest) = pair().await;
        let (upstream, mut service) = pair().await;
        let mut guest = Metered {
            inner: guest,
            counters: Arc::new(Counters::default()),
        };
        sandbox.write_all(payload).await.unwrap();
        let target = Target::parse(&format!("{host}:6379"), None).unwrap();
        let relay = tunnel(
            Prefixed::new(Vec::new(), &mut guest),
            upstream,
            target,
            raw,
            rules,
            &credentials,
            ("p/r", "s"),
        );
        let receive = async {
            let mut received = Vec::new();
            let mut buffer = [0u8; 256];
            while let Ok(Ok(n)) =
                tokio::time::timeout(Duration::from_millis(500), service.read(&mut buffer)).await
            {
                if n == 0 {
                    break;
                }
                received.extend_from_slice(&buffer[..n]);
            }
            drop(sandbox);
            drop(service);
            received
        };
        let (result, received) = tokio::join!(relay, receive);
        (result.is_err(), received)
    }

    const PING: &[u8] = b"*1\r\n$4\r\nPING\r\n";
    const WEBSOCKET: &[u8] = b"GET /ws HTTP/1.1\r\nHost: api.svc:6379\r\nConnection: Upgrade\r\n\
        Upgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n";

    #[tokio::test]
    async fn a_link_without_credential_rules_relays_any_protocol() {
        assert!(raw_link_relay(Some(&link()), &[]));
        let (_, received) = run("api.svc", true, &[], PING).await;
        assert_eq!(received, PING);
    }

    /// A link with a credential rule stays on the inspected path on both the CONNECT and the
    /// plain-proxy WebSocket paths (both read `raw_link_relay`): no raw relay, so a plaintext
    /// WebSocket or other protocol never reaches the service.
    #[tokio::test]
    async fn a_link_with_credential_rules_stays_off_the_raw_relay() {
        let rules = [rule()];
        assert!(!raw_link_relay(Some(&link()), &rules));
        assert!(!raw_link_relay(None, &[]));
        for payload in [PING, WEBSOCKET] {
            let (failed, received) = run("api.svc", false, &rules, payload).await;
            assert!(failed);
            assert!(
                received.is_empty(),
                "{}",
                String::from_utf8_lossy(&received)
            );
        }
    }

    /// A tunnel to a host that is not a declared link still carries only TLS or HTTP: a
    /// non-HTTP payload is refused before a byte reaches the destination.
    #[tokio::test]
    async fn an_unlinked_host_tunnel_refuses_a_non_http_payload() {
        let (_, received) = run("api.example.com", false, &[], PING).await;
        assert!(
            received.is_empty(),
            "{}",
            String::from_utf8_lossy(&received)
        );
    }
}
