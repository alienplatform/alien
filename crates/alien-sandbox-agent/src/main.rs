//! The agent process, as it runs inside a sandbox.
//!
//! Everything it needs is read from the environment at start and fixed for the life of the
//! session. Nothing is negotiated at runtime: the process that placed this agent in the sandbox
//! is the only thing that gets to decide what session it serves and what authorises a request.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use alien_core::sandbox_capability::SandboxSessionIdentity;
use alien_error::{AlienError, Context, IntoAlienError};
use alien_sandbox_agent::error::{ErrorData, Result};
use alien_sandbox_agent::exec::ExecIdentity;
use alien_sandbox_agent::jobs::JobRegistry;
use alien_sandbox_agent::server::{router, AgentAuthorization, AgentState};
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

/// Bytes of each stream kept when the environment does not say.
const DEFAULT_OUTPUT_CAP: usize = 4 * 1024 * 1024;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let state = Arc::new(load_state()?);
    let port: u16 = parse(ENV_PORT)?;

    // All interfaces: on AWS the agent is reached from outside the guest, and a
    // loopback bind would make it unreachable.
    let address = SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .into_alien_error()
        .context(failed(
            "bind the agent listener",
            "the agent could not take its port".to_string(),
        ))?;

    tracing::info!("sandbox agent listening on {address}");

    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .into_alien_error()
    .context(failed(
        "serve the agent protocol",
        "the agent stopped serving".to_string(),
    ))
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

    let isolation = load_isolation()?;

    // SAFETY: both are always-successful getters with no arguments.
    #[cfg(unix)]
    let (agent_uid, agent_gid) = unsafe { (libc::geteuid(), libc::getegid()) };
    #[cfg(not(unix))]
    let (agent_uid, agent_gid) = (u32::MAX, u32::MAX);
    enforce_exec_identity(isolation, exec_identity, agent_uid, agent_gid)?;

    Ok(AgentState {
        session_root,
        authorization: load_authorization()?,
        exec_identity,
        output_cap,
        jobs: JobRegistry::new(),
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
            &format!("'{other}' is not one of: capability, transport"),
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
