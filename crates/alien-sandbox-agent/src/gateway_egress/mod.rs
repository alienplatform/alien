//! Gateway forwarding. DNS returns synthetic addresses so arbitrary TCP retains its original name.
#![expect(
    clippy::result_large_err,
    reason = "Alien errors retain structured metadata and causal context"
)]

mod dns;
mod redirect;

use std::{
    collections::BTreeMap,
    io::Write,
    net::{SocketAddr, ToSocketAddrs},
    os::unix::fs::OpenOptionsExt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};

use alien_error::{AlienError, Context, IntoAlienError};
use base64::Engine;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UdpSocket},
    sync::Semaphore,
    task::JoinSet,
    time::timeout,
};

use crate::error::{ErrorData, Result};
use dns::Names;

const TIMEOUT: Duration = Duration::from_secs(5);
const MAX_HEADER: usize = 32 * 1024;

fn failed(reason: &str) -> ErrorData {
    ErrorData::OperationFailed {
        operation: "sandbox gateway egress".into(),
        reason: reason.into(),
    }
}
fn invalid(reason: &str) -> AlienError<ErrorData> {
    AlienError::new(failed(reason))
}

/// Session egress state. Tokens never enter command environments, even in proxy-only mode.
pub struct GatewayEgress {
    address: RwLock<Option<SocketAddr>>,
    token: RwLock<String>,
    privileged: bool,
    declared_none: bool,
    ready: AtomicBool,
    listeners_live: AtomicBool,
    names: Names,
    image_started: AtomicBool,
    ca_path: RwLock<Option<String>>,
    identity: RwLock<Option<(String, String)>>,
    update_lock: tokio::sync::Mutex<()>,
}

impl GatewayEgress {
    /// Prepare routing before dropping bootstrap capabilities. An absent token is pending until
    /// the authenticated policy endpoint delivers it outside the shared image snapshot.
    pub fn from_env() -> Result<Option<Arc<Self>>> {
        let declared_none = std::env::var("ALIEN_SANDBOX_EGRESS")
            .ok()
            .is_some_and(|policy| {
                policy == "none"
                    || serde_json::from_str::<serde_json::Value>(&policy).is_ok_and(|value| {
                        value == "none" || value == serde_json::json!({"mode":"deny"})
                    })
            });
        let address = match std::env::var("ALIEN_SANDBOX_EGRESS_GATEWAY") {
            Ok(value) => Some(
                value
                    .to_socket_addrs()
                    .into_alien_error()
                    .context(failed("resolve gateway"))?
                    .find(SocketAddr::is_ipv4)
                    .ok_or_else(|| invalid("gateway needs IPv4"))?,
            ),
            Err(_) if std::env::var_os("ALIEN_SANDBOX_EGRESS").is_some() => None,
            Err(_) => return Ok(None),
        };
        let token = String::new();
        let identity = std::env::var("ALIEN_SANDBOX_EGRESS_AUDIENCE")
            .ok()
            .zip(std::env::var("ALIEN_SANDBOX_SESSION_ID").ok());
        let privileged = unsafe { libc::geteuid() } == 0;
        if privileged {
            redirect::install()
                .into_alien_error()
                .context(failed("install transparent forwarding"))?;
        }
        Ok(Some(Arc::new(Self {
            address: RwLock::new(address),
            token: RwLock::new(token),
            privileged,
            declared_none,
            ready: AtomicBool::new(false),
            listeners_live: AtomicBool::new(false),
            names: Names::default(),
            image_started: AtomicBool::new(false),
            ca_path: RwLock::new(None),
            identity: RwLock::new(identity),
            update_lock: tokio::sync::Mutex::new(()),
        })))
    }

    /// Launch image code at most once, after a successful path check.
    pub fn start_image_once(&self, identity: crate::exec::ExecIdentity) -> Result<()> {
        self.ensure_ready()?;
        if std::env::var_os("ALIEN_SANDBOX_EGRESS").is_none()
            || self.image_started.swap(true, Ordering::AcqRel)
        {
            return Ok(());
        }
        let start = || {
            let mut env = BTreeMap::new();
            self.command_environment(&mut env)?;
            crate::image_command::start_with_env(identity, &env)
        };
        let child = match start() {
            Ok(child) => child,
            Err(error) => {
                self.image_started.store(false, Ordering::Release);
                self.ready.store(false, Ordering::Release);
                return Err(error);
            }
        };
        if let Some(mut child) = child {
            tokio::spawn(async move {
                match child.wait().await {
                    Ok(status) => tracing::info!(%status, "image command exited"),
                    Err(error) => tracing::error!(%error, "image command wait failed"),
                }
            });
        }
        Ok(())
    }

    /// Health remains available while create is waiting to deliver the policy.
    pub fn status(&self) -> Result<&'static str> {
        if !self.declared_none && self.current_token()?.is_empty() {
            return Ok("awaiting-egress-policy");
        }
        self.ensure_ready()?;
        Ok("ready")
    }

    /// An immutable deny declaration needs no gateway bootstrap or token.
    pub fn requires_token(&self) -> bool {
        !self.declared_none
    }

    /// Recheck the actual network after restoring a snapshot. Build-time network readiness must
    /// never authorize the restored sandbox, and a failed build-time check must be recoverable.
    pub async fn activate_declared_none(&self) -> Result<()> {
        if !self.declared_none {
            return Err(invalid("gateway policy requires a token"));
        }
        self.ready.store(false, Ordering::Release);
        self.listeners_live.store(false, Ordering::Release);
        self.self_check().await?;
        self.listeners_live.store(true, Ordering::Release);
        Ok(())
    }

    /// Gate session routes until the network path has been verified.
    pub fn ensure_ready(&self) -> Result<()> {
        if self.ready.load(Ordering::Acquire) && self.listeners_live.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(AlienError::new(ErrorData::EgressPathUnexpected))
        }
    }

    /// Start the forwarders. Any listener task failure closes readiness immediately.
    pub async fn start(self: &Arc<Self>) -> Result<()> {
        let result = self.start_inner().await;
        if result.is_err() {
            self.listeners_live.store(false, Ordering::Release);
        }
        result
    }

    async fn start_inner(self: &Arc<Self>) -> Result<()> {
        self.listeners_live.store(true, Ordering::Release);
        if self.declared_none {
            return self.self_check().await;
        }
        let explicit = TcpListener::bind("127.0.0.1:15002")
            .await
            .into_alien_error()
            .context(failed("bind proxy"))?;
        let this = self.clone();
        tokio::spawn(async move {
            if let Err(error) = this.clone().serve(explicit, false).await {
                this.ready.store(false, Ordering::Release);
                this.listeners_live.store(false, Ordering::Release);
                tracing::error!(code = %error.code, "egress forwarder stopped");
            }
        });
        if self.privileged {
            let transparent = TcpListener::bind("127.0.0.1:15001")
                .await
                .into_alien_error()
                .context(failed("bind transparent proxy"))?;
            let udp = UdpSocket::bind("127.0.0.1:15053")
                .await
                .into_alien_error()
                .context(failed("bind DNS UDP"))?;
            let tcp = TcpListener::bind("127.0.0.1:15053")
                .await
                .into_alien_error()
                .context(failed("bind DNS TCP"))?;
            let this = self.clone();
            tokio::spawn(async move {
                if this.clone().serve(transparent, true).await.is_err() {
                    this.ready.store(false, Ordering::Release);
                    this.listeners_live.store(false, Ordering::Release);
                    tracing::error!("transparent forwarder stopped");
                }
            });
            let this = self.clone();
            tokio::spawn(async move {
                if this.dns_udp(udp).await.is_err() {
                    this.ready.store(false, Ordering::Release);
                    this.listeners_live.store(false, Ordering::Release);
                    tracing::error!("DNS forwarder stopped");
                }
            });
            let this = self.clone();
            tokio::spawn(async move {
                if this.dns_tcp(tcp).await.is_err() {
                    this.ready.store(false, Ordering::Release);
                    this.listeners_live.store(false, Ordering::Release);
                    tracing::error!("DNS TCP forwarder stopped");
                }
            });
        }
        if let Ok(certificate) = std::env::var("ALIEN_SANDBOX_EGRESS_CA") {
            self.install_ca(&certificate)?;
        }
        if !self.current_token()?.is_empty() {
            self.self_check().await?;
        }
        Ok(())
    }

    /// Set the deployment endpoint from an authenticated Run hook before token delivery.
    pub async fn set_gateway(
        &self,
        authority: &str,
        audience: &str,
        sandbox_id: &str,
    ) -> Result<()> {
        if self.declared_none {
            return Err(invalid("declared none cannot acquire a gateway"));
        }
        if self.ready.load(Ordering::Acquire) {
            return Err(invalid("gateway already active"));
        }
        if audience.is_empty()
            || sandbox_id.is_empty()
            || audience.len() > 256
            || sandbox_id.len() > 256
        {
            return Err(invalid("invalid sandbox identity"));
        }
        {
            let mut identity = self
                .identity
                .write()
                .map_err(|_| invalid("identity lock poisoned"))?;
            if identity
                .as_ref()
                .is_some_and(|(aud, sub)| aud != audience || sub != sandbox_id)
            {
                return Err(invalid("sandbox identity cannot change"));
            }
            *identity = Some((audience.into(), sandbox_id.into()));
        }
        let address = dns::resolve_gateway(authority, self.privileged)
            .await
            .into_alien_error()
            .context(failed("resolve run-hook gateway"))?;
        *self
            .address
            .write()
            .map_err(|_| invalid("gateway lock poisoned"))? = Some(address);
        Ok(())
    }

    fn gateway_address(&self) -> Result<SocketAddr> {
        self.address
            .read()
            .map_err(|_| invalid("gateway lock poisoned"))?
            .ok_or_else(|| invalid("gateway endpoint pending"))
    }

    /// Install a replacement signed token. Signature verification and policy lookup happen in
    /// the gateway; the self-check refuses invalid credentials before reopening session routes.
    pub async fn update(&self, token: String) -> Result<()> {
        let _update = self.update_lock.lock().await;
        if self.declared_none {
            return Err(invalid("declared none cannot acquire an egress token"));
        }
        validate_token(&token)?;
        let (aud, sub) = token_identity(&token)?;
        if self
            .identity
            .read()
            .map_err(|_| invalid("identity lock poisoned"))?
            .as_ref()
            != Some(&(aud, sub))
        {
            return Err(invalid("token audience or sandbox ID mismatch"));
        }
        self.ready.store(false, Ordering::Release);
        *self
            .token
            .write()
            .map_err(|_| invalid("token lock poisoned"))? = token;
        self.self_check().await
    }

    /// Install the public per-sandbox CA supplied by management. Never accepts a private key.
    pub fn install_ca(&self, certificate: &str) -> Result<()> {
        if certificate.len() > 16 * 1024
            || !certificate.starts_with("-----BEGIN CERTIFICATE-----")
            || certificate.contains("PRIVATE KEY")
        {
            return Err(invalid("invalid egress CA certificate"));
        }
        let path = if self.privileged {
            "/opt/alien/egress-ca-bundle.pem"
        } else {
            "/tmp/alien-egress-ca-bundle.pem"
        };
        if self.privileged {
            std::fs::create_dir_all("/opt/alien")
                .into_alien_error()
                .context(failed("create egress CA directory"))?;
        }
        let system = std::fs::read("/etc/ssl/certs/ca-certificates.crt")
            .into_alien_error()
            .context(failed("read system trust roots"))?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o644)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .into_alien_error()
            .context(failed("create egress trust bundle"))?;
        file.write_all(&system)
            .into_alien_error()
            .context(failed("write trust roots"))?;
        file.write_all(certificate.as_bytes())
            .into_alien_error()
            .context(failed("write egress CA"))?;
        *self
            .ca_path
            .write()
            .map_err(|_| invalid("CA lock poisoned"))? = Some(path.into());
        Ok(())
    }

    /// Configure standard proxy-aware tools without disclosing the gateway token.
    pub fn command_environment(&self, env: &mut BTreeMap<String, String>) -> Result<()> {
        for key in ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"] {
            env.insert(
                key.into(),
                if self.declared_none {
                    String::new()
                } else {
                    "http://127.0.0.1:15002".into()
                },
            );
        }
        for key in ["ALL_PROXY", "all_proxy"] {
            env.insert(key.into(), String::new());
        }
        for key in ["NO_PROXY", "no_proxy"] {
            env.insert(key.into(), "localhost,127.0.0.1,127.0.0.0/8,::1".into());
        }
        env.remove("ALIEN_SANDBOX_EGRESS_TOKEN");
        if let Some(path) = self
            .ca_path
            .read()
            .map_err(|_| invalid("CA lock poisoned"))?
            .as_ref()
        {
            for key in [
                "SSL_CERT_FILE",
                "REQUESTS_CA_BUNDLE",
                "CURL_CA_BUNDLE",
                "NODE_EXTRA_CA_CERTS",
            ] {
                env.insert(key.into(), path.clone());
            }
        }
        Ok(())
    }

    fn current_token(&self) -> Result<String> {
        Ok(self
            .token
            .read()
            .map_err(|_| invalid("token lock poisoned"))?
            .clone())
    }

    async fn connect(&self, address: SocketAddr) -> Result<TcpStream> {
        let socket = redirect::socket(self.privileged)
            .into_alien_error()
            .context(failed("create agent control socket"))?;
        timeout(TIMEOUT, socket.connect(address))
            .await
            .into_alien_error()
            .context(failed("gateway timeout"))?
            .into_alien_error()
            .context(failed("connect gateway"))
    }

    async fn self_check(&self) -> Result<()> {
        // Marked sockets bypass the in-guest filter, proving the external network boundary.
        let canary: SocketAddr = SocketAddr::from(([1, 1, 1, 1], 443));
        let direct = timeout(Duration::from_secs(3), self.connect(canary)).await;
        if matches!(direct, Ok(Ok(_))) {
            return Err(AlienError::new(ErrorData::EgressPathUnexpected));
        }
        if self.declared_none {
            self.ready.store(true, Ordering::Release);
            return Ok(());
        }
        let mut gateway = self.connect(self.gateway_address()?).await?;
        let token = self.current_token()?;
        gateway.write_all(format!("GET /health HTTP/1.1\r\nHost: {}\r\nProxy-Authorization: Bearer {token}\r\nConnection: close\r\n\r\n", self.gateway_address()?).as_bytes())
            .await.into_alien_error().context(failed("request authenticated gateway health"))?;
        let reply = timeout(TIMEOUT, read_header(&mut gateway))
            .await
            .into_alien_error()
            .context(failed("self-check gateway timeout"))??;
        if !reply.starts_with(b"HTTP/1.1 200 ") {
            return Err(AlienError::new(ErrorData::EgressPathUnexpected));
        }
        self.ready.store(true, Ordering::Release);
        Ok(())
    }

    async fn send_connect(&self, upstream: &mut TcpStream, authority: &str) -> Result<()> {
        let token = self.current_token()?;
        if token.is_empty() {
            return Err(invalid("egress token pending"));
        }
        upstream.write_all(format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\nProxy-Authorization: Bearer {token}\r\n\r\n").as_bytes()).await.into_alien_error().context(failed("send gateway CONNECT"))
    }

    async fn serve(self: Arc<Self>, listener: TcpListener, transparent: bool) -> Result<()> {
        let limit = Arc::new(Semaphore::new(256));
        let mut tasks = JoinSet::new();
        loop {
            tokio::select! {
                connection = listener.accept() => {
                    let (stream, _) = connection.into_alien_error().context(failed("accept forwarder"))?;
                    let Ok(permit) = limit.clone().try_acquire_owned() else { continue; };
                    let this = self.clone();
                    tasks.spawn(async move { let _permit = permit; let _result = timeout(Duration::from_secs(3600), this.forward(stream, transparent)).await; });
                }
                result = tasks.join_next(), if !tasks.is_empty() => { if result.is_some_and(|r| r.is_err()) { return Err(invalid("forwarder task panicked")); } }
            }
        }
    }

    async fn forward(&self, mut guest: TcpStream, transparent: bool) -> Result<()> {
        self.ensure_ready()?;
        let mut upstream = self.connect(self.gateway_address()?).await?;
        if transparent {
            let destination = redirect::original_destination(&guest)
                .into_alien_error()
                .context(failed("read original destination"))?;
            let name = self
                .names
                .name(*destination.ip())
                .unwrap_or_else(|| destination.ip().to_string());
            self.send_connect(&mut upstream, &format!("{name}:{}", destination.port()))
                .await?;
            let response = timeout(TIMEOUT, read_header(&mut upstream))
                .await
                .into_alien_error()
                .context(failed("CONNECT response timeout"))??;
            if !response.starts_with(b"HTTP/1.1 200 ") {
                return Err(invalid("gateway refused transparent CONNECT"));
            }
        } else {
            let request = timeout(TIMEOUT, read_header(&mut guest))
                .await
                .into_alien_error()
                .context(failed("proxy header timeout"))??;
            let mut headers = [httparse::EMPTY_HEADER; 64];
            let mut parsed = httparse::Request::new(&mut headers);
            let httparse::Status::Complete(end) = parsed
                .parse(&request)
                .map_err(|_| invalid("invalid proxy request"))?
            else {
                return Err(invalid("incomplete proxy request"));
            };
            let mut head = format!(
                "{} {} HTTP/1.1\r\n",
                parsed.method.ok_or_else(|| invalid("missing method"))?,
                parsed.path.ok_or_else(|| invalid("missing URI"))?
            );
            for header in parsed.headers {
                if !header.name.eq_ignore_ascii_case("proxy-authorization") {
                    head.push_str(header.name);
                    head.push_str(": ");
                    head.push_str(
                        std::str::from_utf8(header.value).map_err(|_| invalid("invalid header"))?,
                    );
                    head.push_str("\r\n");
                }
            }
            head.push_str(&format!(
                "Proxy-Authorization: Bearer {}\r\n\r\n",
                self.current_token()?
            ));
            upstream
                .write_all(head.as_bytes())
                .await
                .into_alien_error()
                .context(failed("forward proxy headers"))?;
            upstream
                .write_all(&request[end..])
                .await
                .into_alien_error()
                .context(failed("forward buffered body"))?;
        }
        tokio::io::copy_bidirectional(&mut guest, &mut upstream)
            .await
            .into_alien_error()
            .context(failed("relay gateway connection"))?;
        Ok(())
    }

    async fn dns_udp(&self, socket: UdpSocket) -> Result<()> {
        let mut packet = [0; 512];
        loop {
            let (n, peer) = socket
                .recv_from(&mut packet)
                .await
                .into_alien_error()
                .context(failed("read DNS"))?;
            if let Some(reply) = self.names.answer(&packet[..n]) {
                socket
                    .send_to(&reply, peer)
                    .await
                    .into_alien_error()
                    .context(failed("reply DNS"))?;
            }
        }
    }

    async fn dns_tcp(&self, listener: TcpListener) -> Result<()> {
        loop {
            let (mut stream, _) = listener
                .accept()
                .await
                .into_alien_error()
                .context(failed("accept DNS"))?;
            // Sequential and bounded: DNS questions are tiny; no unbounded per-query tasks.
            let result = timeout(TIMEOUT, async {
                let n = stream.read_u16().await? as usize;
                if n > 512 {
                    return Err(std::io::Error::other("DNS question too large"));
                }
                let mut packet = vec![0; n];
                stream.read_exact(&mut packet).await?;
                if let Some(reply) = self.names.answer(&packet) {
                    stream.write_u16(reply.len() as u16).await?;
                    stream.write_all(&reply).await?;
                }
                Ok::<_, std::io::Error>(())
            })
            .await;
            if result.is_err() {
                continue;
            }
        }
    }
}

fn validate_token(token: &str) -> Result<()> {
    if token.is_empty()
        || token.len() > 4 * 1024
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        || token.split('.').count() != 3
    {
        return Err(invalid("invalid egress token encoding"));
    }
    Ok(())
}

async fn read_header(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let mut input = Vec::new();
    // Read to the exact boundary so CONNECT never discards server-first protocol bytes.
    loop {
        let byte = stream
            .read_u8()
            .await
            .into_alien_error()
            .context(failed("read proxy header"))?;
        input.push(byte);
        if input.len() > MAX_HEADER {
            return Err(invalid("proxy header too large"));
        }
        if input.ends_with(b"\r\n\r\n") {
            return Ok(input);
        }
    }
}

fn token_identity(token: &str) -> Result<(String, String)> {
    #[derive(serde::Deserialize)]
    struct Identity {
        aud: String,
        sub: String,
    }
    let payload = token
        .split('.')
        .nth(1)
        .ok_or_else(|| invalid("missing token payload"))?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .into_alien_error()
        .context(failed("decode token identity"))?;
    let identity: Identity = serde_json::from_slice(&bytes)
        .into_alien_error()
        .context(failed("parse token identity"))?;
    Ok((identity.aud, identity.sub))
}
