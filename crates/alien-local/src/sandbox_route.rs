//! Authenticated loopback route to the local sandbox manager.
//!
//! The binding provider cannot call the manager in process: `alien-local` depends on
//! `alien-bindings`, so a direct call would be a dependency cycle. Giving the workload Docker
//! socket access instead would be worse — it hands every application the ability to escape its
//! own sandbox. So the provider speaks over loopback, which also means Local exercises the real
//! transport rather than a shortcut.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

use crate::error::{ErrorData, Result};
use crate::sandbox_manager::{LocalSandboxManager, SandboxOutput, SandboxSessionConfig};
use alien_error::{AlienError, IntoAlienError};

/// Bearer token file for one sandbox, inside the deployment state directory.
///
/// Per sandbox: a shared path means whichever sandbox wrote last owns the credential for all of
/// them, and the first one's binding then authenticates with a token that is no longer valid.
fn token_file_name(sandbox: &str) -> String {
    format!("sandbox-manager-{sandbox}.token")
}

/// A request to create a session.
///
/// Carries only an id. The image, limits, egress mode and preview ports come from the template
/// the controller configured — an application must not be able to raise its own ceilings, and
/// a client-supplied limit is a limit the client can choose not to send.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionBody {
    /// Session id within the sandbox
    pub session_id: String,
}

/// A request to run a command.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecBody {
    /// Command and arguments
    pub command: Vec<String>,
}

/// A request to write a file.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteFileBody {
    /// Absolute path inside the session
    pub path: String,
    /// Contents, base64 because a file is arbitrary bytes
    pub contents_base64: String,
}

/// Which preview port to resolve.
#[derive(Debug, Deserialize)]
pub struct PreviewQuery {
    /// Port declared at create time
    pub port: u16,
}

/// An authenticated capability to reach a port inside a session.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewResponse {
    /// Loopback endpoint the port is published on
    pub endpoint: String,
    /// Ports this capability admits
    pub allowed_ports: Vec<u16>,
}

/// Which file to read.
#[derive(Debug, Deserialize)]
pub struct ReadFileQuery {
    /// Absolute path inside the session
    pub path: String,
}

/// A session as the route reports it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBody {
    /// Session id within the sandbox
    pub session_id: String,
    /// Docker container backing it
    pub container_id: String,
}

/// One output frame.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", tag = "stream", content = "dataBase64")]
pub enum OutputFrame {
    /// Bytes written to stdout
    Stdout(String),
    /// Bytes written to stderr
    Stderr(String),
}

/// A finished command.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecResponse {
    /// Frames in production order
    pub output: Vec<OutputFrame>,
    /// Process exit code
    pub exit_code: i64,
}

/// File contents on the way out.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadFileResponse {
    /// Contents, base64 because a file is arbitrary bytes
    pub contents_base64: String,
}

#[derive(Clone)]
struct RouteState {
    manager: Arc<LocalSandboxManager>,
    sandbox: String,
    token: String,
    /// Replaced in place on update. Held behind a lock rather than moved into the router so an
    /// updated declaration — new limits, image, egress or preview ports — reaches sessions
    /// created after it without rebinding the route the workload was already given.
    template: Arc<Mutex<SandboxSessionConfig>>,
}

/// A running loopback route, and where to reach it.
#[derive(Debug)]
pub struct SandboxRoute {
    /// Base URL the binding provider talks to
    pub base_url: String,
    /// File holding the bearer token. The binding carries this path, never the token itself,
    /// so no secret reaches deployment state.
    pub token_path: std::path::PathBuf,
}

/// One serving route, and what is needed to update or stop it.
struct ServingRoute {
    base_url: String,
    token_path: std::path::PathBuf,
    template: Arc<Mutex<SandboxSessionConfig>>,
    /// Dropped to stop the listener. Taken on removal, so a second removal is a no-op.
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    /// The listener task itself. Awaited on removal, because stopping is a signal and a create
    /// already inside the route only finishes when the task does.
    serving: Option<tokio::task::JoinHandle<()>>,
}

/// Routes already serving in this process, keyed by sandbox id.
///
/// The controller runs its health tick every few seconds, and a route per tick would leak a
/// listener each time and hand the workload a different port than the one it was given.
static ROUTES: OnceLock<Mutex<HashMap<String, ServingRoute>>> = OnceLock::new();

impl SandboxRoute {
    /// Returns the sandbox's route, serving it first if this process has not already.
    ///
    /// Idempotent by sandbox id, so a controller can call it on every step.
    pub async fn ensure(
        manager: Arc<LocalSandboxManager>,
        sandbox: &str,
        template: SandboxSessionConfig,
    ) -> Result<Self> {
        let routes = ROUTES.get_or_init(|| Mutex::new(HashMap::new()));

        {
            let serving = routes.lock().expect("no panic holds this lock");
            if let Some(existing) = serving.get(sandbox) {
                // An update changes the template, not the address: the workload already holds
                // this URL, and rebinding would strand it on a dead port.
                *existing.template.lock().expect("no panic holds this lock") = template;
                return Ok(Self {
                    base_url: existing.base_url.clone(),
                    token_path: existing.token_path.clone(),
                });
            }
        }

        let (route, template, shutdown, serving) =
            Self::serve(manager, sandbox, template).await?;

        routes.lock().expect("no panic holds this lock").insert(
            sandbox.to_string(),
            ServingRoute {
                base_url: route.base_url.clone(),
                token_path: route.token_path.clone(),
                template,
                shutdown: Some(shutdown),
                serving: Some(serving),
            },
        );

        Ok(route)
    }

    /// Stops one sandbox's route and removes what it left behind.
    ///
    /// Delete has to reach further than the containers: a route left serving keeps accepting
    /// session creates for a sandbox that no longer exists, and a token file left on disk is a
    /// live credential for it.
    pub async fn remove(sandbox: &str) {
        let Some(routes) = ROUTES.get() else {
            return;
        };

        let removed = routes
            .lock()
            .expect("no panic holds this lock")
            .remove(sandbox);

        let Some(mut route) = removed else {
            return;
        };

        // Dropping the sender is what the listener's graceful shutdown waits on. It is a signal,
        // not a fact: a create already inside the route finishes only when the task does, and a
        // sweep that ran before then could be outlived by that create. So the task is awaited.
        drop(route.shutdown.take());
        if let Some(serving) = route.serving.take() {
            // A panicked listener has already stopped serving, which is the state wanted here.
            let _ = serving.await;
        }

        // Best effort: an already-removed token file is the desired end state.
        let _ = tokio::fs::remove_file(&route.token_path).await;
    }

    /// Binds the route on loopback and serves it until the process ends.
    ///
    /// Port 0: the OS picks, and the binding learns the address from the resource's outputs.
    /// A fixed port would collide between two deployments on one machine.
    async fn serve(
        manager: Arc<LocalSandboxManager>,
        sandbox: &str,
        template: SandboxSessionConfig,
    ) -> Result<(
        Self,
        Arc<Mutex<SandboxSessionConfig>>,
        tokio::sync::oneshot::Sender<()>,
        tokio::task::JoinHandle<()>,
    )> {
        let token = generate_token();
        let token_path = manager.state_dir().join(token_file_name(sandbox));

        if let Some(parent) = token_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .into_alien_error()
                .map_err(|error| {
                    AlienError::new(ErrorData::SandboxSessionFailed {
                        session_id: sandbox.to_string(),
                        operation: format!("create state directory: {error}"),
                    })
                })?;
        }

        write_token_file(&token_path, &token).await?;

        let template = Arc::new(Mutex::new(template));
        let state = RouteState {
            manager,
            sandbox: sandbox.to_string(),
            token,
            template: Arc::clone(&template),
        };

        let router = Router::new()
            .route("/v1/sessions", post(create_session).get(list_sessions))
            .route("/v1/sessions/{session_id}", axum::routing::delete(terminate))
            .route("/v1/sessions/{session_id}/exec", post(exec))
            .route("/v1/sessions/{session_id}/files", get(read_file).put(write_file))
            .route("/v1/sessions/{session_id}/preview", get(preview))
            .with_state(state);

        let listener = TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().expect("literal"))
            .await
            .into_alien_error()
            .map_err(|error| {
                AlienError::new(ErrorData::SandboxSessionFailed {
                    session_id: sandbox.to_string(),
                    operation: format!("bind loopback route: {error}"),
                })
            })?;

        let address = listener.local_addr().into_alien_error().map_err(|error| {
            AlienError::new(ErrorData::SandboxSessionFailed {
                session_id: sandbox.to_string(),
                operation: format!("read route address: {error}"),
            })
        })?;

        let (shutdown, stop) = tokio::sync::oneshot::channel::<()>();
        let serving = tokio::spawn(async move {
            let serve = axum::serve(listener, router).with_graceful_shutdown(async move {
                // Either an explicit stop or the sender being dropped ends the wait.
                let _ = stop.await;
            });
            if let Err(error) = serve.await {
                tracing::error!("local sandbox route stopped: {error}");
            }
        });

        Ok((
            Self {
                base_url: format!("http://{address}"),
                token_path,
            },
            template,
            shutdown,
            serving,
        ))
    }
}

/// Writes the token so that it is never readable by another user on the machine, not even for an
/// instant.
///
/// The mode is set at create time rather than chmod'd after: writing first and restricting second
/// leaves the bearer token world-readable for as long as the two calls take, and this is a
/// credential that grants session creation.
async fn write_token_file(path: &std::path::Path, token: &str) -> Result<()> {
    let failed = |error: std::io::Error| {
        AlienError::new(ErrorData::SandboxSessionFailed {
            session_id: path.display().to_string(),
            operation: format!("write token file: {error}"),
        })
    };

    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    // tokio's own `mode`, not the std extension trait — the file is created already restricted
    // rather than chmod'd after, so the token is never briefly world-readable.
    #[cfg(unix)]
    options.mode(0o600);

    let mut file = options.open(path).await.map_err(failed)?;
    tokio::io::AsyncWriteExt::write_all(&mut file, token.as_bytes())
        .await
        .map_err(failed)?;
    tokio::io::AsyncWriteExt::flush(&mut file)
        .await
        .map_err(failed)?;

    Ok(())
}

fn generate_token() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

/// Compares in constant time, so a caller cannot recover the token one byte at a time.
fn token_matches(expected: &str, presented: &str) -> bool {
    if expected.len() != presented.len() {
        return false;
    }

    expected
        .bytes()
        .zip(presented.bytes())
        .fold(0u8, |differences, (a, b)| differences | (a ^ b))
        == 0
}

fn authorize(state: &RouteState, headers: &HeaderMap) -> std::result::Result<(), StatusCode> {
    let presented = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if token_matches(&state.token, presented) {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

fn failed(error: AlienError<ErrorData>) -> (StatusCode, String) {
    (StatusCode::BAD_GATEWAY, error.to_string())
}

async fn create_session(
    State(state): State<RouteState>,
    headers: HeaderMap,
    Json(body): Json<CreateSessionBody>,
) -> std::result::Result<Json<SessionBody>, (StatusCode, String)> {
    authorize(&state, &headers).map_err(|code| (code, "unauthorized".to_string()))?;

    // Cloned out of the lock: the session create is an await, and the guard is not Send.
    let template = state
        .template
        .lock()
        .expect("no panic holds this lock")
        .clone();

    let handle = state
        .manager
        .create_session(&state.sandbox, &body.session_id, &template)
        .await
        .map_err(failed)?;

    Ok(Json(SessionBody {
        session_id: handle.session_id,
        container_id: handle.container_id,
    }))
}

async fn list_sessions(
    State(state): State<RouteState>,
    headers: HeaderMap,
) -> std::result::Result<Json<Vec<SessionBody>>, (StatusCode, String)> {
    authorize(&state, &headers).map_err(|code| (code, "unauthorized".to_string()))?;

    let sessions = state
        .manager
        .list_sessions(&state.sandbox)
        .await
        .map_err(failed)?;

    Ok(Json(
        sessions
            .into_iter()
            .map(|handle| SessionBody {
                session_id: handle.session_id,
                container_id: handle.container_id,
            })
            .collect(),
    ))
}

async fn container_for(
    state: &RouteState,
    session_id: &str,
) -> std::result::Result<String, (StatusCode, String)> {
    let sessions = state
        .manager
        .list_sessions(&state.sandbox)
        .await
        .map_err(failed)?;

    sessions
        .into_iter()
        .find(|handle| handle.session_id == session_id)
        .map(|handle| handle.container_id)
        .ok_or((StatusCode::NOT_FOUND, format!("no session '{session_id}'")))
}

async fn exec(
    State(state): State<RouteState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<ExecBody>,
) -> std::result::Result<Json<ExecResponse>, (StatusCode, String)> {
    authorize(&state, &headers).map_err(|code| (code, "unauthorized".to_string()))?;

    let container_id = container_for(&state, &session_id).await?;
    let result = state
        .manager
        .exec(&container_id, &body.command)
        .await
        .map_err(failed)?;

    Ok(Json(ExecResponse {
        output: result
            .output
            .into_iter()
            .map(|frame| match frame {
                SandboxOutput::Stdout(bytes) => OutputFrame::Stdout(BASE64.encode(bytes)),
                SandboxOutput::Stderr(bytes) => OutputFrame::Stderr(BASE64.encode(bytes)),
            })
            .collect(),
        exit_code: result.exit_code,
    }))
}

async fn write_file(
    State(state): State<RouteState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<WriteFileBody>,
) -> std::result::Result<StatusCode, (StatusCode, String)> {
    authorize(&state, &headers).map_err(|code| (code, "unauthorized".to_string()))?;

    let contents = BASE64
        .decode(body.contents_base64)
        .map_err(|error| (StatusCode::BAD_REQUEST, format!("bad base64: {error}")))?;

    let container_id = container_for(&state, &session_id).await?;
    state
        .manager
        .write_file(&container_id, &body.path, &contents)
        .await
        .map_err(failed)?;

    Ok(StatusCode::NO_CONTENT)
}

async fn read_file(
    State(state): State<RouteState>,
    Path(session_id): Path<String>,
    Query(query): Query<ReadFileQuery>,
    headers: HeaderMap,
) -> std::result::Result<Json<ReadFileResponse>, (StatusCode, String)> {
    authorize(&state, &headers).map_err(|code| (code, "unauthorized".to_string()))?;

    let container_id = container_for(&state, &session_id).await?;
    let contents = state
        .manager
        .read_file(&container_id, &query.path)
        .await
        .map_err(failed)?;

    Ok(Json(ReadFileResponse {
        contents_base64: BASE64.encode(contents),
    }))
}

async fn preview(
    State(state): State<RouteState>,
    Path(session_id): Path<String>,
    Query(query): Query<PreviewQuery>,
    headers: HeaderMap,
) -> std::result::Result<Json<PreviewResponse>, (StatusCode, String)> {
    authorize(&state, &headers).map_err(|code| (code, "unauthorized".to_string()))?;

    let container_id = container_for(&state, &session_id).await?;
    let endpoint = state
        .manager
        .preview_address(&container_id, query.port)
        .await
        .map_err(failed)?;

    Ok(Json(PreviewResponse {
        endpoint,
        allowed_ports: vec![query.port],
    }))
}

async fn terminate(
    State(state): State<RouteState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> std::result::Result<StatusCode, (StatusCode, String)> {
    authorize(&state, &headers).map_err(|code| (code, "unauthorized".to_string()))?;

    state
        .manager
        .terminate(&state.sandbox, &session_id)
        .await
        .map_err(failed)?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two sandboxes in one deployment share a state directory. With a fixed file name whichever
    /// one started last owned the credential for both, and the other's binding then authenticated
    /// with a token that was no longer on disk.
    #[test]
    fn each_sandbox_owns_its_own_token_file() {
        assert_ne!(token_file_name("agents"), token_file_name("runners"));
        assert!(token_file_name("agents").contains("agents"));
    }

    /// A bearer token that is world-readable even briefly is readable by anything watching the
    /// state directory, so the mode has to be set at create time rather than after the write.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_token_file_is_never_readable_by_anyone_else() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("alien-token-{}", generate_token()));
        tokio::fs::create_dir_all(&dir).await.expect("temp dir");
        let path = dir.join("sandbox-manager-agents.token");

        write_token_file(&path, "secret").await.expect("writes");

        let mode = tokio::fs::metadata(&path)
            .await
            .expect("readable")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "mode was {:o}", mode & 0o777);
        assert_eq!(
            tokio::fs::read_to_string(&path).await.expect("contents"),
            "secret"
        );

        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[test]
    fn token_comparison_rejects_wrong_and_short_tokens() {
        let token = generate_token();

        assert!(token_matches(&token, &token));
        assert!(!token_matches(&token, "short"));
        assert!(!token_matches(&token, &"0".repeat(token.len())));
    }

    #[test]
    fn tokens_are_not_reused_between_routes() {
        assert_ne!(generate_token(), generate_token());
        assert!(generate_token().len() >= 64, "a guessable token is not a token");
    }
}
