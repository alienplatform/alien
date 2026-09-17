//! What a sandbox image must carry for the agent to serve, and the Dockerfile text carrying it.
//!
//! Two images exist and neither is built the way the other is. AWS renders one per deployment onto
//! a customer-supplied base image; GCP builds one static image in CI. Nothing at build time reads
//! the other side, and a value that disagrees is invisible until a session fails: an agent
//! listening on a port no caller dials, or an exec into a uid the image never created.
//!
//! So the values live here once and both images render the block that carries them from this
//! module. The GCP image is committed as generated text because the release workflow builds it
//! with `docker build`, which cannot call a Rust function.

/// Path the agent binary is installed at inside every sandbox image.
///
/// An image that installs one path and entrypoints another exits before it serves, and the
/// session times out on connect.
pub const AGENT_PATH: &str = "/usr/local/bin/alien-sandbox-agent";

/// Port the agent serves unless the platform pins another.
///
/// Defined once because two independent copies are a runtime-only failure: the image build places
/// the agent on one port and the client dials the other, and nothing catches it until a sandbox
/// hangs. AWS scopes its endpoint token to an explicit port set, so this cannot be discovered.
pub const AGENT_PORT: u16 = 8971;

/// How an image ends, and the isolation that ending permits.
///
/// One value rather than two, because `ALIEN_SANDBOX_ISOLATION` and the trailing `USER` describe
/// the same decision from opposite sides: an image that asks for `uid-split` under a non-root
/// `USER` has no privilege left to drop with, and every exec in it fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Isolation {
    /// The agent starts as root and drops to the exec uid before every spawn, so a command can
    /// never rewrite its own supervisor. The image declares no `USER`.
    UidSplit,
    /// The agent starts as the exec uid and supervises commands under it, which is all a runtime
    /// that refuses a root image can offer. The image ends `USER <uid>:<uid>`.
    Platform,
}

impl Isolation {
    /// The `ALIEN_SANDBOX_ISOLATION` value the agent parses.
    pub fn env_value(self) -> &'static str {
        match self {
            Self::UidSplit => "uid-split",
            Self::Platform => "platform",
        }
    }
}

/// How the agent decides a caller may be served.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Authorization {
    /// The connection is the proof. The agent serves an uncapabilitied request only from a socket
    /// peer it cannot trace back to the exec uid, which keeps the supervised command out.
    Transport,
    /// The caller presents a signed token, which is what a network anything can reach requires.
    Capability,
}

impl Authorization {
    /// The `ALIEN_SANDBOX_AUTHORIZATION` value the agent parses.
    pub fn env_value(self) -> &'static str {
        match self {
            Self::Transport => "transport",
            Self::Capability => "capability",
        }
    }
}

/// The values an image must carry for the agent to run in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SandboxImage {
    /// Uid and gid the supervised command runs as.
    pub exec_uid: u32,
    /// Directory a session's files live under, and the only place `exec_uid` may write.
    pub session_root: &'static str,
    /// Port the agent serves, both its own protocol and any lifecycle hooks.
    pub port: u16,
    pub authorization: Authorization,
    pub isolation: Isolation,
}

/// The Lambda MicroVM image, rendered per deployment onto a customer base image.
///
/// The agent runs as root so it can drop to [`SandboxImage::exec_uid`] before every spawn; inside
/// a MicroVM that is contained by hardware virtualisation, which is the tenant boundary. A
/// shared-kernel backend must give the agent `CAP_SETUID` instead of root.
pub const AWS_MICROVM: SandboxImage = SandboxImage {
    exec_uid: 60000,
    session_root: "/sandbox",
    port: AGENT_PORT,
    authorization: Authorization::Transport,
    isolation: Isolation::UidSplit,
};

/// The GCP Agent Platform image, built once in CI and run with nothing layered on top.
///
/// 1000 is the conventional first non-root uid, chosen over the 60000 [`AWS_MICROVM`] uses because
/// that value was never tried against Agent Platform. One uid covers the agent and the commands it
/// supervises: Agent Platform refuses an image that requires root, so there is no second uid to
/// drop to.
///
/// Agent Platform is only known to serve 8080, on the image and on the declared template port
/// alike, and whether it honours a declared port or assumes 8080 has never been established.
/// 8080 is right under either answer; any other number is right under only one.
pub const GCP_AGENT_PLATFORM: SandboxImage = SandboxImage {
    exec_uid: 1000,
    session_root: "/sandbox",
    port: 8080,
    authorization: Authorization::Transport,
    isolation: Isolation::Platform,
};

/// The `RUN` step creating the exec identity and the session root it owns.
pub fn identity_setup(image: &SandboxImage) -> String {
    let SandboxImage {
        exec_uid,
        session_root,
        ..
    } = *image;
    format!(
        r#"RUN printf 'sandbox:x:{exec_uid}:{exec_uid}::{session_root}:/sbin/nologin\n' >> /etc/passwd \
 && printf 'sandbox:x:{exec_uid}:\n' >> /etc/group \
 && mkdir -p {session_root} \
 && chown {exec_uid}:{exec_uid} {session_root} \
 && chmod 0700 {session_root}"#
    )
}

/// The `ENV` block carrying the agent's configuration contract.
pub fn contract_env(image: &SandboxImage) -> String {
    let SandboxImage {
        exec_uid,
        session_root,
        port,
        authorization,
        isolation,
    } = *image;
    format!(
        r#"ENV ALIEN_SANDBOX_ROOT={session_root} \
    ALIEN_SANDBOX_PORT={port} \
    ALIEN_SANDBOX_AUTHORIZATION={authorization} \
    ALIEN_SANDBOX_EXEC_UID={exec_uid} \
    ALIEN_SANDBOX_EXEC_GID={exec_uid} \
    ALIEN_SANDBOX_ISOLATION={isolation}"#,
        authorization = authorization.env_value(),
        isolation = isolation.env_value(),
    )
}

/// `EXPOSE`, the image's ending, and the `ENTRYPOINT`.
pub fn entrypoint(image: &SandboxImage) -> String {
    let ending = match image.isolation {
        Isolation::UidSplit => String::new(),
        Isolation::Platform => format!(
            "# Explicit gid so a runtime that does not read /etc/passwd cannot start the agent in \
             group 0, which\n# makes the exec drop a privilege crossing whose setgroups needs a \
             CAP_SETGID this image lacks, so\n# every exec fails.\nUSER {uid}:{uid}\n",
            uid = image.exec_uid
        ),
    };
    format!(
        "EXPOSE {port}\n{ending}ENTRYPOINT [\"{AGENT_PATH}\"]",
        port = image.port
    )
}

/// Path of the committed GCP Dockerfile, relative to the repository root.
#[cfg(test)]
const GCP_DOCKERFILE: &str = "docker/Dockerfile.alien-sandbox-agent";

/// Set to regenerate the committed GCP Dockerfile instead of comparing against it.
#[cfg(test)]
const GCP_DOCKERFILE_UPDATE: &str = "UPDATE_SANDBOX_AGENT_DOCKERFILE";

/// Renders [`GCP_DOCKERFILE`].
///
/// Everything outside the shared block is here because it is true of this image alone: the
/// `binary-selector` stage, which exists because the release workflow builds one manifest for two
/// architectures from binaries cross-compiled outside Docker; `git`, which the sandboxed command
/// needs and no customer base image is underneath to carry; and `RUST_LOG`, which the agent's own
/// `EnvFilter` needs before it will emit anything.
#[cfg(test)]
fn gcp_agent_platform_dockerfile() -> String {
    let image = &GCP_AGENT_PLATFORM;
    format!(
        r#"# Generated by `cargo test -p alien-core --lib sandbox_image`. Do not edit by hand.
# Regenerate with {GCP_DOCKERFILE_UPDATE}=1 in front of that command.
#
# Multi-arch build for the alien-sandbox-agent Docker image
# Run directly as the GCP Agent Platform sandbox; nothing layers on top of it

FROM docker.io/chainguard/wolfi-base:latest AS binary-selector

COPY target/aarch64-unknown-linux-musl/release/alien-sandbox-agent /tmp/alien-sandbox-agent-aarch64
COPY target/x86_64-unknown-linux-musl/release/alien-sandbox-agent /tmp/alien-sandbox-agent-x86_64

ARG TARGETARCH
RUN case "$TARGETARCH" in \
       amd64)  cp /tmp/alien-sandbox-agent-x86_64 /tmp/alien-sandbox-agent ;; \
       arm64)  cp /tmp/alien-sandbox-agent-aarch64 /tmp/alien-sandbox-agent ;; \
       *)      echo "unsupported TARGETARCH '$TARGETARCH'" >&2; exit 1 ;; \
    esac

FROM docker.io/chainguard/wolfi-base:latest

# git is for the sandboxed command, not the agent, and pulls 24 transitive packages. That cost
# lands here because this image is the sandbox, with no customer base image underneath to carry it.
RUN apk add --no-cache git

# Root-owned and unwritable by uid {exec_uid}: the supervised command runs under that uid and must not
# be able to rewrite its own supervisor.
COPY --from=binary-selector --chown=0:0 --chmod=0755 \
     /tmp/alien-sandbox-agent {AGENT_PATH}

# Numeric ids and a plain append rather than adduser, which differs across base distributions.
# Linux runs a process under a uid with no passwd entry, but tooling inside the sandbox reads one.
{identity}

# The template carries no env, so the contract lives here, and none of it is optional. transport
# serves an uncapabilitied request only from a socket `peer::transport_may_serve` cannot trace
# back to the exec uid, and the agent refuses the mode where /proc/net/tcp is unreadable.
{env}

# The release build resolves tracing-subscriber once across every package it names, and five of
# them ask for env-filter, so the agent's fmt::init() has an EnvFilter under it. Unset, that
# filter discards the startup warning saying this image serves requests without a capability.
ENV RUST_LOG=info

{entrypoint}
"#,
        exec_uid = image.exec_uid,
        identity = identity_setup(image),
        env = contract_env(image),
        entrypoint = entrypoint(image),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn committed_path() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(GCP_DOCKERFILE)
    }

    /// Where two texts first part, in terms a reader can act on. `assert_eq!` on a whole
    /// Dockerfile prints two escaped blobs and says nothing about which line moved.
    fn first_difference(committed: &str, rendered: &str) -> String {
        for (index, (left, right)) in committed.lines().zip(rendered.lines()).enumerate() {
            if left != right {
                let line = index + 1;
                return format!("line {line}: committed {left:?}, contract renders {right:?}");
            }
        }
        format!(
            "committed has {} lines, the contract renders {}",
            committed.lines().count(),
            rendered.lines().count()
        )
    }

    /// The whole file, not chosen properties. A mutation this comparison cannot see is one that
    /// leaves the bytes alone, and there is no such mutation.
    #[test]
    fn the_committed_gcp_dockerfile_is_what_the_contract_renders() {
        let path = committed_path();
        let rendered = gcp_agent_platform_dockerfile();

        if std::env::var_os(GCP_DOCKERFILE_UPDATE).is_some() {
            std::fs::write(&path, &rendered)
                .unwrap_or_else(|error| panic!("{} must be writable: {error}", path.display()));
            return;
        }

        let committed = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{} must be readable: {error}", path.display()));
        assert!(
            committed == rendered,
            "{GCP_DOCKERFILE} has drifted from the contract it is rendered from.\n\
             {}\n\
             Regenerate it: {GCP_DOCKERFILE_UPDATE}=1 cargo test -p alien-core --lib sandbox_image",
            first_difference(&committed, &rendered)
        );
    }

    /// The ending an image declares and the isolation it claims come off one value, so they
    /// cannot disagree. The AWS half is rendered at run time and reaches no committed file, which
    /// is why the whole-file comparison above covers only the GCP side of it.
    #[test]
    fn the_ending_an_image_declares_follows_its_isolation() {
        assert!(!entrypoint(&AWS_MICROVM).contains("USER "));
        assert!(contract_env(&AWS_MICROVM).contains("ALIEN_SANDBOX_ISOLATION=uid-split"));
        assert!(entrypoint(&GCP_AGENT_PLATFORM).contains("\nUSER 1000:1000\n"));
        assert!(contract_env(&GCP_AGENT_PLATFORM).contains("ALIEN_SANDBOX_ISOLATION=platform"));
    }
}
