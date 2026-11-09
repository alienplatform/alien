//! Writes deployment values to the manager that stores them.
//!
//! The platform grants a short-lived token for a deployment's value slots.
//! The CLI sends the values to the manager with that token, then tells the
//! platform only which slots it wrote (a digest and a time, never the value).
//! A platform or manager that does not store values answers in a way that
//! selects the older path, which sends the values to the platform.

use alien_error::{AlienError, Context, GenericError, HumanLayerPresentation, IntoAlienError};
use alien_manager_api::types::{PutValue, PutValuesRequest, ValueKind};
use alien_manager_api::SdkResultExt as _;
use alien_platform_api::ClientInfo as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{ErrorData, Result};

/// The manager accepts at most this many values in one request.
const MAX_VALUES_PER_REQUEST: usize = 100;

/// Platform error codes that mean the deployment's manager does not store
/// values, so the older path applies.
const OLD_PATH_CODES: [&str; 2] = ["VALUES_NOT_SERVED_BY_MANAGER", "VALUE_SCOPE_NOT_AVAILABLE"];

/// A value to store.
#[derive(Clone)]
pub(crate) struct ValueWrite {
    /// `env:<NAME>` or `input:<id>`.
    pub slot: String,
    pub kind: ValueKind,
    pub value: String,
}

impl std::fmt::Debug for ValueWrite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ValueWrite")
            .field("slot", &self.slot)
            .finish_non_exhaustive()
    }
}

/// What the manager recorded for a slot. The platform stores this, not the value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ValueRef {
    pub slot: String,
    pub kind: String,
    pub digest: String,
    pub updated_at: String,
}

/// Where the values went.
#[derive(Debug)]
pub(crate) enum StoredValues {
    /// The manager holds the values. One reference per written slot, in the order written.
    Stored(Vec<ValueRef>),
    /// The platform or the manager does not store values. Send them the older way.
    OldPath,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Grant {
    manager_url: String,
    token: String,
}

#[derive(Debug, PartialEq, Eq)]
enum Reply {
    Granted(Grant),
    OldPath,
}

/// What the older platform's router answers for a path it has no route for.
const OLD_PLATFORM_NOT_FOUND_BODY: &str = "404 Not Found";

/// Reads the platform's answer to a value-access request.
///
/// A 404 whose body is exactly the router's default text means the platform
/// has no such route (an older platform). Any other 404, with or without an
/// error code, is an error: a gateway or firewall page must not make the CLI
/// send values to the platform. A 409 with
/// a code that says the deployment's manager does not store values selects the
/// older path. Every other failure is an error: a manager that stores values
/// and refuses a write is never worked around by sending the value elsewhere.
fn interpret_reply(status: u16, body: &str) -> Result<Reply> {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let code = parsed
        .as_ref()
        .and_then(|body| body.get("code"))
        .and_then(Value::as_str);
    match (status, code) {
        (200..=299, _) => serde_json::from_str(body)
            .map(Reply::Granted)
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: "reading the value access grant".to_string(),
                url: None,
            }),
        (404, _) if body == OLD_PLATFORM_NOT_FOUND_BODY => Ok(Reply::OldPath),
        (409, Some(code)) if OLD_PATH_CODES.contains(&code) => Ok(Reply::OldPath),
        _ => Err::<Reply, _>(platform_error(status, body)).context(ErrorData::ApiRequestFailed {
            message: "requesting access to store values".to_string(),
            url: None,
        }),
    }
}

/// The platform refuses values it must not hold with this code, before it
/// writes anything. The context names the input when an input caused it.
const HELD_VALUE_REFUSAL_CODE: &str = "ALIEN_CLI_UPGRADE_REQUIRED";
const INVALID_STACK_INPUT_CODE: &str = "INVALID_STACK_INPUT_VALUE";

/// True when a create call was refused because the deployment's manager stores
/// the values it carried and the platform must not hold them.
pub(crate) fn is_held_value_refusal(error: &AlienError<GenericError>) -> bool {
    error.code == HELD_VALUE_REFUSAL_CODE
}

/// The input a refused create names, or `None` when the refusal is about
/// environment variables.
pub(crate) fn refused_input(error: &AlienError<GenericError>) -> Option<String> {
    error
        .context
        .as_ref()
        .and_then(|context| context.get("input"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// The withheld input an older platform refuses a create for, because it
/// stores secret answers itself and wants this one in the request. Only its
/// own error for a missing required input counts: any other refusal that
/// happens to mention a name must not make the CLI send a secret.
pub(crate) fn missing_required_input(
    error: &AlienError<GenericError>,
    withheld: &serde_json::Map<String, Value>,
) -> Option<String> {
    if error.http_status_code != Some(400) || error.code != INVALID_STACK_INPUT_CODE {
        return None;
    }
    let input = error.context.as_ref()?.get("inputId")?.as_str()?;
    withheld.contains_key(input).then(|| input.to_string())
}

/// The command that stores `variables` on a deployment that was created
/// without them. Values stay out of the text; the user fills them in.
pub(crate) fn set_env_retry_command(
    deployment_id: &str,
    variables: &[alien_platform_api::types::EnvironmentVariableConfig],
) -> String {
    let mut command = format!("alien deployments set-env {deployment_id} --yes");
    for variable in variables {
        let secret = matches!(
            variable.type_,
            alien_platform_api::types::EnvironmentVariableType::Secret
        );
        let targets = variable
            .target_resources
            .as_ref()
            .filter(|targets| !targets.is_empty())
            .map(|targets| {
                let names: Vec<String> = targets
                    .iter()
                    .filter_map(|target| serde_json::to_value(target).ok())
                    .filter_map(|target| target.as_str().map(str::to_string))
                    .collect();
                format!(":{}", names.join(","))
            });
        let flag = match (secret, targets.is_some()) {
            (false, false) => "--env",
            (true, false) => "--secret",
            (false, true) => "--env-targeted",
            (true, true) => "--secret-targeted",
        };
        command.push_str(&format!(
            " {flag} {}=<value>{}",
            variable.name.as_str(),
            targets.unwrap_or_default()
        ));
    }
    command
}

/// The platform's error answer, keeping its code, message and retry hint.
pub(crate) fn platform_error(status: u16, body: &str) -> AlienError<GenericError> {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let field = |name: &str| {
        parsed
            .as_ref()
            .and_then(|body| body.get(name))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let message = field("message").unwrap_or_else(|| {
        let text: String = body.chars().take(300).collect();
        format!("HTTP {status}: {text}")
    });
    AlienError {
        code: field("code").unwrap_or_else(|| "UNEXPECTED_RESPONSE".to_string()),
        message: message.clone(),
        context: parsed
            .as_ref()
            .and_then(|body| body.get("context"))
            .cloned(),
        hint: field("hint"),
        retryable: parsed
            .as_ref()
            .and_then(|body| body.get("retryable"))
            .and_then(Value::as_bool)
            .unwrap_or(status >= 500),
        internal: false,
        http_status_code: Some(status),
        source: None,
        human_layer_presentation: HumanLayerPresentation::Normal,
        error: Some(GenericError { message }),
    }
}

/// Sends one request to the platform with the client's credentials and returns
/// the status and body.
pub(crate) async fn platform_request(
    client: &alien_platform_api::Client,
    method: reqwest::Method,
    path: &str,
    workspace: &str,
    body: Option<&Value>,
) -> Result<(u16, String)> {
    let mut url = reqwest::Url::parse(&format!(
        "{}{}",
        client.baseurl().trim_end_matches('/'),
        path
    ))
    .into_alien_error()
    .context(ErrorData::ConfigurationError {
        message: "Invalid platform API base URL".to_string(),
    })?;
    url.query_pairs_mut().append_pair("workspace", workspace);
    let mut request = client.client().request(method, url.clone());
    if let Some(body) = body {
        request = request.json(body);
    }
    let response =
        request
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: format!("calling {path}"),
                url: Some(url.to_string()),
            })?;
    let status = response.status().as_u16();
    let text = response
        .text()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("reading the answer from {path}"),
            url: Some(url.to_string()),
        })?;
    Ok((status, text))
}

/// A platform call that must succeed, answered with its JSON body.
pub(crate) async fn platform_json(
    client: &alien_platform_api::Client,
    method: reqwest::Method,
    path: &str,
    workspace: &str,
    body: Option<&Value>,
    action: &str,
) -> Result<Value> {
    let (status, text) = platform_request(client, method, path, workspace, body).await?;
    if !(200..300).contains(&status) {
        return Err(platform_error(status, &text)).context(ErrorData::ApiRequestFailed {
            message: action.to_string(),
            url: None,
        });
    }
    serde_json::from_str(&text)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "parse".to_string(),
            reason: format!("the answer to: {action}"),
        })
}

async fn request_access(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
    slots: &[&str],
) -> Result<Reply> {
    let (status, body) = platform_request(
        client,
        reqwest::Method::POST,
        &format!("/v1/deployments/{deployment_id}/value-access"),
        workspace,
        Some(&json!({ "slots": slots, "access": "write" })),
    )
    .await?;
    interpret_reply(status, &body)
}

/// Stores `writes` in the manager of a deployment.
pub(crate) async fn store_deployment_values(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
    writes: &[ValueWrite],
) -> Result<StoredValues> {
    if writes.is_empty() {
        return Ok(StoredValues::Stored(Vec::new()));
    }
    let slots: Vec<&str> = writes.iter().map(|write| write.slot.as_str()).collect();
    let grant = match request_access(client, workspace, deployment_id, &slots).await? {
        Reply::Granted(grant) => grant,
        Reply::OldPath => return Ok(StoredValues::OldPath),
    };
    let refs = put_values(&grant, deployment_id, writes).await?;
    Ok(StoredValues::Stored(refs))
}

/// Values and tokens go to the URL this names, so it must be a TLS endpoint,
/// or a loopback address for a manager on this machine.
pub(crate) fn checked_manager_url(manager_url: &str) -> Result<reqwest::Url> {
    let refuse = |reason: &str| {
        AlienError::new(ErrorData::ValidationError {
            field: "managerUrl".to_string(),
            message: format!("the manager URL is not safe to send values to ({reason})"),
        })
    };
    let url = reqwest::Url::parse(manager_url).map_err(|_| refuse("it is not a URL"))?;
    match url.scheme() {
        "https" => Ok(url),
        "http" => {
            let loopback = match url.host() {
                Some(url::Host::Domain(domain)) => domain == "localhost",
                Some(url::Host::Ipv4(address)) => address.is_loopback(),
                Some(url::Host::Ipv6(address)) => address.is_loopback(),
                None => false,
            };
            if loopback {
                Ok(url)
            } else {
                Err(refuse("plain http is allowed only for localhost"))
            }
        }
        _ => Err(refuse("it is not https")),
    }
}

fn manager_client(grant: &Grant) -> Result<alien_manager_api::Client> {
    let manager_url = checked_manager_url(&grant.manager_url)?;
    let mut headers = reqwest::header::HeaderMap::new();
    let mut authorization =
        reqwest::header::HeaderValue::from_str(&format!("Bearer {}", grant.token))
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: "the value access token is not a valid header".to_string(),
                url: None,
            })?;
    authorization.set_sensitive(true);
    headers.insert(reqwest::header::AUTHORIZATION, authorization);
    // A redirect would send the value again, possibly over plain http.
    let http = crate::http_client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .default_headers(headers)
        .build()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to build HTTP client".to_string(),
        })?;
    Ok(alien_manager_api::Client::new_with_client(
        manager_url.as_str().trim_end_matches('/'),
        http,
    ))
}

/// Removes slots from the manager of a deployment after the platform stopped
/// referring to them. This only makes the removal faster: the platform removes
/// them too. A failure prints a warning and changes nothing else.
pub(crate) async fn delete_values_best_effort(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
    slots: &[String],
) {
    if slots.is_empty() {
        return;
    }
    if let Err(error) = delete_values(client, workspace, deployment_id, slots).await {
        eprintln!(
            "Warning: could not remove {} from the manager of deployment '{deployment_id}': {error}. The platform removes them too.",
            slots.join(", ")
        );
    }
}

async fn delete_values(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
    slots: &[String],
) -> Result<()> {
    let slot_refs: Vec<&str> = slots.iter().map(String::as_str).collect();
    let grant = match request_access(client, workspace, deployment_id, &slot_refs).await? {
        Reply::Granted(grant) => grant,
        Reply::OldPath => return Ok(()),
    };
    let manager = manager_client(&grant)?;
    let scope = format!("dep:{deployment_id}");
    for slot in slots {
        manager
            .delete_value()
            .scope(&scope)
            .slot(slot)
            .send()
            .await
            .into_sdk_error()
            .await
            .context(ErrorData::ApiRequestFailed {
                message: format!("removing {slot}"),
                url: None,
            })?;
    }
    Ok(())
}

async fn put_values(
    grant: &Grant,
    deployment_id: &str,
    writes: &[ValueWrite],
) -> Result<Vec<ValueRef>> {
    let manager = manager_client(grant)?;
    let scope = format!("dep:{deployment_id}");

    let mut refs = Vec::with_capacity(writes.len());
    for chunk in writes.chunks(MAX_VALUES_PER_REQUEST) {
        let body = PutValuesRequest {
            values: chunk
                .iter()
                .map(|write| PutValue {
                    slot: write.slot.clone(),
                    kind: write.kind,
                    value: write.value.clone(),
                })
                .collect(),
        };
        let stored = manager
            .put_values()
            .scope(&scope)
            .body(body)
            .send()
            .await
            .into_sdk_error()
            .await
            .context(ErrorData::ApiRequestFailed {
                message: "storing values".to_string(),
                url: None,
            })?
            .into_inner();
        for write in chunk {
            let entry = stored
                .slots
                .iter()
                .find(|entry| entry.slot == write.slot)
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ApiRequestFailed {
                        message: format!("the answer did not include slot '{}'", write.slot),
                        url: None,
                    })
                })?;
            refs.push(ValueRef {
                slot: entry.slot.clone(),
                kind: match entry.kind {
                    ValueKind::Plain => "plain",
                    ValueKind::Secret => "secret",
                }
                .to_string(),
                digest: entry.digest.clone(),
                updated_at: entry.updated_at.clone(),
            });
        }
    }
    Ok(refs)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRANT: &str = r#"{"managerUrl":"https://m.example","token":"t","scope":"dep:dep_1","expiresAt":"2026-01-01T00:00:00Z"}"#;

    #[test]
    fn a_grant_carries_the_manager_url_and_token() {
        let reply = interpret_reply(200, GRANT).expect("a grant parses");
        assert_eq!(
            reply,
            Reply::Granted(Grant {
                manager_url: "https://m.example".to_string(),
                token: "t".to_string(),
            })
        );
    }

    #[test]
    fn a_route_the_platform_does_not_have_selects_the_older_path() {
        // What Hono answers for a path it has no route for.
        assert_eq!(
            interpret_reply(404, "404 Not Found").unwrap(),
            Reply::OldPath
        );
    }

    #[test]
    fn a_404_that_is_not_the_routers_default_text_is_an_error() {
        for body in [
            "",
            r#"{"message":"gone"}"#,
            "<html><body>404 Not Found</body></html>",
            "404 Not Found\n<!-- cdn -->",
        ] {
            interpret_reply(404, body)
                .expect_err("only the older platform's own 404 selects the older path");
        }
    }

    #[test]
    fn a_missing_deployment_is_an_error_not_a_fallback() {
        let body =
            r#"{"code":"DEPLOYMENT_NOT_FOUND","message":"Deployment not found","retryable":false}"#;
        let error = interpret_reply(404, body).expect_err("a real 404 is not the older path");
        assert_eq!(error.http_status_code, Some(404));
        assert!(error.to_string().contains("requesting access"), "{error}");
    }

    #[test]
    fn a_manager_without_value_storage_selects_the_older_path() {
        for code in OLD_PATH_CODES {
            let body = format!(r#"{{"code":"{code}","message":"m","retryable":false}}"#);
            assert_eq!(
                interpret_reply(409, &body).unwrap(),
                Reply::OldPath,
                "{code}"
            );
        }
    }

    #[test]
    fn other_refusals_and_outages_are_errors() {
        let cases = [
            (401, "Unauthorized".to_string()),
            (
                403,
                r#"{"code":"VALUE_SLOT_NOT_ALLOWED","message":"no"}"#.to_string(),
            ),
            (
                409,
                r#"{"code":"SOMETHING_ELSE","message":"no"}"#.to_string(),
            ),
            (
                400,
                r#"{"code":"ALIEN_CLI_UPGRADE_REQUIRED","message":"upgrade"}"#.to_string(),
            ),
            (
                503,
                r#"{"code":"MANAGER_OFFLINE","message":"offline","retryable":true}"#.to_string(),
            ),
            (
                503,
                r#"{"code":"VALUES_MANAGER_UNAVAILABLE","message":"isn't reachable","retryable":true}"#
                    .to_string(),
            ),
            (500, "<html>proxy</html>".to_string()),
        ];
        for (status, body) in cases {
            let error = interpret_reply(status, &body)
                .expect_err(&format!("{status} {body} must not select the older path"));
            assert_eq!(error.http_status_code, Some(status), "{status} {body}");
        }
    }

    #[test]
    fn a_manager_url_must_be_https_or_loopback() {
        for url in [
            "https://m.example",
            "https://m.example:8443/base",
            "http://localhost:8080",
            "http://127.0.0.1:9090",
            "http://[::1]:9090",
        ] {
            checked_manager_url(url).unwrap_or_else(|error| panic!("{url} is allowed: {error}"));
        }
        for url in [
            "http://m.example",
            "http://10.0.0.5:8080",
            "http://localhost.evil.example",
            "http://127.0.0.1.evil.example",
            "http://[::2]:80",
            "ftp://m.example",
            "file:///etc/passwd",
            "m.example",
            "",
        ] {
            let error = checked_manager_url(url).expect_err(&format!("{url} is refused"));
            assert!(
                error.to_string().contains("managerUrl")
                    || error.to_string().contains("manager URL"),
                "{url}: {error}"
            );
        }
    }

    #[test]
    fn a_grant_for_a_plain_http_manager_never_gets_a_client() {
        let grant = Grant {
            manager_url: "http://m.example".to_string(),
            token: "canary-token".to_string(),
        };
        let error = manager_client(&grant).expect_err("no client for plain http");
        assert!(!error.to_string().contains("canary-token"), "{error}");
    }

    /// A manager that answers a value write with a redirect must not make the
    /// CLI send the value again to the address it names, for example an
    /// `http://` one.
    #[tokio::test]
    async fn a_redirect_from_the_manager_never_gets_the_value_again() {
        use std::sync::{Arc, Mutex};

        use axum::{extract::State, http::HeaderMap, response::IntoResponse, routing::any, Router};

        const VALUE: &str = "canary-redirect-5b2f7a";
        type Seen = Arc<Mutex<Vec<String>>>;

        async fn record(State(seen): State<Seen>, body: String) -> impl IntoResponse {
            seen.lock().unwrap().push(body);
            axum::http::StatusCode::OK
        }
        async fn serve(router: Router) -> std::net::SocketAddr {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
            addr
        }

        let elsewhere: Seen = Arc::default();
        let second = serve(
            Router::new()
                .route("/{*path}", any(record))
                .with_state(elsewhere.clone()),
        )
        .await;
        let target = format!("http://{second}/v1/values");
        let first_requests: Seen = Arc::default();
        let first = serve(
            Router::new()
                .route(
                    "/{*path}",
                    any(
                        |State(seen): State<Seen>, _headers: HeaderMap, body: String| async move {
                            seen.lock().unwrap().push(body);
                            (
                                axum::http::StatusCode::TEMPORARY_REDIRECT,
                                [(axum::http::header::LOCATION, target)],
                            )
                        },
                    ),
                )
                .with_state(first_requests.clone()),
        )
        .await;

        let grant = Grant {
            manager_url: format!("http://{first}"),
            token: "token".to_string(),
        };
        let write = ValueWrite {
            slot: "env:API_KEY".to_string(),
            kind: ValueKind::Secret,
            value: VALUE.to_string(),
        };
        put_values(&grant, "dep_1", &[write])
            .await
            .expect_err("a redirect is an error, not a second request");

        assert_eq!(
            first_requests.lock().unwrap().len(),
            1,
            "the manager was asked once"
        );
        assert!(
            elsewhere.lock().unwrap().is_empty(),
            "the redirect target received the value: {:?}",
            elsewhere.lock().unwrap()
        );
    }

    #[test]
    fn a_platform_error_keeps_its_code_and_retry_hint() {
        let error = platform_error(
            503,
            r#"{"code":"MANAGER_OFFLINE","message":"offline","retryable":true}"#,
        );
        assert_eq!(error.code, "MANAGER_OFFLINE");
        assert!(error.retryable);
        let error = platform_error(502, "bad gateway");
        assert_eq!(error.code, "UNEXPECTED_RESPONSE");
        assert!(error.retryable);
        assert_eq!(error.message, "HTTP 502: bad gateway");
    }

    #[test]
    fn a_value_never_shows_in_debug_output() {
        let write = ValueWrite {
            slot: "env:API_KEY".to_string(),
            kind: ValueKind::Secret,
            value: "canary-value".to_string(),
        };
        let shown = format!("{write:?}");
        assert!(shown.contains("env:API_KEY"));
        assert!(!shown.contains("canary-value"));
    }
}
