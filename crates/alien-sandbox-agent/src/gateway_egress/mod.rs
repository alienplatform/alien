//! Gateway forwarding. DNS returns synthetic addresses so arbitrary TCP retains its original name.
#![expect(
    clippy::result_large_err,
    reason = "Alien errors retain structured metadata and causal context"
)]

mod dns;
mod guest_network;
#[cfg(test)]
mod guest_network_tests;
mod probe;
mod recovery;
pub use probe::command_path_is_closed;
mod redirect;
mod transport;

use std::{
    collections::BTreeMap,
    io::Write,
    net::{SocketAddr, ToSocketAddrs},
    os::fd::AsRawFd,
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
    io::{AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream, UdpSocket},
    sync::{MutexGuard, Semaphore},
    task::JoinSet,
    time::timeout,
};

use crate::error::{ErrorData, Result};
use dns::Names;
use transport::{accept, read_header};

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
    policy: RwLock<Option<String>>,
    awaiting_delivery: AtomicBool,
    path_verified: AtomicBool,
    privileged: bool,
    command_identity: crate::exec::ExecIdentity,
    routing_ready: AtomicBool,
    declared_none: bool,
    ready: AtomicBool,
    listeners_live: AtomicBool,
    names: Names,
    image_started: AtomicBool,
    ca_path: RwLock<Option<String>>,
    identity: RwLock<Option<(String, String)>>,
    update_lock: tokio::sync::Mutex<()>,
    unanswered_since: std::sync::Mutex<Option<std::time::Instant>>,
}

impl GatewayEgress {
    /// Accepted control sockets inherit the agent mark. A Run hook can predate conntrack
    /// activation, so its replies must not depend on the kernel recognizing reply direction.
    pub fn protect_control_listener(&self, listener: &std::net::TcpListener) -> Result<()> {
        if self.privileged {
            redirect::mark(listener.as_raw_fd())
                .into_alien_error()
                .context(failed("mark agent control listener"))?;
        }
        Ok(())
    }

    /// Prepare routing, or defer it until the AWS Run hook. An absent token is pending until
    /// the authenticated policy endpoint delivers it outside the shared image snapshot.
    pub fn from_env(command_identity: crate::exec::ExecIdentity) -> Result<Option<Arc<Self>>> {
        if crate::egress::legacy_from_env()?.is_some() {
            return Ok(None);
        }
        let declared_none = std::env::var("ALIEN_SANDBOX_EGRESS")
            .ok()
            .is_some_and(|policy| {
                policy == "none"
                    || serde_json::from_str::<serde_json::Value>(&policy)
                        .is_ok_and(|value| value == "none")
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
        let defer_routing = match std::env::var("ALIEN_SANDBOX_EGRESS_RUN_HOOK") {
            Ok(value) if value == "true" => true,
            Ok(value) if value == "false" => false,
            Err(std::env::VarError::NotPresent) => false,
            _ => {
                return Err(invalid(
                    "ALIEN_SANDBOX_EGRESS_RUN_HOOK must be true or false",
                ))
            }
        };
        if defer_routing && !privileged {
            return Err(invalid("Run-hook firewall installation requires root"));
        }
        if privileged {
            guest_network::install()
                .into_alien_error()
                .context(failed("isolate guest sockets before starting image code"))?;
            // IPv6 sockets can exist on kernels without IPv6 netfilter support.
            // Every image command and exec child installs the irreversible socket
            // filter before executing; an installation failure prevents its launch.
            crate::privilege::block_command_ipv6();
            if !defer_routing {
                redirect::install()
                    .into_alien_error()
                    .context(failed("install transparent forwarding"))?;
            }
        }
        Ok(Some(Arc::new(Self {
            address: RwLock::new(address),
            token: RwLock::new(token),
            policy: RwLock::new(None),
            awaiting_delivery: AtomicBool::new(true),
            path_verified: AtomicBool::new(false),
            privileged,
            command_identity,
            routing_ready: AtomicBool::new(!defer_routing),
            declared_none,
            ready: AtomicBool::new(false),
            listeners_live: AtomicBool::new(false),
            names: Names::default(),
            image_started: AtomicBool::new(false),
            ca_path: RwLock::new(None),
            identity: RwLock::new(identity),
            update_lock: tokio::sync::Mutex::new(()),
            unanswered_since: std::sync::Mutex::new(None),
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
        let Ok(_update) = self.update_lock.try_lock() else {
            return Ok("verifying-egress-path");
        };
        if !self.routing_ready.load(Ordering::Acquire) {
            return Ok("awaiting-run-hook");
        }
        if !self.declared_none && self.current_token()?.is_empty() {
            return Ok("awaiting-egress-policy");
        }
        if !self.declared_none && self.awaiting_delivery.load(Ordering::Acquire) {
            if !self.path_verified.load(Ordering::Acquire) {
                return Err(AlienError::new(ErrorData::EgressPathUnexpected));
            }
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
        let _update = self.update_lock.lock().await;
        if !self.declared_none {
            return Err(invalid("gateway policy requires a token"));
        }
        self.ready.store(false, Ordering::Release);
        self.listeners_live.store(false, Ordering::Release);
        self.install_routing_at_run().await?;
        self.self_check().await?;
        self.listeners_live.store(true, Ordering::Release);
        Ok(())
    }

    /// Restored sessions must prove their current network path before accepting more work.
    pub async fn resume(&self) -> Result<()> {
        let _update = self.update_lock.lock().await;
        self.ready.store(false, Ordering::Release);
        if !self.routing_ready.load(Ordering::Acquire) {
            return Err(AlienError::new(ErrorData::EgressPathUnexpected));
        }
        // Delivery and the platform hook can arrive in either order. The token identifies
        // this sandbox; the gateway's current policy remains the authorization authority.
        self.awaiting_delivery.store(false, Ordering::Release);
        match self.self_check().await {
            Ok(()) => Ok(()),
            Err(_)
                if self.path_verified.load(Ordering::Acquire)
                    && self.awaiting_delivery.load(Ordering::Acquire) =>
            {
                // An expired/rejected identity needs management renewal. Keep commands
                // closed, but let the platform finish Resume so renewal can reach us.
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    /// Session requests wait for an active lifecycle check, without retrying its result.
    pub async fn wait_ready(&self) -> Result<()> {
        let _update = self.wait_for_run().await?;
        self.ensure_ready()
    }

    async fn wait_for_run(&self) -> Result<MutexGuard<'_, ()>> {
        timeout(Duration::from_secs(15), async {
            loop {
                let update = self.update_lock.lock().await;
                if self.routing_ready.load(Ordering::Acquire) {
                    return update;
                }
                // Run needs this same lock to install routing. Do not hold it while
                // waiting for a platform hook that may arrive after identity delivery.
                drop(update);
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .map_err(|_| {
            let cause = if self.routing_ready.load(Ordering::Acquire) {
                "check-in-progress"
            } else {
                "awaiting-run-hook"
            };
            tracing::warn!(cause, "egress readiness wait expired");
            AlienError::new(ErrorData::EgressPathUnexpected)
        })
    }

    /// Gate session routes until the network path has been verified.
    pub fn ensure_ready(&self) -> Result<()> {
        if self.routing_ready.load(Ordering::Acquire)
            && (self.declared_none || !self.awaiting_delivery.load(Ordering::Acquire))
            && self.ready.load(Ordering::Acquire)
            && self.listeners_live.load(Ordering::Acquire)
        {
            Ok(())
        } else {
            let cause = if !self.routing_ready.load(Ordering::Acquire) {
                "awaiting-run-hook"
            } else if self.awaiting_delivery.load(Ordering::Acquire)
                && self.path_verified.load(Ordering::Acquire)
            {
                "token-rejected"
            } else {
                "check-failed"
            };
            tracing::warn!(cause, "egress session refused");
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
            return if self.routing_ready.load(Ordering::Acquire) {
                self.self_check().await
            } else {
                Ok(())
            };
        }
        tokio::spawn(self.clone().monitor_policy());
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
        let _update = self.update_lock.lock().await;
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
        self.install_routing_at_run().await?;
        Ok(())
    }

    // The supervisor retains NET_ADMIN before creating worker threads. Installing here lets
    // the platform finish snapshot restoration before enabling the command UID rules.
    // Callers hold update_lock, so token installation cannot race this transaction.
    async fn install_routing_at_run(&self) -> Result<()> {
        if !self.routing_ready.load(Ordering::Acquire) {
            redirect::install_at_run()
                .await
                .into_alien_error()
                .context(failed("install Run-hook transparent forwarding"))?;
            self.routing_ready.store(true, Ordering::Release);
        }
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
    pub async fn update(&self, token: Option<String>, policy: String) -> Result<()> {
        let _update = self.wait_for_run().await?;
        if self.declared_none {
            return Err(invalid("declared none cannot acquire an egress token"));
        }
        let full_delivery = token.is_some();
        let token = match token {
            Some(token) => token,
            None => self.current_token()?,
        };
        validate_token(&token)?;
        let (aud, sub) = token_identity(&token)?;
        if self
            .identity
            .read()
            .map_err(|_| invalid("identity lock poisoned"))?
            .as_ref()
            != Some(&(aud.clone(), sub.clone()))
        {
            return Err(invalid("token audience or sandbox ID mismatch"));
        }
        if !full_delivery {
            // The gateway verifies the installed identity, including expiry. Refreshing
            // only the recovery copy must neither rotate its token nor release Resume.
            if !matches!(self.policy_request(None).await?, 200 | 404) {
                return Err(AlienError::new(ErrorData::EgressPathUnexpected));
            }
            return self.retain_record(policy, &aud, &sub);
        }
        self.retain_record(policy, &aud, &sub)?;
        self.awaiting_delivery.store(false, Ordering::Release);
        self.ready.store(false, Ordering::Release);
        *self
            .token
            .write()
            .map_err(|_| invalid("token lock poisoned"))? = token;
        self.self_check().await?;
        self.awaiting_delivery.store(false, Ordering::Release);
        Ok(())
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

    async fn check_path(&self) -> Result<()> {
        self.path_verified.store(false, Ordering::Release);
        // Marked sockets bypass the in-guest filter, proving the external network boundary.
        let canary: SocketAddr = SocketAddr::from(([1, 1, 1, 1], 443));
        let direct = timeout(Duration::from_secs(3), self.connect(canary)).await;
        if matches!(direct, Ok(Ok(_))) {
            return Err(AlienError::new(ErrorData::EgressPathUnexpected));
        }
        probe::supervisor_udp(self.privileged).await?;
        probe::command(self.command_identity).await?;
        self.path_verified.store(true, Ordering::Release);
        Ok(())
    }

    async fn self_check(&self) -> Result<()> {
        self.check_path().await.inspect_err(|error| {
            tracing::warn!(cause = "check-failed", code = %error.code, "physical egress check failed");
        })?;
        if !self.declared_none {
            self.check_policy().await.inspect_err(|error| {
                let cause = if self.awaiting_delivery.load(Ordering::Acquire) {
                    "token-rejected"
                } else {
                    "check-failed"
                };
                tracing::warn!(cause, code = %error.code, "gateway identity check failed");
            })?;
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
                connection = accept(&listener) => {
                    let (stream, _) = connection;
                    let Ok(permit) = limit.clone().try_acquire_owned() else { continue; };
                    let this = self.clone();
                    tasks.spawn(async move { let _permit = permit; let _result = timeout(Duration::from_secs(3600), this.forward(stream, transparent)).await; });
                }
                result = tasks.join_next(), if !tasks.is_empty() => { if let Some(Err(error)) = result { tracing::error!(%error, "forwarder task failed"); } }
            }
        }
    }

    async fn forward(&self, guest: TcpStream, transparent: bool) -> Result<()> {
        self.ensure_ready()?;
        let mut guest = BufReader::new(guest);
        let mut upstream = BufReader::new(self.connect(self.gateway_address()?).await?);
        if transparent {
            let destination = redirect::original_destination(guest.get_ref())
                .into_alien_error()
                .context(failed("read original destination"))?;
            let name = self
                .names
                .name(*destination.ip())
                .unwrap_or_else(|| destination.ip().to_string());
            self.send_connect(
                upstream.get_mut(),
                &format!("{name}:{}", destination.port()),
            )
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
            let (head, end) = gateway_request_head(&request, &self.current_token()?)?;
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
            let (n, peer) = match socket.recv_from(&mut packet).await {
                Ok(received) => received,
                Err(error) => {
                    tracing::warn!(%error, "DNS receive failed; retrying");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            };
            if let Some(reply) = self.names.answer(&packet[..n]) {
                if let Err(error) = socket.send_to(&reply, peer).await {
                    tracing::warn!(%error, "DNS reply failed");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }

    async fn dns_tcp(self: &Arc<Self>, listener: TcpListener) -> Result<()> {
        let capacity = Arc::new(Semaphore::new(32));
        let mut tasks = JoinSet::new();
        loop {
            tokio::select! {
                connection = accept(&listener) => {
                    let (mut stream, _) = connection;
                    let Ok(permit) = capacity.clone().try_acquire_owned() else { continue; };
                    let this = self.clone();
                    tasks.spawn(async move {
                        let _permit = permit;
                        let _ = timeout(TIMEOUT, async {
                            let n = stream.read_u16().await? as usize;
                            if n > 512 { return Err(std::io::Error::other("DNS question too large")); }
                            let mut packet = vec![0; n];
                            stream.read_exact(&mut packet).await?;
                            if let Some(reply) = this.names.answer(&packet) {
                                stream.write_u16(reply.len() as u16).await?;
                                stream.write_all(&reply).await?;
                            }
                            Ok::<_, std::io::Error>(())
                        }).await;
                    });
                }
                result = tasks.join_next(), if !tasks.is_empty() => {
                    if let Some(Err(error)) = result { tracing::error!(%error, "DNS task failed"); }
                }
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

/// The guest's proxy request head as the gateway receives it: HTTP/1.1, the guest's headers
/// without any proxy credentials it sent, and the session's identity token. Returns the head and
/// where the guest's buffered body starts.
///
/// HTTP/1.1 requires a `Host` header and the gateway refuses a request without exactly one.
/// Clients that speak HTTP/1.0 to a proxy, such as Python 3.11 and earlier opening a tunnel
/// (`CONNECT host:443 HTTP/1.0`), send none. The head takes it from the request target then: the
/// authority of a `CONNECT`, or of an absolute `http://` URI. A `Host` the guest sent is kept as
/// sent, so the gateway still checks it against the target.
fn gateway_request_head(request: &[u8], token: &str) -> Result<(String, usize)> {
    // Reserve one gateway field for the identity header inserted below.
    let mut headers = [httparse::EMPTY_HEADER; 127];
    let mut parsed = httparse::Request::new(&mut headers);
    let httparse::Status::Complete(end) = parsed
        .parse(request)
        .map_err(|_| invalid("invalid proxy request"))?
    else {
        return Err(invalid("incomplete proxy request"));
    };
    let method = parsed.method.ok_or_else(|| invalid("missing method"))?;
    let target = parsed.path.ok_or_else(|| invalid("missing URI"))?;
    let mut head = format!("{method} {target} HTTP/1.1\r\n");
    let mut has_host = false;
    for header in parsed.headers.iter() {
        if header.name.eq_ignore_ascii_case("proxy-authorization") {
            continue;
        }
        has_host |= header.name.eq_ignore_ascii_case("host");
        head.push_str(header.name);
        head.push_str(": ");
        head.push_str(std::str::from_utf8(header.value).map_err(|_| invalid("invalid header"))?);
        head.push_str("\r\n");
    }
    if !has_host {
        let authority = if method.eq_ignore_ascii_case("CONNECT") {
            target.to_string()
        } else {
            let rest = target
                .get(..7)
                .filter(|scheme| scheme.eq_ignore_ascii_case("http://"))
                .map(|_| &target[7..])
                .ok_or_else(|| invalid("a proxy request needs an absolute http URI"))?;
            let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
            if authority.is_empty() {
                return Err(invalid("a proxy request needs an absolute http URI"));
            }
            authority.to_string()
        };
        head.push_str(&format!("Host: {authority}\r\n"));
    }
    head.push_str(&format!("Proxy-Authorization: Bearer {token}\r\n\r\n"));
    Ok((head, end))
}

#[cfg(test)]
mod forwarder_tests {
    use super::gateway_request_head;

    fn hosts(head: &str) -> Vec<&str> {
        head.lines()
            .filter_map(|line| line.strip_prefix("Host: "))
            .collect()
    }

    /// Python 3.11 and earlier open a tunnel with `CONNECT host:443 HTTP/1.0` and no Host. The
    /// gateway refuses an HTTP/1.1 request without exactly one Host, so every HTTPS request from
    /// such a guest got 403. The forwarded head carries the target's authority as its Host.
    #[test]
    fn an_http_1_0_tunnel_without_host_gets_its_target_as_host() {
        let (head, end) =
            gateway_request_head(b"CONNECT example.com:443 HTTP/1.0\r\n\r\nbody", "token")
                .unwrap();
        assert!(head.starts_with("CONNECT example.com:443 HTTP/1.1\r\n"), "{head}");
        assert_eq!(hosts(&head), ["example.com:443"]);
        assert!(head.ends_with("Proxy-Authorization: Bearer token\r\n\r\n"));
        assert_eq!(end, "CONNECT example.com:443 HTTP/1.0\r\n\r\n".len());
    }

    /// A plain HTTP/1.0 proxy request without Host takes the absolute URI's authority.
    #[test]
    fn an_http_1_0_plain_request_without_host_gets_the_uri_authority() {
        let (head, _) =
            gateway_request_head(b"GET http://example.com/path HTTP/1.0\r\n\r\n", "token")
                .unwrap();
        assert_eq!(hosts(&head), ["example.com"]);
    }

    /// A Host the guest sent is forwarded as sent, never replaced or duplicated, so the
    /// gateway's own mismatch check still applies; guest proxy credentials never pass.
    #[test]
    fn a_guest_host_is_kept_and_guest_credentials_are_dropped() {
        let (head, _) = gateway_request_head(
            b"CONNECT example.com:443 HTTP/1.1\r\nHost: other.com:443\r\nProxy-Authorization: Basic guest\r\n\r\n",
            "token",
        )
        .unwrap();
        assert_eq!(hosts(&head), ["other.com:443"]);
        assert!(!head.contains("Basic guest"));
        assert_eq!(head.matches("Proxy-Authorization").count(), 1);
    }
}
