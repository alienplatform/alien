//! The Operator image, served from the manager's registry.
//!
//! Charts point the Operator at `<manager>/alien-operator:<tag>`. Pulls are
//! passed through to the configured upstream image (the public Operator image
//! by default), so an environment that can only reach the manager still gets
//! its Operator, and upgrades need no new registry access.

use std::sync::{LazyLock, Mutex};

use axum::{
    body::Body,
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
};

use super::AppState;
use crate::auth::{Scope, Subject};

/// Repository name the Operator image is served under.
pub const OPERATOR_REPOSITORY: &str = "alien-operator";

/// Where the Operator image comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamImage {
    /// Registry host, e.g. `ghcr.io`.
    pub registry: String,
    /// Repository path in that registry, e.g. `alienplatform/alien-operator`.
    pub repository: String,
    /// Tag, e.g. `v3.3.25`.
    pub tag: String,
    /// Use plain HTTP (a registry on a private network without TLS).
    pub insecure: bool,
}

impl UpstreamImage {
    /// Parse `registry/repository:tag`.
    pub fn parse(image: &str, insecure: bool) -> Option<Self> {
        let (registry, rest) = image.split_once('/')?;
        let name_start = rest.rfind('/').map_or(0, |i| i + 1);
        let (repository, tag) = match rest[name_start..].rfind(':') {
            Some(i) => (&rest[..name_start + i], &rest[name_start + i + 1..]),
            None => (rest, "latest"),
        };
        Some(Self {
            registry: registry.to_string(),
            repository: repository.to_string(),
            tag: tag.to_string(),
            insecure,
        })
    }

    fn url(&self, operation: &str) -> String {
        let scheme = if self.insecure { "http" } else { "https" };
        format!(
            "{scheme}://{}/v2/{}/{operation}",
            self.registry, self.repository
        )
    }
}

/// Anonymous pull token for the upstream repository, reused until rejected.
static PULL_TOKEN: LazyLock<Mutex<Option<String>>> = LazyLock::new(Default::default);

/// Serve a pull of `alien-operator/...` from the upstream image.
pub async fn serve(
    state: &AppState,
    subject: &Subject,
    method: &Method,
    path: &str,
    headers: &HeaderMap,
) -> Response {
    if matches!(
        subject.scope,
        Scope::Commands { .. } | Scope::RemoteBindings { .. } | Scope::Telemetry { .. }
    ) {
        return oci_error(StatusCode::FORBIDDEN, "DENIED", "token cannot pull images");
    }
    let Some(upstream) = state
        .charts
        .as_ref()
        .and_then(|settings| settings.operator_upstream.clone())
    else {
        return oci_error(
            StatusCode::NOT_FOUND,
            "NAME_UNKNOWN",
            "this manager does not serve the Operator image",
        );
    };
    let operation = &path[OPERATOR_REPOSITORY.len() + 1..];
    if !(operation.starts_with("manifests/")
        || operation.starts_with("blobs/")
        || operation == "tags/list")
    {
        return oci_error(StatusCode::NOT_FOUND, "NAME_UNKNOWN", "unknown path");
    }

    let url = upstream.url(operation);
    let mut attempt = 0;
    loop {
        attempt += 1;
        let mut request = state.http_client.request(method.clone(), &url);
        if let Some(accept) = headers.get(header::ACCEPT) {
            request = request.header(header::ACCEPT, accept);
        }
        let token = PULL_TOKEN
            .lock()
            .expect("pull token lock is never held across a panic")
            .clone();
        if let Some(token) = &token {
            request = request.bearer_auth(token);
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(e) => {
                return oci_error(
                    StatusCode::BAD_GATEWAY,
                    "UNKNOWN",
                    format!("upstream Operator registry unreachable: {e}"),
                )
            }
        };
        if response.status() == reqwest::StatusCode::UNAUTHORIZED && attempt == 1 {
            let challenge = response
                .headers()
                .get(header::WWW_AUTHENTICATE)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            match challenge {
                Some(challenge) => match fetch_pull_token(state, &challenge).await {
                    Ok(token) => {
                        *PULL_TOKEN
                            .lock()
                            .expect("pull token lock is never held across a panic") = Some(token);
                        continue;
                    }
                    Err(message) => return oci_error(StatusCode::BAD_GATEWAY, "UNKNOWN", message),
                },
                None => {
                    return oci_error(
                        StatusCode::BAD_GATEWAY,
                        "UNKNOWN",
                        "upstream Operator registry requires credentials",
                    )
                }
            }
        }
        return relay(response);
    }
}

/// Anonymous token from a `Bearer realm=...,service=...,scope=...` challenge.
async fn fetch_pull_token(state: &AppState, challenge: &str) -> Result<String, String> {
    let params = challenge
        .strip_prefix("Bearer ")
        .ok_or_else(|| format!("unsupported registry auth challenge: {challenge}"))?;
    let mut realm = None;
    let mut query = Vec::new();
    for part in params.split(',') {
        let Some((key, value)) = part.trim().split_once('=') else {
            continue;
        };
        let value = value.trim_matches('"').to_string();
        match key {
            "realm" => realm = Some(value),
            "service" | "scope" => query.push((key.to_string(), value)),
            _ => {}
        }
    }
    let realm = realm.ok_or_else(|| "registry auth challenge has no realm".to_string())?;
    let body: serde_json::Value = state
        .http_client
        .get(&realm)
        .query(&query)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("registry token request failed: {e}"))?
        .json()
        .await
        .map_err(|e| format!("registry token response invalid: {e}"))?;
    body.get("token")
        .or_else(|| body.get("access_token"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| "registry token response has no token".to_string())
}

fn relay(upstream: reqwest::Response) -> Response {
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let headers: Vec<_> = [
        header::CONTENT_TYPE,
        header::CONTENT_LENGTH,
        header::ETAG,
        header::HeaderName::from_static("docker-content-digest"),
    ]
    .into_iter()
    .filter_map(|name| {
        let value = HeaderValue::from_bytes(upstream.headers().get(&name)?.as_bytes()).ok()?;
        Some((name, value))
    })
    .collect();
    let mut response = Response::new(Body::from_stream(upstream.bytes_stream()));
    *response.status_mut() = status;
    response.headers_mut().extend(headers);
    response
}

fn oci_error(status: StatusCode, code: &'static str, message: impl Into<String>) -> Response {
    let body = serde_json::json!({ "errors": [{ "code": code, "message": message.into() }] });
    (status, axum::Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::UpstreamImage;

    #[test]
    fn parses_upstream_images() {
        assert_eq!(
            UpstreamImage::parse("ghcr.io/alienplatform/alien-operator:v3.3.25", false),
            Some(UpstreamImage {
                registry: "ghcr.io".to_string(),
                repository: "alienplatform/alien-operator".to_string(),
                tag: "v3.3.25".to_string(),
                insecure: false,
            })
        );
        assert_eq!(
            UpstreamImage::parse("registry.internal:5000/ops/alien-operator", true).map(|image| (
                image.registry,
                image.repository,
                image.tag
            )),
            Some((
                "registry.internal:5000".to_string(),
                "ops/alien-operator".to_string(),
                "latest".to_string()
            ))
        );
        assert_eq!(UpstreamImage::parse("alien-operator:dev", false), None);
    }
}
