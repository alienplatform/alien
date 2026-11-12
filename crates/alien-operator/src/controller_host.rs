//! The host of one extension's controller in its own pod on Kubernetes, started as
//! `alien-operator extension-controller`.
//!
//! The Operator creates the pod, sends it the controller binary its release pins, and steps it
//! over HTTP. The host runs the binary as its child and speaks the step protocol to it over
//! stdin and stdout, as the Operator does where it runs a controller beside itself. The pod
//! holds nothing of the Operator's: it has no Kubernetes token, and its only cloud identity is
//! the extension's own, which the host resolves for each step and sends to the controller in
//! the step.
//!
//! - `GET /v1/controller`: 200 once the binary is loaded, 404 before.
//! - `PUT /v1/controller`: takes the binary. Refused unless it has the sha256 the pod was
//!   created with, so it does not matter who sends it.
//! - `POST /v1/step`: one step. Refused unless its token was signed by the Operator that
//!   created the pod, for this pod, its deployment and the request's body.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use alien_client_config::ClientConfigExt as _;
use alien_core::{AwsCredentials, AzureCredentials, ClientConfig, Platform};
use alien_infra::extension_bundle::MAX_OBJECT_BYTES;
use alien_infra::step_protocol::protocol::{
    codes, Credentials, ResponseBody, StepError, StepRequest, StepResponse, PROTOCOL_VERSION,
};
use alien_infra::step_protocol::token::{self, Expected};
use alien_infra::step_protocol::{
    ForwardedCredentials, StdioTransport, Transport as _, TransportFailure, CONNECT_TIMEOUT,
    HOST_GRACE,
};
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use clap::Parser;
use ed25519_compact::PublicKey;
use serde_json::{json, Value};

/// The first argument that starts the binary as a controller's host.
pub const COMMAND: &str = "extension-controller";

/// The `project` claim of a step token the Operator signs. An Operator runs one deployment and
/// knows no project; the host pins the deployment instead.
pub const NO_PROJECT: &str = "";

/// The variables of the pod's environment that say which identity the pod runs as. The host
/// reads only these, so no other credential in the environment, and no metadata service, can
/// stand in for the pod's own identity.
const AWS_POD_IDENTITY: [&str; 2] = ["AWS_ROLE_ARN", "AWS_WEB_IDENTITY_TOKEN_FILE"];
const AWS_ENV: [&str; 5] = [
    "AWS_REGION",
    "AWS_DEFAULT_REGION",
    "AWS_ACCOUNT_ID",
    "AWS_ROLE_SESSION_NAME",
    "AWS_SERVICE_OVERRIDES_ENDPOINTS",
];
const AZURE_POD_IDENTITY: [&str; 3] = [
    "AZURE_CLIENT_ID",
    "AZURE_TENANT_ID",
    "AZURE_FEDERATED_TOKEN_FILE",
];
const AZURE_ENV: [&str; 3] = [
    "AZURE_SUBSCRIPTION_ID",
    "AZURE_AUTHORITY_HOST",
    "AZURE_REGION",
];

/// What the Operator starts a controller's host with. None of it is secret.
#[derive(Parser, Debug, Clone)]
#[command(name = COMMAND, about = "Hosts one extension's controller in its own pod")]
pub struct HostArgs {
    /// The address to serve on.
    #[arg(long)]
    pub listen: SocketAddr,
    /// The extension whose controller this is.
    #[arg(long)]
    pub extension: String,
    /// The sha256 the release pins for the controller binary, as hex.
    #[arg(long)]
    pub sha256: String,
    /// The deployment whose steps the host takes.
    #[arg(long)]
    pub deployment: String,
    /// The URL the Operator reaches this host at, which a step token names as its audience.
    #[arg(long)]
    pub audience: String,
    /// The issuer a step token names: the Operator that created the pod.
    #[arg(long)]
    pub issuer: String,
    /// The Operator's step-token key, an Ed25519 public key as base64.
    #[arg(long = "trusted-key")]
    pub trusted_key: String,
    /// A folder the host can write, for the binary.
    #[arg(long, default_value = "/tmp")]
    pub dir: PathBuf,
}

struct Host {
    args: HostArgs,
    key: PublicKey,
    /// The pod's environment, read once at start.
    env: HashMap<String, String>,
    /// The controller process, once its binary arrived and passed its check.
    controller: tokio::sync::RwLock<Option<Arc<StdioTransport>>>,
}

/// Runs the host named by the process's arguments until it fails.
pub fn main() -> ! {
    let args = HostArgs::parse_from(std::env::args().skip(1));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    let served = runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind(args.listen)
            .await
            .map_err(|e| format!("cannot listen on {}: {e}", args.listen))?;
        serve(listener, args, std::env::vars().collect()).await
    });
    if let Err(message) = served {
        eprintln!("Error: {message}");
    }
    std::process::exit(1);
}

/// Serves a controller's host on `listener`, in a pod whose environment is `env`.
pub async fn serve(
    listener: tokio::net::TcpListener,
    args: HostArgs,
    env: HashMap<String, String>,
) -> std::result::Result<(), String> {
    let key = base64::engine::general_purpose::STANDARD
        .decode(&args.trusted_key)
        .map_err(|e| format!("the trusted key is not base64: {e}"))
        .and_then(|bytes| {
            PublicKey::from_slice(&bytes)
                .map_err(|e| format!("the trusted key is not an Ed25519 key: {e}"))
        })?;
    let host = Arc::new(Host {
        args,
        key,
        env,
        controller: tokio::sync::RwLock::new(None),
    });
    let app = Router::new()
        .route("/v1/controller", get(loaded).put(load))
        .route("/v1/step", post(step))
        .layer(DefaultBodyLimit::max(MAX_OBJECT_BYTES as usize))
        .with_state(host);
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("the server stopped: {e}"))
}

async fn loaded(State(host): State<Arc<Host>>) -> StatusCode {
    if host.controller.read().await.is_some() {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

/// Takes the controller binary. Its check is against the sha256 the pod was created with, and
/// the process runs a private copy of the bytes that passed.
async fn load(State(host): State<Arc<Host>>, bytes: Bytes) -> Response {
    let mut controller = host.controller.write().await;
    if controller.is_some() {
        return StatusCode::NO_CONTENT.into_response();
    }
    let args = &host.args;
    let path = args.dir.join(format!("{}-controller", args.extension));
    if let Err(error) = tokio::fs::write(&path, &bytes).await {
        return unavailable(format!("cannot store the binary: {error}"));
    }
    let verified =
        StdioTransport::verified(&args.extension, &path, &args.sha256, &args.dir, Vec::new()).await;
    if let Err(error) = tokio::fs::remove_file(&path).await {
        tracing::warn!(path = %path.display(), %error, "Could not remove the received binary");
    }
    match verified {
        Ok(transport) => {
            *controller = Some(Arc::new(transport));
            StatusCode::NO_CONTENT.into_response()
        }
        Err(error) => {
            let body = json!({ "error": { "code": error.code, "message": error.message } });
            (StatusCode::BAD_REQUEST, Json(body)).into_response()
        }
    }
}

/// 503, which the Operator takes as a controller it can't reach yet and tries again.
fn unavailable(message: String) -> Response {
    let body = json!({ "error": { "code": codes::UNREACHABLE, "message": message } });
    (StatusCode::SERVICE_UNAVAILABLE, Json(body)).into_response()
}

fn refused(step_id: &str, code: &str, message: String) -> Response {
    let reply = StepResponse {
        protocol: PROTOCOL_VERSION,
        step_id: step_id.to_string(),
        body: ResponseBody::Refused {
            error: StepError {
                code: code.to_string(),
                message,
                retryable: false,
            },
            supported_protocols: Some(vec![PROTOCOL_VERSION]),
        },
    };
    (StatusCode::BAD_REQUEST, Json(reply)).into_response()
}

async fn step(State(host): State<Arc<Host>>, headers: HeaderMap, raw: Bytes) -> Response {
    // The step's time counts from here: resolving the pod's identity uses it up.
    let received = chrono::Utc::now();
    let args = &host.args;
    let body: Value = serde_json::from_slice(&raw).unwrap_or_default();
    let step_id = body["stepId"].as_str().unwrap_or_default().to_string();
    if body["protocol"].as_u64() != Some(u64::from(PROTOCOL_VERSION)) {
        return refused(
            &step_id,
            codes::PROTOCOL_UNSUPPORTED,
            format!("this host speaks step protocol {PROTOCOL_VERSION}"),
        );
    }
    let mut request: StepRequest = match serde_json::from_value(body) {
        Ok(request) => request,
        Err(e) => {
            return refused(
                &step_id,
                codes::STEP_REFUSED,
                format!("cannot read the request: {e}"),
            )
        }
    };
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default();
    let checked = token::verify(
        token,
        &[host.key],
        Expected {
            issuer: &args.issuer,
            audience: &args.audience,
            projects: &[NO_PROJECT],
            deployment_id: &args.deployment,
            extension: &args.extension,
            resource_id: &request.resource_id,
            step_id: &request.step_id,
            body: &raw,
        },
        received.timestamp(),
    );
    let claims = match checked {
        // The token names the pod's deployment and extension; the request must too.
        Ok(claims)
            if request.deployment_id == args.deployment
                && token::extension_of(&request.resource_type) == args.extension =>
        {
            claims
        }
        Ok(_) => return unauthorized(token::TokenRefusal::Mismatch.to_string()),
        Err(refusal) => return unauthorized(refusal.to_string()),
    };
    let Some(controller) = host.controller.read().await.clone() else {
        return unavailable("the controller binary has not arrived yet".to_string());
    };
    request.credentials = match own_identity(request.platform, &host.env).await {
        Ok(credentials) => credentials,
        Err(Identity::Missing(message)) => return refused(&step_id, codes::STEP_REFUSED, message),
        Err(Identity::Unresolved(message)) => return unavailable(message),
    };
    // The Operator stamped the deadline and the token's `iat` on its own clock, so their
    // difference is the step's time whatever the skew between the two pods, less what the
    // request may have lost on the way: up to a second to `iat`'s whole seconds, and up to the
    // Operator's connect timeout. The Operator waits its grace past the deadline for the reply.
    let budget = chrono::DateTime::parse_from_rfc3339(&request.deadline)
        .ok()
        .zip(chrono::DateTime::from_timestamp(claims.iat, 0))
        .and_then(|(deadline, issued)| {
            (deadline.with_timezone(&chrono::Utc) - issued - chrono::Duration::seconds(1))
                .to_std()
                .ok()
        })
        .and_then(|budget| budget.checked_sub(CONNECT_TIMEOUT))
        .unwrap_or_default();
    // A task of its own: a request that goes away would otherwise stop the exchange midway,
    // and the controller process would be replaced with the step it was running.
    let run = tokio::spawn(async move { controller.step(&request, budget + HOST_GRACE).await });
    match run.await {
        Ok(Ok(reply)) => {
            let status = match reply.body {
                ResponseBody::Refused { .. } => StatusCode::BAD_REQUEST,
                _ => StatusCode::OK,
            };
            (status, Json(reply)).into_response()
        }
        Ok(Err(TransportFailure {
            message,
            retryable: true,
            ..
        })) => unavailable(message),
        Ok(Err(TransportFailure { code, message, .. })) => refused(&step_id, code, message),
        Err(error) => unavailable(format!("the step did not finish: {error}")),
    }
}

fn unauthorized(reason: String) -> Response {
    let body = json!({ "error": { "code": codes::AUTH_REJECTED, "message": reason } });
    (StatusCode::UNAUTHORIZED, Json(body)).into_response()
}

/// Why a step on a cloud got no credentials.
enum Identity {
    /// The pod runs as no identity of its own. Another try finds the same pod.
    Missing(String),
    /// The pod's identity could not be turned into credentials now.
    Unresolved(String),
}

/// The credentials of the pod's own identity for a step on `platform`, or `None` on a platform
/// with no cloud identity. Only the identity the pod's service account is bound to counts: a
/// pod without one is refused, never given whatever else its node could answer with.
async fn own_identity(
    platform: Platform,
    env: &HashMap<String, String>,
) -> std::result::Result<Option<Credentials>, Identity> {
    let (required, optional): (&[&str], &[&str]) = match platform {
        Platform::Aws => (&AWS_POD_IDENTITY, &AWS_ENV),
        Platform::Azure => (&AZURE_POD_IDENTITY, &AZURE_ENV),
        Platform::Gcp => {
            return Err(Identity::Missing(
                "a controller's pod has no identity it can use on GKE, which serves a pod's \
                 identity from the metadata address the pod may not reach"
                    .to_string(),
            ))
        }
        _ => return Ok(None),
    };
    if let Some(missing) = required.iter().find(|name| !env.contains_key(**name)) {
        return Err(Identity::Missing(format!(
            "the controller's pod has no identity of its own: {missing} is not set, so its \
             service account is not bound to the extension's identity"
        )));
    }
    let own: HashMap<String, String> = required
        .iter()
        .chain(optional)
        .filter_map(|name| Some((name.to_string(), env.get(*name)?.clone())))
        .collect();
    let config = ClientConfig::from_env(platform, &own)
        .await
        .map_err(|error| Identity::Unresolved(error.to_string()))?;
    let bound = match &config {
        ClientConfig::Aws(aws) => matches!(aws.credentials, AwsCredentials::WebIdentity { .. }),
        ClientConfig::Azure(azure) => {
            matches!(azure.credentials, AzureCredentials::WorkloadIdentity { .. })
        }
        _ => false,
    };
    if !bound {
        return Err(Identity::Missing(
            "the controller's pod resolved credentials that are not its service account's"
                .to_string(),
        ));
    }
    ForwardedCredentials::new(config)
        .resolve()
        .await
        .map(Some)
        .map_err(|error| Identity::Unresolved(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    fn missing(result: std::result::Result<Option<Credentials>, Identity>) -> String {
        match result {
            Err(Identity::Missing(message)) => message,
            Err(Identity::Unresolved(message)) => panic!("tried to resolve: {message}"),
            Ok(credentials) => panic!("got {credentials:?}"),
        }
    }

    /// Credentials that are not the pod's own never stand in for its identity: with no
    /// service-account binding in the environment the step is refused, whatever else is there,
    /// and nothing is asked of a metadata service.
    #[tokio::test]
    async fn only_the_pod_s_service_account_identity_counts() {
        let keys = env(&[
            ("AWS_ACCESS_KEY_ID", "AKIAOTHER"),
            ("AWS_SECRET_ACCESS_KEY", "secret"),
            ("AWS_REGION", "us-east-2"),
        ]);
        assert!(missing(own_identity(Platform::Aws, &keys).await).contains("AWS_ROLE_ARN"));

        let secret = env(&[
            ("AZURE_CLIENT_ID", "client"),
            ("AZURE_TENANT_ID", "tenant"),
            ("AZURE_CLIENT_SECRET", "secret"),
            ("AZURE_SUBSCRIPTION_ID", "subscription"),
        ]);
        assert!(missing(own_identity(Platform::Azure, &secret).await)
            .contains("AZURE_FEDERATED_TOKEN_FILE"));

        let adc = env(&[("GOOGLE_APPLICATION_CREDENTIALS", "/var/run/secrets/token")]);
        assert!(missing(own_identity(Platform::Gcp, &adc).await).contains("metadata address"));
    }

    /// A step on a platform with no cloud identity carries no credentials.
    #[tokio::test]
    async fn a_step_off_the_clouds_gets_no_credentials() {
        for platform in [Platform::Local, Platform::Kubernetes] {
            assert!(matches!(own_identity(platform, &env(&[])).await, Ok(None)));
        }
    }
}
