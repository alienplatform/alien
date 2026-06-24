//! `alien debug` — run a local command (or interactive shell) against a
//! deployment using credentials provided by the manager.
//!
//! The CLI's job is narrow and self-contained:
//!
//! 1. Resolve the deployment to a `dep_...` ID.
//! 2. `POST /v1/debug/sessions` to request a debug session.
//! 3. Materialize any files the response asks for under a per-session temp dir
//!    (chmod 0600 on Unix), build the merged env, and `exec` the user command
//!    (or a `$SHELL` if no command was given) with that env set.
//! 4. On exit, drop the temp dir so credential files are removed.
//!
//! The exact meaning of the env vars and kubeconfig in the response is decided
//! by the manager — this module only marshals what the contract returns.

use crate::error::{ErrorData, Result};
use crate::execution_context::{ExecutionMode, ManagerContext};
use alien_error::{AlienError, Context, IntoAlienError};
use clap::Parser;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use tempfile::TempDir;

const DEBUG_SESSIONS_PATH: &str = "/v1/debug/sessions";

/// Arguments for `alien debug`.
#[derive(Parser, Debug, Clone)]
#[command(
    about = "Run a command against a deployment using credentials provided by the manager",
    long_about = "Request a debug session from the manager for a deployment, then run a \
local command (or an interactive shell) with the env the manager returns.

DEPLOYMENT can be a deployment ID (`dep_...`), a deployment name, or `<group>/<name>`.

EXAMPLES:
    alien debug dep_abc123 -- aws sts get-caller-identity
    alien debug acme/prod -- gcloud projects list
    alien debug acme/prod -- kubectl get pods

    # No `--` arg drops you into an interactive shell with the env set:
    alien debug acme/prod

See also: https://alien.dev/docs/debug"
)]
pub struct DebugArgs {
    /// Deployment ID (`dep_...`), deployment name, or `<group>/<name>`.
    pub deployment: String,

    /// Command and arguments to execute. Everything after `--` is forwarded verbatim.
    /// If omitted, an interactive shell ($SHELL, or /bin/sh) is spawned instead.
    #[arg(last = true)]
    pub cmd: Vec<String>,

    /// Emit errors as JSON. The spawned command's stdout/stderr are always passed
    /// through unchanged.
    #[arg(long)]
    pub json: bool,
}

/// Wire request body for `POST /v1/debug/sessions`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateDebugSessionRequest {
    /// The resolved deployment ID (always `dep_...`).
    deployment_id: String,
}

// Wire types live in `alien-debug-session` so the manager (push mode) and
// the agent (pull mode) can produce identical payloads. The CLI only
// consumes — no provider-specific knowledge needed here.
use alien_core::debug_session::{DebugCredFile, DebugSessionResponse};

/// Top-level entry for `alien debug`.
pub async fn debug_task(args: DebugArgs, ctx: ExecutionMode) -> Result<()> {
    let (_, project_link) = ctx.resolve_project(None, true).await?;
    // `debug` never pushes images, so skip the artifact-registry repo
    // provisioning step — saves ~10s per invocation against cloud managers.
    let manager = ctx
        .resolve_manager_metadata_only(&project_link.project_id, "aws")
        .await?;

    let is_dev = ctx.is_dev();
    let deployment_id = resolve_deployment_id(&manager, &args.deployment, is_dev).await?;

    // No CLI-side caching: every invocation asks the manager to create-or-
    // reuse a session. The manager controls session lifetime, token rotation,
    // and registry eviction.
    let session = request_debug_session(&manager, &deployment_id).await?;
    let session = resolve_pending_session(&manager, session).await?;
    exec_with_session(&args.deployment, session, &args.cmd).await
}

/// If the manager returned a `Pending` session (pull-mode kubernetes async
/// flow), long-poll `poll_url` until it resolves to a ready `Pull`/`Push`
/// payload or the deadline passes. All other variants pass through unchanged.
///
/// The manager controls cadence via `poll_interval_ms` in the initial reply;
/// we honour that as the floor and back off linearly up to 5s on repeated
/// `Pending` responses. Errors during polling bubble up — there's no point
/// retrying when the manager is telling us the session is broken.
async fn resolve_pending_session(
    manager: &ManagerContext,
    session: DebugSessionResponse,
) -> Result<DebugSessionResponse> {
    let pending = match session {
        DebugSessionResponse::Pending(p) => p,
        other => return Ok(other),
    };

    let deadline = chrono::DateTime::parse_from_rfc3339(&pending.deadline)
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!(
                "Manager returned an invalid Pending deadline '{}'",
                pending.deadline
            ),
            url: Some(pending.poll_url.clone()),
        })?
        .with_timezone(&chrono::Utc);

    let min_interval = std::time::Duration::from_millis(pending.poll_interval_ms.max(250) as u64);
    let max_interval = std::time::Duration::from_secs(5);
    let mut interval = min_interval;

    loop {
        if chrono::Utc::now() >= deadline {
            return Err(AlienError::new(ErrorData::ApiRequestFailed {
                message: format!(
                    "Debug session '{}' did not become ready before deadline {}",
                    pending.session_id, pending.deadline
                ),
                url: Some(pending.poll_url.clone()),
            }));
        }

        let response = manager
            .http_client
            .get(&pending.poll_url)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: "Failed to poll debug session".to_string(),
                url: Some(pending.poll_url.clone()),
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(AlienError::new(ErrorData::ApiRequestFailed {
                message: format!(
                    "Manager rejected debug session poll (HTTP {}): {}",
                    status.as_u16(),
                    body.trim()
                ),
                url: Some(pending.poll_url.clone()),
            }));
        }

        let next: DebugSessionResponse =
            response
                .json()
                .await
                .into_alien_error()
                .context(ErrorData::ApiRequestFailed {
                    message: "Manager returned a malformed debug session poll response".to_string(),
                    url: Some(pending.poll_url.clone()),
                })?;

        match next {
            DebugSessionResponse::Pending(_) => {
                tokio::time::sleep(interval).await;
                interval = (interval + std::time::Duration::from_millis(500)).min(max_interval);
            }
            ready => return Ok(ready),
        }
    }
}

/// Dev-mode entry: wraps [`debug_task`] with `ExecutionMode::Dev`.
pub async fn debug_task_dev(args: DebugArgs, port: u16) -> Result<()> {
    debug_task(args, ExecutionMode::Dev { port }).await
}

/// Parse the user-supplied deployment spec and resolve it to a `dep_...` ID.
///
/// Delegates to the shared `deployment_resolver` so the spec form, server-side
/// `search` filtering, and "not found / ambiguous" messages stay in sync
/// across `debug`, `commands`, and `deployments {get,delete,retry,redeploy}`.
async fn resolve_deployment_id(
    manager: &ManagerContext,
    spec: &str,
    is_dev: bool,
) -> Result<String> {
    let deployment = crate::deployment_resolver::resolve(&manager.client, spec, is_dev).await?;
    Ok(deployment.id.to_string())
}

/// `POST /v1/debug/sessions` — request a debug session for the given deployment.
async fn request_debug_session(
    manager: &ManagerContext,
    deployment_id: &str,
) -> Result<DebugSessionResponse> {
    let url = format!(
        "{}{}",
        manager.manager_url.trim_end_matches('/'),
        DEBUG_SESSIONS_PATH
    );

    let request_body = CreateDebugSessionRequest {
        deployment_id: deployment_id.to_string(),
    };

    let response = manager
        .http_client
        .post(&url)
        .json(&request_body)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: "Failed to create debug session".to_string(),
            url: Some(url.clone()),
        })?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message: format!(
                "Manager rejected debug session request (HTTP {}): {}",
                status.as_u16(),
                body.trim()
            ),
            url: Some(url),
        }));
    }

    response
        .json::<DebugSessionResponse>()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Manager returned a malformed debug session response".to_string(),
            url: Some(url),
        })
}

/// Materialize files, build the env, and execute the user command (or a shell).
async fn exec_with_session(
    deployment_label: &str,
    session: DebugSessionResponse,
    cmd: &[String],
) -> Result<()> {
    // The temp dir is kept alive for the lifetime of the child process via the
    // returned guard. Dropping it removes the credential files from disk.
    let cred_dir = TempDir::new()
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create temp dir".to_string(),
            file_path: "<tempdir>".to_string(),
            reason: "Failed to create temporary directory for debug credentials".to_string(),
        })?;

    // Used to build the interactive-shell banner / prompt. Captured from the
    // session shape because the response variant tells us push vs pull and
    // which cloud the loopback was opened for.
    let mut session_kind = SessionKind {
        mode: SessionMode::Push,
        provider: None,
    };

    // `_push_tunnel_guard` keeps the loopback proxy + WebSocket alive for
    // the lifetime of this function. For non-PushTunnel sessions it's None.
    let (env, setup_script, _push_tunnel_guard) = match session {
        DebugSessionResponse::Push(push) => {
            let env = materialize_session(&cred_dir, push.env, push.files)?;
            (env, push.setup_script, None)
        }
        DebugSessionResponse::Pull(pull) => {
            session_kind.mode = SessionMode::Pull;
            session_kind.provider = if pull.aws_endpoint_url.is_some() {
                Some("aws")
            } else if pull.gcp_endpoint_url.is_some() {
                Some("gcp")
            } else if pull.azure_endpoint_url.is_some() {
                Some("azure")
            } else {
                None
            };
            let kubeconfig_path =
                write_session_file(cred_dir.path(), "kubeconfig", &pull.kubeconfig)?;
            let mut env = materialize_session(&cred_dir, pull.env, pull.files)?;
            // The kubeconfig env var always wins — if the manager also set one
            // explicitly, this re-sets it to the path we actually wrote.
            env.insert(
                "KUBECONFIG".to_string(),
                kubeconfig_path.display().to_string(),
            );

            // If the manager advertised cloud-proxy URLs, also spawn the
            // matching local loopbacks so the user can run
            // `alien debug … -- aws/gcloud/az …` against the operator's
            // in-cluster cloud identity. Loopback guards live for the
            // child's run.
            let token = pull.cloud_proxy_token.clone();
            let mut cloud_guards: Vec<crate::commands::debug_tunnel::PushTunnelGuard> = Vec::new();

            if let (Some(url), Some(tok)) = (pull.aws_endpoint_url, token.clone()) {
                let (mut e, g) =
                    crate::commands::debug_tunnel::spawn_pull_aws_loopback(&url, &tok).await?;
                env.append(&mut e);
                cloud_guards.push(g);
            }
            if let (Some(url), Some(tok)) = (pull.gcp_endpoint_url, token.clone()) {
                let (mut e, g) =
                    crate::commands::debug_tunnel::spawn_pull_gcp_loopback(&url, &tok).await?;
                env.append(&mut e);
                let gcloud_cfg = cred_dir.path().join("gcloud-config");
                let mut isolation =
                    crate::commands::debug_tunnel::build_gcp_isolation_env(&gcloud_cfg, None)?;
                env.append(&mut isolation);
                cloud_guards.push(g);
            }
            if let (Some(url), Some(tok)) = (pull.azure_endpoint_url, token.clone()) {
                let (mut e, g) =
                    crate::commands::debug_tunnel::spawn_pull_azure_loopback(&url, &tok).await?;
                env.append(&mut e);
                cloud_guards.push(g);
            }

            (
                env,
                None,
                if cloud_guards.is_empty() {
                    None
                } else {
                    Some(crate::commands::debug_tunnel::PushTunnelGuard::merge(
                        cloud_guards,
                    ))
                },
            )
        }
        DebugSessionResponse::Pending(_) => {
            // `resolve_pending_session` runs before this point and is supposed
            // to long-poll until the manager hands back a ready Push or Pull.
            // Reaching this arm means a programming error in the caller chain.
            return Err(AlienError::new(ErrorData::ApiRequestFailed {
                message: "BUG: exec_with_session received an unresolved Pending session — \
                          resolve_pending_session must run first"
                    .to_string(),
                url: None,
            }));
        }
        DebugSessionResponse::PushTunnel(tunnel) => {
            session_kind.mode = SessionMode::Push;
            session_kind.provider = Some(match tunnel.provider.as_str() {
                "aws" => "aws",
                "gcp" => "gcp",
                "azure" => "azure",
                _ => "cloud",
            });
            // Push-mode tunnel: dial the manager's WebSocket, bring up a
            // local HTTP loopback proxy, set the cloud-CLI's endpoint env
            // to point at loopback. Bytes the child process sends flow
            // through the WebSocket; the manager re-signs with the
            // impersonated identity and proxies to the real cloud endpoint.
            let (mut env, guard) =
                crate::commands::debug_tunnel::spawn_push_tunnel(&tunnel).await?;
            if tunnel.provider == "gcp" {
                // Isolate gcloud from the user's local login state — the
                // manager owns the identity; gcloud must not attach a
                // personal OAuth token or default to a local project.
                let gcloud_cfg = cred_dir.path().join("gcloud-config");
                let mut isolation =
                    crate::commands::debug_tunnel::build_gcp_isolation_env(&gcloud_cfg, None)?;
                env.append(&mut isolation);
            }
            (env, None, Some(guard))
        }
    };

    if let Some(script) = setup_script {
        run_setup_script(&script, &env).await?;
    }

    let region = extract_region_from_env(&env, session_kind.provider);
    let status = spawn_child(
        deployment_label,
        &session_kind,
        region.as_deref(),
        &cred_dir,
        cmd,
        &env,
    )
    .await?;

    // The temp dir must outlive the child. Drop happens here, after wait.
    drop(cred_dir);

    if let Some(code) = status.code() {
        if code != 0 {
            std::process::exit(code);
        }
    } else {
        // Killed by signal (Unix). Surface as non-zero exit.
        std::process::exit(1);
    }

    Ok(())
}

/// Write any cred files to `dir` and merge their `env_var` mappings into `env`.
fn materialize_session(
    dir: &TempDir,
    mut env: BTreeMap<String, String>,
    files: Vec<DebugCredFile>,
) -> Result<BTreeMap<String, String>> {
    for file in files {
        if file.file_name.contains('/') || file.file_name.contains('\\') || file.file_name == ".." {
            return Err(AlienError::new(ErrorData::ApiRequestFailed {
                message: format!(
                    "Manager returned an unsafe debug credential filename: '{}'",
                    file.file_name
                ),
                url: None,
            }));
        }
        let path = write_session_file(dir.path(), &file.file_name, &file.content)?;
        if let Some(var) = file.env_var {
            env.insert(var, path.display().to_string());
        }
    }
    Ok(env)
}

/// Write `content` to `dir/name` with 0600 perms (Unix). Returns the absolute path.
fn write_session_file(dir: &std::path::Path, name: &str, content: &str) -> Result<PathBuf> {
    let path = dir.join(name);
    std::fs::write(&path, content)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: path.display().to_string(),
            reason: "Failed to write debug credential file".to_string(),
        })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&path, perms)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "chmod".to_string(),
                file_path: path.display().to_string(),
                reason: "Failed to restrict debug credential file permissions".to_string(),
            })?;
    }
    Ok(path)
}

/// Run the manager-supplied setup snippet (`sh -c <script>`) with the
/// merged debug-session env. Used today by Azure to `az login` before the
/// user's command. Output is forwarded to stderr so the user can see what
/// it's doing; a non-zero exit aborts the debug session.
async fn run_setup_script(script: &str, env: &BTreeMap<String, String>) -> Result<()> {
    let status = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(script)
        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .into_alien_error()
        .context(ErrorData::LocalServiceFailed {
            service: "sh".to_string(),
            reason: "Failed to run debug-session setup script".to_string(),
        })?;

    if !status.success() {
        return Err(AlienError::new(ErrorData::LocalServiceFailed {
            service: "sh".to_string(),
            reason: format!(
                "Debug-session setup script exited with status {}.",
                status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".to_string())
            ),
        }));
    }

    Ok(())
}

/// Compact summary used to build the interactive-shell banner and prompt.
struct SessionKind {
    mode: SessionMode,
    /// "aws" | "gcp" | "azure" | None.
    provider: Option<&'static str>,
}

#[derive(Clone, Copy)]
enum SessionMode {
    Push,
    Pull,
}

impl SessionMode {
    fn label(self) -> &'static str {
        match self {
            SessionMode::Push => "push",
            SessionMode::Pull => "pull",
        }
    }
}

/// Spawn the user's command (or interactive shell) with the merged env.
async fn spawn_child(
    deployment_label: &str,
    session_kind: &SessionKind,
    region: Option<&str>,
    cred_dir: &TempDir,
    cmd: &[String],
    env: &BTreeMap<String, String>,
) -> Result<std::process::ExitStatus> {
    let (program, args): (String, Vec<String>) = if cmd.is_empty() {
        // No user command — drop into an interactive shell with a branded
        // prompt + banner so it's obvious every command runs against the
        // remote deployment. We honor $SHELL but only special-case bash/zsh
        // for prompt customization; other shells get a vanilla session.
        build_interactive_shell(deployment_label, session_kind, region, cred_dir)?
    } else {
        (cmd[0].clone(), cmd[1..].to_vec())
    };

    let mut command = tokio::process::Command::new(&program);
    command
        .args(&args)
        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let mut child = command
        .spawn()
        .into_alien_error()
        .context(ErrorData::LocalServiceFailed {
            service: program.clone(),
            reason: "Failed to spawn debug command. Is it installed and on PATH?".to_string(),
        })?;

    child
        .wait()
        .await
        .into_alien_error()
        .context(ErrorData::LocalServiceFailed {
            service: program,
            reason: "Failed to wait for debug command".to_string(),
        })
}

/// Build the program + args for an interactive debug shell, optionally
/// writing a per-shell rc file into `cred_dir` so the prompt makes it
/// obvious every command runs against the remote deployment.
///
/// Recognized shells:
///   * `bash` — pass `--rcfile <path>` to a generated rc that sets PS1 and
///     prints the banner. We deliberately don't re-source `~/.bashrc` so
///     user aliases don't leak in and accidentally redirect commands.
///   * `zsh` — set `ZDOTDIR=<cred_dir>` and write a `.zshrc` there.
///   * anything else — fall through to a plain interactive shell with no
///     prompt customization; print the banner before spawning so the user
///     still sees the context.
fn build_interactive_shell(
    deployment_label: &str,
    session_kind: &SessionKind,
    region: Option<&str>,
    cred_dir: &TempDir,
) -> Result<(String, Vec<String>)> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let shell_name = std::path::Path::new(&shell)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("sh");
    let banner = render_banner(deployment_label, session_kind, region);
    // Show only the deployment name in the prompt — strip any `<group>/`
    // prefix so the line stays short.
    let deployment_name = deployment_label
        .rsplit('/')
        .next()
        .unwrap_or(deployment_label);
    let prompt_tag = deployment_name.to_string();

    match shell_name {
        "bash" => {
            let rc_path = cred_dir.path().join("bashrc");
            let rc = format!(
                "{banner}\nexport PS1='\\[\\e[32m\\]\u{25CB}\\[\\e[0m\\] \\[\\e[36m\\]{tag}\\[\\e[0m\\] ❯ '\n",
                banner = shell_echo_block(&banner),
                tag = prompt_tag,
            );
            std::fs::write(&rc_path, rc).into_alien_error().context(
                ErrorData::FileOperationFailed {
                    operation: "write".to_string(),
                    file_path: rc_path.display().to_string(),
                    reason: "Failed to write bash rc for debug shell".to_string(),
                },
            )?;
            Ok((
                shell,
                vec![
                    "--rcfile".to_string(),
                    rc_path.display().to_string(),
                    "-i".to_string(),
                ],
            ))
        }
        "zsh" => {
            // ZDOTDIR redirects zsh to our per-session rc dir so we don't
            // pollute the user's real ~/.zshrc. We write two files:
            //   - .zshenv (always sourced, even for non-interactive shells)
            //     prints the banner and sets PS1.
            //   - .zshrc (sourced for interactive shells) does the same so
            //     either invocation mode prints the banner.
            //
            // We deliberately use simple `print -P` for the banner instead
            // of `printf '%s\n' '...'` because ANSI escape bytes embedded
            // in single-quoted strings inside the rc file can confuse
            // zsh's parser into staying in a continuation state when the
            // user later types `exit` (visible as a stuck `function>` PS2
            // prompt). `print -P` accepts %-escapes natively and never
            // sees a literal ESC byte in the source.
            let zshrc = cred_dir.path().join(".zshrc");
            // The user is one stray `exit()` away from defining an empty
            // function that shadows the builtin and recurses to FUNCNEST.
            // Alias `exit` to the builtin so a typo is harmless.
            let rc = format!(
                "alias exit='builtin exit'\n\
                 print -P '%F{{green}}\u{25CB}%f alien · attached to %B{deployment}%b · {mode}-mode'\n\
                 print -P '%F{{green}}\u{2713}%f session ready · type `exit` to end'\n\
                 export PS1=$'%F{{green}}\u{25CB}%f %F{{cyan}}{tag}%f \u{276F} '\n",
                deployment = deployment_label,
                mode = session_kind.mode.label(),
                tag = prompt_tag,
            );
            std::fs::write(&zshrc, &rc).into_alien_error().context(
                ErrorData::FileOperationFailed {
                    operation: "write".to_string(),
                    file_path: zshrc.display().to_string(),
                    reason: "Failed to write zsh rc for debug shell".to_string(),
                },
            )?;
            std::env::set_var("ZDOTDIR", cred_dir.path());
            Ok((shell, vec!["-i".to_string()]))
        }
        _ => {
            // Unknown shell: print the banner here, then exec a plain shell.
            eprintln!("{}", banner);
            Ok((shell, Vec::new()))
        }
    }
}

/// Render the multi-line shell banner shown when entering the interactive
/// debug shell. Plain ASCII + ANSI colors; no emoji.
fn render_banner(
    deployment_label: &str,
    session_kind: &SessionKind,
    region: Option<&str>,
) -> String {
    let mut parts = vec![format!("attached to \x1b[1m{deployment_label}\x1b[0m")];
    if let Some(p) = session_kind.provider {
        parts.push(p.to_string());
    }
    if let Some(r) = region {
        parts.push(r.to_string());
    }
    parts.push(format!("{}-mode", session_kind.mode.label()));
    let line = parts.join(" \x1b[2m·\x1b[0m ");
    format!(
        "\x1b[32m\u{25CB}\x1b[0m alien · {line}\n\
         \x1b[32m\u{2713}\x1b[0m session ready · type `exit` to end"
    )
}

/// Pull a human-readable region label out of the env we're about to hand to
/// the child. Different providers use different env-var names; we check the
/// canonical one for each. Returns `None` for unknown / unset.
fn extract_region_from_env(
    env: &BTreeMap<String, String>,
    provider: Option<&'static str>,
) -> Option<String> {
    let key = match provider? {
        "aws" => "AWS_REGION",
        "gcp" => "CLOUDSDK_COMPUTE_REGION",
        "azure" => "AZURE_DEFAULTS_LOCATION",
        _ => return None,
    };
    env.get(key).cloned()
}

/// Wrap a multi-line string into a series of `printf` lines safe to embed
/// in a shell rc file. Avoids quoting headaches with the banner's ANSI
/// escapes by using `printf '%s\n'`.
fn shell_echo_block(text: &str) -> String {
    text.lines()
        .map(|line| {
            // Escape single quotes for printf '...'.
            let escaped = line.replace('\'', r"'\''");
            format!("printf '%s\\n' '{escaped}'")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
