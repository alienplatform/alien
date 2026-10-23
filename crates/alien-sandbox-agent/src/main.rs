//! The agent process, as it runs inside a sandbox.
//!
//! Everything it needs is read from the environment at start and fixed for the life of the
//! session. Nothing is negotiated at runtime: the process that placed this agent in the sandbox
//! is the only thing that gets to decide what session it serves and what authorises a request.

use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use alien_core::sandbox_capability::SandboxSessionIdentity;
use alien_error::{AlienError, Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use alien_sandbox_agent::error::{ErrorData, Result};
use alien_sandbox_agent::exec::ExecIdentity;
use alien_sandbox_agent::jobs::JobRegistry;
use alien_sandbox_agent::server::{router, AgentAuthorization, AgentState};
use alien_sandbox_agent::validate;
use axum::serve::{Listener, ListenerExt};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use ed25519_compact::PublicKey;

/// Directory the session's files live under. Every caller-supplied path resolves against it.
const ENV_ROOT: &str = "ALIEN_SANDBOX_ROOT";
/// Port the agent listens on.
const ENV_PORT: &str = "ALIEN_SANDBOX_PORT";
/// `capability` or `transport` — see [`AgentAuthorization`].
const ENV_AUTHORIZATION: &str = "ALIEN_SANDBOX_AUTHORIZATION";
/// Session this agent serves. Required under `capability`.
const ENV_SESSION_ID: &str = "ALIEN_SANDBOX_SESSION_ID";
/// Lifecycle generation the session started under. Required under `capability`.
const ENV_GENERATION: &str = "ALIEN_SANDBOX_GENERATION";
/// Base64 Ed25519 public key that signs capabilities. Required under `capability`.
const ENV_PUBLIC_KEY: &str = "ALIEN_SANDBOX_PUBLIC_KEY";
/// Bytes of each output stream kept before truncation.
const ENV_OUTPUT_CAP: &str = "ALIEN_SANDBOX_OUTPUT_CAP";
/// Unprivileged uid commands run as. Required — see [`load_state`].
const ENV_EXEC_UID: &str = "ALIEN_SANDBOX_EXEC_UID";
/// Its primary group.
const ENV_EXEC_GID: &str = "ALIEN_SANDBOX_EXEC_GID";
/// Which isolation model is in force: `uid-split` or `platform`. Declared, never defaulted — it
/// selects a security model, so an unset value must fail to start rather than silently pick one.
const ENV_ISOLATION: &str = "ALIEN_SANDBOX_ISOLATION";
/// New package boot contract; absent in released packages.
const ENV_IMAGE_COMMAND: &str = "ALIEN_SANDBOX_IMAGE_COMMAND";

/// Bytes of each stream kept when the environment does not say.
const DEFAULT_OUTPUT_CAP: usize = 4 * 1024 * 1024;

fn main() -> Result<()> {
    #[cfg(target_os = "linux")]
    if std::env::args().nth(1).as_deref() == Some("--egress-path-check") {
        std::process::exit(
            if alien_sandbox_agent::gateway_egress::command_path_is_closed() {
                0
            } else {
                1
            },
        );
    }

    // Before anything reads the environment: the validate hook runs this binary as the exec
    // identity through the same spawn path as a command, which hands it no environment at all.
    if std::env::args().nth(1).as_deref() == Some(validate::IDENTITY_ARGUMENT) {
        // SAFETY: both are always-successful getters with no arguments.
        let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
        println!("{uid} {gid}");
        return Ok(());
    }

    tracing_subscriber::fmt::init();

    let aws_image_transport =
        image_command_enabled()? && alien_sandbox_agent::image_command::defer_to_run_hook()?;
    if aws_image_transport && std::env::var(ENV_AUTHORIZATION).as_deref() == Ok("native-file") {
        return Err(invalid(
            ENV_AUTHORIZATION,
            "native-file cannot be combined with AWS Run-hook transport",
        ));
    }
    let state = Arc::new(load_state()?);
    let port: u16 = parse(ENV_PORT)?;
    let network_admin = std::env::var_os("ALIEN_SANDBOX_EGRESS").is_some() || aws_image_transport;
    #[cfg(target_os = "linux")]
    if aws_image_transport && state.gateway_egress.is_none() {
        alien_sandbox_agent::gateway_egress::isolate_control_callers()?;
    }
    #[cfg(target_os = "linux")]
    if image_command_enabled()? && std::env::var(ENV_AUTHORIZATION).as_deref() == Ok("native-file")
    {
        alien_sandbox_agent::gateway_egress::isolate_image_devices()?;
    }
    if network_admin || image_command_enabled()? {
        #[cfg(target_os = "linux")]
        if let Some(policy) = alien_sandbox_agent::egress::legacy_from_env()? {
            alien_sandbox_agent::egress::install(&policy, state.exec_identity.uid)?;
        }
        #[cfg(target_os = "linux")]
        if unsafe { libc::geteuid() } == 0 {
            alien_sandbox_agent::privilege::prepare_identity(
                state.exec_identity,
                &state.session_root,
            )
            .into_alien_error()
            .context(failed(
                "prepare declared command identity",
                "cannot write the image's passwd/group entries".to_string(),
            ))?;
            if image_command_enabled()? {
                alien_sandbox_agent::protected_files::protect(
                    &state.agent_binary,
                    std::path::Path::new("/opt/alien"),
                )
                .into_alien_error()
                .context(failed(
                    "protect agent bootstrap files",
                    "cannot make the agent binary and metadata read-only before image startup"
                        .into(),
                ))?;
            }
            alien_sandbox_agent::privilege::restrict_supervisor(network_admin)
                .into_alien_error()
                .context(failed(
                    "reduce supervisor capabilities",
                    "cannot restrict the supervisor grant".to_string(),
                ))?;
            // Ensure the writable root agrees with the built image's command identity before
            // commands or uploaded files can exist.
            let path = std::ffi::CString::new(state.session_root.as_os_str().as_encoded_bytes())
                .into_alien_error()
                .context(failed("prepare session root", "invalid path".to_string()))?;
            if unsafe {
                libc::chown(
                    path.as_ptr(),
                    state.exec_identity.uid,
                    state.exec_identity.gid,
                )
            } != 0
            {
                return Err(std::io::Error::last_os_error()
                    .into_alien_error()
                    .context(failed(
                        "own session root",
                        "cannot apply declared command identity".to_string(),
                    )));
            }
        }
        #[cfg(not(target_os = "linux"))]
        return Err(invalid("ALIEN_SANDBOX_EGRESS", "requires Linux netfilter"));
    }

    // All interfaces: on AWS the agent is reached from outside the guest, and a
    // loopback bind would make it unreachable.
    let address = SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));
    let std_listener = std::net::TcpListener::bind(address)
        .into_alien_error()
        .context(failed(
            "bind the agent listener",
            "the agent could not take its port".to_string(),
        ))?;
    #[cfg(target_os = "linux")]
    if let Some(egress) = &state.gateway_egress {
        egress.protect_control_listener(&std_listener)?;
    }

    // Linux capabilities are per thread. Build the runtime only after bootstrap reduction,
    // so every worker inherits the reduced set rather than keeping AWS's ALL grant.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_alien_error()
        .context(failed(
            "start agent runtime",
            "could not create the runtime".to_string(),
        ))?;
    let result = runtime.block_on(async move {
        let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .into_alien_error().context(failed("listen for shutdown", "cannot install SIGTERM handler".into()))?;
        let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
            .into_alien_error().context(failed("listen for shutdown", "cannot install SIGINT handler".into()))?;
        #[cfg(target_os = "linux")]
        if let Some(egress) = &state.gateway_egress {
            match egress.start().await {
                Ok(()) => { if egress.ensure_ready().is_ok() { state.start_image_once()?; } }
                Err(error) => tracing::error!(code = %error.code, "egress startup failed; session routes remain unavailable"),
            }
        }
        #[cfg(target_os = "linux")]
        if state.gateway_egress.is_none() && !state.image_process.waiting_for_activation()? {
            state.start_image_once()?;
        }
        #[cfg(not(target_os = "linux"))]
        if !state.image_process.waiting_for_activation()? {
            state.start_image_once()?;
        }
        tracing::info!("sandbox agent listening on {address}");

        let server = axum::serve(
            // tap_io is a no-op that wraps the listener in axum's TapIo, which is what makes the
            // SocketAddr connect-info available for a custom listener (the orphan rule blocks impls
            // straight onto SocketAddr).
            BlockingListener::new(std_listener, address).tap_io(|_| {}),
            router(Arc::clone(&state)).into_make_service_with_connect_info::<SocketAddr>(),
        );
        let outcome = tokio::select! {
            result = std::future::IntoFuture::into_future(server) => result
                .into_alien_error().context(failed("serve the agent protocol", "the agent stopped serving".into())),
            result = alien_sandbox_agent::children::reap_orphans() => result
                .into_alien_error().context(failed("reap orphaned children", "cannot collect abandoned processes".into())),
            _ = terminate.recv() => Ok(()),
            _ = interrupt.recv() => Ok(()),
        };
        state.image_process.shutdown().await?;
        outcome
    });
    // A blocking accept may still own a listener clone. It must not keep shutdown
    // waiting for another network connection after the boot group has stopped.
    runtime.shutdown_timeout(std::time::Duration::from_secs(1));
    result
}

/// A listener whose accept blocks in the `accept(2)` syscall instead of waiting on an epoll edge.
///
/// The GCP Agent Platform detaches and reattaches a sandbox's data plane on pause/resume. A tokio
/// `TcpListener` registered with epoll then stops receiving readiness for the listen socket across
/// that reattach and never accepts again — the process stays alive and the socket stays in LISTEN,
/// but no connection is served. A blocking `accept()` re-wakes on the next connection regardless,
/// which is why a plain blocking server survives the same transition. Each accepted connection is a
/// fresh tokio stream, so only the long-lived listener needs this.
struct BlockingListener {
    inner: Arc<std::net::TcpListener>,
    local: SocketAddr,
}

impl BlockingListener {
    fn new(listener: std::net::TcpListener, local: SocketAddr) -> Self {
        Self {
            inner: Arc::new(listener),
            local,
        }
    }
}

impl Listener for BlockingListener {
    type Io = tokio::net::TcpStream;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let listener = Arc::clone(&self.inner);
            match tokio::task::spawn_blocking(move || listener.accept()).await {
                Ok(Ok((stream, peer))) => {
                    if let Err(error) = stream.set_nonblocking(true) {
                        tracing::warn!(%error, "dropping a connection that would not go non-blocking");
                        continue;
                    }
                    match tokio::net::TcpStream::from_std(stream) {
                        Ok(stream) => return (stream, peer),
                        Err(error) => {
                            tracing::warn!(%error, "dropping a connection tokio would not adopt")
                        }
                    }
                }
                Ok(Err(error)) => {
                    tracing::warn!(%error, "accept failed; retrying");
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                Err(error) => tracing::warn!(%error, "the accept task failed; retrying"),
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(self.local)
    }
}

/// Which boundary keeps untrusted code away from the agent that supervises it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Isolation {
    /// The command runs under a uid distinct from the agent's, and the drop to it must work, so the
    /// agent's binary and state stay unreachable to it.
    UidSplit,
    /// The container or VM is the only boundary: the command runs as the agent's own user because
    /// the platform allows no other. Accepted only where a uid split is impossible.
    Platform,
}

fn load_isolation() -> Result<Isolation> {
    match required(ENV_ISOLATION)?.as_str() {
        "uid-split" => Ok(Isolation::UidSplit),
        "platform" => Ok(Isolation::Platform),
        other => Err(invalid(
            ENV_ISOLATION,
            &format!("'{other}' is not one of: uid-split, platform"),
        )),
    }
}

/// The identity rules, kept pure so they can be tested without touching process state. Root is
/// refused in both models; the agent's own uid/gid is refused only under uid-split, where a
/// command sharing the agent's identity is the escalation the split exists to prevent. Platform
/// accepts it because the platform runs everything as one user — the concession is same-uid, never
/// root.
fn enforce_exec_identity(
    isolation: Isolation,
    exec: ExecIdentity,
    agent_uid: u32,
    agent_gid: u32,
) -> Result<()> {
    if exec.uid == 0 {
        return Err(invalid(ENV_EXEC_UID, "must not be root"));
    }
    if exec.gid == 0 {
        return Err(invalid(ENV_EXEC_GID, "must not be the root group"));
    }
    if isolation == Isolation::UidSplit {
        if exec.uid == agent_uid {
            return Err(invalid(ENV_EXEC_UID, "must not be the agent's own user"));
        }
        if exec.gid == agent_gid {
            return Err(invalid(ENV_EXEC_GID, "must not be the agent's own group"));
        }
    }
    Ok(())
}

fn image_command_enabled() -> Result<bool> {
    match std::env::var(ENV_IMAGE_COMMAND) {
        Ok(value) if value == "true" => Ok(true),
        Err(std::env::VarError::NotPresent) => Ok(false),
        _ => Err(invalid(ENV_IMAGE_COMMAND, "must be true when set")),
    }
}

fn load_state() -> Result<AgentState> {
    let root = PathBuf::from(required(ENV_ROOT)?);

    // Canonical up front, because every path check compares against it. A root that is itself a
    // symlink would make each comparison a false negative.
    let session_root = root.canonicalize().into_alien_error().context(failed(
        &format!("resolve {ENV_ROOT} '{}'", root.display()),
        "the session root must exist before the agent starts".to_string(),
    ))?;

    let output_cap = match std::env::var(ENV_OUTPUT_CAP) {
        Ok(_) => parse(ENV_OUTPUT_CAP)?,
        Err(_) => DEFAULT_OUTPUT_CAP,
    };

    // Required, with no fall back to the agent's own identity. A command running as the agent
    // can read and write the agent's binary and state, which is the escalation the uid split
    // exists to prevent — so an image that forgets to set it must fail to start, not run wide.
    let exec_identity = ExecIdentity {
        uid: parse(ENV_EXEC_UID)?,
        gid: parse(ENV_EXEC_GID)?,
    };

    // A prebuilt bundle cannot be inspected by a setup emitter. Treat an explicit declaration
    // as an assertion, never an override of the identity selected from the image at build time.
    if std::env::var_os("ALIEN_SANDBOX_EXPECTED_EXEC_UID").is_some() {
        let declared: u32 = parse("ALIEN_SANDBOX_EXPECTED_EXEC_UID")?;
        if declared != exec_identity.uid {
            return Err(invalid("ALIEN_SANDBOX_EXPECTED_EXEC_UID", "privilegedSupervisor.commandUid disagrees with the built image; rebuild the bundle with the matching command uid"));
        }
    }

    let isolation = load_isolation()?;

    // SAFETY: both are always-successful getters with no arguments.
    #[cfg(unix)]
    let (agent_uid, agent_gid) = unsafe { (libc::geteuid(), libc::getegid()) };
    #[cfg(not(unix))]
    let (agent_uid, agent_gid) = (u32::MAX, u32::MAX);
    enforce_exec_identity(isolation, exec_identity, agent_uid, agent_gid)?;

    let agent_binary = std::env::current_exe().into_alien_error().context(failed(
        "locate the agent binary",
        "the validate hook needs the path of this executable".to_string(),
    ))?;

    let authorization = load_authorization()?;
    let wait_for_run_hook = alien_sandbox_agent::image_command::defer_to_run_hook()?;
    #[cfg(target_os = "linux")]
    let gateway_egress =
        alien_sandbox_agent::gateway_egress::GatewayEgress::from_env(exec_identity)?;
    // Gateway readiness already waits for Run and its network checks. Only modes
    // without that state machine need a separate image activation gate.
    #[cfg(target_os = "linux")]
    let wait_for_run_hook = wait_for_run_hook && gateway_egress.is_none();
    Ok(AgentState {
        image_process: if image_command_enabled()? {
            if agent_uid != 0 || isolation != Isolation::UidSplit {
                return Err(invalid(
                    ENV_IMAGE_COMMAND,
                    "requires a root agent with uid-split isolation",
                ));
            }
            if authorization.requires_activation() || wait_for_run_hook {
                alien_sandbox_agent::image_command::ImageProcess::awaiting_activation()
            } else {
                alien_sandbox_agent::image_command::ImageProcess::with_image_user()
            }
        } else if std::env::var_os("ALIEN_SANDBOX_EGRESS").is_some() {
            Default::default()
        } else {
            alien_sandbox_agent::image_command::ImageProcess::not_configured()
        },
        session_root,
        authorization,
        exec_identity,
        output_cap,
        jobs: JobRegistry::new(),
        #[cfg(target_os = "linux")]
        gateway_egress,
        agent_binary,
    })
}

/// Reads the authorization mode.
///
/// Deliberately has no default: an unset mode failing to start is a sandbox that never accepts a
/// request, where a defaulted one would be a sandbox that accepts every request.
fn load_authorization() -> Result<AgentAuthorization> {
    match required(ENV_AUTHORIZATION)?.as_str() {
        // This surface cannot bind loopback — on AWS it is reached from outside the guest — so
        // the assumption it rests on is stated out loud at startup instead. Nothing here can tell
        // which platform booted the agent, and only one of them makes that assumption true.
        "transport" => {
            // Refusing here rather than serving on an unanswered question: this mode tells the
            // agent's own commands apart from its caller by reading the socket table, and a
            // table it cannot read makes every caller look legitimate.
            if !alien_sandbox_agent::peer::attribution_works() {
                return Err(invalid(
                    ENV_AUTHORIZATION,
                    "cannot be 'transport' where the socket table is unreadable",
                ));
            }

            tracing::warn!(
                "authorization=transport: requests are accepted without a capability. This \
                 assumes the guest serves exactly one session and that the transport in front of \
                 it is the only route to this port. Any platform where reaching the agent does \
                 not prove which session the caller holds must set {ENV_AUTHORIZATION}=capability."
            );
            Ok(AgentAuthorization::Transport)
        }
        #[cfg(unix)]
        "native-file" => {
            if unsafe { libc::geteuid() } != 0 || !image_command_enabled()? {
                return Err(invalid(
                    ENV_AUTHORIZATION,
                    "native-file requires a packaged root agent",
                ));
            }
            Ok(AgentAuthorization::NativeFile(
                alien_sandbox_agent::native_token::NativeToken::create(std::path::Path::new(
                    alien_core::sandbox_image::NATIVE_TOKEN_PATH,
                ))?,
            ))
        }
        "capability" => {
            let encoded = required(ENV_PUBLIC_KEY)?;
            let bytes = BASE64
                .decode(&encoded)
                .map_err(|error| invalid(ENV_PUBLIC_KEY, &format!("not valid base64: {error}")))?;
            let public_key = PublicKey::from_slice(&bytes).map_err(|error| {
                invalid(ENV_PUBLIC_KEY, &format!("not an Ed25519 key: {error}"))
            })?;

            Ok(AgentAuthorization::Capability {
                public_key,
                identity: SandboxSessionIdentity {
                    session_id: required(ENV_SESSION_ID)?,
                    generation: parse(ENV_GENERATION)?,
                },
            })
        }
        other => Err(invalid(
            ENV_AUTHORIZATION,
            &format!("'{other}' is not one of: capability, transport, native-file"),
        )),
    }
}

fn required(name: &str) -> Result<String> {
    std::env::var(name).map_err(|_| invalid(name, "is required"))
}

fn parse<T: std::str::FromStr>(name: &str) -> Result<T>
where
    T::Err: std::fmt::Display,
{
    required(name)?
        .parse()
        .map_err(|error| invalid(name, &format!("{error}")))
}

fn invalid(name: &str, reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ConfigInvalid {
        setting: name.to_string(),
        reason: reason.to_string(),
    })
}

fn failed(operation: &str, reason: String) -> ErrorData {
    ErrorData::OperationFailed {
        operation: operation.to_string(),
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exec(uid: u32, gid: u32) -> ExecIdentity {
        ExecIdentity { uid, gid }
    }

    #[test]
    fn platform_accepts_the_agents_own_user() {
        enforce_exec_identity(Isolation::Platform, exec(1000, 1000), 1000, 1000)
            .expect("platform runs the command as the agent's own user");
    }

    #[test]
    fn uid_split_refuses_the_agents_own_user() {
        let error = enforce_exec_identity(Isolation::UidSplit, exec(1000, 1000), 1000, 1000)
            .expect_err("uid-split refuses the agent's own user");
        assert!(error.to_string().contains("agent's own user"), "{error}");
    }

    #[test]
    fn uid_split_accepts_a_distinct_user() {
        // The AWS shape: the agent runs as root, the command as an unprivileged uid.
        enforce_exec_identity(Isolation::UidSplit, exec(60000, 60000), 0, 0)
            .expect("a distinct exec uid is exactly what uid-split is for");
    }

    #[test]
    fn root_is_refused_in_both_models() {
        for isolation in [Isolation::UidSplit, Isolation::Platform] {
            enforce_exec_identity(isolation, exec(0, 5), 1000, 1000)
                .expect_err("root uid is refused regardless of the model");
            enforce_exec_identity(isolation, exec(5, 0), 1000, 1000)
                .expect_err("root gid is refused regardless of the model");
        }
    }
}
