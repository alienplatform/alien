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

use bollard::container::{
    Config, CreateContainerOptions, ListContainersOptions, RemoveContainerOptions,
};
use bollard::image::CreateImageOptions;
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::models::{HostConfig, PortBinding};
use bollard::network::CreateNetworkOptions;
use bollard::Docker;
use futures::StreamExt;
use tokio::io::AsyncWriteExt;

use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};

/// Label carrying the sandbox resource a container belongs to.
const LABEL_SANDBOX: &str = "dev.alien.sandbox";
/// Label carrying the session id within that sandbox.
const LABEL_SESSION: &str = "dev.alien.sandbox.session";

/// Unprivileged uid/gid the workload runs as. `nobody` exists in every common base image.
const SANDBOX_USER: &str = "65534:65534";

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
}

/// A session the manager is tracking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxSessionHandle {
    /// Session id within the sandbox
    pub session_id: String,
    /// Docker container backing it
    pub container_id: String,
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
    state_dir: PathBuf,
}

impl LocalSandboxManager {
    /// Connects to the local Docker daemon.
    pub fn new(state_dir: PathBuf) -> Result<Self> {
        let docker = Docker::connect_with_local_defaults()
            .into_alien_error()
            .context(ErrorData::DockerConnectionFailed {
                reason: "could not reach the local Docker daemon".to_string(),
            })?;

        Ok(Self { docker, state_dir })
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
        let name = Self::container_name(sandbox, session_id);

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

        self.ensure_image(&config.image).await?;

        let network_mode = match config.egress {
            // "none" gives no interface at all, which is a stronger and simpler guarantee than
            // a private network with its gateway firewalled off.
            SandboxEgressMode::Deny => "none".to_string(),
            SandboxEgressMode::Allow => {
                let network = Self::network_name(sandbox);
                self.ensure_egress_network(&network).await?;
                network
            }
        };

        let env: Vec<String> = config
            .env
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect();

        let labels = HashMap::from([
            (LABEL_SANDBOX.to_string(), sandbox.to_string()),
            (LABEL_SESSION.to_string(), session_id.to_string()),
        ]);

        // Bound to 127.0.0.1, never 0.0.0.0: a preview is for the developer's own machine, and
        // publishing on all interfaces would expose untrusted code to the local network.
        let port_bindings: HashMap<String, Option<Vec<PortBinding>>> = config
            .preview_ports
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

        let exposed_ports: HashMap<String, HashMap<(), ()>> = config
            .preview_ports
            .iter()
            .map(|port| (format!("{port}/tcp"), HashMap::new()))
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
                // 1777 because the workload runs as an unprivileged uid: a tmpfs mounted with
                // the default mode is root-owned, and the session's only writable area would
                // not be writable by the process using it.
                format!("rw,noexec,nosuid,mode=1777,size={}", config.scratch_bytes),
            )])),
            cap_drop: Some(vec!["ALL".to_string()]),
            security_opt: Some(vec!["no-new-privileges:true".to_string()]),
            pids_limit: config.pids_limit,
            memory: Some(config.memory_bytes),
            nano_cpus: Some((config.cpu_cores * 1_000_000_000.0) as i64),
            // No restart policy: a sandbox that exits stays exited. Restarting hostile code
            // would silently hand it another attempt.
            ..Default::default()
        };

        let container_config = Config {
            image: Some(config.image.clone()),
            user: Some(SANDBOX_USER.to_string()),
            working_dir: Some(SESSION_ROOT.to_string()),
            env: Some(env),
            labels: Some(labels),
            exposed_ports: if exposed_ports.is_empty() {
                None
            } else {
                Some(exposed_ports)
            },
            // Hold the session open without a shell of its own; commands arrive through exec.
            entrypoint: Some(vec!["/bin/sh".to_string()]),
            cmd: Some(vec!["-c".to_string(), "while true; do sleep 3600; done".to_string()]),
            host_config: Some(host_config),
            ..Default::default()
        };

        let created = self
            .docker
            .create_container(
                Some(CreateContainerOptions {
                    name: name.clone(),
                    platform: None,
                }),
                container_config,
            )
            .await
            .into_alien_error()
            .context(ErrorData::SandboxSessionFailed {
                session_id: session_id.to_string(),
                operation: "create".to_string(),
            })?;

        self.docker
            .start_container::<String>(&created.id, None)
            .await
            .into_alien_error()
            .context(ErrorData::SandboxSessionFailed {
                session_id: session_id.to_string(),
                operation: "start".to_string(),
            })?;

        Ok(SandboxSessionHandle {
            session_id: session_id.to_string(),
            container_id: created.id,
        })
    }

    /// Pulls the sandbox image if the daemon does not already have it.
    ///
    /// A sandbox cannot start without its root filesystem, and Docker's create call does not
    /// pull. Inspecting first keeps the common case off the network.
    async fn ensure_image(&self, image: &str) -> Result<()> {
        if self.docker.inspect_image(image).await.is_ok() {
            return Ok(());
        }

        let mut pull = self.docker.create_image(
            Some(CreateImageOptions {
                from_image: image.to_string(),
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

        Ok(())
    }

    async fn ensure_egress_network(&self, name: &str) -> Result<()> {
        match self.docker.inspect_network::<String>(name, None).await {
            Ok(_) => Ok(()),
            Err(_) => {
                self.docker
                    .create_network(CreateNetworkOptions {
                        name: name.to_string(),
                        driver: "bridge".to_string(),
                        options: HashMap::from([(
                            "com.docker.network.bridge.enable_icc".to_string(),
                            "false".to_string(),
                        )]),
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
    pub async fn exec(
        &self,
        container_id: &str,
        command: &[String],
    ) -> Result<SandboxExecResult> {
        let exec = self
            .docker
            .create_exec(
                container_id,
                CreateExecOptions {
                    cmd: Some(command.to_vec()),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    user: Some(SANDBOX_USER.to_string()),
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
        if let StartExecResults::Attached { output: mut stream, .. } = started {
            while let Some(frame) = stream.next().await {
                let frame = frame.into_alien_error().context(ErrorData::SandboxSessionFailed {
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
        let exit_code = inspect
            .exit_code
            .ok_or_else(|| {
                AlienError::new(ErrorData::SandboxSessionFailed {
                    session_id: container_id.to_string(),
                    operation: "exec finished without an exit code".to_string(),
                })
            })?;

        Ok(SandboxExecResult { output, exit_code })
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
                    user: Some(SANDBOX_USER.to_string()),
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

        if let StartExecResults::Attached { mut input, mut output } = started {
            input
                .write_all(contents)
                .await
                .into_alien_error()
                .context(ErrorData::SandboxSessionFailed {
                    session_id: container_id.to_string(),
                    operation: "write file contents".to_string(),
                })?;
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

        let host_port = inspected
            .network_settings
            .and_then(|settings| settings.ports)
            .and_then(|ports| ports.get(&format!("{port}/tcp")).cloned().flatten())
            .and_then(|bindings| bindings.first().and_then(|binding| binding.host_port.clone()))
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
                filters,
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
            Ok(()) => Ok(()),
            // An already-gone session is the desired end state. Every other failure leaves the
            // container running, and terminate is the containment kill switch — reporting success
            // there tells the caller untrusted code has stopped when it has not.
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(()),
            Err(error) => Err(error).into_alien_error().context(ErrorData::SandboxSessionFailed {
                session_id: session_id.to_string(),
                operation: format!("remove container '{name}'"),
            })?,
        }
    }

    /// Removes every session belonging to a sandbox.
    ///
    /// Run at manager startup so a CLI restart does not leave containers behind, and at
    /// teardown so the Frozen parent's children go first.
    pub async fn reap(&self, sandbox: &str) -> Result<usize> {
        let sessions = self.list_sessions(sandbox).await?;
        let count = sessions.len();

        for session in sessions {
            self.terminate(sandbox, &session.session_id).await?;
        }

        // Best effort: Docker refuses to remove a network with members, which is the correct
        // outcome rather than an error to propagate.
        let _ = self.docker.remove_network(&Self::network_name(sandbox)).await;

        Ok(count)
    }
}

/// Resolves a caller-supplied path against the session root.
///
/// An absolute path means "under the session root", not "on the container's filesystem" — the
/// same rule the in-sandbox agent applies on the cloud backends. Reading it as host-absolute
/// would let a caller name any file in the image, and the session root is the only writable
/// area anyway.
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

    if relative.split('/').any(|part| part == ".." || part.is_empty()) {
        return Err(refused("must not traverse"));
    }

    Ok(format!("{SESSION_ROOT}/{relative}"))
}

#[cfg(test)]
mod tests {
    use super::*;

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
            resolve_in_root(path)
                .expect_err(&format!("'{path}' must be refused before it reaches Docker"));
        }
    }
}
