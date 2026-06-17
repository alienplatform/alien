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
use serde::{Deserialize, Serialize};
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

/// Wire response body. Discriminated by `kind`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum DebugSessionResponse {
    Push(PushDebugSession),
    Pull(PullDebugSession),
}

impl DebugSessionResponse {
    /// RFC3339 expiry of the underlying credentials, if the manager surfaced
    /// one. Sessions without an expiry can't be cached — we always re-mint.
    fn expires_at(&self) -> Option<&str> {
        match self {
            Self::Push(p) => p.expires_at.as_deref(),
            Self::Pull(p) => p.expires_at.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PushDebugSession {
    /// Short identifier shown to the user (e.g. for logging which provider is active).
    provider: String,
    /// Environment variables to set on the spawned process.
    env: BTreeMap<String, String>,
    /// Files to materialize before exec. If `env_var` is set, it's bound to the
    /// resulting absolute file path.
    #[serde(default)]
    files: Vec<DebugCredFile>,
    /// RFC3339 timestamp shown to the user when the session expires.
    #[serde(default)]
    expires_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DebugCredFile {
    /// Filename — no path components. Written under a per-session temp dir.
    file_name: String,
    /// File contents.
    content: String,
    /// If set, the CLI binds this env var to the file's absolute path.
    #[serde(default)]
    env_var: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PullDebugSession {
    /// Kubeconfig YAML written to a temp file; `KUBECONFIG` is bound to it.
    kubeconfig: String,
    /// Additional env vars to set on the spawned process.
    #[serde(default)]
    env: BTreeMap<String, String>,
    /// Files to materialize alongside the kubeconfig.
    #[serde(default)]
    files: Vec<DebugCredFile>,
    /// RFC3339 timestamp shown to the user when the session expires.
    #[serde(default)]
    expires_at: Option<String>,
}

/// Top-level entry for `alien debug`.
pub async fn debug_task(args: DebugArgs, ctx: ExecutionMode) -> Result<()> {
    let (_, project_link) = ctx.resolve_project(None, true).await?;
    let manager = ctx.resolve_manager(&project_link.project_id, "aws").await?;

    let is_dev = ctx.is_dev();
    let deployment_id = resolve_deployment_id(&manager, &args.deployment, is_dev).await?;

    let session = get_or_create_debug_session(&manager, &deployment_id).await?;
    exec_with_session(session, &args.cmd).await
}

/// Return a cached session for this deployment if one exists and isn't about
/// to expire; otherwise mint a fresh one and persist it for next time.
///
/// `alien debug` is often run repeatedly in quick succession (one command,
/// shell, next command). Re-minting on every call burns a STS / Iam
/// `generateAccessToken` round-trip per invocation. Cache lifetime is bounded
/// by the credential's own expiry — there's no way to use it past that.
async fn get_or_create_debug_session(
    manager: &ManagerContext,
    deployment_id: &str,
) -> Result<DebugSessionResponse> {
    if let Some(cached) = session_cache::load(deployment_id) {
        return Ok(cached);
    }
    let session = create_debug_session(manager, deployment_id).await?;
    // Best-effort: cache failures must not break the command.
    if let Err(e) = session_cache::save(deployment_id, &session) {
        tracing::debug!("Failed to cache debug session: {e}");
    }
    Ok(session)
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
async fn create_debug_session(
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
async fn exec_with_session(session: DebugSessionResponse, cmd: &[String]) -> Result<()> {
    // The temp dir is kept alive for the lifetime of the child process via the
    // returned guard. Dropping it removes the credential files from disk.
    let cred_dir = TempDir::new()
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create temp dir".to_string(),
            file_path: "<tempdir>".to_string(),
            reason: "Failed to create temporary directory for debug credentials".to_string(),
        })?;

    let (mode_label, env, expires_at) = match session {
        DebugSessionResponse::Push(push) => {
            let env = materialize_session(&cred_dir, push.env, push.files)?;
            (format!("push/{}", push.provider), env, push.expires_at)
        }
        DebugSessionResponse::Pull(pull) => {
            let kubeconfig_path = write_session_file(
                cred_dir.path(),
                "kubeconfig",
                &pull.kubeconfig,
            )?;
            let mut env = materialize_session(&cred_dir, pull.env, pull.files)?;
            // The kubeconfig env var always wins — if the manager also set one
            // explicitly, this re-sets it to the path we actually wrote.
            env.insert(
                "KUBECONFIG".to_string(),
                kubeconfig_path.display().to_string(),
            );
            ("pull/kubernetes".to_string(), env, pull.expires_at)
        }
    };

    match expires_at.as_deref() {
        Some(exp) => eprintln!("[alien debug] {} session — expires {}", mode_label, exp),
        None => eprintln!("[alien debug] {} session", mode_label),
    }

    let status = spawn_child(cmd, &env).await?;

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
        if file.file_name.contains('/') || file.file_name.contains('\\') || file.file_name == ".."
        {
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

/// Spawn the user's command (or interactive shell) with the merged env.
async fn spawn_child(
    cmd: &[String],
    env: &BTreeMap<String, String>,
) -> Result<std::process::ExitStatus> {
    let (program, args): (String, Vec<String>) = if cmd.is_empty() {
        // Interactive shell fallback. Honor $SHELL; fall back to /bin/sh.
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        (shell, Vec::new())
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

/// On-disk cache of debug sessions, keyed by deployment ID.
///
/// Stored at `<config_dir>/alien/debug_sessions.json` with 0600 perms on
/// Unix. The cache holds the same `DebugSessionResponse` the manager would
/// have returned — we trust its `expires_at` and won't reuse an entry whose
/// expiry is within `REFRESH_WINDOW` of now (avoids handing out creds that
/// expire mid-command).
///
/// Reads and writes are best-effort: any failure falls back to minting a
/// fresh session. The cache is a UX optimization, not a correctness path.
mod session_cache {
    use super::DebugSessionResponse;
    use chrono::{DateTime, Duration, Utc};
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// Skew margin: never reuse a session within this window of its expiry.
    /// Picked at 60s to outlast clock drift + a typical command's runtime.
    const REFRESH_WINDOW: Duration = Duration::seconds(60);

    #[derive(serde::Serialize, serde::Deserialize, Default)]
    struct Store {
        #[serde(default)]
        sessions: HashMap<String, DebugSessionResponse>,
    }

    fn cache_path() -> Option<PathBuf> {
        Some(
            dirs::config_dir()?
                .join("alien")
                .join("debug_sessions.json"),
        )
    }

    fn read_store() -> Option<Store> {
        let path = cache_path()?;
        let contents = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&contents).ok()
    }

    fn write_store(store: &Store) -> std::io::Result<()> {
        let path = cache_path().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no user config dir available",
            )
        })?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(store)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        // Write to a sibling temp file, fix perms, then atomic rename. Keeps
        // concurrent readers from seeing a partial file.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Return a cached session for `deployment_id` if one exists and won't
    /// expire within `REFRESH_WINDOW`. Otherwise `None`.
    pub fn load(deployment_id: &str) -> Option<DebugSessionResponse> {
        let store = read_store()?;
        let session = store.sessions.get(deployment_id)?.clone();
        let expiry_str = session.expires_at()?;
        let expiry = DateTime::parse_from_rfc3339(expiry_str).ok()?.with_timezone(&Utc);
        if expiry - Utc::now() <= REFRESH_WINDOW {
            return None;
        }
        Some(session)
    }

    /// Persist a freshly-minted session for `deployment_id`. Sessions without
    /// an `expires_at` are not cached — we have no way to know when they go
    /// stale.
    pub fn save(deployment_id: &str, session: &DebugSessionResponse) -> std::io::Result<()> {
        if session.expires_at().is_none() {
            return Ok(());
        }
        let mut store = read_store().unwrap_or_default();
        store
            .sessions
            .insert(deployment_id.to_string(), session.clone());
        write_store(&store)
    }
}
