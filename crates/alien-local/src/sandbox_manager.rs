//! Local sandbox sessions on Docker.
//!
//! Deliberately not `LocalContainerManager`. That manager attaches a shared network, maps
//! `host.docker.internal:host-gateway` into every container, and restarts on exit — all
//! correct for a service and all wrong for hostile code. Reusing it would be a security
//! regression dressed as reuse.
//!
//! **Docker is a shared kernel.** Hardening narrows the attack surface; it does not make
//! container escape out of scope. Local is development-only for untrusted code unless a
//! sandboxed runtime such as gVisor or Kata is present and verified.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use alien_bindings::sandbox_egress_token::GeneratedSigningKey;

use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::models::{ContainerCreateBody, HostConfig, NetworkCreateRequest, PortBinding};
use bollard::query_parameters::{
    CreateContainerOptions, CreateImageOptions, KillContainerOptions, ListContainersOptions,
    RemoveContainerOptions,
};
use bollard::Docker;
use futures::StreamExt;
use tokio::io::AsyncWriteExt;

use crate::error::{ErrorData, Result};
use crate::sandbox_gateway::{LocalGateway, SessionEgress};
use alien_error::{AlienError, Context, IntoAlienError};

/// Label carrying the sandbox resource a container belongs to.
const LABEL_SANDBOX: &str = "dev.alien.sandbox";
/// Label carrying the session id within that sandbox.
pub(crate) const LABEL_SESSION: &str = "dev.alien.sandbox.session";
/// Label carrying the `uid:gid` every command in the session runs as. Set by the manager when it
/// creates the container and fixed from then on: nothing inside the container can change it.
const LABEL_EXEC_USER: &str = "dev.alien.sandbox.exec-user";
/// Prefix of the environment names the manager sets in a session. A session's own environment
/// may not use it, so code in the session never reads a value the declaration chose.
const RESERVED_ENV_PREFIX: &str = "ALIEN_SANDBOX_";

/// Where a session's writable area is mounted, and what every caller-supplied path resolves
/// against. The same root the in-sandbox agent uses on the cloud backends, so one path means
/// the same file everywhere.
const SESSION_ROOT: &str = "/sandbox";

/// Outbound network policy for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxEgressMode {
    /// No network interface at all.
    Deny,
    /// A session-private bridge with internet access.
    Allow,
}

/// What a session is allowed to consume.
#[derive(Debug, Clone, PartialEq)]
pub struct SandboxSessionConfig {
    /// Image used as the session's root filesystem
    pub image: String,
    /// CPU ceiling in cores
    pub cpu_cores: f64,
    /// Memory ceiling in bytes
    pub memory_bytes: i64,
    /// Maximum number of processes, which bounds fork bombs
    pub pids_limit: Option<i64>,
    /// Writable scratch size in bytes; the root filesystem itself is read-only
    pub scratch_bytes: u64,
    /// Outbound network policy
    pub egress: SandboxEgressMode,
    /// Ports eligible for a preview capability. Docker fixes published ports at create time,
    /// so this cannot be decided later — which is also the property that stops an application
    /// widening its own ingress at runtime.
    pub preview_ports: Vec<u16>,
    /// Environment placed in the session
    pub env: HashMap<String, String>,
    /// Services of the deployment the session reaches through the sandbox's egress gateway.
    /// With `Allow`, the session sits alone on its own internal network with the gateway, which
    /// is its only way out; these are the services it can reach through it.
    pub links: Vec<SandboxLink>,
    /// The declaration's `maxLifetimeSeconds`: the route ends each session this long after it
    /// is created. `None`, a session runs until it is terminated.
    pub max_lifetime: Option<std::time::Duration>,
    /// The policy a session's gateway enforces when the create names none: the declaration's
    /// default. `None`, the whole public internet.
    pub egress_policy: Option<alien_core::EgressPolicy>,
}

/// A service a sandbox links: a container on `deployment-network`, by its resource id, on its
/// declared ports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxLink {
    /// Resource id; the service answers as `<id>.svc` on `deployment-network`.
    pub id: String,
    /// Declared ports; every other port stays refused.
    pub ports: Vec<u16>,
}

/// A session the manager is tracking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxSessionHandle {
    /// Session id within the sandbox
    pub session_id: String,
    /// Docker container backing it
    pub container_id: String,
    /// Whether the container is running. One that stopped does not start again: there is no
    /// restart policy, and a session killed from the host stays stopped.
    pub running: bool,
}

/// One frame of a running command's output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxOutput {
    /// Bytes written to stdout
    Stdout(Vec<u8>),
    /// Bytes written to stderr
    Stderr(Vec<u8>),
}

/// A finished command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxExecResult {
    /// Frames in the order they were produced
    pub output: Vec<SandboxOutput>,
    /// Process exit code
    pub exit_code: i64,
}

/// Creates and destroys hardened Docker sessions for one deployment.
#[derive(Debug)]
pub struct LocalSandboxManager {
    docker: Docker,
    docker_host: String,
    state_dir: PathBuf,
    /// Signs this process's gateway tokens and policy writes. Generated per process: a restart
    /// reaps every gateway and session.
    gateway_key: GeneratedSigningKey,
    /// Running gateways, by sandbox. Held across a gateway's create so two sessions do not
    /// start two.
    gateways: tokio::sync::Mutex<HashMap<String, Arc<LocalGateway>>>,
    /// Image of the egress gateway every sandbox that reaches anything runs behind.
    gateway_image: String,
    /// Seeds the session CAs of every gateway. Per process, like `gateway_key`, so a gateway
    /// recreated for changed links keeps issuing the CA its sessions already trust.
    gateway_secret: String,
}

impl LocalSandboxManager {
    /// Connects to the local Docker daemon.
    pub fn new(state_dir: PathBuf) -> Result<Self> {
        let (docker, docker_host) = crate::docker_connection::connect_docker_with_host()?;

        Ok(Self {
            docker,
            docker_host,
            state_dir,
            gateway_key: alien_bindings::sandbox_egress_token::generate_signing_key(),
            gateways: Default::default(),
            gateway_image: alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_IMAGE.to_string(),
            gateway_secret: new_gateway_secret(),
        })
    }

    /// Runs sandboxes with links behind this gateway image instead of the one this release
    /// pins, such as a gateway built from source.
    pub fn with_gateway_image(mut self, image: impl Into<String>) -> Self {
        self.gateway_image = image.into();
        self
    }

    /// Where this manager keeps its session state.
    pub fn state_dir(&self) -> &PathBuf {
        &self.state_dir
    }

    fn container_name(sandbox: &str, session_id: &str) -> String {
        format!("alien-sbx-{sandbox}-{session_id}")
    }

    /// One egress network per sandbox, not per session.
    ///
    /// A bridge per session does not isolate sessions: every bridge lives on the same host and
    /// the host routes between them, so a neighbouring session stays reachable. One bridge with
    /// inter-container communication disabled blocks session-to-session traffic natively and
    /// still allows outbound.
    fn network_name(sandbox: &str) -> String {
        format!("alien-sbx-net-{sandbox}")
    }

    /// Creates a session and leaves it running until terminated.
    pub async fn create_session(
        &self,
        sandbox: &str,
        session_id: &str,
        config: &SandboxSessionConfig,
    ) -> Result<SandboxSessionHandle> {
        if let Some(reserved) = config
            .env
            .keys()
            .find(|name| name.starts_with(RESERVED_ENV_PREFIX))
        {
            return Err(AlienError::new(ErrorData::SandboxSessionFailed {
                session_id: session_id.to_string(),
                operation: format!(
                    "environment variable {reserved:?} is reserved: names starting with \
                     {RESERVED_ENV_PREFIX} are set by the sandbox itself"
                ),
            }));
        }

        // Docker accepts port bindings on a network-less container and silently drops them, so
        // a preview port under deny egress would look configured and never resolve.
        if !config.preview_ports.is_empty() && config.egress == SandboxEgressMode::Deny {
            return Err(AlienError::new(ErrorData::SandboxSessionFailed {
                session_id: session_id.to_string(),
                operation: "preview ports require egress; a session with no interface cannot \
                            serve one"
                    .to_string(),
            }));
        }

        let image_reference = self.ensure_image(&config.image, sandbox).await?;
        let image =
            crate::sandbox_identity::inspect(&self.docker, &image_reference, sandbox).await?;

        // Every session that reaches anything goes through the sandbox's gateway, links or not,
        // so its policy and credential rules apply as on the cloud platforms. A session's
        // preview ports are published by a relay container beside it (see
        // `start_preview_relays`), because Docker publishes nothing from an internal network.
        // A gateway image that predates admin protocol 2 keeps the earlier behavior: only
        // sessions with links run behind it, and it publishes no preview relays.
        // An image with no digest (a local build, a `:dev` tag) cannot be judged by name: it is
        // taken as current until its gateway has started and said which protocol it serves.
        let gateway_current = self.gateway_serves_credentials(sandbox).await;
        let linked = config.egress == SandboxEgressMode::Allow
            && (gateway_current || !config.links.is_empty());
        let created = async {
            let session_egress = if linked {
                let gateway = self.gateway(sandbox, &config.links).await?;
                if gateway.serves_credentials() || !config.links.is_empty() {
                    Some(
                        crate::sandbox_gateway::attach_session(
                            &self.docker,
                            &gateway,
                            sandbox,
                            session_id,
                            config
                                .egress_policy
                                .as_ref()
                                .unwrap_or(&alien_core::EgressPolicy::Internet),
                        )
                        .await?,
                    )
                } else {
                    // An older gateway behind an undigested tag: only linked sessions use it.
                    None
                }
            } else {
                None
            };
            self.create_session_container(sandbox, session_id, config, image, session_egress)
                .await
        }
        .await;
        if created.is_err() && linked {
            // The container never ran; its network and policy go with it. The create error is
            // what the caller needs, so a cleanup failure is only logged.
            let gateway = self.gateways.lock().await.get(sandbox).cloned();
            if let Err(error) = crate::sandbox_gateway::detach_session(
                &self.docker,
                gateway.as_deref(),
                sandbox,
                session_id,
            )
            .await
            {
                tracing::warn!(%error, session_id, "could not remove a failed session's network");
            }
        }
        created
    }

    async fn create_session_container(
        &self,
        sandbox: &str,
        session_id: &str,
        config: &SandboxSessionConfig,
        image: crate::sandbox_identity::SandboxImage,
        session_egress: Option<SessionEgress>,
    ) -> Result<SandboxSessionHandle> {
        let name = Self::container_name(sandbox, session_id);
        let network_mode = match (&session_egress, config.egress) {
            (Some(egress), _) => egress.network.clone(),
            // "none" gives no interface at all, which is a stronger and simpler guarantee than
            // a private network with its gateway firewalled off.
            (None, SandboxEgressMode::Deny) => "none".to_string(),
            (None, SandboxEgressMode::Allow) => {
                let network = Self::network_name(sandbox);
                self.ensure_egress_network(&network).await?;
                network
            }
        };

        let mut env: Vec<String> = config
            .env
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect();
        env.extend([
            format!("ALIEN_SANDBOX_EXEC_UID={}", image.identity.uid),
            format!("ALIEN_SANDBOX_EXEC_GID={}", image.identity.gid),
            format!("ALIEN_SANDBOX_ROOT={SESSION_ROOT}"),
        ]);
        let mut ca_mount = None;
        if let Some(egress) = &session_egress {
            env.extend(egress.env.iter().cloned());
            if let Some(ca) = &egress.ca {
                let directory = self.ca_directory(&name);
                write_ca_files(&directory, ca, None)
                    .await
                    .into_alien_error()
                    .context(ErrorData::SandboxSessionFailed {
                        session_id: session_id.to_string(),
                        operation: "write the session's CA".to_string(),
                    })?;
                env.extend(ca_env());
                ca_mount = Some(directory);
            }
        }

        let labels = HashMap::from([
            (LABEL_SANDBOX.to_string(), sandbox.to_string()),
            (LABEL_SESSION.to_string(), session_id.to_string()),
            (
                LABEL_EXEC_USER.to_string(),
                format!("{}:{}", image.identity.uid, image.identity.gid),
            ),
        ]);

        // Bound to 127.0.0.1, never 0.0.0.0: a preview is for the developer's own machine, and
        // publishing on all interfaces would expose untrusted code to the local network.
        let direct_preview_ports: &[u16] = if session_egress.is_some() {
            &[]
        } else {
            &config.preview_ports
        };
        let port_bindings: HashMap<String, Option<Vec<PortBinding>>> = direct_preview_ports
            .iter()
            .map(|port| {
                (
                    format!("{port}/tcp"),
                    Some(vec![PortBinding {
                        host_ip: Some("127.0.0.1".to_string()),
                        host_port: None,
                    }]),
                )
            })
            .collect();

        let exposed_ports: Vec<String> = direct_preview_ports
            .iter()
            .map(|port| format!("{port}/tcp"))
            .collect();

        let host_config = HostConfig {
            network_mode: Some(network_mode),
            port_bindings: if port_bindings.is_empty() {
                None
            } else {
                Some(port_bindings)
            },
            // Never map the host gateway in. LocalContainerManager does, which is why this
            // manager exists.
            extra_hosts: None,
            readonly_rootfs: Some(true),
            tmpfs: Some(HashMap::from([(
                SESSION_ROOT.to_string(),
                format!(
                    "rw,noexec,nosuid,mode=0700,uid={},gid={},size={}",
                    image.identity.uid, image.identity.gid, config.scratch_bytes
                ),
            )])),
            // CAP_KILL is the one capability kept, and only root processes hold it: the idle
            // loop and the signal shell. It is what lets that shell stop the session's user,
            // which root without it cannot. Every command runs as the session's user (see
            // `session_user`), so it holds none, and no-new-privileges stops it from becoming
            // root through a setuid binary.
            cap_drop: Some(vec!["ALL".to_string()]),
            cap_add: Some(vec!["KILL".to_string()]),
            security_opt: Some(vec!["no-new-privileges:true".to_string()]),
            init: Some(true),
            // The session's CA and trust bundle, read-only, for HTTPS the gateway intercepts.
            binds: ca_mount
                .as_ref()
                .map(|directory| vec![format!("{}:{CA_MOUNT}:ro", directory.display())]),
            pids_limit: config.pids_limit,
            memory: Some(config.memory_bytes),
            nano_cpus: Some((config.cpu_cores * 1_000_000_000.0) as i64),
            // No restart policy: a sandbox that exits stays exited. Restarting hostile code
            // would silently hand it another attempt.
            ..Default::default()
        };

        let container_config = ContainerCreateBody {
            image: Some(image.id),
            // Root, so that the signal shell, a root exec, holds CAP_KILL. Some Docker versions
            // (20.10, for one) give an exec only the capabilities its container's user holds, so
            // under a non-root container user a root exec holds none. No command runs as this user:
            // each exec names its own.
            user: Some(ROOT_USER.to_string()),
            // Docker can create/chmod WorkingDir after mounting tmpfs, resetting its owner.
            // Enter the session root on exec, after the mount has its final ownership.
            working_dir: Some("/".to_string()),
            env: Some(env),
            labels: Some(labels),
            exposed_ports: if exposed_ports.is_empty() {
                None
            } else {
                Some(exposed_ports)
            },
            // Hold the session open without a shell of its own; commands arrive through exec. One
            // sleep that never forks again, so a session out of process slots does not spin it.
            // Whole seconds, which every `sleep` reads, unlike `infinity`. Docker's init runs it
            // as pid 1's child, and reaps every process orphaned in the session: a command killed
            // with its children leaves them to pid 1, and unreaped they would hold process slots.
            entrypoint: Some(vec!["sleep".to_string()]),
            cmd: Some(vec!["2147483647".to_string()]),
            host_config: Some(host_config),
            ..Default::default()
        };

        let created = self
            .docker
            .create_container(
                Some(CreateContainerOptions {
                    name: Some(name.clone()),
                    ..Default::default()
                }),
                container_config,
            )
            .await
            .into_alien_error()
            .context(ErrorData::SandboxSessionFailed {
                session_id: session_id.to_string(),
                operation: "create".to_string(),
            })?;

        if let (Some(directory), Some(ca)) = (
            &ca_mount,
            session_egress
                .as_ref()
                .and_then(|egress| egress.ca.as_ref()),
        ) {
            // The image's own roots stay trusted: the bundle is them plus the session's CA.
            let system = self.system_bundle(&created.id).await;
            write_ca_files(directory, ca, system.as_deref())
                .await
                .into_alien_error()
                .context(ErrorData::SandboxSessionFailed {
                    session_id: session_id.to_string(),
                    operation: "write the session's trust bundle".to_string(),
                })?;
        }

        self.docker
            .start_container(&created.id, None)
            .await
            .into_alien_error()
            .context(ErrorData::SandboxSessionFailed {
                session_id: session_id.to_string(),
                operation: "start".to_string(),
            })?;

        let relays = self.gateway_serves_credentials(sandbox).await;
        if let Some(egress) = session_egress.as_ref().filter(|_| relays) {
            if let Err(error) = crate::sandbox_gateway::start_preview_relays(
                &self.docker,
                &self.gateway_image,
                session_id,
                &name,
                &egress.network,
                &config.preview_ports,
            )
            .await
            {
                let _ =
                    crate::sandbox_gateway::remove_preview_relays(&self.docker, session_id, &name)
                        .await;
                let _ = self
                    .docker
                    .remove_container(
                        &created.id,
                        Some(RemoveContainerOptions {
                            force: true,
                            v: true,
                            ..Default::default()
                        }),
                    )
                    .await;
                return Err(error);
            }
        }

        Ok(SandboxSessionHandle {
            session_id: session_id.to_string(),
            container_id: created.id,
            running: true,
        })
    }

    /// Whether the sandbox's gateway serves admin protocol 2. A pinned image answers by its
    /// digest; an undigested one by the protocol its running gateway reported (taken as current
    /// until one has started).
    async fn gateway_serves_credentials(&self, sandbox: &str) -> bool {
        if !alien_core::sandbox_egress::gateway_image_serves_credentials(&self.gateway_image) {
            return false;
        }
        match self.gateways.lock().await.get(sandbox) {
            Some(gateway) => gateway.serves_credentials(),
            None => true,
        }
    }

    /// The sandbox's gateway, started or replaced for these links.
    async fn gateway(&self, sandbox: &str, links: &[SandboxLink]) -> Result<Arc<LocalGateway>> {
        let mut gateways = self.gateways.lock().await;
        self.ensure_gateway(&mut gateways, sandbox, links).await
    }

    /// Applies the sandbox's current links to its running gateway, if it has one: a link
    /// added or removed in the stack takes effect for the sessions already running, not only
    /// for the next one. With no links left, the gateway keeps proxying those sessions'
    /// internet egress and reaches nothing private. A sandbox without a running gateway is
    /// left alone; its next linked session starts one with these links.
    pub async fn apply_links(&self, sandbox: &str, links: &[SandboxLink]) -> Result<()> {
        let mut gateways = self.gateways.lock().await;
        if !gateways.contains_key(sandbox) {
            return Ok(());
        }
        self.ensure_gateway(&mut gateways, sandbox, links)
            .await
            .map(drop)
    }

    async fn ensure_gateway(
        &self,
        gateways: &mut HashMap<String, Arc<LocalGateway>>,
        sandbox: &str,
        links: &[SandboxLink],
    ) -> Result<Arc<LocalGateway>> {
        let range = crate::sandbox_gateway::deployment_network_range(&self.docker).await?;
        let links: Vec<alien_core::sandbox_links::GatewayLink> = links
            .iter()
            .map(|link| alien_core::sandbox_links::GatewayLink {
                id: link.id.clone(),
                host: format!("{}.svc", link.id),
                ports: link.ports.clone(),
                ranges: vec![range],
                addresses: Vec::new(),
            })
            .collect();
        let gateway = crate::sandbox_gateway::ensure_gateway(
            &self.docker,
            &self.gateway_key,
            &self.gateway_secret,
            sandbox,
            &self.gateway_image,
            &links,
            gateways.get(sandbox).cloned(),
        )
        .await?;
        gateways.insert(sandbox.to_string(), Arc::clone(&gateway));
        Ok(gateway)
    }

    /// Replaces a session's credential rules on its sandbox's gateway. Returns once the gateway
    /// holds them, so the session's next request uses them.
    pub async fn update_credentials(
        &self,
        sandbox: &str,
        session_id: &str,
        rules: &[alien_core::SandboxCredentialRule],
    ) -> Result<()> {
        if !self.gateway_serves_credentials(sandbox).await {
            return Err(AlienError::new(ErrorData::SandboxSessionFailed {
                session_id: session_id.to_string(),
                operation: format!(
                    "update credentials: {}",
                    alien_core::sandbox_egress::CREDENTIALS_UNAVAILABLE
                ),
            }));
        }
        let gateway = self
            .gateways
            .lock()
            .await
            .get(sandbox)
            .cloned()
            .ok_or_else(|| {
                AlienError::new(ErrorData::SandboxSessionFailed {
                    session_id: session_id.to_string(),
                    operation: "update credentials: this sandbox runs no egress gateway in \
                                alien dev. A sandbox with egress `none` reaches nothing to put a \
                                credential on"
                        .to_string(),
                })
            })?;
        gateway.update_credentials(session_id, rules).await
    }

    /// Host directory with one session's CA and trust bundle, bind-mounted read-only.
    fn ca_directory(&self, container: &str) -> PathBuf {
        self.state_dir.join("sandbox-egress-ca").join(container)
    }

    /// The image's CA bundle, read from the created container before it starts.
    async fn system_bundle(&self, container_id: &str) -> Option<String> {
        use futures::TryStreamExt;
        for path in SYSTEM_BUNDLES {
            let archive: Vec<u8> = match self
                .docker
                .download_from_container(
                    container_id,
                    Some(bollard::query_parameters::DownloadFromContainerOptions {
                        path: path.to_string(),
                    }),
                )
                .map_ok(|chunk| chunk.to_vec())
                .try_concat()
                .await
            {
                Ok(archive) => archive,
                Err(_) => continue,
            };
            let mut entries = tar::Archive::new(archive.as_slice());
            let Ok(mut entries) = entries.entries() else {
                continue;
            };
            if let Some(Ok(mut entry)) = entries.next() {
                let mut text = String::new();
                if std::io::Read::read_to_string(&mut entry, &mut text).is_ok()
                    && text.contains("BEGIN CERTIFICATE")
                {
                    return Some(text);
                }
            }
        }
        None
    }

    /// Makes the sandbox image available to the daemon and returns the reference to create from.
    ///
    /// A sandbox cannot start without its root filesystem, and Docker's create call does not
    /// pull. A build directory or archive is loaded the way a local Container's is; any other
    /// reference is inspected first, which keeps the common case off the network.
    async fn ensure_image(&self, image: &str, sandbox: &str) -> Result<String> {
        if std::path::Path::new(image).exists() {
            return crate::container_manager::load_built_image(
                &self.docker,
                &self.docker_host,
                std::path::Path::new(image),
                sandbox,
            )
            .await;
        }
        if self.docker.inspect_image(image).await.is_ok() {
            return Ok(image.to_string());
        }

        let mut pull = self.docker.create_image(
            Some(CreateImageOptions {
                from_image: Some(image.to_string()),
                ..Default::default()
            }),
            None,
            None,
        );

        while let Some(progress) = pull.next().await {
            progress
                .into_alien_error()
                .context(ErrorData::SandboxSessionFailed {
                    session_id: image.to_string(),
                    operation: "pull sandbox image".to_string(),
                })?;
        }

        Ok(image.to_string())
    }

    async fn ensure_egress_network(&self, name: &str) -> Result<()> {
        match self.docker.inspect_network(name, None).await {
            Ok(_) => Ok(()),
            Err(_) => {
                self.docker
                    .create_network(NetworkCreateRequest {
                        name: name.to_string(),
                        driver: Some("bridge".to_string()),
                        enable_ipv6: Some(false),
                        options: Some(HashMap::from([(
                            "com.docker.network.bridge.enable_icc".to_string(),
                            "false".to_string(),
                        )])),
                        ..Default::default()
                    })
                    .await
                    .into_alien_error()
                    .context(ErrorData::SandboxSessionFailed {
                        session_id: name.to_string(),
                        operation: "create network".to_string(),
                    })?;
                Ok(())
            }
        }
    }

    /// Runs a command inside a session and collects its output and exit code.
    pub async fn exec(&self, container_id: &str, command: &[String]) -> Result<SandboxExecResult> {
        let exec = self
            .docker
            .create_exec(
                container_id,
                CreateExecOptions {
                    cmd: Some(command.to_vec()),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    user: Some(self.session_user(container_id).await?),
                    working_dir: Some(SESSION_ROOT.to_string()),
                    ..Default::default()
                },
            )
            .await
            .into_alien_error()
            .context(ErrorData::SandboxSessionFailed {
                session_id: container_id.to_string(),
                operation: "create exec".to_string(),
            })?;

        let started = self
            .docker
            .start_exec(&exec.id, None)
            .await
            .into_alien_error()
            .context(ErrorData::SandboxSessionFailed {
                session_id: container_id.to_string(),
                operation: "start exec".to_string(),
            })?;

        let mut output = Vec::new();
        if let StartExecResults::Attached {
            output: mut stream, ..
        } = started
        {
            while let Some(frame) = stream.next().await {
                let frame = frame
                    .into_alien_error()
                    .context(ErrorData::SandboxSessionFailed {
                        session_id: container_id.to_string(),
                        operation: "read exec output".to_string(),
                    })?;

                match frame {
                    bollard::container::LogOutput::StdOut { message } => {
                        output.push(SandboxOutput::Stdout(message.to_vec()))
                    }
                    bollard::container::LogOutput::StdErr { message } => {
                        output.push(SandboxOutput::Stderr(message.to_vec()))
                    }
                    _ => {}
                }
            }
        }

        let inspect = self
            .docker
            .inspect_exec(&exec.id)
            .await
            .into_alien_error()
            .context(ErrorData::SandboxSessionFailed {
                session_id: container_id.to_string(),
                operation: "inspect exec".to_string(),
            })?;

        // A missing exit code means the command did not finish, which is not success.
        let exit_code = inspect.exit_code.ok_or_else(|| {
            AlienError::new(ErrorData::SandboxSessionFailed {
                session_id: container_id.to_string(),
                operation: "exec finished without an exit code".to_string(),
            })
        })?;

        Ok(SandboxExecResult { output, exit_code })
    }

    /// Starts a command with stdin attached, for an interactive command.
    ///
    /// Returns the attached stdin, the output stream, and a handle to read the exit code once
    /// the output ends. `cwd` resolves against the session root as file paths do; `env` is added
    /// to the container's own, as Docker's exec does.
    pub async fn exec_interactive(
        &self,
        container_id: &str,
        command: &[String],
        cwd: Option<&str>,
        env: &std::collections::BTreeMap<String, String>,
    ) -> Result<InteractiveExec> {
        let working_dir = match cwd {
            Some(path) => resolve_dir_in_root(path)?,
            None => SESSION_ROOT.to_string(),
        };
        self.exec_attached(
            container_id,
            attached_exec_options(
                command,
                working_dir,
                env,
                self.session_user(container_id).await?,
            ),
        )
        .await
    }

    /// Starts the session's signal shell: `command` as root, stdin attached, never privileged.
    ///
    /// Root, because the session's user can kill anything that runs as that user, including the
    /// shell meant to stop it. As root in a container that keeps only `CAP_KILL`, the shell
    /// holds that one capability: it can signal the session's user, and the session's user,
    /// holding none, cannot signal it. The root filesystem is read-only, so the command cannot
    /// replace what the shell runs.
    pub async fn exec_signal_shell(
        &self,
        container_id: &str,
        command: &[String],
    ) -> Result<InteractiveExec> {
        self.exec_attached(
            container_id,
            attached_exec_options(
                command,
                SESSION_ROOT.to_string(),
                &std::collections::BTreeMap::new(),
                ROOT_USER.to_string(),
            ),
        )
        .await
    }

    /// The user every command in the session runs as: the image's resolved, non-root identity,
    /// from the label the manager set when it created the container. Named on each exec,
    /// because the container's own user is root. Missing, or root, it fails rather than fall
    /// back to root.
    async fn session_user(&self, container_id: &str) -> Result<String> {
        let inspected = self
            .docker
            .inspect_container(container_id, None)
            .await
            .into_alien_error()
            .context(ErrorData::SandboxSessionFailed {
                session_id: container_id.to_string(),
                operation: "inspect for the session's user".to_string(),
            })?;
        let label = inspected
            .config
            .and_then(|config| config.labels)
            .and_then(|labels| labels.get(LABEL_EXEC_USER).cloned());
        exec_user(label.as_deref()).map_err(|reason| {
            AlienError::new(ErrorData::SandboxSessionFailed {
                session_id: container_id.to_string(),
                operation: reason.to_string(),
            })
        })
    }

    async fn exec_attached(
        &self,
        container_id: &str,
        options: CreateExecOptions<String>,
    ) -> Result<InteractiveExec> {
        let exec = self
            .docker
            .create_exec(container_id, options)
            .await
            .into_alien_error()
            .context(ErrorData::SandboxSessionFailed {
                session_id: container_id.to_string(),
                operation: "create interactive exec".to_string(),
            })?;

        let started = self
            .docker
            .start_exec(&exec.id, None)
            .await
            .into_alien_error()
            .context(ErrorData::SandboxSessionFailed {
                session_id: container_id.to_string(),
                operation: "start interactive exec".to_string(),
            })?;

        let StartExecResults::Attached { input, output } = started else {
            return Err(AlienError::new(ErrorData::SandboxSessionFailed {
                session_id: container_id.to_string(),
                operation: "start interactive exec: Docker did not attach".to_string(),
            }));
        };

        Ok(InteractiveExec {
            exec_id: exec.id,
            input,
            output,
        })
    }

    /// Kills every process in a session by killing its container, which has no restart policy
    /// and so stays stopped. The last resort for a command that cannot be stopped from inside
    /// it. A container that is already gone or stopped is the end this asks for.
    pub async fn kill_session(&self, container_id: &str) -> Result<()> {
        match self
            .docker
            .kill_container(container_id, None::<KillContainerOptions>)
            .await
        {
            Ok(())
            | Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404 | 409,
                ..
            }) => Ok(()),
            Err(error) => Err(error)
                .into_alien_error()
                .context(ErrorData::SandboxSessionFailed {
                    session_id: container_id.to_string(),
                    operation: "kill container".to_string(),
                }),
        }
    }

    /// The exit code of a finished exec, or `None` if Docker has none for it.
    pub async fn exec_exit_code(&self, exec_id: &str) -> Result<Option<i64>> {
        let inspect = self
            .docker
            .inspect_exec(exec_id)
            .await
            .into_alien_error()
            .context(ErrorData::SandboxSessionFailed {
                session_id: exec_id.to_string(),
                operation: "inspect interactive exec".to_string(),
            })?;
        Ok(inspect.exit_code)
    }

    /// Writes one file into a session.
    ///
    /// Streamed through an exec's stdin rather than Docker's archive-upload API: that API
    /// extracts through the container filesystem layer and is refused outright when the root
    /// filesystem is read-only, even when the target is a writable tmpfs. The path is validated
    /// and passed as an argv element, so it never reaches a shell for interpretation.
    pub async fn write_file(&self, container_id: &str, path: &str, contents: &[u8]) -> Result<()> {
        let path = resolve_in_root(path)?;

        let exec = self
            .docker
            .create_exec(
                container_id,
                CreateExecOptions {
                    // Parent directories are created, matching the in-sandbox agent the cloud
                    // backends run. Without it the same `write_files` call succeeds on AWS and
                    // fails on Local for any path with a directory in it.
                    cmd: Some(vec![
                        "/bin/sh".to_string(),
                        "-c".to_string(),
                        "mkdir -p \"$(dirname \"$1\")\" && cat > \"$1\"".to_string(),
                        "sh".to_string(),
                        path.to_string(),
                    ]),
                    attach_stdin: Some(true),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    user: Some(self.session_user(container_id).await?),
                    working_dir: Some(SESSION_ROOT.to_string()),
                    ..Default::default()
                },
            )
            .await
            .into_alien_error()
            .context(ErrorData::SandboxSessionFailed {
                session_id: container_id.to_string(),
                operation: "create upload exec".to_string(),
            })?;

        let started = self
            .docker
            .start_exec(&exec.id, None)
            .await
            .into_alien_error()
            .context(ErrorData::SandboxSessionFailed {
                session_id: container_id.to_string(),
                operation: "start upload exec".to_string(),
            })?;

        if let StartExecResults::Attached {
            mut input,
            mut output,
        } = started
        {
            input.write_all(contents).await.into_alien_error().context(
                ErrorData::SandboxSessionFailed {
                    session_id: container_id.to_string(),
                    operation: "write file contents".to_string(),
                },
            )?;
            input
                .shutdown()
                .await
                .into_alien_error()
                .context(ErrorData::SandboxSessionFailed {
                    session_id: container_id.to_string(),
                    operation: "close file stream".to_string(),
                })?;

            // Drain so the command observes EOF and finishes before the exit code is read.
            while output.next().await.is_some() {}
        }

        let inspect = self
            .docker
            .inspect_exec(&exec.id)
            .await
            .into_alien_error()
            .context(ErrorData::SandboxSessionFailed {
                session_id: container_id.to_string(),
                operation: "inspect upload exec".to_string(),
            })?;

        match inspect.exit_code {
            Some(0) => Ok(()),
            other => Err(AlienError::new(ErrorData::SandboxSessionFailed {
                session_id: container_id.to_string(),
                operation: format!("write '{path}' exited with {other:?}"),
            })),
        }
    }

    /// Reads one file out of a session.
    ///
    /// Via exec rather than Docker's archive-download API, which reads the container filesystem
    /// layer and cannot see tmpfs mounts — and the session's only writable area is a tmpfs.
    pub async fn read_file(&self, container_id: &str, path: &str) -> Result<Vec<u8>> {
        let path = resolve_in_root(path)?;

        let result = self
            .exec(container_id, &["/bin/cat".to_string(), path.to_string()])
            .await?;

        if result.exit_code != 0 {
            return Err(AlienError::new(ErrorData::SandboxSessionFailed {
                session_id: container_id.to_string(),
                operation: format!("read '{path}' exited with {}", result.exit_code),
            }));
        }

        Ok(result
            .output
            .into_iter()
            .filter_map(|frame| match frame {
                SandboxOutput::Stdout(bytes) => Some(bytes),
                SandboxOutput::Stderr(_) => None,
            })
            .flatten()
            .collect())
    }

    /// Returns the loopback address a declared preview port is published on.
    ///
    /// Refuses a port that was not declared at create time: Docker fixed the published set
    /// then, and an undeclared port has nowhere to be reachable from anyway.
    pub async fn preview_address(&self, container_id: &str, port: u16) -> Result<String> {
        let inspected = self
            .docker
            .inspect_container(container_id, None)
            .await
            .into_alien_error()
            .context(ErrorData::SandboxSessionFailed {
                session_id: container_id.to_string(),
                operation: "inspect for preview".to_string(),
            })?;

        // A session behind the gateway is previewed through its relay; any other publishes the
        // port itself.
        let relayed = match inspected.name.as_deref() {
            Some(name) => {
                crate::sandbox_gateway::preview_host_port(
                    &self.docker,
                    name.trim_start_matches('/'),
                    port,
                )
                .await
            }
            None => None,
        };
        let host_port = relayed
            .or_else(|| {
                inspected
                    .network_settings
                    .and_then(|settings| settings.ports)
                    .and_then(|ports| ports.get(&format!("{port}/tcp")).cloned().flatten())
                    .and_then(|bindings| {
                        bindings
                            .first()
                            .and_then(|binding| binding.host_port.clone())
                    })
            })
            .ok_or_else(|| {
                AlienError::new(ErrorData::SandboxSessionFailed {
                    session_id: container_id.to_string(),
                    operation: format!("port {port} was not declared as a preview port"),
                })
            })?;

        Ok(format!("http://127.0.0.1:{host_port}"))
    }

    /// Lists the sessions this manager is tracking for a sandbox.
    pub async fn list_sessions(&self, sandbox: &str) -> Result<Vec<SandboxSessionHandle>> {
        let filters = HashMap::from([(
            "label".to_string(),
            vec![format!("{LABEL_SANDBOX}={sandbox}")],
        )]);

        let containers = self
            .docker
            .list_containers(Some(ListContainersOptions {
                all: true,
                filters: Some(filters),
                ..Default::default()
            }))
            .await
            .into_alien_error()
            .context(ErrorData::SandboxSessionFailed {
                session_id: sandbox.to_string(),
                operation: "list sessions".to_string(),
            })?;

        Ok(containers
            .into_iter()
            .filter_map(|container| {
                let session_id = container.labels.as_ref()?.get(LABEL_SESSION)?.clone();
                Some(SandboxSessionHandle {
                    session_id,
                    container_id: container.id?,
                    running: container.state
                        == Some(bollard::models::ContainerSummaryStateEnum::RUNNING),
                })
            })
            .collect())
    }

    /// Removes a session and its private network. Idempotent: an absent session is success.
    pub async fn terminate(&self, sandbox: &str, session_id: &str) -> Result<()> {
        let name = Self::container_name(sandbox, session_id);

        let removed = self
            .docker
            .remove_container(
                &name,
                Some(RemoveContainerOptions {
                    force: true,
                    v: true,
                    ..Default::default()
                }),
            )
            .await;

        match removed {
            Ok(()) => {}
            // An already-gone session is the desired end state. Every other failure leaves the
            // container running, and terminate is the containment kill switch — reporting success
            // there tells the caller untrusted code has stopped when it has not.
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => {}
            Err(error) => {
                return Err(error)
                    .into_alien_error()
                    .context(ErrorData::SandboxSessionFailed {
                        session_id: session_id.to_string(),
                        operation: format!("remove container '{name}'"),
                    })
            }
        }
        crate::sandbox_gateway::remove_preview_relays(&self.docker, session_id, &name).await?;
        // A session behind the gateway had its own network and CA; its policy, credential rules,
        // network and CA go with it. Nothing to do for any other session.
        let _ = tokio::fs::remove_dir_all(self.ca_directory(&name)).await;
        let gateway = self.gateways.lock().await.get(sandbox).cloned();
        crate::sandbox_gateway::detach_session(
            &self.docker,
            gateway.as_deref(),
            sandbox,
            session_id,
        )
        .await
    }

    /// Removes every session and temporary image inspection belonging to a sandbox.
    ///
    /// Run at manager startup so a CLI restart does not leave containers behind, and at
    /// teardown so the Frozen parent's children go first.
    pub async fn reap(&self, sandbox: &str) -> Result<usize> {
        let terminated = async {
            let sessions = self.list_sessions(sandbox).await?;
            let count = sessions.len();
            let mut first_error = None;
            for session in sessions {
                if let Err(error) = self.terminate(sandbox, &session.session_id).await {
                    first_error.get_or_insert(error);
                }
            }
            first_error.map_or(Ok(count), Err)
        }
        .await;

        // Best effort: Docker refuses to remove a network with members, which is the correct
        // outcome rather than an error to propagate.
        let _ = self
            .docker
            .remove_network(&Self::network_name(sandbox))
            .await;

        // The sandbox's gateway and the session networks a previous run left behind.
        let gateway = async {
            self.gateways.lock().await.remove(sandbox);
            crate::sandbox_gateway::remove_gateway(&self.docker, sandbox).await?;
            crate::sandbox_gateway::sweep_session_networks(&self.docker, sandbox).await
        }
        .await;

        // Attempt every independent cleanup before reporting failure. Session errors take
        // precedence because they can mean untrusted commands are still running.
        let inspections = crate::sandbox_identity::reap(&self.docker, sandbox).await;
        let terminated = terminated?;
        gateway?;
        Ok(terminated + inspections?)
    }
}

/// Where a session finds its CA (`ca.pem`) and trust bundle (`bundle.pem`).
pub const CA_MOUNT: &str = "/etc/alien-egress";

/// Where images keep their system CA bundle, most common first.
const SYSTEM_BUNDLES: [&str; 3] = [
    "/etc/ssl/certs/ca-certificates.crt",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/ssl/cert.pem",
];

/// The variables common TLS clients read their trust from: OpenSSL, curl and Python read the
/// bundle; Node adds `ca.pem` to its own roots.
fn ca_env() -> [String; 4] {
    [
        format!("SSL_CERT_FILE={CA_MOUNT}/bundle.pem"),
        format!("REQUESTS_CA_BUNDLE={CA_MOUNT}/bundle.pem"),
        format!("CURL_CA_BUNDLE={CA_MOUNT}/bundle.pem"),
        format!("NODE_EXTRA_CA_CERTS={CA_MOUNT}/ca.pem"),
    ]
}

/// The CA carries no private key; the gateway's never leaves the gateway.
async fn write_ca_files(
    directory: &std::path::Path,
    ca: &str,
    system: Option<&str>,
) -> std::io::Result<()> {
    if ca.contains("PRIVATE KEY") {
        return Err(std::io::Error::other("the sandbox CA must not carry a key"));
    }
    tokio::fs::create_dir_all(directory).await?;
    tokio::fs::write(directory.join("ca.pem"), ca).await?;
    let mut bundle = system.unwrap_or_default().trim_end().to_string();
    if !bundle.is_empty() {
        bundle.push('\n');
    }
    bundle.push_str(ca);
    tokio::fs::write(directory.join("bundle.pem"), bundle).await
}

fn new_gateway_secret() -> String {
    use base64::Engine;
    use rand::RngCore;
    let mut secret = [0u8; 32];
    rand::rng().fill_bytes(&mut secret);
    base64::engine::general_purpose::STANDARD.encode(secret)
}

/// Resolves a caller-supplied path against the session root.
///
/// An absolute path means "under the session root", not "on the container's filesystem" — the
/// same rule the in-sandbox agent applies on the cloud backends. Reading it as host-absolute
/// would let a caller name any file in the image, and the session root is the only writable
/// area anyway.
/// A command started by [`LocalSandboxManager::exec_interactive`].
/// Reads the session's `uid:gid` label. Root is refused for either id, as the image identity
/// resolution refuses it.
fn exec_user(label: Option<&str>) -> std::result::Result<String, &'static str> {
    let label = label.ok_or("the session has no recorded user")?;
    let (uid, gid) = label
        .split_once(':')
        .and_then(|(uid, gid)| Some((uid.parse::<u32>().ok()?, gid.parse::<u32>().ok()?)))
        .ok_or("the session's recorded user is not uid:gid")?;
    if uid == 0 || gid == 0 {
        return Err("the session's recorded user is root");
    }
    Ok(format!("{uid}:{gid}"))
}

/// The user the signal shell runs as. Only root holds the one capability the container keeps.
const ROOT_USER: &str = "0";

/// An attached exec: an interactive command as the session's user, or the signal shell as root.
/// Never privileged: that would hand every capability, and the devices, to a process inside a
/// container the command controls.
fn attached_exec_options(
    command: &[String],
    working_dir: String,
    env: &std::collections::BTreeMap<String, String>,
    user: String,
) -> CreateExecOptions<String> {
    CreateExecOptions {
        cmd: Some(command.to_vec()),
        attach_stdin: Some(true),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        env: Some(
            env.iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect(),
        ),
        // Always named: the container's own user is root.
        user: Some(user),
        privileged: Some(false),
        working_dir: Some(working_dir),
        ..Default::default()
    }
}

pub struct InteractiveExec {
    /// Docker's id for the exec, to read its exit code once output ends
    pub exec_id: String,
    /// The command's stdin
    pub input: std::pin::Pin<Box<dyn tokio::io::AsyncWrite + Send>>,
    /// stdout and stderr, as Docker frames them
    pub output: std::pin::Pin<
        Box<
            dyn futures::Stream<
                    Item = std::result::Result<
                        bollard::container::LogOutput,
                        bollard::errors::Error,
                    >,
                > + Send,
        >,
    >,
}

/// Resolves a working directory against the session root. The root itself is allowed, unlike for
/// a file path.
fn resolve_dir_in_root(path: &str) -> Result<String> {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() || trimmed == SESSION_ROOT || trimmed == "." {
        return Ok(SESSION_ROOT.to_string());
    }
    let relative = trimmed.strip_prefix(SESSION_ROOT).unwrap_or(trimmed);
    resolve_in_root(relative)
}

fn resolve_in_root(path: &str) -> Result<String> {
    let refused = |reason: &str| {
        AlienError::new(ErrorData::SandboxSessionFailed {
            session_id: path.to_string(),
            operation: format!("path {reason}"),
        })
    };

    // A trailing slash names a directory, and these operations act on files. Checked before any
    // trimming, which would make "/work/" indistinguishable from the file "/work".
    if path.ends_with('/') {
        return Err(refused("must name a file, not a directory"));
    }

    let relative = path.trim_start_matches('/');
    if relative.is_empty() {
        return Err(refused("is empty"));
    }

    if relative
        .split('/')
        .any(|part| part == ".." || part.is_empty())
    {
        return Err(refused("must not traverse"));
    }

    Ok(format!("{SESSION_ROOT}/{relative}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    /// A privileged exec would hand every capability to a process in a container the command
    /// controls. Neither attached exec asks for one, and only the signal shell runs as root.
    #[test]
    fn no_attached_exec_is_privileged_and_only_the_signal_shell_is_root() {
        let command = attached_exec_options(
            &["cat".to_string()],
            SESSION_ROOT.to_string(),
            &std::collections::BTreeMap::new(),
            "1000:1000".to_string(),
        );
        let shell = attached_exec_options(
            &["sh".to_string()],
            SESSION_ROOT.to_string(),
            &std::collections::BTreeMap::new(),
            ROOT_USER.to_string(),
        );
        for options in [&command, &shell] {
            assert_eq!(options.privileged, Some(false));
        }
        assert_eq!(command.user.as_deref(), Some("1000:1000"));
        assert_eq!(shell.user.as_deref(), Some("0"));
    }

    #[test]
    fn the_exec_user_is_the_label_and_never_root() {
        assert_eq!(exec_user(Some("1000:1001")).as_deref(), Ok("1000:1001"));
        for refused in [
            None,
            Some("0:1000"),
            Some("1000:0"),
            Some("1000"),
            Some("a:b"),
        ] {
            assert!(exec_user(refused).is_err(), "{refused:?}");
        }
    }

    /// Against a gateway image that predates admin protocol 2, the credentials call is refused
    /// with a clear message before any gateway is looked up.
    #[tokio::test]
    async fn credentials_are_refused_with_a_gateway_that_predates_them() {
        let server = MockServer::start_async().await;
        let manager = LocalSandboxManager {
            docker: Docker::connect_with_http(&server.base_url(), 5, bollard::API_DEFAULT_VERSION)
                .unwrap(),
            state_dir: PathBuf::new(),
            gateway_key: alien_bindings::sandbox_egress_token::generate_signing_key(),
            gateways: Default::default(),
            gateway_image: alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_IMAGE.to_string(),
            gateway_secret: new_gateway_secret(),
        };
        let manager = manager.with_gateway_image(
            "ghcr.io/alienplatform/alien-egress-gateway@sha256:40a22a2aebf068da82d4310043875b383cdeba9cfa0aa90a649d67f70398a037",
        );
        let error = manager
            .update_credentials("box", "s1", &[])
            .await
            .unwrap_err();
        assert!(
            format!("{error:?}").contains("per-sandbox credentials are not available"),
            "{error:?}"
        );
    }

    /// A declaration that names the manager's own variables is refused before Docker is asked
    /// for anything, so it never chooses what code in the session reads.
    #[tokio::test]
    async fn a_session_environment_cannot_set_the_sandbox_variables() {
        let server = MockServer::start_async().await;
        let any = server
            .mock_async(|when, then| {
                when.any_request();
                then.status(500);
            })
            .await;
        let manager = LocalSandboxManager {
            docker: Docker::connect_with_http(&server.base_url(), 5, bollard::API_DEFAULT_VERSION)
                .unwrap(),
            state_dir: PathBuf::new(),
            docker_host: String::new(),
            gateway_key: alien_bindings::sandbox_egress_token::generate_signing_key(),
            gateways: Default::default(),
            gateway_image: String::new(),
            gateway_secret: new_gateway_secret(),
        };
        for name in [
            "ALIEN_SANDBOX_EXEC_UID",
            "ALIEN_SANDBOX_EXEC_GID",
            "ALIEN_SANDBOX_ROOT",
        ] {
            let error = manager
                .create_session(
                    "box",
                    "s1",
                    &SandboxSessionConfig {
                        image: "alpine".to_string(),
                        cpu_cores: 1.0,
                        memory_bytes: 1 << 28,
                        pids_limit: None,
                        scratch_bytes: 1 << 24,
                        egress: SandboxEgressMode::Deny,
                        preview_ports: Vec::new(),
                        env: HashMap::from([(name.to_string(), "0".to_string())]),
                        links: Vec::new(),
                        max_lifetime: None,
                        egress_policy: None,
                    },
                )
                .await
                .expect_err("refused");
            assert!(error.to_string().contains("is reserved"), "{error}");
        }
        any.assert_hits_async(0).await;
    }

    #[tokio::test]
    async fn a_failed_inspection_cleanup_does_not_keep_sessions_alive() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path_contains("/containers/json")
                    .query_param("filters", r#"{"label":["dev.alien.sandbox=box"]}"#);
                then.json_body(serde_json::json!([
                    {"Id":"one", "Labels":{"dev.alien.sandbox.session":"s1"}},
                    {"Id":"two", "Labels":{"dev.alien.sandbox.session":"s2"}}
                ]));
            })
            .await;
        let terminated = server
            .mock_async(|when, then| {
                when.method(DELETE)
                    .path_contains("/containers/alien-sbx-box-")
                    .query_param("force", "true");
                then.status(204);
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path_contains("/containers/json")
                    .query_param(
                        "filters",
                        r#"{"label":["dev.alien.sandbox.inspection=box"]}"#,
                    );
                then.json_body(serde_json::json!([{"Id":"inspection"}]));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(DELETE).path_contains("/containers/inspection");
                then.status(500)
                    .json_body(serde_json::json!({"message":"removal failed"}));
            })
            .await;
        let manager = LocalSandboxManager {
            docker: Docker::connect_with_http(&server.base_url(), 5, bollard::API_DEFAULT_VERSION)
                .unwrap(),
            state_dir: PathBuf::new(),
            docker_host: String::new(),
            gateway_key: alien_bindings::sandbox_egress_token::generate_signing_key(),
            gateways: Default::default(),
            gateway_image: String::new(),
            gateway_secret: new_gateway_secret(),
        };

        assert!(
            manager.reap("box").await.is_err(),
            "report the inspection cleanup failure"
        );
        terminated.assert_hits_async(2).await;
    }

    #[tokio::test]
    async fn a_failed_session_removal_does_not_skip_other_cleanup() {
        for inspection_status in [204, 500] {
            let server = MockServer::start_async().await;
            server
                .mock_async(|when, then| {
                    when.method(GET)
                        .path_contains("/containers/json")
                        .query_param("filters", r#"{"label":["dev.alien.sandbox=box"]}"#);
                    then.json_body(serde_json::json!([
                        {"Id":"one", "Labels":{"dev.alien.sandbox.session":"s1"}},
                        {"Id":"two", "Labels":{"dev.alien.sandbox.session":"s2"}}
                    ]));
                })
                .await;
            let failed = server
                .mock_async(|when, then| {
                    when.method(DELETE)
                        .path_contains("/containers/alien-sbx-box-s1")
                        .query_param("force", "true");
                    then.status(500)
                        .json_body(serde_json::json!({"message":"session removal failed"}));
                })
                .await;
            let terminated = server
                .mock_async(|when, then| {
                    when.method(DELETE)
                        .path_contains("/containers/alien-sbx-box-s2")
                        .query_param("force", "true");
                    then.status(204);
                })
                .await;
            let network = server
                .mock_async(|when, then| {
                    when.method(DELETE)
                        .path_contains("/networks/alien-sbx-net-box");
                    then.status(204);
                })
                .await;
            server
                .mock_async(|when, then| {
                    when.method(GET)
                        .path_contains("/containers/json")
                        .query_param(
                            "filters",
                            r#"{"label":["dev.alien.sandbox.inspection=box"]}"#,
                        );
                    then.json_body(serde_json::json!([{"Id":"inspection"}]));
                })
                .await;
            let inspection = server
                .mock_async(|when, then| {
                    when.method(DELETE)
                        .path_contains("/containers/inspection")
                        .query_param("v", "true");
                    then.status(inspection_status)
                        .json_body(serde_json::json!({"message":"inspection removal failed"}));
                })
                .await;
            let manager = LocalSandboxManager {
                docker: Docker::connect_with_http(
                    &server.base_url(),
                    5,
                    bollard::API_DEFAULT_VERSION,
                )
                .unwrap(),
                state_dir: PathBuf::new(),
                docker_host: String::new(),
                gateway_key: alien_bindings::sandbox_egress_token::generate_signing_key(),
                gateways: Default::default(),
                gateway_image: String::new(),
                gateway_secret: new_gateway_secret(),
            };

            let error = manager
                .reap("box")
                .await
                .expect_err("session removal failed");
            assert!(
                error.to_string().contains("alien-sbx-box-s1"),
                "preserve the session termination failure even if inspection cleanup fails: {error}"
            );
            failed.assert_async().await;
            terminated.assert_async().await;
            network.assert_async().await;
            inspection.assert_async().await;
        }
    }

    #[test]
    fn container_and_network_names_are_session_scoped() {
        assert_eq!(
            LocalSandboxManager::container_name("agent", "s1"),
            "alien-sbx-agent-s1"
        );
        // One egress network per sandbox: a bridge per session does not isolate sessions.
        assert_eq!(
            LocalSandboxManager::network_name("agent"),
            "alien-sbx-net-agent"
        );
    }

    /// Absolute and relative name the same file, and both sit under the session root. A caller
    /// that works against AWS must not have to rewrite its paths for Local.
    #[test]
    fn paths_resolve_against_the_session_root() {
        assert_eq!(
            resolve_in_root("/work/main.py").expect("absolute path"),
            "/sandbox/work/main.py"
        );
        assert_eq!(
            resolve_in_root("work/main.py").expect("relative path"),
            "/sandbox/work/main.py"
        );
        assert_eq!(
            resolve_in_root("main.py").expect("bare name"),
            "/sandbox/main.py"
        );
    }

    #[test]
    fn traversal_and_directory_paths_are_refused() {
        for path in [
            "/work/../etc/passwd",
            "../etc/passwd",
            "/work/..",
            "/work/",
            "/",
            "",
            "work//main.py",
        ] {
            resolve_in_root(path).expect_err(&format!(
                "'{path}' must be refused before it reaches Docker"
            ));
        }
    }
}
