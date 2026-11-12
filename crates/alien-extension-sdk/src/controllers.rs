//! The controller binary's side of the step protocol: `run_controllers` and what a handler adds
//! to Alien's `ResourceControllerContext`.

use crate::{AlienResource, ResourceTypeOf};
use alien_core::{AwsCredentials, Platform};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_infra::step_protocol::host::{step_client_config, ControllerHost};
use alien_infra::step_protocol::protocol::{codes, StepRequest, PROTOCOL_VERSION};
use alien_infra::step_protocol::token::{self, Expected, TokenRefusal};
use alien_infra::{
    DefaultPlatformServiceProvider, ErrorData, ResourceController, ResourceControllerContext,
    Result,
};
use base64::Engine as _;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::future::Future;
use std::process::ExitCode;
use std::sync::Arc;

/// The controllers a binary runs, which `run_controllers` passes to its closure.
pub struct Controllers {
    host: ControllerHost,
    problems: Vec<String>,
}

impl Controllers {
    /// Runs `controller` for resources of `resource_type` on `platform`. A new resource starts
    /// from `controller` as it is.
    pub fn register<R, C>(
        &mut self,
        resource_type: ResourceTypeOf<R>,
        platform: Platform,
        controller: C,
    ) where
        R: AlienResource,
        C: ResourceController + DeserializeOwned + 'static,
    {
        if let Err(problem) =
            self.host
                .register::<R, C>(resource_type.resource_type(), platform, controller)
        {
            self.problems.push(problem);
        }
    }
}

/// Builds the controllers `register` adds and answers steps for them, as the arguments say:
///
/// - none: one message per line on stdin, one reply per line on stdout, as the Operator, the
///   setup CLI and `alien dev` start it;
/// - `--http <address> --audience <controller-url> --issuer <manager-url> --project <id>`:
///   serves `POST /v1/step` for a push deployment, refusing a request whose token the manager at
///   `manager-url` didn't sign for `controller-url`, for a deployment of project `id` (repeat
///   `--project` to serve several), and for that request's body. The manager's keys come from the JWKS it
///   publishes, fetched again, at most once a minute, when a token names a key the server
///   hasn't seen; until then, and while they can't be fetched, a step is answered 503, which
///   the manager retries. Repeating
///   `--trusted-key <base64>` pins the keys instead. TLS stays with whatever terminates it in
///   front;
/// - `--controllers`: prints the type and platforms of each controller, as JSON, for
///   `alien extensions build`.
pub fn run_controllers(register: impl FnOnce(&mut Controllers)) -> ExitCode {
    let mut controllers = Controllers {
        host: ControllerHost::new(
            step_client_config,
            Arc::new(DefaultPlatformServiceProvider::default()),
            None,
        ),
        problems: Vec::new(),
    };
    register(&mut controllers);
    if !controllers.problems.is_empty() {
        eprintln!("{}", controllers.problems.join("\n"));
        return ExitCode::FAILURE;
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    match serve(controllers.host, &args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn serve(host: ControllerHost, args: &[String]) -> std::result::Result<(), String> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("the runtime did not start: {e}"))?;
    match args.first().map(String::as_str) {
        None => host
            .serve_stdio(&runtime)
            .map_err(|e| format!("stdin or stdout failed: {e}")),
        Some("--controllers") => {
            let mut listed: BTreeMap<&str, Vec<Platform>> = BTreeMap::new();
            for (resource_type, platform) in host.registrations() {
                listed.entry(resource_type).or_default().push(platform);
            }
            for platforms in listed.values_mut() {
                platforms.sort_by_key(|platform| platform.to_string());
            }
            println!("{}", json!(listed));
            Ok(())
        }
        Some("--http") => {
            let server = HttpServer::from_args(&args[1..])?;
            runtime.block_on(server.serve(host))
        }
        Some(other) => Err(format!(
            "unknown argument `{other}`: run with no arguments for stdin and stdout, \
             `--http <address> --audience <url> --issuer <manager-url>` to serve HTTP, \
             or `--controllers` to list the controllers"
        )),
    }
}

/// Where the JWKS of a manager's step-token keys is, under its URL.
const STEP_TOKEN_JWKS_PATH: &str = "/.well-known/alien-step-jwks.json";

/// How often a server fetches its manager's JWKS again when a token names a key it doesn't have.
const JWKS_REFETCH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

/// How often a server with no keys yet tries its manager's JWKS: often enough to recover soon
/// after the manager does, but not once per request, which anyone can send.
const JWKS_RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

/// The settings of `--http`.
struct HttpServer {
    address: String,
    audience: String,
    issuer: String,
    /// The projects whose deployments the server takes steps for.
    projects: Vec<String>,
    keys: TrustedKeys,
}

/// The keys a server checks step tokens against.
enum TrustedKeys {
    /// The keys `--trusted-key` pins.
    Pinned(Vec<ed25519_compact::PublicKey>),
    /// The manager's JWKS.
    Published {
        jwks_url: String,
        client: reqwest::Client,
        fetched: tokio::sync::Mutex<Fetched>,
    },
}

/// The manager's keys as last fetched.
#[derive(Default)]
struct Fetched {
    keys: Vec<ed25519_compact::PublicKey>,
    /// Keys an earlier fetch listed and a later one didn't, each kept until a token it signed
    /// can no longer be valid.
    retired: Vec<(ed25519_compact::PublicKey, std::time::Instant)>,
    /// When the last fetch was tried.
    tried: Option<std::time::Instant>,
    /// Why the last fetch failed, until one works.
    failure: Option<String>,
}

impl TrustedKeys {
    /// The keys to check a token against. With no keys yet, a call fetches the JWKS at most once
    /// per [`JWKS_RETRY_INTERVAL`]; with keys, `refresh` fetches it again, at most once per
    /// [`JWKS_REFETCH_INTERVAL`], for a token the keys at hand didn't verify. Errs while the keys
    /// a check needs can't be fetched, or a refresh isn't due yet, so the step is answered as
    /// unavailable and retried, never refused for good.
    async fn get(
        &self,
        refresh: bool,
    ) -> std::result::Result<Vec<ed25519_compact::PublicKey>, String> {
        let (jwks_url, client, fetched) = match self {
            TrustedKeys::Pinned(keys) => return Ok(keys.clone()),
            TrustedKeys::Published {
                jwks_url,
                client,
                fetched,
            } => (jwks_url, client, fetched),
        };
        let mut fetched = fetched.lock().await;
        let now = std::time::Instant::now();
        fetched.retired.retain(|(_, until)| *until > now);
        if fetched.keys.is_empty() || refresh {
            let interval = if fetched.keys.is_empty() {
                JWKS_RETRY_INTERVAL
            } else {
                JWKS_REFETCH_INTERVAL
            };
            if fetched.tried.is_some_and(|at| at.elapsed() < interval) {
                return Err(fetched.failure.clone().unwrap_or_else(|| {
                    format!("the manager's keys were fetched less than {interval:?} ago")
                }));
            }
            // Stamped before the fetch, so a request that goes away mid-fetch still spaces the
            // next, and again when it ends, so the requests that waited on a slow one don't
            // each fetch again.
            fetched.tried = Some(now);
            let result = fetch_jwks(client, jwks_url).await;
            fetched.tried = Some(std::time::Instant::now());
            match result {
                Ok(keys) => {
                    let until = std::time::Instant::now() + token::TOKEN_LIFETIME;
                    let dropped: Vec<_> = fetched
                        .keys
                        .iter()
                        .filter(|key| !keys.contains(key))
                        .map(|key| (*key, until))
                        .collect();
                    fetched.retired.retain(|(key, _)| !keys.contains(key));
                    fetched.retired.extend(dropped);
                    fetched.keys = keys;
                    fetched.failure = None;
                }
                Err(reason) => {
                    let failure =
                        format!("cannot fetch the manager's keys from {jwks_url}: {reason}");
                    fetched.failure = Some(failure.clone());
                    return Err(failure);
                }
            }
        }
        Ok(fetched
            .keys
            .iter()
            .chain(fetched.retired.iter().map(|(key, _)| key))
            .copied()
            .collect())
    }
}

/// Why a step's token wasn't accepted.
#[derive(Debug)]
enum CheckFailure {
    /// The token failed a check.
    Refused(String),
    /// The keys to check it against can't be fetched now.
    KeysUnavailable(String),
}

async fn fetch_jwks(
    client: &reqwest::Client,
    url: &str,
) -> std::result::Result<Vec<ed25519_compact::PublicKey>, String> {
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let jwks: Value = response.json().await.map_err(|e| e.to_string())?;
    let keys = token::keys_from_jwks(&jwks);
    if keys.is_empty() {
        return Err("the JWKS lists no Ed25519 key".to_string());
    }
    Ok(keys)
}

impl HttpServer {
    fn from_args(args: &[String]) -> std::result::Result<Self, String> {
        let address = args.first().cloned().ok_or("`--http` needs an address")?;
        let (mut audience, mut issuer, mut projects, mut pinned) =
            (None, None, Vec::new(), Vec::new());
        let mut rest = args[1..].iter();
        while let Some(flag) = rest.next() {
            let value = rest
                .next()
                .ok_or_else(|| format!("`{flag}` needs a value"))?;
            match flag.as_str() {
                "--audience" => audience = Some(value.clone()),
                "--issuer" => issuer = Some(value.trim_end_matches('/').to_string()),
                "--project" => projects.push(value.clone()),
                "--trusted-key" => {
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(value)
                        .map_err(|e| format!("a trusted key is not base64: {e}"))?;
                    pinned.push(
                        ed25519_compact::PublicKey::from_slice(&bytes)
                            .map_err(|e| format!("a trusted key is not an Ed25519 key: {e}"))?,
                    );
                }
                other => return Err(format!("unknown argument `{other}`")),
            }
        }
        let issuer: String =
            issuer.ok_or("`--http` needs `--issuer`, the URL of the Alien manager")?;
        if projects.is_empty() {
            return Err(
                "`--http` needs `--project`, the id of a project whose deployments use the controller"
                    .to_string(),
            );
        }
        // Without pinned keys the server fetches the issuer's JWKS, which must come over TLS; a
        // test serves one on loopback. With pinned keys the issuer is only compared with the
        // token's `iss`, so a manager on a plain-HTTP URL, as `alien serve` runs locally, works.
        let local = cfg!(test)
            && (issuer.starts_with("http://127.0.0.1:") || issuer.starts_with("http://localhost:"));
        if pinned.is_empty() && !issuer.starts_with("https://") && !local {
            return Err(format!(
                "`--issuer` must be an https URL to fetch its keys from, not `{issuer}`; pass `--trusted-key` to pin them instead"
            ));
        }
        let keys = if pinned.is_empty() {
            TrustedKeys::Published {
                jwks_url: format!("{issuer}{STEP_TOKEN_JWKS_PATH}"),
                client: reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(10))
                    .build()
                    .map_err(|e| format!("cannot build the HTTP client: {e}"))?,
                fetched: tokio::sync::Mutex::new(Fetched::default()),
            }
        } else {
            TrustedKeys::Pinned(pinned)
        };
        Ok(Self {
            address,
            audience: audience.ok_or("`--http` needs `--audience`, the controller URL")?,
            issuer,
            projects,
            keys,
        })
    }

    async fn serve(self, host: ControllerHost) -> std::result::Result<(), String> {
        let listener = tokio::net::TcpListener::bind(&self.address)
            .await
            .map_err(|e| format!("cannot listen on {}: {e}", self.address))?;
        let state = Arc::new((self, host));
        let app = axum::Router::new()
            .route("/v1/step", axum::routing::post(step))
            .with_state(state);
        axum::serve(listener, app)
            .await
            .map_err(|e| format!("the server stopped: {e}"))
    }

    /// Checks `token` for `request`, against the manager's current keys and, when they don't
    /// verify it, against its keys fetched again.
    async fn check(
        &self,
        token: &str,
        request: &StepRequest,
        body: &[u8],
    ) -> std::result::Result<token::Claims, CheckFailure> {
        let extension = token::extension_of(&request.resource_type);
        let projects: Vec<&str> = self.projects.iter().map(String::as_str).collect();
        let expected = Expected {
            issuer: &self.issuer,
            audience: &self.audience,
            projects: &projects,
            deployment_id: &request.deployment_id,
            extension,
            resource_id: &request.resource_id,
            step_id: &request.step_id,
            body,
        };
        token::addressed_to(token, &self.issuer, &self.audience)
            .map_err(|refusal| CheckFailure::Refused(refusal.to_string()))?;
        let now = || chrono::Utc::now().timestamp();
        let keys = |refresh| async move {
            self.keys
                .get(refresh)
                .await
                .map_err(CheckFailure::KeysUnavailable)
        };
        let checked = match token::verify(token, &keys(false).await?, expected, now()) {
            Err(TokenRefusal::Signature) => {
                token::verify(token, &keys(true).await?, expected, now())
            }
            checked => checked,
        };
        checked.map_err(|refusal| CheckFailure::Refused(refusal.to_string()))
    }
}

/// `POST /v1/step`: 400 with a `refused` reply for a protocol this side doesn't speak or a body
/// it can't read, 401 for a token that fails a check, 503 while the keys to check it with can't be
/// fetched yet, 200 with the step's reply otherwise.
async fn step(
    axum::extract::State(state): axum::extract::State<Arc<(HttpServer, ControllerHost)>>,
    headers: axum::http::HeaderMap,
    raw: axum::body::Bytes,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::Json;

    // The step's time counts from here: fetching keys and checking the token use it up.
    let received = chrono::Utc::now();
    let (server, host) = state.as_ref();
    let body: Value = serde_json::from_slice(&raw).unwrap_or_default();
    let step_id = body["stepId"].as_str().unwrap_or_default().to_string();
    let refused = |code: &str, message: String| {
        let reply = json!({
            "protocol": PROTOCOL_VERSION, "stepId": step_id, "result": "refused",
            "error": { "code": code, "message": message, "retryable": false },
            "supportedProtocols": [PROTOCOL_VERSION]
        });
        (StatusCode::BAD_REQUEST, Json(reply)).into_response()
    };
    if body["protocol"].as_u64() != Some(u64::from(PROTOCOL_VERSION)) {
        return refused(
            codes::PROTOCOL_UNSUPPORTED,
            format!("this controller speaks step protocol {PROTOCOL_VERSION}"),
        );
    }
    let request: StepRequest = match serde_json::from_value(body) {
        Ok(request) => request,
        Err(e) => return refused(codes::STEP_REFUSED, format!("cannot read the request: {e}")),
    };
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default();
    let claims = match server.check(token, &request, &raw).await {
        Ok(claims) => claims,
        Err(CheckFailure::Refused(reason)) => {
            let body = json!({ "error": { "code": codes::AUTH_REJECTED, "message": reason } });
            return (StatusCode::UNAUTHORIZED, Json(body)).into_response();
        }
        Err(CheckFailure::KeysUnavailable(reason)) => {
            // The caller is not authenticated yet, so the reason stays in this server's log.
            eprintln!("{reason}");
            let body = json!({ "error": {
                "code": codes::UNREACHABLE,
                "message": "the controller cannot check step tokens yet; try again"
            } });
            return (StatusCode::SERVICE_UNAVAILABLE, Json(body)).into_response();
        }
    };
    let request = deadline_on_this_clock(request, &claims, received);
    let reply = host.handle(request).await;
    let status = match reply.body {
        alien_infra::step_protocol::protocol::ResponseBody::Refused { .. } => {
            StatusCode::BAD_REQUEST
        }
        _ => StatusCode::OK,
    };
    (status, Json(reply)).into_response()
}

/// `request` with its deadline moved onto this machine's clock. The manager stamps the deadline
/// and the token's `iat` on its own clock, so their difference is the step's time whatever the
/// skew between the two machines; the step gets that long from `received`, when the request
/// arrived, less what the request may have lost before it arrived: up to a second to `iat`'s
/// whole seconds, and up to the manager's connect timeout to connecting. The manager waits its
/// grace past the deadline for the reply, so the controller's deadline falls by the manager's.
fn deadline_on_this_clock(
    mut request: StepRequest,
    claims: &token::Claims,
    received: chrono::DateTime<chrono::Utc>,
) -> StepRequest {
    let (Ok(deadline), Some(issued), Ok(connecting)) = (
        chrono::DateTime::parse_from_rfc3339(&request.deadline),
        chrono::DateTime::from_timestamp(claims.iat, 0),
        chrono::Duration::from_std(alien_infra::step_protocol::CONNECT_TIMEOUT),
    ) else {
        return request;
    };
    let lost = chrono::Duration::seconds(1) + connecting;
    let budget = (deadline.with_timezone(&chrono::Utc) - issued - lost).clamp(
        chrono::Duration::zero(),
        chrono::Duration::seconds(MAX_STEP_SECONDS),
    );
    request.deadline = (received + budget).to_rfc3339();
    request
}

/// The longest step a manager asks for, past which a deadline is taken as this long.
const MAX_STEP_SECONDS: i64 = 120;

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_compact::{KeyPair, Seed};
    use std::sync::Mutex;

    /// Serves the JWKS of whichever key `current` holds, counting the fetches.
    async fn jwks_server(
        current: Arc<Mutex<KeyPair>>,
        fetches: Arc<std::sync::atomic::AtomicUsize>,
    ) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new().route(
            STEP_TOKEN_JWKS_PATH,
            axum::routing::get(move || {
                let (current, fetches) = (current.clone(), fetches.clone());
                async move {
                    fetches.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let key = current.lock().unwrap().pk;
                    axum::Json(json!({ "keys": [token::jwk(&key)] }))
                }
            }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        url
    }

    /// Without pinned keys the server takes its manager's published keys, and fetches them again
    /// for a token they don't verify once the last fetch is a minute old, so a rotated key is
    /// trusted without a restart. The key the manager stopped publishing still checks the tokens
    /// it signed, for as long as one can be valid.
    #[tokio::test]
    async fn the_published_keys_are_fetched_and_fetched_again_after_a_rotation() {
        let (first, second) = (
            KeyPair::from_seed(Seed::new([1; 32])),
            KeyPair::from_seed(Seed::new([2; 32])),
        );
        let current = Arc::new(Mutex::new(first.clone()));
        let fetches = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let issuer = jwks_server(current.clone(), fetches.clone()).await;
        let server = HttpServer::from_args(&[
            "127.0.0.1:0".to_string(),
            "--audience".to_string(),
            "https://controllers.acme.dev".to_string(),
            "--issuer".to_string(),
            issuer,
            "--project".to_string(),
            "prj_1".to_string(),
        ])
        .unwrap();
        let fetched = |n| assert_eq!(fetches.load(std::sync::atomic::Ordering::SeqCst), n);

        assert_eq!(server.keys.get(false).await.unwrap(), vec![first.pk]);
        // A refresh within the minute fetches nothing and answers that the keys are unavailable.
        assert!(server.keys.get(true).await.is_err());
        fetched(1);

        *current.lock().unwrap() = second.clone();
        let TrustedKeys::Published { fetched: cache, .. } = &server.keys else {
            panic!("the keys are published")
        };
        cache.lock().await.tried = Some(std::time::Instant::now() - JWKS_REFETCH_INTERVAL);
        assert_eq!(server.keys.get(false).await.unwrap(), vec![first.pk]);
        assert_eq!(
            server.keys.get(true).await.unwrap(),
            vec![second.pk, first.pk]
        );
        fetched(2);
    }

    /// A token signed with a key the server hasn't fetched yet is checked against the manager's
    /// keys fetched again, so a rotation takes no restart.
    #[tokio::test]
    async fn a_token_from_a_rotated_key_is_checked_against_the_keys_fetched_again() {
        let (first, second) = (
            KeyPair::from_seed(Seed::new([1; 32])),
            KeyPair::from_seed(Seed::new([2; 32])),
        );
        let current = Arc::new(Mutex::new(first.clone()));
        let fetches = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let issuer = jwks_server(current.clone(), fetches.clone()).await;
        let server = HttpServer::from_args(&[
            "127.0.0.1:0".to_string(),
            "--audience".to_string(),
            "https://controllers.acme.dev".to_string(),
            "--issuer".to_string(),
            issuer.clone(),
            "--project".to_string(),
            "prj_1".to_string(),
        ])
        .unwrap();
        server.keys.get(false).await.unwrap();
        *current.lock().unwrap() = second.clone();
        let TrustedKeys::Published { fetched: cache, .. } = &server.keys else {
            panic!("the keys are published")
        };
        cache.lock().await.tried = Some(std::time::Instant::now() - JWKS_REFETCH_INTERVAL);
        let request: StepRequest = serde_json::from_value(json!({
            "protocol": 1, "stepId": "s1", "deploymentId": "dep_1", "resourceId": "zone",
            "resourceType": "acme/dns-zone", "platform": "test", "resourcePrefix": "p",
            "initialSetupAuthority": "importedHandoff", "lifecycle": "live",
            "resource": { "type": "acme/dns-zone", "id": "zone" }, "previousResource": null,
            "dependencies": {}, "deploymentConfig": {}, "setupRun": false, "setupRunId": null,
            "state": null, "transitions": [], "credentials": null,
            "deadline": "2099-01-01T00:00:00Z"
        }))
        .unwrap();
        let now = chrono::Utc::now().timestamp();
        let signed = token::mint(
            &token::Claims {
                iss: issuer,
                aud: "https://controllers.acme.dev".to_string(),
                project: "prj_1".to_string(),
                deployment_id: "dep_1".to_string(),
                extension: "acme".to_string(),
                resource_id: "zone".to_string(),
                step_id: "s1".to_string(),
                body_sha256: token::body_sha256(BODY),
                iat: now,
                exp: now + 60,
                jti: "j".to_string(),
            },
            &second.sk,
        )
        .unwrap();

        server.check(&signed, &request, BODY).await.unwrap();
        assert_eq!(fetches.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    /// A step's time is counted on the manager's clock, from the token's `iat` to the deadline,
    /// so a controller whose clock runs ahead or behind still gets all of it, less what the
    /// request may lose before it arrives.
    #[test]
    fn a_skewed_clock_still_gets_the_step_s_whole_time() {
        let request: StepRequest = serde_json::from_value(json!({
            "protocol": 1, "stepId": "s1", "deploymentId": "dep_1", "resourceId": "zone",
            "resourceType": "acme/dns-zone", "platform": "test", "resourcePrefix": "p",
            "initialSetupAuthority": "importedHandoff", "lifecycle": "live",
            "resource": { "type": "acme/dns-zone", "id": "zone" }, "previousResource": null,
            "dependencies": {}, "deploymentConfig": {}, "setupRun": false, "setupRunId": null,
            "state": null, "transitions": [], "credentials": null,
            // The manager's clock is ten minutes behind this one.
            "deadline": (chrono::Utc::now() - chrono::Duration::minutes(10)
                + chrono::Duration::seconds(60)).to_rfc3339()
        }))
        .unwrap();
        let claims = token::Claims {
            iss: "i".into(),
            aud: "a".into(),
            project: "p".into(),
            deployment_id: "dep_1".into(),
            extension: "acme".into(),
            resource_id: "zone".into(),
            step_id: "s1".into(),
            body_sha256: String::new(),
            iat: (chrono::Utc::now() - chrono::Duration::minutes(10)).timestamp(),
            exp: 0,
            jti: "j".into(),
        };
        let moved = deadline_on_this_clock(request, &claims, chrono::Utc::now());
        let left = chrono::DateTime::parse_from_rfc3339(&moved.deadline)
            .unwrap()
            .with_timezone(&chrono::Utc)
            - chrono::Utc::now();
        assert!(
            left > chrono::Duration::seconds(50) && left <= chrono::Duration::seconds(55),
            "{left}"
        );
    }

    /// The step's time counts from when its request arrived, so the time spent fetching keys
    /// and checking the token comes out of it.
    #[test]
    fn the_step_s_time_counts_from_its_arrival() {
        let now = chrono::Utc::now();
        let request: StepRequest = serde_json::from_value(json!({
            "protocol": 1, "stepId": "s1", "deploymentId": "dep_1", "resourceId": "zone",
            "resourceType": "acme/dns-zone", "platform": "test", "resourcePrefix": "p",
            "initialSetupAuthority": "importedHandoff", "lifecycle": "live",
            "resource": { "type": "acme/dns-zone", "id": "zone" }, "previousResource": null,
            "dependencies": {}, "deploymentConfig": {}, "setupRun": false, "setupRunId": null,
            "state": null, "transitions": [], "credentials": null,
            "deadline": (now + chrono::Duration::seconds(60)).to_rfc3339()
        }))
        .unwrap();
        let claims = token::Claims {
            iss: "i".into(),
            aud: "a".into(),
            project: "p".into(),
            deployment_id: "dep_1".into(),
            extension: "acme".into(),
            resource_id: "zone".into(),
            step_id: "s1".into(),
            body_sha256: String::new(),
            iat: now.timestamp(),
            exp: 0,
            jti: "j".into(),
        };
        // A key fetch took 10 s after the request arrived.
        let moved = deadline_on_this_clock(request, &claims, now - chrono::Duration::seconds(10));
        let left = chrono::DateTime::parse_from_rfc3339(&moved.deadline)
            .unwrap()
            .with_timezone(&chrono::Utc)
            - chrono::Utc::now();
        assert!(
            left > chrono::Duration::seconds(42) && left <= chrono::Duration::seconds(45),
            "{left}"
        );
    }

    /// A token for another issuer or audience is refused before the manager's keys are fetched,
    /// so anyone can't make the server fetch them.
    #[tokio::test]
    async fn a_token_for_another_issuer_is_refused_without_a_key_fetch() {
        let key = KeyPair::from_seed(Seed::new([1; 32]));
        let current = Arc::new(Mutex::new(key.clone()));
        let fetches = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let issuer = jwks_server(current, fetches.clone()).await;
        let server = HttpServer::from_args(&[
            "127.0.0.1:0".to_string(),
            "--audience".to_string(),
            "https://controllers.acme.dev".to_string(),
            "--issuer".to_string(),
            issuer,
            "--project".to_string(),
            "prj_1".to_string(),
        ])
        .unwrap();
        let request: StepRequest = serde_json::from_value(json!({
            "protocol": 1, "stepId": "s1", "deploymentId": "dep_1", "resourceId": "zone",
            "resourceType": "acme/dns-zone", "platform": "test", "resourcePrefix": "p",
            "initialSetupAuthority": "importedHandoff", "lifecycle": "live",
            "resource": { "type": "acme/dns-zone", "id": "zone" }, "previousResource": null,
            "dependencies": {}, "deploymentConfig": {}, "setupRun": false, "setupRunId": null,
            "state": null, "transitions": [], "credentials": null,
            "deadline": "2099-01-01T00:00:00Z"
        }))
        .unwrap();
        let now = chrono::Utc::now().timestamp();
        let forged = token::mint(
            &token::Claims {
                iss: "https://someone-else.example".to_string(),
                aud: "https://controllers.acme.dev".to_string(),
                project: "prj_1".to_string(),
                deployment_id: "dep_1".to_string(),
                extension: "acme".to_string(),
                resource_id: "zone".to_string(),
                step_id: "s1".to_string(),
                body_sha256: token::body_sha256(BODY),
                iat: now,
                exp: now + 60,
                jti: "j".to_string(),
            },
            &key.sk,
        )
        .unwrap();
        for token in [forged.as_str(), "not-a-token"] {
            assert!(matches!(
                server.check(token, &request, BODY).await,
                Err(CheckFailure::Refused(_))
            ));
        }
        assert_eq!(fetches.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    /// The body a test's token is signed for.
    const BODY: &[u8] = br#"{"stepId":"s1"}"#;

    /// A server takes steps only for the project it serves: a release of another project that
    /// names the server's URL gets a token the manager signed, for that project, which the server
    /// refuses.
    #[tokio::test]
    async fn a_step_for_another_project_is_refused() {
        let key = KeyPair::from_seed(Seed::new([1; 32]));
        let server = HttpServer::from_args(&[
            "127.0.0.1:0".to_string(),
            "--audience".to_string(),
            "https://controllers.acme.dev".to_string(),
            "--issuer".to_string(),
            "https://manager.example.com".to_string(),
            "--project".to_string(),
            "prj_vendor".to_string(),
            "--trusted-key".to_string(),
            base64::engine::general_purpose::STANDARD.encode(key.pk.as_ref()),
        ])
        .unwrap();
        let request: StepRequest = serde_json::from_value(json!({
            "protocol": 1, "stepId": "s1", "deploymentId": "dep_1", "resourceId": "zone",
            "resourceType": "acme/dns-zone", "platform": "test", "resourcePrefix": "p",
            "initialSetupAuthority": "importedHandoff", "lifecycle": "live",
            "resource": { "type": "acme/dns-zone", "id": "zone" }, "previousResource": null,
            "dependencies": {}, "deploymentConfig": {}, "setupRun": false, "setupRunId": null,
            "state": null, "transitions": [], "credentials": null,
            "deadline": "2099-01-01T00:00:00Z"
        }))
        .unwrap();
        let now = chrono::Utc::now().timestamp();
        let signed_for = |project: &str| {
            token::mint(
                &token::Claims {
                    iss: "https://manager.example.com".to_string(),
                    aud: "https://controllers.acme.dev".to_string(),
                    project: project.to_string(),
                    deployment_id: "dep_1".to_string(),
                    extension: "acme".to_string(),
                    resource_id: "zone".to_string(),
                    step_id: "s1".to_string(),
                    body_sha256: token::body_sha256(BODY),
                    iat: now,
                    exp: now + 60,
                    jti: "j".to_string(),
                },
                &key.sk,
            )
            .unwrap()
        };

        match server
            .check(&signed_for("prj_attacker"), &request, BODY)
            .await
        {
            Err(CheckFailure::Refused(reason)) => {
                assert!(
                    reason.contains("another project (prj_attacker)"),
                    "{reason}"
                )
            }
            other => panic!("refused, not {other:?}"),
        }
        server
            .check(&signed_for("prj_vendor"), &request, BODY)
            .await
            .unwrap();
    }

    /// While the manager's JWKS can't be fetched, a check answers that the keys are unavailable,
    /// which the manager retries, rather than refusing the token for good; with no keys yet each
    /// check fetches again, so the server recovers as soon as the manager does.
    #[tokio::test]
    async fn keys_that_cannot_be_fetched_make_a_step_unavailable_not_refused() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let key = KeyPair::from_seed(Seed::new([1; 32]));
        let (up, fetches) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicUsize::new(0)),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!("http://{}", listener.local_addr().unwrap());
        let (served_up, counted, pk) = (up.clone(), fetches.clone(), key.pk);
        let app = axum::Router::new().route(
            STEP_TOKEN_JWKS_PATH,
            axum::routing::get(move || {
                let (up, counted) = (served_up.clone(), counted.clone());
                async move {
                    use axum::response::IntoResponse;
                    counted.fetch_add(1, Ordering::SeqCst);
                    if up.load(Ordering::SeqCst) {
                        axum::Json(json!({ "keys": [token::jwk(&pk)] })).into_response()
                    } else {
                        axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response()
                    }
                }
            }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let server = HttpServer::from_args(&[
            "127.0.0.1:0".to_string(),
            "--audience".to_string(),
            "https://controllers.acme.dev".to_string(),
            "--issuer".to_string(),
            issuer,
            "--project".to_string(),
            "prj_1".to_string(),
        ])
        .unwrap();

        // Anyone can send a step, so with no keys the fetches are spaced, not one per request.
        for _ in 0..3 {
            assert!(server.keys.get(false).await.is_err());
        }
        assert_eq!(fetches.load(Ordering::SeqCst), 1);

        up.store(true, Ordering::SeqCst);
        let TrustedKeys::Published { fetched: cache, .. } = &server.keys else {
            panic!("the keys are published")
        };
        cache.lock().await.tried = Some(std::time::Instant::now() - JWKS_RETRY_INTERVAL);
        assert_eq!(server.keys.get(false).await.unwrap(), vec![key.pk]);
        assert_eq!(fetches.load(Ordering::SeqCst), 2);

        // With keys at hand, a refetch for an unknown key that fails is unavailable too, and
        // the keys at hand still check the tokens they sign.
        up.store(false, Ordering::SeqCst);
        cache.lock().await.tried = Some(std::time::Instant::now() - JWKS_REFETCH_INTERVAL);
        assert!(server.keys.get(true).await.is_err());
        assert_eq!(server.keys.get(false).await.unwrap(), vec![key.pk]);
    }

    /// The issuer's keys come over TLS, so a plain-HTTP issuer is refused, unless the keys are
    /// pinned and nothing is fetched from it.
    #[test]
    fn the_issuer_must_be_https() {
        let error = HttpServer::from_args(&[
            "127.0.0.1:0".to_string(),
            "--audience".to_string(),
            "https://controllers.acme.dev".to_string(),
            "--issuer".to_string(),
            "http://manager.example.com".to_string(),
            "--project".to_string(),
            "prj_1".to_string(),
        ])
        .err()
        .unwrap();
        assert!(error.contains("must be an https URL"), "{error}");

        HttpServer::from_args(&[
            "127.0.0.1:0".to_string(),
            "--audience".to_string(),
            "https://controllers.acme.dev".to_string(),
            "--issuer".to_string(),
            "http://manager.internal:7300".to_string(),
            "--project".to_string(),
            "prj_1".to_string(),
            "--trusted-key".to_string(),
            base64::engine::general_purpose::STANDARD
                .encode(KeyPair::from_seed(Seed::new([1; 32])).pk.as_ref()),
        ])
        .expect("pinned keys need no https issuer");
    }

    /// A server takes steps from one manager, and says so.
    #[test]
    fn http_needs_its_issuer() {
        let error = HttpServer::from_args(&[
            "127.0.0.1:0".to_string(),
            "--audience".to_string(),
            "https://controllers.acme.dev".to_string(),
        ])
        .err()
        .unwrap();
        assert!(error.contains("--issuer"), "{error}");

        let error = HttpServer::from_args(&[
            "127.0.0.1:0".to_string(),
            "--audience".to_string(),
            "https://controllers.acme.dev".to_string(),
            "--issuer".to_string(),
            "https://manager.example.com".to_string(),
        ])
        .err()
        .unwrap();
        assert!(error.contains("--project"), "{error}");
    }

    /// A server for `issuer` that fetches its published keys.
    fn server_for(issuer: String) -> HttpServer {
        HttpServer::from_args(&[
            "127.0.0.1:0".to_string(),
            "--audience".to_string(),
            "https://controllers.acme.dev".to_string(),
            "--issuer".to_string(),
            issuer,
            "--project".to_string(),
            "prj_1".to_string(),
        ])
        .unwrap()
    }

    fn step_request() -> StepRequest {
        serde_json::from_value(json!({
            "protocol": 1, "stepId": "s1", "deploymentId": "dep_1", "resourceId": "zone",
            "resourceType": "acme/dns-zone", "platform": "test", "resourcePrefix": "p",
            "initialSetupAuthority": "importedHandoff", "lifecycle": "live",
            "resource": { "type": "acme/dns-zone", "id": "zone" }, "previousResource": null,
            "dependencies": {}, "deploymentConfig": {}, "setupRun": false, "setupRunId": null,
            "state": null, "transitions": [], "credentials": null,
            "deadline": "2099-01-01T00:00:00Z"
        }))
        .unwrap()
    }

    /// A token for [`step_request`] from `issuer`, signed with `key`.
    fn signed_by(issuer: &str, key: &KeyPair) -> String {
        let now = chrono::Utc::now().timestamp();
        token::mint(
            &token::Claims {
                iss: issuer.to_string(),
                aud: "https://controllers.acme.dev".to_string(),
                project: "prj_1".to_string(),
                deployment_id: "dep_1".to_string(),
                extension: "acme".to_string(),
                resource_id: "zone".to_string(),
                step_id: "s1".to_string(),
                body_sha256: token::body_sha256(BODY),
                iat: now,
                exp: now + 60,
                jti: "j".to_string(),
            },
            &key.sk,
        )
        .unwrap()
    }

    /// Serves `jwks` as the issuer's JWKS, counting the fetches, each `delay` late.
    async fn jwks_document_server(
        jwks: Arc<Mutex<Value>>,
        fetches: Arc<std::sync::atomic::AtomicUsize>,
        delay: std::time::Duration,
    ) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new().route(
            STEP_TOKEN_JWKS_PATH,
            axum::routing::get(move || {
                let (jwks, fetches) = (jwks.clone(), fetches.clone());
                async move {
                    fetches.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    tokio::time::sleep(delay).await;
                    let document = jwks.lock().unwrap().clone();
                    axum::Json(document)
                }
            }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        url
    }

    /// A fetch whose request went away midway still spaces the next, so requests that give up on
    /// a slow JWKS don't each start another fetch.
    #[tokio::test]
    async fn a_fetch_cut_off_midway_still_spaces_the_next() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let key = KeyPair::from_seed(Seed::new([1; 32]));
        let fetches = Arc::new(AtomicUsize::new(0));
        let issuer = jwks_document_server(
            Arc::new(Mutex::new(json!({ "keys": [token::jwk(&key.pk)] }))),
            fetches.clone(),
            std::time::Duration::from_secs(5),
        )
        .await;
        let server = server_for(issuer);

        let cut_off = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            server.keys.get(false),
        )
        .await;
        assert!(cut_off.is_err(), "the fetch outlasts the request");
        assert!(server.keys.get(false).await.is_err());
        assert_eq!(fetches.load(Ordering::SeqCst), 1);
    }

    /// A token no key verifies makes the server fetch the JWKS again at most once a minute. A
    /// real token signed with a key published since then is answered as unavailable until the
    /// next fetch, which the manager retries, and never refused for good.
    #[tokio::test]
    async fn a_bad_signature_never_gets_a_real_token_refused() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let (first, second, other) = (
            KeyPair::from_seed(Seed::new([1; 32])),
            KeyPair::from_seed(Seed::new([2; 32])),
            KeyPair::from_seed(Seed::new([3; 32])),
        );
        let current = Arc::new(Mutex::new(first.clone()));
        let fetches = Arc::new(AtomicUsize::new(0));
        let issuer = jwks_server(current.clone(), fetches.clone()).await;
        let server = server_for(issuer.clone());
        server.keys.get(false).await.unwrap();
        let TrustedKeys::Published { fetched: cache, .. } = &server.keys else {
            panic!("the keys are published")
        };
        cache.lock().await.tried = Some(std::time::Instant::now() - JWKS_REFETCH_INTERVAL);
        let request = step_request();

        // Right issuer and audience, wrong key: it takes the minute's fetch.
        assert!(matches!(
            server
                .check(&signed_by(&issuer, &other), &request, BODY)
                .await,
            Err(CheckFailure::Refused(_))
        ));
        assert_eq!(fetches.load(Ordering::SeqCst), 2);

        *current.lock().unwrap() = second.clone();
        let real = signed_by(&issuer, &second);
        match server.check(&real, &request, BODY).await {
            Err(CheckFailure::KeysUnavailable(_)) => {}
            other => panic!("unavailable until the next fetch, not {other:?}"),
        }

        cache.lock().await.tried = Some(std::time::Instant::now() - JWKS_REFETCH_INTERVAL);
        server.check(&real, &request, BODY).await.unwrap();
    }

    /// A JWKS that lists no Ed25519 key is a failed fetch: the keys the server holds still check
    /// the tokens they sign.
    #[tokio::test]
    async fn a_jwks_without_an_ed25519_key_keeps_the_held_keys() {
        let key = KeyPair::from_seed(Seed::new([1; 32]));
        let jwks = Arc::new(Mutex::new(json!({ "keys": [token::jwk(&key.pk)] })));
        let issuer = jwks_document_server(
            jwks.clone(),
            Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            std::time::Duration::ZERO,
        )
        .await;
        let server = server_for(issuer);
        assert_eq!(server.keys.get(false).await.unwrap(), vec![key.pk]);

        *jwks.lock().unwrap() = json!({ "keys": [{ "kty": "RSA", "n": "x", "e": "AQAB" }] });
        let TrustedKeys::Published { fetched: cache, .. } = &server.keys else {
            panic!("the keys are published")
        };
        cache.lock().await.tried = Some(std::time::Instant::now() - JWKS_REFETCH_INTERVAL);
        assert!(server.keys.get(true).await.is_err());
        assert_eq!(server.keys.get(false).await.unwrap(), vec![key.pk]);
    }

    /// Alien stops waiting for a reply its grace after the deadline it stamped. The token's `iat`
    /// is in whole seconds and connecting takes up to the host's connect timeout, so the
    /// controller's deadline, counted from when the request arrived, must still fall by Alien's.
    #[test]
    fn the_controller_s_deadline_is_never_after_alien_s() {
        let stamped = chrono::DateTime::from_timestamp(1_800_000_000, 999_000_000).unwrap();
        let alien_deadline = stamped
            + chrono::Duration::from_std(alien_infra::step_protocol::STEP_DEADLINE).unwrap();
        let mut request = step_request();
        request.deadline = alien_deadline.to_rfc3339();
        let claims = token::Claims {
            iss: "i".into(),
            aud: "a".into(),
            project: "p".into(),
            deployment_id: "dep_1".into(),
            extension: "acme".into(),
            resource_id: "zone".into(),
            step_id: "s1".into(),
            body_sha256: String::new(),
            iat: stamped.timestamp(),
            exp: 0,
            jti: "j".into(),
        };
        let received = stamped
            + chrono::Duration::from_std(alien_infra::step_protocol::CONNECT_TIMEOUT).unwrap();
        let moved = deadline_on_this_clock(request, &claims, received);
        let controller_deadline = chrono::DateTime::parse_from_rfc3339(&moved.deadline)
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert!(
            controller_deadline <= alien_deadline,
            "{controller_deadline} is after {alien_deadline}"
        );
    }
}

/// What a controller's handler reads besides Alien's own context.
pub trait ControllerContextExt {
    /// An AWS SDK config for any `aws-sdk-*` client, built from the step alone: its credentials
    /// and region, the latest behavior version, standard retries, an HTTPS client and a Tokio
    /// sleep. Nothing comes from the environment or an AWS profile, so `AWS_ENDPOINT_URL`, FIPS,
    /// dual-stack and retry settings there don't apply.
    ///
    /// The HTTPS client trusts the system's root certificates, read by rustls-native-certs, which
    /// also reads `SSL_CERT_FILE` and `SSL_CERT_DIR`. It ignores HTTP proxies, so a controller
    /// behind a proxy builds its own client.
    fn aws_sdk_config(&self) -> impl Future<Output = Result<aws_types::SdkConfig>> + Send;

    /// The outputs of `id`, a resource this one depends on, as `T`. A controller reads a
    /// dependency's outputs, never its saved state.
    fn dependency_outputs<T: DeserializeOwned>(&self, id: &str) -> Result<T>;
}

impl ControllerContextExt for ResourceControllerContext<'_> {
    fn aws_sdk_config(&self) -> impl Future<Output = Result<aws_types::SdkConfig>> + Send {
        let config = self.get_aws_config().cloned();
        async move { sdk_config_of(config?) }
    }

    fn dependency_outputs<T: DeserializeOwned>(&self, id: &str) -> Result<T> {
        let own = self.desired_config.id();
        let not_found = || {
            AlienError::new(ErrorData::DependencyNotFound {
                resource_id: own.to_string(),
                dependency_id: id.to_string(),
            })
        };
        if id == own {
            return Err(not_found());
        }
        let outputs = self
            .state
            .resources
            .get(id)
            .ok_or_else(not_found)?
            .outputs
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: own.to_string(),
                    dependency_id: id.to_string(),
                })
            })?;
        let mut value = serde_json::to_value(outputs).into_alien_error().context(
            ErrorData::ResourceStateSerializationFailed {
                resource_id: id.to_string(),
                message: "the dependency's outputs did not serialize".to_string(),
            },
        )?;
        if let Value::Object(fields) = &mut value {
            fields.shift_remove("type");
        }
        serde_json::from_value(value)
            .into_alien_error()
            .context(ErrorData::ResourceConfigInvalid {
                message: format!("the outputs of '{id}' did not read as the type asked for"),
                resource_id: Some(own.to_string()),
            })
    }
}

fn sdk_config_of(config: alien_core::AwsClientConfig) -> Result<aws_types::SdkConfig> {
    use aws_smithy_http_client::tls;

    let AwsCredentials::AccessKeys {
        access_key_id,
        secret_access_key,
        session_token,
    } = config.credentials
    else {
        return Err(AlienError::new(ErrorData::ClientConfigInvalid {
            platform: Platform::Aws,
            message: "the step's AWS credentials are not access keys".to_string(),
        }));
    };
    let credentials = aws_credential_types::Credentials::new(
        access_key_id,
        secret_access_key,
        session_token,
        None,
        "alien-step",
    );
    let http_client = aws_smithy_http_client::Builder::new()
        .tls_provider(tls::Provider::Rustls(
            tls::rustls_provider::CryptoMode::AwsLc,
        ))
        .build_https();
    Ok(aws_types::SdkConfig::builder()
        .behavior_version(
            aws_smithy_runtime_api::client::behavior_version::BehaviorVersion::latest(),
        )
        .region(aws_types::region::Region::new(config.region))
        .credentials_provider(
            aws_credential_types::provider::SharedCredentialsProvider::new(credentials),
        )
        .retry_config(aws_types::sdk_config::RetryConfig::standard())
        .http_client(http_client)
        .sleep_impl(aws_smithy_async::rt::sleep::TokioSleep::new())
        .time_source(aws_smithy_async::time::SystemTimeSource::new())
        .build())
}
