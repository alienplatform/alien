//! `setup-credential`: ask the manager for a short-lived key that a person with
//! cluster access can use to rerun an installation, without a saved setup token.
//!
//! The command runs inside the Operator's container (`kubectl exec`), with the
//! container's environment and data volume. It reads the Operator's stored daily
//! identity and asks the manager for a key that works for this deployment only,
//! for a few minutes. The key is written to stdout as one JSON line and nowhere
//! else: nothing is traced, and a failure writes a fixed code to stderr.

use std::{ffi::OsString, path::PathBuf, time::Duration};

use chrono::{DateTime, Utc};
use clap::Parser;
use reqwest::{redirect::Policy, StatusCode};
use serde::{Deserialize, Serialize};

use crate::{cli::load_encryption_key, db::OperatorDb};

/// The longest life the Operator accepts for the key it hands out. The manager
/// issues 15 minutes; the margin covers clock skew.
const MAX_LIFETIME: chrono::Duration = chrono::Duration::minutes(20);

#[derive(Parser, Debug)]
#[command(name = "operator setup-credential", about = "Print a short-lived key for rerunning this installation")]
struct SetupCredentialArgs {
    #[arg(long, env = "DATA_DIR")]
    data_dir: Option<String>,

    #[arg(long, env = "OPERATOR_ENCRYPTION_KEY_FILE")]
    encryption_key_file: Option<PathBuf>,
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
struct Issued {
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
}

impl Failure {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::NotEnrolled => "not-enrolled",
            Self::Refused => "refused",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
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

async fn run(args: SetupCredentialArgs) -> Result<String, Failure> {
    let data_dir = args
        .data_dir
        .filter(|dir| !dir.is_empty())
        .unwrap_or_else(|| "/var/lib/operator".to_string());
    let encryption_key = load_encryption_key(args.encryption_key_file.as_deref())
        .await
        .map_err(|_| Failure::Unavailable)?;
    let db = OperatorDb::new(&data_dir, &encryption_key)
        .await
        .map_err(|_| Failure::Unavailable)?;
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
            Ok(issued) => return print(issued),
            Err(failure @ Failure::Refused) => outcome = failure,
            Err(failure) => return Err(failure),
        }
    }
    Err(outcome)
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

async fn request(client: &reqwest::Client, url: &reqwest::Url, key: &str) -> Result<Issued, Failure> {
    let response = client
        .post(url.clone())
        .bearer_auth(key)
        .send()
        .await
        .map_err(|_| Failure::Unavailable)?;
    match response.status() {
        status if status.is_success() => {}
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::CONFLICT => {
            return Err(Failure::Refused)
        }
        StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED => {
            return Err(Failure::Unsupported)
        }
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
