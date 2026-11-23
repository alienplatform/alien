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
    config::GatewayLink,
    connections::{Connections, IdentityPermit, SourcePermit},
    error::{denied, invalid, unavailable, Result},
    io::{Counters, Metered, Prefixed},
    policy::{check_addresses, check_link_addresses, PolicyExt, Target},
    protocol::{read_client_hello, request_target, MAX_HEADERS, MAX_HEADER_BYTES},
    sandbox_credentials::RuleTarget,
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
    /// Gateway-only CA and upstream TLS trust.
    pub credentials: Arc<crate::credentials::Credentials>,
    /// Each sandbox's current credential rules, shared by all configuration snapshots.
    pub sandbox_credentials: Arc<crate::sandbox_credentials::CredentialStore>,
    /// Total accepted proxy connections.
    pub accepted: Arc<std::sync::atomic::AtomicU64>,
    /// Total denied or failed proxy connections.
    pub denied: Arc<std::sync::atomic::AtomicU64>,
    pub connections: Arc<Connections>,
    pub resolver: hickory_resolver::TokioResolver,
    pub replication: Arc<crate::replication::Replication>,
}

impl Gateway {
    /// Apply a verified policy record. A deletion drops the sandbox's credential rules on the
    /// same replica at once, so no rule outlives its sandbox's grant.
    pub fn apply_policy(&self, record: crate::record::Record, only_missing: bool) -> Result<bool> {
        let deleted = record.claims.deleted;
        let (aud, sub) = (record.claims.aud.clone(), record.claims.sub.clone());
        let applied = self.policies.apply(record, only_missing)?;
        if deleted && self.policies.resolve_now(&aud, &sub).is_none() {
            self.sandbox_credentials.remove(&aud, &sub)?;
        }
        Ok(applied)
    }

    /// Verify a signed credential record against this deployment's keys and links.
    pub fn verify_credentials(
        &self,
        signed: String,
    ) -> Result<crate::sandbox_credentials::CredentialRecord> {
        crate::sandbox_credentials::CredentialRecord::verify(
            signed,
            &self.keys,
            &self.config.audiences,
            |aud| self.config.link_ids(aud),
            chrono::Utc::now().timestamp_millis().max(0) as u64,
        )
    }

    /// Which sandbox and version a credential record is for, without its rules.
    pub fn credential_identity(&self, signed: &str) -> Result<crate::record::Summary> {
        crate::sandbox_credentials::CredentialRecord::identity(
            signed,
            &self.keys,
            &self.config.audiences,
            chrono::Utc::now().timestamp_millis().max(0) as u64,
        )
    }

    /// Store a credential set. It needs the sandbox's live policy: rules never predate or
    /// outlive the sandbox they belong to.
    pub fn apply_credentials(
        &self,
        record: crate::sandbox_credentials::CredentialRecord,
    ) -> Result<()> {
        if self
            .policies
            .resolve_now(&record.aud, &record.sub)
            .is_none()
        {
            return Err(alien_error::AlienError::new(
                crate::error::ErrorData::PolicyMissing,
            ));
        }
        self.apply_credential_record(record).map(|_| ())
    }

    /// Store a verified set. When it changes the sandbox's rules, the sandbox's connections that
    /// relay a covered destination unread end before this returns, so the write's
    /// acknowledgement means every later request is read and carries the rules.
    pub fn apply_credential_record(
        &self,
        record: crate::sandbox_credentials::CredentialRecord,
    ) -> Result<bool> {
        let (aud, sub, rules) = (record.aud.clone(), record.sub.clone(), record.rules.clone());
        let applied = self.sandbox_credentials.apply(record)?;
        if applied {
            self.connections.close_uninspected(&aud, &sub, &rules);
        }
        Ok(applied)
    }

    /// Store a credential set, catching up from the peers once when this replica does not hold
    /// the sandbox's policy yet: a new grant counts once one replica holds it, so another can
    /// lag until its next anti-entropy pass. A sandbox no replica knows stays refused.
    pub async fn apply_credentials_caught_up(
        &self,
        live: &crate::runtime::LiveGateway,
        record: crate::sandbox_credentials::CredentialRecord,
    ) -> Result<()> {
        match self.apply_credentials(record.clone()) {
            Err(error)
                if error.code == "SANDBOX_EGRESS_POLICY_MISSING" && self.replication.enabled() =>
            {
                // A failed catch-up is transient: the caller retries on 503, not 400.
                if let Err(error) = self.replication.catch_up(live).await {
                    tracing::warn!(code = %error.code, "catch-up before a credential write failed");
                    return Err(alien_error::AlienError::new(
                        crate::error::ErrorData::GatewayUnavailable,
                    ));
                }
                self.apply_credentials(record)
            }
            result => result,
        }
    }

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
    // The sandbox's own rules for this destination, read again for every request.
    let injection = crate::credentials::Injection {
        store: gateway.sandbox_credentials.clone(),
        aud: claims.aud.clone(),
        sub: claims.sub.clone(),
        target: match &link {
            Some(link) => RuleTarget::Link(link.id.clone()),
            None => RuleTarget::Host(target.host.clone()),
        },
        port: target.port,
    };
    // Mark before reading the rules: a rule change after this point either shows in the
    // lookup or finds the mark and ends the connection.
    identity.set_uninspected(Some((injection.target.clone(), injection.port)));
    let raw = raw_link_relay(link.as_ref(), injection.current().is_empty());
    if raw {
        crate::tunnel::keep_alive(&stream.inner);
    }
    if !connect {
        // A credential crosses the internet only inside TLS; a link is inside the deployment.
        if link.is_none() && !injection.current().is_empty() {
            refuse(stream, false).await?;
            return Err(denied("credential destination requires TLS"));
        }
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
        let upstream = gateway.connect(&target, identity, link.as_ref()).await?;
        audit.allowed = true;
        log_open(audit);
        let Some(head) = websocket else {
            identity.set_uninspected(None);
            return crate::http_proxy::forward(
                Prefixed::new(input, stream),
                upstream,
                target,
                link.is_some().then_some(&injection),
            )
            .await;
        };
        return relay_websocket(stream, input[head_len..].to_vec(), upstream, &head).await;
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
        link.is_some(),
        &injection,
        &gateway.credentials,
        identity,
    )
    .await
}

/// Largest body of a refused WebSocket handshake answer the gateway passes on.
const MAX_REFUSED_HANDSHAKE_BODY: usize = 64 * 1024;

/// Carry a plain-proxy WebSocket to a declared link. The handshake goes to the service as one
/// HTTP request, and only a `101` that answers `Upgrade: websocket` turns the connection into a
/// relay. Any other answer is passed on with `Connection: close` and the connection ends, so
/// bytes the sandbox sent after the handshake (`buffered`, and anything later) never reach the
/// service unless it accepted the WebSocket.
async fn relay_websocket(
    guest: &mut Metered<TcpStream>,
    buffered: Vec<u8>,
    upstream: TcpStream,
    head: &[u8],
) -> Result<()> {
    use http_body_util::BodyExt;
    let request = origin_request(head)?;
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(upstream))
            .await
            .into_alien_error()
            .context(unavailable("initialize upstream WebSocket handshake"))?;
    let driver = tokio::spawn(connection.with_upgrades());
    let response = match sender.send_request(request).await {
        Ok(response) => response,
        Err(_) => {
            driver.abort();
            guest
                .write_all(
                    b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
                )
                .await
                .into_alien_error()
                .context(unavailable("send WebSocket handshake failure"))?;
            return Err(alien_error::AlienError::new(unavailable(
                "upstream WebSocket handshake",
            )));
        }
    };
    let upgrades: Vec<_> = response
        .headers()
        .get_all(http::header::UPGRADE)
        .iter()
        .collect();
    let accepted = response.status() == http::StatusCode::SWITCHING_PROTOCOLS
        && upgrades.len() == 1
        && upgrades[0].as_bytes().eq_ignore_ascii_case(b"websocket");
    if !accepted {
        let informational = response.status().is_informational();
        let status = response.status();
        let headers = response.headers().clone();
        let body = timeout(
            HEADER_TIMEOUT,
            http_body_util::Limited::new(response.into_body(), MAX_REFUSED_HANDSHAKE_BODY)
                .collect(),
        )
        .await;
        driver.abort();
        let answer = match body {
            Ok(Ok(body)) if !informational => {
                let body = body.to_bytes();
                let mut answer = response_head(status, &headers, Some(body.len()));
                answer.extend_from_slice(&body);
                answer
            }
            _ => b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
                .to_vec(),
        };
        guest
            .write_all(&answer)
            .await
            .into_alien_error()
            .context(unavailable("forward WebSocket handshake answer"))?;
        return Err(denied("service did not accept the WebSocket"));
    }
    let answer = response_head(response.status(), response.headers(), None);
    let upgraded = hyper::upgrade::on(response)
        .await
        .into_alien_error()
        .context(unavailable("upgrade WebSocket"))?;
    let _ = driver.await;
    let parts = upgraded
        .downcast::<hyper_util::rt::TokioIo<TcpStream>>()
        .map_err(|_| alien_error::AlienError::new(unavailable("upgraded WebSocket connection")))?;
    let mut upstream = parts.io.into_inner();
    guest
        .write_all(&answer)
        .await
        .into_alien_error()
        .context(unavailable("forward WebSocket acceptance"))?;
    guest
        .write_all(&parts.read_buf)
        .await
        .into_alien_error()
        .context(unavailable("forward early WebSocket payload"))?;
    upstream
        .write_all(&buffered)
        .await
        .into_alien_error()
        .context(unavailable("forward buffered WebSocket payload"))?;
    crate::tunnel::relay(guest, &mut upstream)
        .await
        .into_alien_error()
        .context(unavailable("relay WebSocket"))?;
    Ok(())
}

/// The handshake request from its origin-form head (see `protocol::websocket_origin_head`).
fn origin_request(head: &[u8]) -> Result<http::Request<http_body_util::Empty<bytes::Bytes>>> {
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut parsed = httparse::Request::new(&mut headers);
    parsed
        .parse(head)
        .map_err(|_| invalid("malformed WebSocket handshake"))?;
    let mut request = http::Request::builder().method(http::Method::GET).uri(
        parsed
            .path
            .ok_or_else(|| invalid("missing request target"))?,
    );
    for header in parsed.headers.iter() {
        request = request.header(header.name, header.value);
    }
    request
        .body(http_body_util::Empty::new())
        .map_err(|_| invalid("malformed WebSocket handshake"))
}

/// A response head for the sandbox. With `body_len`, the answer is a complete response that
/// ends the connection: the service's framing and connection fields are replaced.
fn response_head(
    status: http::StatusCode,
    headers: &http::HeaderMap,
    body_len: Option<usize>,
) -> Vec<u8> {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\n",
        status.as_u16(),
        status.canonical_reason().unwrap_or("")
    )
    .into_bytes();
    for (name, value) in headers {
        if body_len.is_some()
            && [
                http::header::CONTENT_LENGTH,
                http::header::TRANSFER_ENCODING,
                http::header::CONNECTION,
                http::header::UPGRADE,
                http::header::HeaderName::from_static("keep-alive"),
            ]
            .contains(name)
        {
            continue;
        }
        head.extend_from_slice(name.as_str().as_bytes());
        head.extend_from_slice(b": ");
        head.extend_from_slice(value.as_bytes());
        head.extend_from_slice(b"\r\n");
    }
    if let Some(len) = body_len {
        head.extend_from_slice(
            format!("Content-Length: {len}\r\nConnection: close\r\n").as_bytes(),
        );
    }
    head.extend_from_slice(b"\r\n");
    head
}

/// Whether a connection may carry any protocol to a declared link. The link is already pinned
/// to the linked service: its exact name, a declared port and addresses inside the link's
/// ranges, rechecked by the sweep. A destination with credential rules stays on the inspected
/// TLS path, so the credential is only ever added by the gateway to a matching request.
fn raw_link_relay(link: Option<&GatewayLink>, no_rules: bool) -> bool {
    link.is_some() && no_rules
}

/// What may flow through an acknowledged CONNECT tunnel. `raw` (a declared link without
/// credential rules) and an explicit IP destination relay any protocol; any other destination
/// must speak TLS or HTTP, and credential destinations TLS only.
async fn tunnel(
    mut guest: Prefixed<&mut Metered<TcpStream>>,
    mut upstream: TcpStream,
    target: Target,
    raw: bool,
    link: bool,
    injection: &crate::credentials::Injection,
    credentials: &crate::credentials::Credentials,
    identity: &IdentityPermit,
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
        identity.set_uninspected(None);
        if link {
            return crate::http_proxy::forward(guest, upstream, target, Some(injection)).await;
        }
        if !injection.current().is_empty() {
            return Err(denied("credential destination requires TLS"));
        }
        return crate::http_proxy::forward(guest, upstream, target, None).await;
    }
    let hello = timeout(HEADER_TIMEOUT, read_client_hello(&mut guest, &target))
        .await
        .into_alien_error()
        .context(unavailable("ClientHello timeout"))??;
    // Interception is decided from the rules as they are when the ClientHello arrives.
    if !injection.current().is_empty() {
        if link {
            return Err(denied("credential link requires plain HTTP"));
        }
        identity.set_uninspected(None);
        return credentials
            .forward(Prefixed::new(hello, guest), upstream, target, injection)
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

    use super::{raw_link_relay, relay_websocket, tunnel, GatewayLink};
    use crate::{
        credentials::{Credentials, Injection},
        io::{Counters, Metered, Prefixed},
        policy::Target,
        sandbox_credentials::{CredentialRecord, CredentialStore, Rule, RuleTarget},
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

    fn rule() -> Rule {
        Rule {
            target: RuleTarget::Link("api".into()),
            port: Some(6379),
            method: None,
            path: "/".into(),
            header: http::HeaderName::from_static("authorization"),
            value: http::HeaderValue::from_static("Bearer real"),
        }
    }

    fn injection(rules: &[Rule]) -> Injection {
        let store = Arc::new(CredentialStore::new(1 << 20));
        store
            .apply(CredentialRecord {
                signed: String::new(),
                aud: "p/r".into(),
                sub: "s".into(),
                ver: (1, String::new()),
                rules: Arc::new(rules.to_vec()),
            })
            .unwrap();
        Injection {
            store,
            aud: "p/r".into(),
            sub: "s".into(),
            target: RuleTarget::Link("api".into()),
            port: 6379,
        }
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
    async fn run(host: &str, raw: bool, rules: &[Rule], payload: &[u8]) -> (bool, Vec<u8>) {
        let injection = injection(rules);
        let credentials = Credentials::new(&[], Some([7; 32])).unwrap();
        let (mut sandbox, guest) = pair().await;
        let (upstream, mut service) = pair().await;
        let mut guest = Metered {
            inner: guest,
            counters: Arc::new(Counters::default()),
        };
        sandbox.write_all(payload).await.unwrap();
        let target = Target::parse(&format!("{host}:6379"), None).unwrap();
        let registry = Arc::new(crate::connections::Connections::default());
        let now = chrono::Utc::now().timestamp() as u64;
        let (identity, _cancel) = registry
            .register(
                crate::token::Claims {
                    iss: "alien-sandbox".into(),
                    aud: "p/r".into(),
                    sub: "s".into(),
                    iat: now,
                    exp: now + 600,
                },
                None,
                guest.counters.clone(),
            )
            .unwrap();
        let relay = tunnel(
            Prefixed::new(Vec::new(), &mut guest),
            upstream,
            target,
            raw,
            true,
            &injection,
            &credentials,
            &identity,
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
        assert!(raw_link_relay(Some(&link()), true));
        let (_, received) = run("api.svc", true, &[], PING).await;
        assert_eq!(received, PING);
    }

    /// A link with a credential rule stays on the inspected path on both the CONNECT and the
    /// plain-proxy WebSocket paths (both read `raw_link_relay`): no raw relay, so a plaintext
    /// WebSocket or other protocol never reaches the service.
    #[tokio::test]
    async fn a_link_with_credential_rules_stays_off_the_raw_relay() {
        let rules = [rule()];
        assert!(!raw_link_relay(Some(&link()), rules.is_empty()));
        assert!(!raw_link_relay(None, true));
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

    /// The plain-proxy WebSocket path relays only after the service accepts the upgrade. A
    /// service that answers with an ordinary error and keeps the connection open never sees the
    /// bytes the sandbox sent after the handshake; the sandbox gets the answer with
    /// `Connection: close`. A service that accepts gets the buffered bytes, and its early bytes
    /// after the `101` reach the sandbox.
    #[tokio::test]
    async fn a_plain_proxy_websocket_relays_only_after_the_service_accepts() {
        for (answer, accepted) in [
            (b"HTTP/1.1 400 Bad Request\r\nContent-Length: 3\r\n\r\nno!".as_slice(), false),
            (
                b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\nearly",
                true,
            ),
        ] {
            let (mut sandbox, guest) = pair().await;
            let (upstream, mut service) = pair().await;
            let mut guest = Metered {
                inner: guest,
                counters: Arc::new(Counters::default()),
            };
            let pipelined = b"GET /pipelined HTTP/1.1\r\nHost: api.svc:6379\r\n\r\n".to_vec();
            let serve = async {
                let mut received = Vec::new();
                let mut buffer = [0u8; 1024];
                while !received.windows(4).any(|v| v == b"\r\n\r\n") {
                    let n = service.read(&mut buffer).await.unwrap();
                    received.extend_from_slice(&buffer[..n]);
                }
                service.write_all(answer).await.unwrap();
                while let Ok(Ok(n)) =
                    tokio::time::timeout(Duration::from_millis(500), service.read(&mut buffer)).await
                {
                    if n == 0 {
                        break;
                    }
                    received.extend_from_slice(&buffer[..n]);
                }
                drop(service);
                received
            };
            let read = async move {
                let mut answered = Vec::new();
                let mut buffer = [0u8; 1024];
                while let Ok(Ok(n)) =
                    tokio::time::timeout(Duration::from_millis(800), sandbox.read(&mut buffer)).await
                {
                    if n == 0 {
                        break;
                    }
                    answered.extend_from_slice(&buffer[..n]);
                }
                answered
            };
            let (result, received, answered) = tokio::join!(
                relay_websocket(&mut guest, pipelined, upstream, WEBSOCKET),
                serve,
                read
            );
            let received = String::from_utf8_lossy(&received).to_string();
            let answered = String::from_utf8_lossy(&answered).to_string();
            assert!(received.starts_with("GET /ws HTTP/1.1\r\n"), "{received}");
            assert_eq!(received.contains("/pipelined"), accepted, "{received}");
            if accepted {
                assert!(answered.starts_with("HTTP/1.1 101 Switching Protocols\r\n"), "{answered}");
                assert!(answered.ends_with("\r\n\r\nearly"), "{answered}");
            } else {
                assert!(result.is_err());
                assert!(answered.starts_with("HTTP/1.1 400 Bad Request\r\n"), "{answered}");
                assert!(answered.contains("Connection: close\r\n"), "{answered}");
                assert!(answered.ends_with("\r\n\r\nno!"), "{answered}");
            }
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
