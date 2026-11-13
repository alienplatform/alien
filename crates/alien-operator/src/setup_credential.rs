//! `setup-credential`: ask the manager for a short-lived key that a person with
//! cluster access can use to rerun an installation, without a saved setup token.
//!
//! The running Operator holds its encrypted state exclusively, so a second
//! process cannot read the Operator's key. Instead the Operator listens on a
//! Unix socket in a private directory of its data volume, and the command
//! (`kubectl exec`, same container, same user) connects to it. The Operator asks
//! the manager for a key that works for this deployment only, for a few
//! minutes. The command writes the key to stdout as one JSON line and nowhere
//! else: nothing is traced, and a failure writes a fixed code to stderr.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

use chrono::{DateTime, Utc};
use clap::Parser;
use reqwest::{redirect::Policy, StatusCode};
use serde::{Deserialize, Serialize};

use crate::db::OperatorDb;

/// The longest life the Operator accepts for the key it hands out. The manager
/// issues 15 minutes; the margin covers clock skew.
const MAX_LIFETIME: chrono::Duration = chrono::Duration::minutes(20);

#[derive(Parser, Debug)]
#[command(
    name = "operator setup-credential",
    about = "Print a short-lived key for rerunning this installation"
)]
struct SetupCredentialArgs {}

/// Where the running Operator listens. The directory is local to the container
/// (the chart mounts an emptyDir at /tmp), not the data volume, so no socket file
/// outlives its Operator on a volume that may be shared or snapshotted.
fn socket_path() -> PathBuf {
    Path::new("/tmp")
        .join("operator-setup-credential")
        .join("setup-credential.sock")
}

/// What the command prints on success. Not `Debug`: it holds a key.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Printed<'a> {
    credential: &'a str,
    expires_at: DateTime<Utc>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Issued {
    token: String,
    expires_at: DateTime<Utc>,
}

/// Why no key was printed. The code is the only thing written to stderr.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Failure {
    /// This Operator has no confirmed identity yet.
    NotEnrolled,
    /// The manager does not accept this Operator's key for the request.
    Refused,
    /// The manager is older than this command, or cannot issue the key.
    Unsupported,
    /// The manager or the local state could not be reached or read.
    Unavailable,
    /// The deployment is being removed; no rerun key is issued.
    Removing,
    /// No Operator is listening in this container.
    NotRunning,
    /// The socket is not the Operator's own: another owner, or open to others.
    UnsafeSocket,
}

impl Failure {
    pub(crate) fn from_code(code: &str) -> Self {
        match code {
            "not-enrolled" => Self::NotEnrolled,
            "refused" => Self::Refused,
            "unsupported" => Self::Unsupported,
            "removing" => Self::Removing,
            "not-running" => Self::NotRunning,
            "socket-unsafe" => Self::UnsafeSocket,
            _ => Self::Unavailable,
        }
    }

    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::NotEnrolled => "not-enrolled",
            Self::Refused => "refused",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::Removing => "removing",
            Self::NotRunning => "not-running",
            Self::UnsafeSocket => "socket-unsafe",
        }
    }
}

/// Entry point for `operator setup-credential [flags]`. Returns the exit code.
pub fn main(arguments: impl IntoIterator<Item = OsString>) -> i32 {
    let args = match SetupCredentialArgs::try_parse_from(
        std::iter::once(OsString::from("operator setup-credential")).chain(arguments),
    ) {
        Ok(args) => args,
        Err(error) => {
            let _ = error.print();
            return 2;
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return fail(Failure::Unavailable),
    };
    match runtime.block_on(run(args)) {
        Ok(line) => {
            println!("{line}");
            0
        }
        Err(failure) => fail(failure),
    }
}

fn fail(failure: Failure) -> i32 {
    eprintln!("setup-credential: {}", failure.code());
    1
}

/// The client side: connect to the running Operator and relay its answer.
async fn run(args: SetupCredentialArgs) -> Result<String, Failure> {
    #[cfg(unix)]
    {
        use tokio::io::AsyncReadExt;

        let path = socket_path();
        check_socket(&path, own_uid())?;
        let mut stream = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::net::UnixStream::connect(&path),
        )
        .await
        .map_err(|_| Failure::Unavailable)?
        .map_err(|_| Failure::NotRunning)?;
        let mut answer = Vec::new();
        tokio::time::timeout(
            Duration::from_secs(30),
            (&mut stream).take(16 * 1024).read_to_end(&mut answer),
        )
        .await
        .map_err(|_| Failure::Unavailable)?
        .map_err(|_| Failure::Unavailable)?;
        let answer: Answer = serde_json::from_slice(&answer).map_err(|_| Failure::Unavailable)?;
        match (answer.credential, answer.error) {
            (Some(credential), None) => print(Issued {
                token: credential,
                expires_at: answer.expires_at.ok_or(Failure::Unavailable)?,
            }),
            (None, Some(code)) => Err(Failure::from_code(&code)),
            _ => Err(Failure::Unavailable),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = args;
        Err(Failure::Unsupported)
    }
}

/// The user id this process runs as.
#[cfg(unix)]
fn own_uid() -> u32 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self")
        .map(|metadata| metadata.uid())
        .unwrap_or(u32::MAX)
}

/// Refuse a socket that is not this user's own, or that others can open. The
/// directory is checked too: it is where another user could swap the socket.
#[cfg(unix)]
fn check_socket(path: &Path, uid: u32) -> Result<(), Failure> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    let socket = std::fs::symlink_metadata(path).map_err(|_| Failure::NotRunning)?;
    let directory = path
        .parent()
        .and_then(|dir| std::fs::symlink_metadata(dir).ok())
        .ok_or(Failure::NotRunning)?;
    let own = socket.file_type().is_socket()
        && socket.uid() == uid
        && socket.mode() & 0o177 == 0
        && directory.is_dir()
        && directory.uid() == uid
        && directory.mode() & 0o077 == 0;
    if own {
        Ok(())
    } else {
        Err(Failure::UnsafeSocket)
    }
}

/// What the Operator answers on the socket. Not `Debug`: it holds a key.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Answer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    credential: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expires_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Ask the manager for a key, using the Operator's stored daily identity.
pub(crate) async fn mint(db: &OperatorDb) -> Result<Issued, Failure> {
    let identity = db
        .read_enrolled_daily_identity()
        .await
        .map_err(|_| Failure::Unavailable)?
        .ok_or(Failure::NotEnrolled)?;
    let url = endpoint(&identity.manager_url, &identity.deployment_id)?;
    let client = reqwest::Client::builder()
        .redirect(Policy::none())
        .http1_only()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| Failure::Unavailable)?;

    // A rotation the Operator started may have installed its successor on the
    // manager before the Operator saved the answer, so the successor is tried
    // when the current key is refused.
    let mut keys = vec![identity.current.as_str()];
    keys.extend(identity.pending.as_deref());
    let mut outcome = Failure::Refused;
    for key in keys {
        match request(&client, &url, key).await {
            Ok(issued) => return Ok(issued),
            Err(failure @ Failure::Refused) => outcome = failure,
            Err(failure) => return Err(failure),
        }
    }
    Err(outcome)
}

/// The Operator side: answer each connection on the socket with one key.
#[cfg(unix)]
pub async fn serve(db: std::sync::Arc<OperatorDb>, cancel: tokio_util::sync::CancellationToken) {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    let path = socket_path();
    let Some(dir) = path.parent() else { return };
    // A private directory, so the socket is never reachable by another user
    // between its creation and the permission change.
    let prepared = (|| -> std::io::Result<tokio::net::UnixListener> {
        match std::fs::DirBuilder::new().mode(0o700).create(dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        let _ = std::fs::remove_file(&path);
        let listener = tokio::net::UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        Ok(listener)
    })();
    let listener = match prepared {
        Ok(listener) => listener,
        Err(error) => {
            tracing::warn!(%error, "The setup-credential socket could not be created");
            return;
        }
    };
    // One request at a time: each one revokes the previous key at the manager.
    let busy = std::sync::Arc::new(tokio::sync::Mutex::new(()));
    loop {
        let stream = tokio::select! {
            _ = cancel.cancelled() => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => stream,
                Err(_) => continue,
            },
        };
        let db = db.clone();
        let busy = busy.clone();
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            let mut stream = stream;
            let _guard = busy.lock().await;
            // Read at most nothing from the client: the request carries no input.
            let answer = match mint(&db).await {
                Ok(issued) => {
                    tracing::info!("Issued a short-lived setup key");
                    Answer {
                        credential: Some(issued.token),
                        expires_at: Some(issued.expires_at),
                        error: None,
                    }
                }
                Err(failure) => Answer {
                    credential: None,
                    expires_at: None,
                    error: Some(failure.code().to_string()),
                },
            };
            if let Ok(bytes) = serde_json::to_vec(&answer) {
                let _ = stream.write_all(&bytes).await;
            }
            let _ = stream.shutdown().await;
        });
    }
    let _ = std::fs::remove_file(&path);
}

fn endpoint(manager_url: &str, deployment_id: &str) -> Result<reqwest::Url, Failure> {
    let mut url = reqwest::Url::parse(manager_url).map_err(|_| Failure::Unavailable)?;
    let secure = url.scheme() == "https"
        || (url.scheme() == "http"
            && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")));
    if !secure || !url.username().is_empty() || url.password().is_some() || url.query().is_some() {
        return Err(Failure::Unavailable);
    }
    url.path_segments_mut()
        .map_err(|_| Failure::Unavailable)?
        .pop_if_empty()
        .extend(["v1", "deployments", deployment_id, "setup-credential"]);
    Ok(url)
}

async fn request(
    client: &reqwest::Client,
    url: &reqwest::Url,
    key: &str,
) -> Result<Issued, Failure> {
    let response = client
        .post(url.clone())
        .bearer_auth(key)
        .send()
        .await
        .map_err(|_| Failure::Unavailable)?;
    match response.status() {
        status if status.is_success() => {}
        StatusCode::CONFLICT => {
            let body = response.bytes().await.map_err(|_| Failure::Unavailable)?;
            let removing = body.len() <= 16 * 1024
                && serde_json::from_slice::<serde_json::Value>(&body)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("code")
                            .and_then(|code| code.as_str().map(String::from))
                    })
                    .is_some_and(|code| code == "DEPLOYMENT_REMOVING");
            return Err(if removing {
                Failure::Removing
            } else {
                Failure::Refused
            });
        }
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => return Err(Failure::Refused),
        StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED => return Err(Failure::Unsupported),
        _ => return Err(Failure::Unavailable),
    }
    let bytes = response.bytes().await.map_err(|_| Failure::Unavailable)?;
    if bytes.len() > 16 * 1024 {
        return Err(Failure::Unavailable);
    }
    serde_json::from_slice(&bytes).map_err(|_| Failure::Unavailable)
}

fn print(issued: Issued) -> Result<String, Failure> {
    let now = Utc::now();
    if issued.token.is_empty() || issued.expires_at <= now || issued.expires_at > now + MAX_LIFETIME
    {
        return Err(Failure::Unavailable);
    }
    serde_json::to_string(&Printed {
        credential: &issued.token,
        expires_at: issued.expires_at,
    })
    .map_err(|_| Failure::Unavailable)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    fn bound(
        dir_mode: u32,
        socket_mode: u32,
    ) -> (tempfile::TempDir, PathBuf, std::os::unix::net::UnixListener) {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("s");
        std::fs::DirBuilder::new()
            .mode(dir_mode)
            .create(&dir)
            .unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(dir_mode)).unwrap();
        let path = dir.join("setup-credential.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(socket_mode)).unwrap();
        (root, path, listener)
    }

    #[test]
    fn an_own_private_socket_is_accepted() {
        let (_root, path, _listener) = bound(0o700, 0o600);
        assert_eq!(check_socket(&path, own_uid()), Ok(()));
    }

    #[test]
    fn a_socket_open_to_others_or_owned_by_another_user_is_refused() {
        let (_root, path, _listener) = bound(0o700, 0o666);
        assert_eq!(check_socket(&path, own_uid()), Err(Failure::UnsafeSocket));
        let (_root, path, _listener) = bound(0o755, 0o600);
        assert_eq!(check_socket(&path, own_uid()), Err(Failure::UnsafeSocket));
        let (_root, path, _listener) = bound(0o700, 0o600);
        assert_eq!(
            check_socket(&path, own_uid().wrapping_add(1)),
            Err(Failure::UnsafeSocket)
        );
    }

    #[test]
    fn a_missing_socket_means_no_running_operator() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            check_socket(&root.path().join("absent").join("x.sock"), own_uid()),
            Err(Failure::NotRunning)
        );
    }
}
