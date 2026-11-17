//! Interceptors: an extension's code that changes a cloud call after Alien builds it and before
//! Alien signs it, and changes the response before Alien reads it.
//!
//! A deployment's chain travels on the clients built for it, never on a client config or in a
//! global, so two deployments in one process never see each other's interceptors. An
//! interceptor never sees the credentials Alien signs with. It may change what a call says about
//! the deployment's own resources, and Alien refuses any change to where a call goes, to its
//! method and path, to which identity it names, to an image in a cloud registry, or to a header
//! that carries or protects a credential; on Kubernetes it may change only an object's labels. A
//! call that changes who has access, acts as another identity or carries a Kubernetes credential
//! is never offered, and no call may become one.

use crate::error::{ErrorData, Result};
use crate::identity::{decoded, is_identity_call};
use alien_error::{AlienError, Context, IntoAlienError};
use base64::Engine;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_LENGTH};
use reqwest::{Method, Url};
#[cfg(not(target_arch = "wasm32"))]
use reqwest::{ResponseBuilderExt, StatusCode};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::sync::Arc;

/// The API a call goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cloud {
    Aws,
    Gcp,
    Azure,
    Kubernetes,
}

impl std::fmt::Display for Cloud {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Cloud::Aws => "aws",
            Cloud::Gcp => "gcp",
            Cloud::Azure => "azure",
            Cloud::Kubernetes => "kubernetes",
        })
    }
}

/// A call as an interceptor sees it: the request Alien built, without credentials.
#[derive(Debug, Clone)]
pub struct HookRequest {
    pub cloud: Cloud,
    /// The service the call is signed for: the SigV4 signing name on AWS (`rds`), Alien's
    /// endpoint key on Google Cloud (`cloudsql`), `management` for Azure Resource Manager, and
    /// `kubernetes` for the API server.
    pub service: String,
    pub method: Method,
    pub url: Url,
    /// The request's headers, without `authorization`, `proxy-authorization` and
    /// `x-amz-security-token`.
    pub headers: HeaderMap,
    /// The body as text, or `None` when the request has no body. A binary body is never
    /// offered, so `matches` sees `None` for it as well, and a call it matches fails.
    pub body: Option<String>,
}

/// A response as an interceptor sees it, before Alien reads it.
#[derive(Debug, Clone)]
pub struct HookResponse {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: String,
}

/// One extension's interceptor.
pub trait CallInterceptor: Send + Sync {
    /// The extension's name, which every error this interceptor causes carries.
    fn extension(&self) -> &str;

    /// Whether this interceptor sees `request`, the call as the interceptors before it left it.
    /// Only the interceptors that match a call see its request and its response. An error fails
    /// the call, as a failing `modify_request` does.
    fn matches(&self, request: &HookRequest) -> Result<bool>;

    fn modify_request(&self, request: &mut HookRequest) -> Result<()>;

    /// Changes `response`, error responses included. `request` is the request as sent. An error
    /// fails the call for good, even one the interceptor marked retryable.
    fn modify_response(&self, request: &HookRequest, response: &mut HookResponse) -> Result<()>;
}

/// A deployment's interceptors, in the order its stack lists extensions. Each one gets the
/// request as the previous one left it.
#[derive(Clone, Default)]
pub struct InterceptorChain {
    interceptors: Vec<Arc<dyn CallInterceptor>>,
    /// Set for a call whose body, sent or received, is an object's bytes.
    object_payload: bool,
}

impl std::fmt::Debug for InterceptorChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(
                self.interceptors
                    .iter()
                    .map(|interceptor| interceptor.extension()),
            )
            .finish()
    }
}

/// Headers an interceptor may not set, change or remove: they pick the destination, the method
/// the server runs, the project billed or the object an S3 copy reads, carry a credential, are
/// signed with one, or say how the body is read.
const PROTECTED_HEADERS: [&str; 12] = [
    "host",
    "content-type",
    "x-http-method-override",
    "x-http-method",
    "authorization",
    "x-amz-date",
    "x-amz-security-token",
    "proxy-authorization",
    "x-goog-user-project",
    "x-goog-api-key",
    "x-ms-authorization-auxiliary",
    "x-amz-copy-source",
];

/// Query parameters Google Cloud reads the project billed, the caller's credential or the
/// format of the body and response from, in any case, so an interceptor may not set, change or
/// remove them on a Google Cloud call. Every system parameter written with a leading `$` (such
/// as `$ct`, which sets the body's content type) is protected the same way.
const PROTECTED_GCP_QUERY: [&str; 6] = [
    "userproject",
    "key",
    "access_token",
    "oauth_token",
    "bearer_token",
    "alt",
];

/// Kubernetes' impersonation headers (`impersonate-user`, `impersonate-group`,
/// `impersonate-extra-*`, ...) set who the call acts as, so an interceptor may not set, change or
/// remove any of them.
const PROTECTED_HEADER_PREFIX: &str = "impersonate-";

/// AWS operations that read or change a resource's policy, its grants or who may call it,
/// through the resource's own service: SNS and SQS permissions, KMS key policies and grants,
/// ECR repository policies, and Secrets Manager and CloudWatch Logs resource policies.
const AWS_POLICY_OPERATIONS: [&str; 13] = [
    "AddPermission",
    "RemovePermission",
    "PutKeyPolicy",
    "GetKeyPolicy",
    "CreateGrant",
    "RetireGrant",
    "RevokeGrant",
    "SetRepositoryPolicy",
    "GetRepositoryPolicy",
    "DeleteRepositoryPolicy",
    "PutResourcePolicy",
    "GetResourcePolicy",
    "DeleteResourcePolicy",
];

/// The WebAssembly reqwest cannot rebuild a response, and no WebAssembly build of Alien gives
/// its clients interceptors, so a call one matches fails before it is sent.
const WASM_UNSUPPORTED: &str = "interceptors do not run in WebAssembly builds of Alien";

/// Headers that carry a credential. The interceptor's view leaves them out, and the request
/// keeps them as they were.
const CREDENTIAL_HEADERS: [&str; 3] = [
    "authorization",
    "proxy-authorization",
    "x-amz-security-token",
];

/// Where a signed request carries a credential: these headers, and on Google Cloud these query
/// parameters. A response can quote them, as AWS quotes the signed headers, session token
/// included, when a signature does not match.
const CREDENTIAL_CARRIERS: [&str; 5] = [
    "authorization",
    "proxy-authorization",
    "x-amz-security-token",
    "x-goog-api-key",
    "x-ms-authorization-auxiliary",
];
const CREDENTIAL_QUERY: [&str; 3] = ["access_token", "key", "oauth_token"];

impl InterceptorChain {
    pub fn new(interceptors: Vec<Arc<dyn CallInterceptor>>) -> Self {
        Self {
            interceptors,
            object_payload: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.interceptors.is_empty()
    }

    /// The same interceptors, for a call that sends or returns an object's bytes (an S3 or Cloud
    /// Storage object, a Lambda payload). Those bytes are never offered, whatever their content
    /// type, so the call fails before it is sent when an interceptor matches it.
    pub fn for_object_payload(&self) -> Self {
        Self {
            interceptors: self.interceptors.clone(),
            object_payload: true,
        }
    }

    /// Runs the interceptors that match `request`, which Alien built and has not sent, on it.
    /// Returns the hook its response must pass through, or `None` when no interceptor matched
    /// and the request is left as it was.
    pub fn intercept(
        &self,
        cloud: Cloud,
        service: &str,
        request: &mut reqwest::Request,
    ) -> Result<Option<ResponseHook>> {
        if self.is_empty() {
            return Ok(None);
        }
        let body = match request.body() {
            _ if self.object_payload => Err(()),
            None => Ok(None),
            Some(body) => match body.as_bytes() {
                None => Err(()),
                Some([]) => Ok(None),
                Some(bytes) => String::from_utf8(bytes.to_vec()).map(Some).map_err(|_| ()),
            },
        };
        let mut headers = request.headers().clone();
        for name in CREDENTIAL_HEADERS {
            headers.remove(name);
        }
        let original = HookRequest {
            cloud,
            service: service.to_string(),
            method: request.method().clone(),
            url: request.url().clone(),
            headers,
            body: body.clone().unwrap_or_default(),
        };
        // Not even `matches` sees it, so an interceptor that matches every call never fails it.
        if changes_access(&original) {
            return Ok(None);
        }

        if body.is_err() || cfg!(target_arch = "wasm32") {
            let mut first_match = None;
            for interceptor in &self.interceptors {
                if matches(interceptor.as_ref(), &original)? {
                    first_match = Some(interceptor);
                    break;
                }
            }
            let Some(interceptor) = first_match else {
                return Ok(None);
            };
            let reason = if body.is_err() {
                format!(
                    "the {cloud} {service} call {} {} carries a binary body, which interceptors are not offered",
                    original.method,
                    original.url.path()
                )
            } else {
                WASM_UNSUPPORTED.to_string()
            };
            return Err(failed(interceptor.extension(), reason));
        }

        let mut view = original.clone();
        let mut matched = Vec::new();
        for interceptor in &self.interceptors {
            if !matches(interceptor.as_ref(), &view)? {
                continue;
            }
            interceptor
                .modify_request(&mut view)
                .context_keeping_retry(ErrorData::InterceptorFailed {
                    extension: interceptor.extension().to_string(),
                    message: format!("modify_request failed on the {cloud} {service} call"),
                })?;
            // A JSON body is sent as it was parsed, and checked in that form, so a key written
            // twice or an escape a cloud would decode can't carry a change the checks never saw.
            if view.body != original.body {
                if let Some(Ok(parsed)) = view
                    .body
                    .as_deref()
                    .map(serde_json::from_str::<serde_json::Value>)
                {
                    view.body = Some(parsed.to_string());
                }
            }
            if let Some(reason) = refused_change(&original, &view) {
                return Err(failed(interceptor.extension(), reason));
            }
            matched.push(interceptor.clone());
        }
        if matched.is_empty() {
            return Ok(None);
        }

        let mut headers = view.headers.clone();
        for name in CREDENTIAL_HEADERS {
            for value in request.headers().get_all(name) {
                headers.append(name, value.clone());
            }
        }
        *request.method_mut() = view.method.clone();
        *request.url_mut() = view.url.clone();
        *request.headers_mut() = headers;
        if view.body != original.body {
            let body = view.body.clone().unwrap_or_default();
            rewrite_body_digests(request.headers_mut(), body.as_bytes())?;
            *request.body_mut() = view.body.clone().map(reqwest::Body::from);
        }

        let mut hook = ResponseHook {
            matched: matched.into(),
            request: Arc::new(view),
            secrets: Vec::new(),
        };
        // A client that signs before its interceptors run has its credential on the request
        // already; one that signs after adds it with `remember_credentials`.
        hook.remember_credentials(request);
        Ok(Some(hook))
    }

    /// Refuses `url` unless it is on `endpoint`, the endpoint this client was configured with.
    /// A response names URLs Alien then sends its credential to (an operation to poll, a next
    /// page), and an interceptor can rewrite a response, so a client with interceptors signs only
    /// for its own endpoint.
    pub fn check_destination(&self, url: &Url, endpoint: &str) -> Result<()> {
        if self.is_empty() {
            return Ok(());
        }
        let endpoint =
            Url::parse(endpoint)
                .into_alien_error()
                .context(ErrorData::InvalidClientConfig {
                    message: format!("The client's endpoint '{endpoint}' is not a URL"),
                    errors: None,
                })?;
        if url.origin() == endpoint.origin() {
            return Ok(());
        }
        Err(AlienError::new(ErrorData::DestinationRefused {
            url: url.origin().ascii_serialization(),
            endpoint: endpoint.origin().ascii_serialization(),
        }))
    }
}

/// The interceptors that matched one call, to run on its response.
#[derive(Clone)]
pub struct ResponseHook {
    matched: Arc<[Arc<dyn CallInterceptor>]>,
    request: Arc<HookRequest>,
    /// The credentials the call was sent with, cleared from its response before any interceptor
    /// reads it.
    secrets: Vec<String>,
}

impl std::fmt::Debug for ResponseHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(
                self.matched
                    .iter()
                    .map(|interceptor| interceptor.extension()),
            )
            .finish()
    }
}

impl ResponseHook {
    /// Records the credentials `request` carries, so they are cleared from the response before
    /// an interceptor reads it. A client that signs after the interceptors ran calls this with
    /// the signed request.
    pub fn remember_credentials(&mut self, request: &reqwest::Request) {
        for name in CREDENTIAL_CARRIERS {
            for value in request.headers().get_all(name) {
                let Ok(value) = value.to_str() else {
                    continue;
                };
                self.remember(value);
                // `Bearer <token>`, alone or listed, as Azure's auxiliary header lists them.
                for part in value.split(',') {
                    if let Some((_, credential)) = part.trim().split_once(' ') {
                        self.remember(credential);
                    }
                }
            }
        }
        for (name, value) in request.url().query_pairs() {
            if CREDENTIAL_QUERY
                .iter()
                .any(|carrier| name.eq_ignore_ascii_case(carrier))
            {
                self.remember(&value);
            }
        }
    }

    fn remember(&mut self, secret: &str) {
        let secret = secret.trim();
        // Short of any real credential, and short enough to match ordinary text.
        if secret.len() >= 8 && !self.secrets.iter().any(|known| known == secret) {
            self.secrets.push(secret.to_string());
        }
    }

    /// `bytes` with each recorded credential replaced by `[redacted]`, longest first, so one
    /// that contains another is cleared whole.
    fn cleared(&self, bytes: &[u8]) -> Vec<u8> {
        let mut secrets: Vec<&String> = self.secrets.iter().collect();
        secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
        secrets.into_iter().fold(bytes.to_vec(), |bytes, secret| {
            blank(bytes, secret.as_bytes())
        })
    }

    /// Reads the whole response, lets each interceptor that matched the call change it, and
    /// returns a response Alien reads as if the cloud had sent it.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn apply(&self, response: reqwest::Response) -> Result<reqwest::Response> {
        let status = response.status();
        let url = response.url().clone();
        let headers = response.headers().clone();
        let read = response.bytes().await.into_alien_error();
        // This runs inside the retry loop, and after a 2xx the cloud already acted on the call.
        let bytes = if status.is_success() {
            read.context(ErrorData::HttpResponseBodyReadFailed {
                http_status: status.as_u16(),
            })?
        } else {
            read.context(ErrorData::HttpRequestFailed {
                message: "Failed to read response body".to_string(),
            })?
        };
        let Ok(body) = String::from_utf8(bytes.to_vec()) else {
            return Err(failed(
                self.matched[0].extension(),
                format!(
                    "the response to the {} {} call {} {} has a binary body, which interceptors are not offered",
                    self.request.cloud,
                    self.request.service,
                    self.request.method,
                    self.request.url.path()
                ),
            ));
        };

        // Cleared before any interceptor runs: one could encode an echoed credential into what
        // it leaves, where clearing it later would no longer find it.
        let mut cleared_headers = HeaderMap::with_capacity(headers.len());
        for (name, value) in &headers {
            // `[redacted]` in place of the credential keeps the value a valid header.
            if let Ok(value) = HeaderValue::from_bytes(&self.cleared(value.as_bytes())) {
                cleared_headers.append(name.clone(), value);
            }
        }
        let body = String::from_utf8(self.cleared(body.as_bytes())).unwrap_or(body);
        let mut view = HookResponse {
            status: status.as_u16(),
            headers: cleared_headers,
            body,
        };
        let received = (view.status, view.headers.clone(), view.body.clone());
        for interceptor in self.matched.iter() {
            // Final whatever the hook said: the cloud already acted on the call, and a retry
            // would send it again.
            interceptor
                .modify_response(&self.request, &mut view)
                .context(ErrorData::InterceptorFailed {
                    extension: interceptor.extension().to_string(),
                    message: format!(
                        "modify_response failed on the {} {} call",
                        self.request.cloud, self.request.service
                    ),
                })?;
            let Ok(left) = StatusCode::from_u16(view.status) else {
                return Err(failed(
                    interceptor.extension(),
                    format!("{} is not an HTTP status", view.status),
                ));
            };
            // The cloud already acted on a call it answered with success, so a failure a hook
            // makes of it fails the call for good: a client would retry a status such as 429,
            // and send a create again.
            if status.is_success() && !left.is_success() {
                return Err(failed(
                    interceptor.extension(),
                    format!(
                        "it turned the {} {} call's success ({}) into {left}, after the cloud had acted on it",
                        self.request.cloud,
                        self.request.service,
                        status.as_u16()
                    ),
                ));
            }
            if self.request.cloud == Cloud::Kubernetes
                && (view.status, &view.headers, &view.body)
                    != (received.0, &received.1, &received.2)
            {
                return Err(failed(
                    interceptor.extension(),
                    "on Kubernetes an interceptor may not change a response".to_string(),
                ));
            }
        }

        let mut headers = view.headers;
        // The body's own length decides it; a stale one would cut or stall the read.
        headers.remove(CONTENT_LENGTH);
        let last = &self.matched[self.matched.len() - 1];
        let mut rebuilt = http::Response::builder()
            .status(view.status)
            .url(url)
            .body(view.body)
            .into_alien_error()
            .context(ErrorData::InterceptorFailed {
                extension: last.extension().to_string(),
                message: "the response it left could not be rebuilt".to_string(),
            })?;
        *rebuilt.headers_mut() = headers;
        Ok(reqwest::Response::from(rebuilt))
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn apply(&self, _response: reqwest::Response) -> Result<reqwest::Response> {
        Err(failed(
            self.matched[0].extension(),
            format!(
                "{WASM_UNSUPPORTED}: the {} {} call",
                self.request.cloud, self.request.service
            ),
        ))
    }
}

/// Keeps an interceptor's failure final under a wrapper that marks the error retryable: the same
/// request fails the same way again. A failure before the call was sent that the interceptor
/// itself marked retryable, as a hook stopped at its deadline is, stays retryable.
pub fn keep_interceptor_failure_final<T>(result: Result<T>) -> Result<T> {
    result.map_err(|mut error| {
        let final_failure =
            |code: &str, retryable: bool| is_interceptor_failure(code) && !retryable;
        let mut caused_by_interceptor = final_failure(&error.code, error.retryable);
        let mut layer = error.source.as_deref();
        while let Some(source) = layer {
            caused_by_interceptor |= final_failure(&source.code, source.retryable);
            layer = source.source.as_deref();
        }
        if caused_by_interceptor {
            error.retryable = false;
        }
        error
    })
}

/// `bytes` with each occurrence of `secret` replaced by `[redacted]`.
fn blank(bytes: Vec<u8>, secret: &[u8]) -> Vec<u8> {
    const BLANK: &[u8] = b"[redacted]";
    let mut cleared = Vec::new();
    let mut rest = bytes.as_slice();
    while let Some(at) = rest
        .windows(secret.len())
        .position(|window| window == secret)
    {
        cleared.extend_from_slice(&rest[..at]);
        cleared.extend_from_slice(BLANK);
        rest = &rest[at + secret.len()..];
    }
    if cleared.is_empty() {
        return bytes;
    }
    cleared.extend_from_slice(rest);
    cleared
}

fn is_interceptor_failure(code: &str) -> bool {
    matches!(code, "INTERCEPTOR_FAILED" | "DESTINATION_REFUSED")
}

/// Passes `response` through `hook`, when the call had one.
pub async fn apply_response_hook(
    hook: Option<&ResponseHook>,
    response: reqwest::Response,
) -> Result<reqwest::Response> {
    match hook {
        Some(hook) => hook.apply(response).await,
        None => Ok(response),
    }
}

/// Asks `interceptor` whether it sees `request`, naming it in the error when it can't answer.
fn matches(interceptor: &dyn CallInterceptor, request: &HookRequest) -> Result<bool> {
    interceptor
        .matches(request)
        .context_keeping_retry(ErrorData::InterceptorFailed {
            extension: interceptor.extension().to_string(),
            message: format!(
                "matches failed on the {} {} call",
                request.cloud, request.service
            ),
        })
}

/// `.context` for an interceptor's error from `matches` or `modify_request`, before the call is
/// sent. The failure is final unless the interceptor itself said it may pass, as a hook stopped
/// at its deadline may.
trait KeepingRetry<T> {
    fn context_keeping_retry(self, context: ErrorData) -> Result<T>;
}

impl<T> KeepingRetry<T> for Result<T> {
    fn context_keeping_retry(self, context: ErrorData) -> Result<T> {
        self.map_err(|error| {
            let retryable = error.retryable && error.code == "INTERCEPTOR_FAILED";
            let mut wrapped = Err::<(), _>(error).context(context).unwrap_err();
            wrapped.retryable = retryable;
            wrapped
        })
    }
}

fn failed(extension: &str, message: String) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::InterceptorFailed {
        extension: extension.to_string(),
        message,
    })
}

/// Why the change from `original` to `changed` is refused, if it is.
fn refused_change(original: &HookRequest, changed: &HookRequest) -> Option<String> {
    if original.cloud == Cloud::Kubernetes && !only_labels_changed(original, changed) {
        return Some(
            "on Kubernetes an interceptor may change only the object's metadata.labels".to_string(),
        );
    }
    if changed.url.origin() != original.url.origin() {
        return Some(format!(
            "it moved the call from {} to {}; it may change the query, not the scheme, host or port",
            original.url.origin().ascii_serialization(),
            changed.url.origin().ascii_serialization()
        ));
    }
    if changed.cloud != original.cloud || changed.service != original.service {
        return Some(format!(
            "it changed the call's cloud or service from {} {} to {} {}",
            original.cloud, original.service, changed.cloud, changed.service
        ));
    }
    if changes_access(changed) {
        return Some(
            "it turned the call into one that changes who has access or acts as another identity"
                .to_string(),
        );
    }
    if is_json(original) && !is_json(changed) {
        return Some("it changed a JSON body into one that is not JSON".to_string());
    }
    // The checks read a body by its format, and the cloud by the call's content type, which a
    // call Alien sends without a body may not match.
    if original.body.is_none() && changed.body.is_some() {
        return Some("it added a body to a call Alien sends without one".to_string());
    }
    // A JSON body is sent as it was parsed, so a body read as something else may not become one.
    if !is_json(original) && is_json(changed) {
        return Some("it changed a body that is not JSON into JSON".to_string());
    }
    if named_identities(changed) != named_identities(original) {
        return Some(
            "it changed which identity the call names; a call acts only as the identities Alien gives it"
                .to_string(),
        );
    }
    if let Some(image) = registry_images(changed)
        .difference(&registry_images(original))
        .next()
    {
        return Some(format!(
            "it named {image}, a reference to a cloud registry the call didn't name; only an image from a public registry may be added"
        ));
    }
    if changed.method != original.method || changed.url.path() != original.url.path() {
        return Some(format!(
            "it changed {} {} to {} {}; the method and path say which resource the call acts on and how",
            original.method,
            original.url.path(),
            changed.method,
            changed.url.path()
        ));
    }
    if original.cloud == Cloud::Aws && aws_operations(original) != aws_operations(changed) {
        return Some(
            "it changed the AWS operation, in the Action parameter or the X-Amz-Target header"
                .to_string(),
        );
    }
    // A query parameter can be the operation, such as S3's `?logging` or API Gateway's
    // `?mode=import`, so on AWS a parameter's value may change, its name not.
    if original.cloud == Cloud::Aws && query_names(original) != query_names(changed) {
        return Some(
            "it added or removed a query parameter; on AWS the parameters a call has are part of its operation"
                .to_string(),
        );
    }
    if original.cloud == Cloud::Gcp {
        let values = |request: &HookRequest, name: &str| -> Vec<String> {
            request
                .url
                .query_pairs()
                .filter(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.into_owned())
                .collect()
        };
        let system: BTreeSet<String> = [original, changed]
            .into_iter()
            .flat_map(|request| {
                request
                    .url
                    .query_pairs()
                    .map(|(key, _)| key.to_ascii_lowercase())
            })
            .filter(|key| key.starts_with('$'))
            .collect();
        if let Some(name) = PROTECTED_GCP_QUERY
            .into_iter()
            .map(str::to_string)
            .chain(
                PROTECTED_GCP_QUERY
                    .into_iter()
                    .map(|name| format!("${name}")),
            )
            .chain(system)
            .find(|name| values(original, name) != values(changed, name))
        {
            return Some(format!(
                "it set, changed or removed the '{name}' query parameter"
            ));
        }
    }
    // Header names are stored lowercase, so the prefix matches whatever case was sent.
    let impersonation = original
        .headers
        .keys()
        .chain(changed.headers.keys())
        .map(|name| name.as_str())
        .filter(|name| name.starts_with(PROTECTED_HEADER_PREFIX));
    PROTECTED_HEADERS
        .into_iter()
        .chain(impersonation)
        .find(|name| {
            !original
                .headers
                .get_all(*name)
                .iter()
                .eq(changed.headers.get_all(*name).iter())
        })
        .map(|name| format!("it set, changed or removed the '{name}' header"))
}

/// Whether `request` changes who has access, acts as another identity, reads a policy Alien then
/// writes back, or carries a Kubernetes Secret or service account token. The path is read
/// as the cloud reads it: percent-decoded and in any case.
fn changes_access(request: &HookRequest) -> bool {
    let path = decoded(request.url.path());
    // A cloud may fold a character that is not ASCII into one that is, and reach a name the
    // checks look for.
    if !path.is_ascii() {
        return true;
    }
    let path = path.to_ascii_lowercase();
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let query: Vec<(String, String)> = request.url.query_pairs().into_owned().collect();
    if is_identity_call(
        request.cloud,
        &[request.service.as_str()],
        request.method.as_str(),
        &segments,
        &query,
    ) {
        return true;
    }
    match request.cloud {
        // Alien reads a policy, adds its binding and writes the policy back, so a read's
        // response decides the write.
        Cloud::Gcp => {
            segments.iter().any(|s| s.contains("iampolicy"))
                || (request.service == "storage"
                    && (segments.last() == Some(&"iam")
                        || segments
                            .iter()
                            .any(|s| matches!(*s, "acl" | "defaultobjectacl"))))
        }
        Cloud::Aws => {
            aws_operations(request).iter().any(|operation| {
                // A target is `<service>.<operation>`.
                let name = operation.rsplit('.').next().unwrap_or(operation);
                AWS_POLICY_OPERATIONS.contains(&name)
            }) || request.body.as_deref().is_some_and(sets_a_policy)
                // The query protocol reads parameters from the URL as well as the form body.
                || form_sets_a_policy(request.url.query().unwrap_or_default())
                || (request.service == "s3"
                    && (query
                        .iter()
                        .any(|(name, _)| matches!(name.as_str(), "policy" | "acl"))
                        || request.headers.keys().any(|name| {
                            name == "x-amz-acl" || name.as_str().starts_with("x-amz-grant-")
                        })))
                || (request.service == "lambda" && segments.contains(&"policy"))
                || (request.service == "apigateway"
                    && request.body.as_deref().is_some_and(sets_an_api_policy))
        }
        // A Secret holds the deployment's registry credential, a token request mints a service
        // account's token, and a token review carries one.
        Cloud::Kubernetes => {
            segments.contains(&"tokenreviews")
                || segments
                    .windows(3)
                    .any(|w| w[0] == "namespaces" && w[2] == "secrets")
                || segments.windows(2).any(|w| w == ["v1", "secrets"])
                || segments
                    .windows(3)
                    .any(|w| w[0] == "serviceaccounts" && w[2] == "token")
        }
        Cloud::Azure => false,
    }
}

/// The identities a call's body or query names, each with where it names it: a Google service
/// account's email, an AWS IAM role or instance profile ARN, an Azure managed identity's
/// resource ID, and a field that names an identity by its name. A call that drops one may act as
/// a default identity instead, and a name kept in another field no longer picks the identity, so
/// a change of any kind counts.
fn named_identities(request: &HookRequest) -> BTreeSet<(String, String)> {
    let body = request.body.as_deref().unwrap_or_default();
    let query = request.url.query().unwrap_or_default();
    let json = serde_json::from_str::<serde_json::Value>(body).ok();
    // Every identity the text names, wherever it sits, so a duplicate JSON key or a body that is
    // neither JSON nor a form still counts. A JSON escape hides a name from the raw text, so the
    // decoded strings are read too.
    let mut texts = vec![body.to_string(), decoded(body), decoded(query)];
    if let Some(value) = &json {
        json_strings(value, &mut texts);
    }
    let mut named = BTreeSet::new();
    for text in texts {
        let tokens = text.split(|c: char| !(c.is_ascii_alphanumeric() || "@._:/+=,-".contains(c)));
        for token in tokens {
            if names_an_identity(token) {
                named.insert(("*".to_string(), token.to_ascii_lowercase()));
            }
        }
    }
    let pairs = |location: &str, text: &str, named: &mut BTreeSet<(String, String)>| {
        for (key, value) in form_urlencoded::parse(text.as_bytes()) {
            if key.split('.').any(identity_field)
                || names_an_identity(&key)
                || names_an_identity(&value)
            {
                named.insert((format!("{location}{key}"), value.into_owned()));
            }
        }
    };
    pairs("?", query, &mut named);
    match &json {
        Some(value) => identity_fields(value, "", false, &mut named),
        None => pairs("&", body, &mut named),
    }
    named
}

/// Whether `text` contains a Google service account's email, an AWS IAM ARN or an Azure managed
/// identity's resource ID.
fn names_an_identity(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains(".gserviceaccount.com")
        || (text.contains("arn:aws") && text.contains(":iam::"))
        || text.contains("/providers/microsoft.managedidentity/")
}

/// Whether a field by this name names the identity a resource acts as, whatever its value.
fn identity_field(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "role" | "identity" | "serviceaccount" | "serviceaccountname" | "serviceaccountemail"
    ) || name.ends_with("rolearn")
        || name.starts_with("iaminstanceprofile")
}

/// Each identity in `value` with its JSON pointer: a key or string that names one, and the whole
/// value of a field named for one. `in_service_accounts` is set under a `serviceAccounts` field,
/// whose `email` names the account.
fn identity_fields(
    value: &serde_json::Value,
    pointer: &str,
    in_service_accounts: bool,
    named: &mut BTreeSet<(String, String)>,
) {
    match value {
        serde_json::Value::String(text) if names_an_identity(text) => {
            named.insert((pointer.to_string(), text.clone()));
        }
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let at = format!("{pointer}/{index}");
                identity_fields(item, &at, in_service_accounts, named);
            }
        }
        serde_json::Value::Object(fields) => {
            // A label or tag may be named `role`, and no cloud reads an identity from a key in a
            // labels, tags or annotations map; only a value shaped like an identity counts there.
            let free_form = pointer
                .rsplit('/')
                .next()
                .is_some_and(|parent| matches!(parent, "labels" | "tags" | "Tags" | "annotations"));
            for (key, field) in fields {
                let at = pointer_to(pointer, key);
                if names_an_identity(key) {
                    named.insert((at.clone(), key.clone()));
                }
                if !free_form && (identity_field(key) || (in_service_accounts && key == "email")) {
                    every_leaf(field, &at, named);
                }
                let under = key.eq_ignore_ascii_case("serviceaccounts");
                identity_fields(field, &at, under, named);
            }
        }
        _ => {}
    }
}

/// Every value in `value` with its JSON pointer, objects and arrays as `{}` and `[]`, so a field
/// counts whatever order its keys are written in.
fn every_leaf(value: &serde_json::Value, pointer: &str, named: &mut BTreeSet<(String, String)>) {
    match value {
        serde_json::Value::Object(fields) => {
            named.insert((pointer.to_string(), "{}".to_string()));
            for (key, field) in fields {
                every_leaf(field, &pointer_to(pointer, key), named);
            }
        }
        serde_json::Value::Array(items) => {
            named.insert((pointer.to_string(), "[]".to_string()));
            for (index, item) in items.iter().enumerate() {
                every_leaf(item, &format!("{pointer}/{index}"), named);
            }
        }
        scalar => {
            named.insert((pointer.to_string(), scalar.to_string()));
        }
    }
}

/// The JSON pointer to `key` in the object at `pointer`.
fn pointer_to(pointer: &str, key: &str) -> String {
    format!("{pointer}/{}", key.replace('~', "~0").replace('/', "~1"))
}

/// Whether `changed` differs from `original` in at most labels added to the object's top-level
/// `metadata.labels`. A Kubernetes call is signed by an identity that reaches what every
/// deployment on the cluster shares, so nothing else of it may change.
fn only_labels_changed(original: &HookRequest, changed: &HookRequest) -> bool {
    if original.method == changed.method
        && original.url == changed.url
        && original.headers == changed.headers
        && original.body == changed.body
    {
        return true;
    }
    type Labels = serde_json::Map<String, serde_json::Value>;
    let split = |request: &HookRequest| -> Option<(serde_json::Value, Labels)> {
        let mut object = match request.body.as_deref() {
            None => return Some((serde_json::Value::Null, Labels::new())),
            Some(body) => serde_json::from_str::<serde_json::Value>(body).ok()?,
        };
        let labels = match object
            .get_mut("metadata")
            .and_then(|m| m.as_object_mut())
            .and_then(|metadata| metadata.remove("labels"))
        {
            None | Some(serde_json::Value::Null) => Labels::new(),
            Some(serde_json::Value::Object(labels))
                if labels.values().all(serde_json::Value::is_string) =>
            {
                labels
            }
            Some(_) => return None,
        };
        Some((object, labels))
    };
    let (Some((before, kept)), Some((after, labels))) = (split(original), split(changed)) else {
        return false;
    };
    // Alien and the cluster select and clean up objects by these labels (`alien.dev/deployment`,
    // `kubernetes.io/...`, `managed-by`), so an interceptor adds only other labels and leaves every
    // label Alien set as it was.
    let reserved = |key: &str| key.contains('/') || key == "managed-by";
    original.method == changed.method
        && original.url == changed.url
        && original.headers == changed.headers
        && original.body.is_some() == changed.body.is_some()
        && before == after
        && kept
            .iter()
            .all(|(key, value)| labels.get(key) == Some(value))
        && labels
            .keys()
            .all(|key| kept.contains_key(key) || !reserved(key))
}

/// Whether the request's body, when it has one, is JSON.
fn is_json(request: &HookRequest) -> bool {
    request
        .body
        .as_deref()
        .is_some_and(|body| serde_json::from_str::<serde_json::Value>(body).is_ok())
}

/// Every key and string value in `value`.
fn json_strings(value: &serde_json::Value, texts: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => texts.push(text.clone()),
        serde_json::Value::Array(items) => items.iter().for_each(|item| json_strings(item, texts)),
        serde_json::Value::Object(fields) => {
            for (key, field) in fields {
                texts.push(key.clone());
                json_strings(field, texts);
            }
        }
        _ => {}
    }
}

/// The operations an AWS call names: its `Action` parameters, in the query and in a form body in
/// any case, and its `X-Amz-Target` headers.
fn aws_operations(request: &HookRequest) -> Vec<String> {
    let body = request.body.as_deref().unwrap_or_default();
    let targets = request
        .headers
        .get_all("x-amz-target")
        .iter()
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned());
    request
        .url
        .query_pairs()
        .chain(form_urlencoded::parse(body.as_bytes()))
        .filter(|(name, _)| name.eq_ignore_ascii_case("action"))
        .map(|(_, value)| value.into_owned())
        .chain(targets)
        .collect()
}

/// The names of a URL's query parameters, each as often as it appears.
fn query_names(request: &HookRequest) -> Vec<String> {
    let mut names: Vec<String> = request
        .url
        .query_pairs()
        .map(|(name, _)| name.into_owned())
        .collect();
    names.sort();
    names
}

/// Hosts of the cloud registries a deployment's images can be pulled from with Alien's grants:
/// Amazon ECR in every endpoint form (IPv4, dual-stack, FIPS), Artifact Registry and its `gcr.io`
/// hosts, and Azure Container Registry.
const CLOUD_REGISTRIES: [&str; 5] = [".dkr.ecr", ".dkr-ecr", "pkg.dev", "gcr.io", ".azurecr."];

/// Every reference to an image in a cloud registry that `request` names, lowercase, wherever it
/// sits: in the query or the body, in any field, raw and percent-decoded. Alien sets up the
/// grants a deployment pulls those images with, so an interceptor may not add or change one.
fn registry_images(request: &HookRequest) -> BTreeSet<String> {
    let body = request.body.as_deref().unwrap_or_default();
    // A changed JSON body is checked as it is sent, written out again, so a host written with a
    // `\u` escape is read as the cloud reads it.
    let texts = [
        body.to_string(),
        decoded(body),
        decoded(request.url.query().unwrap_or_default()),
    ];
    texts
        .iter()
        .flat_map(|text| {
            text.split(|c: char| !(c.is_alphanumeric() || "@._:/+-~%".contains(c)))
                .map(|token| token.to_lowercase())
                .collect::<Vec<_>>()
        })
        // A reference spelled with a character outside ASCII counts too, whatever it may fold to.
        .filter(|token| {
            CLOUD_REGISTRIES.iter().any(|host| token.contains(host))
                || (!token.is_ascii() && token.contains('.') && token.contains('/'))
        })
        .collect()
}

/// Whether an AWS body sets a resource policy: a `Policy` field (KMS, EventBridge), a
/// `ResourcePolicy` field (DynamoDB), a `Policy` attribute (SQS, SNS) in JSON, or one named in a
/// form.
fn sets_a_policy(body: &str) -> bool {
    if let Ok(serde_json::Value::Object(fields)) = serde_json::from_str(body) {
        return fields.iter().any(|(key, field)| {
            key.eq_ignore_ascii_case("Policy")
                || key.eq_ignore_ascii_case("ResourcePolicy")
                || (key.eq_ignore_ascii_case("Attributes")
                    && field.as_object().is_some_and(|attributes| {
                        attributes
                            .keys()
                            .any(|name| name.eq_ignore_ascii_case("Policy"))
                    }))
        });
    }
    form_sets_a_policy(body)
}

/// Whether an API Gateway body writes the API's resource policy: a `policy` field, an OpenAPI
/// document's `x-amazon-apigateway-policy`, or a patch to `/policy`, at any depth and in any
/// case.
fn sets_an_api_policy(body: &str) -> bool {
    fn writes(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(fields) => fields.iter().any(|(key, field)| {
                matches!(
                    key.to_ascii_lowercase().as_str(),
                    "policy" | "x-amazon-apigateway-policy"
                ) || ((key.eq_ignore_ascii_case("path") || key.eq_ignore_ascii_case("from"))
                    && field
                        .as_str()
                        .is_some_and(|path| path.to_ascii_lowercase().starts_with("/policy")))
                    || writes(field)
            }),
            serde_json::Value::Array(items) => items.iter().any(writes),
            _ => false,
        }
    }
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(value) => writes(&value),
        // An OpenAPI document may be YAML.
        Err(_) => body.to_ascii_lowercase().contains("policy"),
    }
}

/// Whether a form, in a body or a URL's query, names a `Policy` field or attribute.
fn form_sets_a_policy(form: &str) -> bool {
    form_urlencoded::parse(form.as_bytes()).any(|(name, value)| {
        let name = name.to_ascii_lowercase();
        name == "policy"
            || (value.eq_ignore_ascii_case("Policy")
                && (name.ends_with("name") || name.ends_with(".key")))
    })
}

/// Rewrites the headers that describe the body, when the request carries them, so a signature
/// never covers a digest of a body that is no longer sent.
fn rewrite_body_digests(headers: &mut HeaderMap, body: &[u8]) -> Result<()> {
    if headers.contains_key(CONTENT_LENGTH) {
        headers.insert(CONTENT_LENGTH, HeaderValue::from(body.len()));
    }
    let sha256 = format!("{:x}", Sha256::digest(body));
    let md5 = base64::engine::general_purpose::STANDARD.encode(md5::compute(body).0);
    for (name, digest) in [("x-amz-content-sha256", sha256), ("content-md5", md5)] {
        if headers.contains_key(name) {
            let value = HeaderValue::try_from(digest).into_alien_error().context(
                ErrorData::GenericError {
                    message: format!("Failed to write the '{name}' header"),
                },
            )?;
            headers.insert(name, value);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestInterceptor;
    use std::sync::Mutex;

    fn chain(interceptors: Vec<TestInterceptor>) -> InterceptorChain {
        InterceptorChain::new(
            interceptors
                .into_iter()
                .map(|interceptor| Arc::new(interceptor) as Arc<dyn CallInterceptor>)
                .collect(),
        )
    }

    fn rds_request(body: &str) -> reqwest::Request {
        reqwest::Client::new()
            .post("https://rds.us-east-1.amazonaws.com/")
            .header("content-type", "application/x-www-form-urlencoded")
            .header("content-length", body.len())
            .header(
                "x-amz-content-sha256",
                format!("{:x}", Sha256::digest(body)),
            )
            .header("content-md5", "stale")
            .body(body.to_string())
            .build()
            .unwrap()
    }

    fn body_text(request: &reqwest::Request) -> String {
        String::from_utf8(request.body().unwrap().as_bytes().unwrap().to_vec()).unwrap()
    }

    fn assert_fails_naming(error: AlienError<ErrorData>, extension: &str) {
        assert_eq!(error.code, "INTERCEPTOR_FAILED", "{error}");
        assert!(
            !error.retryable,
            "an interceptor failure is not retried unless the interceptor says it may pass"
        );
        assert!(
            matches!(&error.error, Some(ErrorData::InterceptorFailed { extension: name, .. }) if name == extension),
            "{error}"
        );
    }

    #[test]
    fn a_call_no_interceptor_matches_is_left_as_it_was() {
        let interceptors = chain(vec![TestInterceptor::new("acme", Cloud::Aws, "s3")
            .on_request(|_| panic!("an S3 interceptor never sees an RDS call"))]);
        let mut request = reqwest::Client::new()
            .put("https://bucket.s3.amazonaws.com/object")
            .body(vec![0xff, 0xfe, 0x00])
            .build()
            .unwrap();
        let before = format!("{request:?}");
        assert!(interceptors
            .intercept(Cloud::Aws, "rds", &mut request)
            .unwrap()
            .is_none());
        assert_eq!(format!("{request:?}"), before);
        assert_eq!(
            request.body().unwrap().as_bytes(),
            Some(&[0xff, 0xfe, 0x00][..])
        );
    }

    /// An interceptor that can't say whether it sees a call, as a hook that traps or runs out of
    /// time can't.
    struct CannotMatch;

    impl CallInterceptor for CannotMatch {
        fn extension(&self) -> &str {
            "acme"
        }

        fn matches(&self, _request: &HookRequest) -> Result<bool> {
            Err(AlienError::new(ErrorData::GenericError {
                message: "the hook ran out of time".to_string(),
            }))
        }

        fn modify_request(&self, _request: &mut HookRequest) -> Result<()> {
            panic!("a call the interceptor did not match is never offered to it")
        }

        fn modify_response(&self, _: &HookRequest, _: &mut HookResponse) -> Result<()> {
            panic!("a call the interceptor did not match is never offered to it")
        }
    }

    #[test]
    fn an_interceptor_that_cannot_answer_matches_fails_the_call() {
        let interceptors = InterceptorChain::new(vec![Arc::new(CannotMatch)]);
        let mut text = rds_request("Action=CreateDBCluster");
        let error = interceptors
            .intercept(Cloud::Aws, "rds", &mut text)
            .unwrap_err();
        assert_fails_naming(error, "acme");
        assert_eq!(body_text(&text), "Action=CreateDBCluster");

        let mut binary = reqwest::Client::new()
            .put("https://bucket.s3.amazonaws.com/object")
            .body(vec![0xff, 0xfe, 0x00])
            .build()
            .unwrap();
        let error = interceptors
            .intercept(Cloud::Aws, "s3", &mut binary)
            .unwrap_err();
        assert_fails_naming(error, "acme");
    }

    #[test]
    fn a_change_of_destination_or_credentials_is_refused() {
        let cases: Vec<(&str, fn(&mut HookRequest))> = vec![
            ("another host", |r| {
                r.url.set_host(Some("evil.example.com")).unwrap()
            }),
            ("another port", |r| r.url.set_port(Some(8443)).unwrap()),
            ("another scheme", |r| r.url.set_scheme("http").unwrap()),
            ("a Host header", |r| {
                r.headers
                    .insert("host", HeaderValue::from_static("evil.example.com"));
            }),
            ("an Authorization header", |r| {
                r.headers
                    .insert("authorization", HeaderValue::from_static("Bearer forged"));
            }),
            ("a session token", |r| {
                r.headers
                    .insert("x-amz-security-token", HeaderValue::from_static("stolen"));
            }),
            ("a signing date", |r| {
                r.headers
                    .insert("x-amz-date", HeaderValue::from_static("20000101T000000Z"));
            }),
            ("the signing service", |r| r.service = "ec2".to_string()),
            ("the cloud", |r| r.cloud = Cloud::Gcp),
        ];
        for (what, change) in cases {
            let interceptors = chain(vec![TestInterceptor::new("acme", Cloud::Aws, "rds")
                .on_request(move |r| {
                    change(r);
                    Ok(())
                })]);
            let mut request = rds_request("Action=CreateDBCluster");
            let error = interceptors
                .intercept(Cloud::Aws, "rds", &mut request)
                .expect_err(what);
            assert_fails_naming(error, "acme");
            assert_eq!(
                request.url().host_str(),
                Some("rds.us-east-1.amazonaws.com")
            );
        }
    }

    /// Headers that pick the project billed, the method the server runs or the object an S3 copy
    /// reads, add a credential, or set who the call acts as.
    #[test]
    fn a_header_that_picks_the_project_or_identity_is_refused() {
        use reqwest::header::HeaderName;

        let cases = [
            (
                Cloud::Gcp,
                "cloudsql",
                "https://sqladmin.googleapis.com/v1/projects/p/instances",
                "x-goog-user-project",
            ),
            (
                Cloud::Gcp,
                "cloudsql",
                "https://sqladmin.googleapis.com/v1/projects/p/instances",
                "x-goog-api-key",
            ),
            (
                Cloud::Azure,
                "management",
                "https://management.azure.com/subscriptions/sub",
                "x-ms-authorization-auxiliary",
            ),
            (
                Cloud::Gcp,
                "cloudsql",
                "https://sqladmin.googleapis.com/v1/projects/p/instances",
                "X-HTTP-Method-Override",
            ),
            (
                Cloud::Azure,
                "management",
                "https://management.azure.com/subscriptions/sub",
                "X-HTTP-Method",
            ),
            (
                Cloud::Kubernetes,
                "kubernetes",
                "https://cluster.local/api/v1/namespaces/ns/pods",
                "Impersonate-User",
            ),
            (
                Cloud::Kubernetes,
                "kubernetes",
                "https://cluster.local/api/v1/namespaces/ns/pods",
                "Impersonate-Extra-Scopes",
            ),
            (
                Cloud::Aws,
                "s3",
                "https://bucket.s3.amazonaws.com/copy",
                "x-amz-copy-source",
            ),
        ];
        for (cloud, service, url, name) in cases {
            let header = HeaderName::from_bytes(name.as_bytes()).unwrap();
            for already_sent in [false, true] {
                let set = header.clone();
                let interceptors = chain(vec![TestInterceptor::new("acme", cloud, service)
                    .on_request(move |r| {
                        r.headers
                            .insert(set.clone(), HeaderValue::from_static("other"));
                        Ok(())
                    })]);
                let mut request = reqwest::Client::new().get(url);
                if already_sent {
                    request = request.header(header.clone(), "original");
                }
                let mut request = request.build().unwrap();
                let error = interceptors
                    .intercept(cloud, service, &mut request)
                    .expect_err(name);
                assert_fails_naming(error, "acme");
                assert_eq!(
                    request.headers().get(&header).is_some(),
                    already_sent,
                    "{name}"
                );
            }
        }
    }

    /// Google Cloud reads the project billed, an API key, a credential and the body's or
    /// response's format from these parameters, in any case.
    #[test]
    fn a_query_parameter_that_picks_the_project_billed_or_an_api_key_is_refused() {
        let url = "https://storage.googleapis.com/storage/v1/b/bucket/o?userProject=the-project";
        for (what, change) in [
            ("added", "userProject=the-project&userProject=other"),
            ("changed", "userProject=other"),
            ("removed", "prefix=a"),
            (
                "added in another case",
                "userProject=the-project&USERPROJECT=other",
            ),
            (
                "added with a $",
                "userProject=the-project&%24userProject=other",
            ),
            ("an API key", "userProject=the-project&key=other"),
            ("an API key with a $", "userProject=the-project&$Key=other"),
            (
                "an access token",
                "userProject=the-project&access_token=other",
            ),
            (
                "an OAuth token",
                "userProject=the-project&oauth_token=other",
            ),
            (
                "a bearer token",
                "userProject=the-project&bearer_token=other",
            ),
            ("a response format", "userProject=the-project&alt=media"),
            (
                "a content type for the body",
                "userProject=the-project&%24ct=application%2Fjson%2Bprotobuf",
            ),
            (
                "another system parameter",
                "userProject=the-project&$Fields=name",
            ),
        ] {
            let interceptors = chain(vec![TestInterceptor::new("acme", Cloud::Gcp, "storage")
                .on_request(move |r| {
                    r.url.set_query(Some(change));
                    Ok(())
                })]);
            let mut request = reqwest::Client::new().get(url).build().unwrap();
            let error = interceptors
                .intercept(Cloud::Gcp, "storage", &mut request)
                .expect_err(what);
            assert_fails_naming(error, "acme");
            assert_eq!(request.url().as_str(), url, "{what}");
        }

        // Every other parameter can change, as can these on another cloud's call.
        for (cloud, service, url, query) in [
            (
                Cloud::Gcp,
                "storage",
                url,
                "prefix=a&userProject=the-project",
            ),
            (
                Cloud::Aws,
                "rds",
                "https://rds.us-east-1.amazonaws.com/?key=x&userProject=y",
                "key=a&userProject=b",
            ),
        ] {
            let interceptors = chain(vec![TestInterceptor::new("acme", cloud, service)
                .on_request(move |r| {
                    r.url.set_query(Some(query));
                    Ok(())
                })]);
            let mut request = reqwest::Client::new().get(url).build().unwrap();
            interceptors
                .intercept(cloud, service, &mut request)
                .unwrap()
                .expect("the interceptor matched");
            assert_eq!(request.url().query(), Some(query));
        }
    }

    #[test]
    fn a_query_value_and_other_headers_can_change() {
        let interceptors = chain(vec![TestInterceptor::new("acme", Cloud::Aws, "rds")
            .on_request(|r| {
                r.url.set_query(Some("extension=1"));
                r.headers
                    .insert("x-extension", HeaderValue::from_static("acme"));
                Ok(())
            })]);
        let mut request = rds_request("Action=CreateDBCluster");
        request.url_mut().set_query(Some("extension=0"));
        interceptors
            .intercept(Cloud::Aws, "rds", &mut request)
            .unwrap()
            .expect("the interceptor matched");
        assert_eq!(
            request.url().as_str(),
            "https://rds.us-east-1.amazonaws.com/?extension=1"
        );
        assert_eq!(request.method(), Method::POST);
        assert_eq!(request.headers()["x-extension"], "acme");
        assert_eq!(body_text(&request), "Action=CreateDBCluster");
    }

    /// A request `(cloud, service, method, url, headers, body)` as Alien builds it.
    type Built = (
        Cloud,
        &'static str,
        Method,
        &'static str,
        &'static [(&'static str, &'static str)],
        Option<&'static str>,
    );

    fn build((_, _, method, url, headers, body): &Built) -> reqwest::Request {
        let mut builder = reqwest::Client::new().request(method.clone(), *url);
        for (name, value) in *headers {
            builder = builder.header(*name, *value);
        }
        if let Some(body) = body {
            builder = builder.body(*body);
        }
        builder.build().unwrap()
    }

    /// Runs `change` on `built` and expects the chain to refuse it before anything is sent.
    fn assert_change_refused(what: &str, built: Built, change: fn(&mut HookRequest)) {
        let (cloud, service) = (built.0, built.1);
        let interceptors = chain(vec![TestInterceptor::new("acme", cloud, service)
            .on_request(move |r| {
                change(r);
                Ok(())
            })]);
        let mut request = build(&built);
        let before = format!("{request:?}");
        let error = interceptors
            .intercept(cloud, service, &mut request)
            .expect_err(what);
        assert_fails_naming(error, "acme");
        assert_eq!(format!("{request:?}"), before, "{what}");
    }

    const CLOUD_RUN_SERVICE: Built = (
        Cloud::Gcp,
        "cloudrun",
        Method::PATCH,
        "https://run.googleapis.com/v2/projects/p/locations/l/services/web",
        &[("content-type", "application/json")],
        Some(r#"{"labels":{"team":"a"}}"#),
    );

    /// An image in a cloud registry stays as Alien named it, however the change names one; an
    /// image from a public registry may be added.
    #[test]
    fn an_image_in_a_cloud_registry_is_never_added_or_changed() {
        const FUNCTION: Built = (
            Cloud::Aws,
            "lambda",
            Method::POST,
            "https://lambda.us-east-1.amazonaws.com/2015-03-31/functions",
            &[("content-type", "application/json")],
            Some(
                r#"{"FunctionName":"f","Code":{"ImageUri":"1.dkr.ecr.us-east-1.amazonaws.com/artifacts-a@sha256:00"}}"#,
            ),
        );
        const SERVICE: Built = (
            Cloud::Gcp,
            "run",
            Method::POST,
            "https://run.googleapis.com/v2/projects/p/locations/l/services",
            &[("content-type", "application/json")],
            Some(
                r#"{"template":{"containers":[{"image":"us-docker.pkg.dev/m/artifacts-a/web:1"}]}}"#,
            ),
        );
        const TEMPLATE: Built = (
            Cloud::Gcp,
            "aiplatform",
            Method::POST,
            "https://us-central1-aiplatform.googleapis.com/v1beta1/projects/p/locations/l/templates",
            &[("content-type", "application/json")],
            Some(r#"{"spec":{"imageUri":"docker.io/library/python:3.12"}}"#),
        );
        const SETTINGS: Built = (
            Cloud::Aws,
            "ssm",
            Method::POST,
            "https://ssm.us-east-1.amazonaws.com/",
            &[
                ("content-type", "application/x-amz-json-1.1"),
                ("x-amz-target", "AmazonSSM.PutParameter"),
            ],
            Some(r#"{"value":"REGISTRY=1.dkr.ecr.us-east-1.amazonaws.com/r:1,TAG=1"}"#),
        );
        let refused: [(&str, Built, fn(&mut HookRequest)); 11] = [
            (
                "a registry host with a character outside ASCII",
                FUNCTION,
                |r| {
                    r.body = Some(
                    "{\"FunctionName\":\"f\",\"Code\":{\"ImageUri\":\"1.d\u{212a}r.ecr.us-east-1.amazonaws.com/artifacts-b:1\"}}"
                        .into(),
                )
                },
            ),
            ("a FIPS registry host", FUNCTION, |r| {
                r.body = Some(
                    r#"{"FunctionName":"f","Code":{"ImageUri":"1.dkr.ecr-fips.us-east-1.amazonaws.com/artifacts-b:1"}}"#
                        .into(),
                )
            }),
            ("a dual-stack FIPS registry host", FUNCTION, |r| {
                r.body = Some(
                    r#"{"FunctionName":"f","Code":{"ImageUri":"1.dkr-ecr-fips.us-east-1.on.aws/artifacts-b:1"}}"#
                        .into(),
                )
            }),
            ("a gcr.io host", SERVICE, |r| {
                r.body = Some(
                    r#"{"template":{"containers":[{"image":"us-docker.pkg.dev/m/artifacts-a/web:1"},{"image":"us.gcr.io/m/artifacts-b:1"}]}}"#
                        .into(),
                )
            }),
            ("a regional Artifact Registry host", SERVICE, |r| {
                r.body = Some(
                    r#"{"template":{"containers":[{"image":"docker.us-east1.rep.pkg.dev/m/artifacts-b/web:1"}]}}"#
                        .into(),
                )
            }),
            ("another repository", FUNCTION, |r| {
                r.body = Some(
                    r#"{"FunctionName":"f","Code":{"ImageUri":"1.dkr.ecr.us-east-1.amazonaws.com/artifacts-b:latest"}}"#
                        .into(),
                )
            }),
            ("another tag of the same repository", FUNCTION, |r| {
                r.body = Some(
                    r#"{"FunctionName":"f","Code":{"ImageUri":"1.dkr.ecr.us-east-1.amazonaws.com/artifacts-a:v2"}}"#
                        .into(),
                )
            }),
            (
                "the registry host in another case and with a port",
                FUNCTION,
                |r| {
                    r.body = Some(
                    r#"{"FunctionName":"f","Code":{"ImageUri":"1.DKR.ECR.us-east-1.amazonaws.com:443/artifacts-b:1"}}"#
                        .into(),
                )
                },
            ),
            ("a second container from the same registry", SERVICE, |r| {
                r.body = Some(
                    r#"{"template":{"containers":[{"image":"us-docker.pkg.dev/m/artifacts-a/web:1"},{"image":"us-docker.pkg.dev/m/artifacts-b/web:1"}]}}"#
                        .into(),
                )
            }),
            (
                "a cloud registry's image in a field of another name",
                TEMPLATE,
                |r| {
                    r.body = Some(
                    r#"{"spec":{"image_uri":"us-central1-docker.pkg.dev/m/artifacts-b/sandbox:1"}}"#
                        .into(),
                )
                },
            ),
            (
                "a cloud registry's image where the call named a public one",
                TEMPLATE,
                |r| {
                    r.body = Some(
                    r#"{"spec":{"imageUri":"us-central1-docker.pkg.dev/m/artifacts-b/sandbox:1"}}"#
                        .into(),
                )
                },
            ),
        ];
        for (what, built, change) in refused {
            assert_change_refused(what, built, change);
        }
        let sent: [(&str, Built, fn(&mut HookRequest)); 3] = [
            ("a setting beside an image in one string", SETTINGS, |r| {
                r.body = Some(
                    r#"{"value":"REGISTRY=1.dkr.ecr.us-east-1.amazonaws.com/r:1,TAG=2"}"#.into(),
                )
            }),
            ("another field beside the image", FUNCTION, |r| {
                r.body = Some(
                    r#"{"FunctionName":"f","Code":{"ImageUri":"1.dkr.ecr.us-east-1.amazonaws.com/artifacts-a@sha256:00"},"Description":"d"}"#
                        .into(),
                )
            }),
            ("a sidecar from a public registry", SERVICE, |r| {
                r.body = Some(
                    r#"{"template":{"containers":[{"image":"us-docker.pkg.dev/m/artifacts-a/web:1"},{"image":"ghcr.io/vendor/agent:1"}]}}"#
                        .into(),
                )
            }),
        ];
        for (what, built, change) in sent {
            assert_change_sent(what, built, change);
        }
    }

    /// The method and the path say which resource a call acts on and what it does to it.
    #[test]
    fn a_change_of_method_or_path_is_refused() {
        let cases: [(&str, fn(&mut HookRequest)); 4] = [
            ("another method", |r| r.method = Method::DELETE),
            ("a sibling service", |r| {
                r.url
                    .set_path("/v2/projects/p/locations/l/services/billing")
            }),
            ("a trailing slash", |r| {
                r.url.set_path("/v2/projects/p/locations/l/services/web/")
            }),
            ("another project", |r| {
                r.url.set_path("/v2/projects/q/locations/l/services/web")
            }),
        ];
        for (what, change) in cases {
            assert_change_refused(what, CLOUD_RUN_SERVICE, change);
        }
    }

    /// Which identity a call names decides who the resource acts as, wherever the name sits.
    #[test]
    fn a_change_to_the_identity_a_call_names_is_refused() {
        const LAMBDA: Built = (
            Cloud::Aws,
            "lambda",
            Method::PUT,
            "https://lambda.us-east-1.amazonaws.com/2015-03-31/functions/fn/configuration",
            &[("content-type", "application/json")],
            Some(r#"{"Role":"arn:aws:iam::111111111111:role/app-fn","Timeout":30}"#),
        );
        const APP: Built = (
            Cloud::Azure,
            "management",
            Method::PUT,
            "https://management.azure.com/subscriptions/s/resourceGroups/rg/providers/Microsoft.App/containerApps/web",
            &[("content-type", "application/json")],
            Some(r#"{"location":"eastus"}"#),
        );
        let cases: [(&str, Built, fn(&mut HookRequest)); 8] = [
            ("a body that is no longer JSON", LAMBDA, |r| {
                r.body = Some("Role: arn:aws:iam::111111111111:role/admin\n".into())
            }),
            ("a body no JSON parser reads alike", LAMBDA, |r| {
                r.body = Some(
                    r#"{"Description":"\ud800","Role":"arn:aws:iam::111111111111:role/admin","Timeout":30}"#
                        .into(),
                )
            }),
            ("another content type", LAMBDA, |r| {
                r.headers
                    .insert("content-type", HeaderValue::from_static("application/yaml"));
            }),
            ("an escaped role", LAMBDA, |r| {
                r.body = Some(
                    r#"{"Role":"\u0061rn:aws:iam::111111111111:role/admin","Description":"arn:aws:iam::111111111111:role/app-fn","Timeout":30}"#
                        .into(),
                )
            }),
            ("another service account", CLOUD_RUN_SERVICE, |r| {
                r.body = Some(
                    r#"{"labels":{"team":"a"},"template":{"serviceAccount":"other@p.iam.gserviceaccount.com"}}"#
                        .into(),
                )
            }),
            ("another role", LAMBDA, |r| {
                r.body =
                    Some(r#"{"Role":"arn:aws:iam::111111111111:role/admin","Timeout":30}"#.into())
            }),
            ("no role", LAMBDA, |r| {
                r.body = Some(r#"{"Timeout":30}"#.into())
            }),
            ("a managed identity added", APP, |r| {
                r.body = Some(
                    r#"{"location":"eastus","identity":{"type":"UserAssigned","userAssignedIdentities":{"/subscriptions/s/resourceGroups/other/providers/Microsoft.ManagedIdentity/userAssignedIdentities/admin":{}}}}"#
                        .into(),
                )
            }),
        ];
        for (what, built, change) in cases {
            assert_change_refused(what, built, change);
        }
    }

    const LAUNCH_TEMPLATE: Built = (
        Cloud::Aws,
        "ec2",
        Method::POST,
        "https://ec2.us-east-1.amazonaws.com/",
        &[("content-type", "application/x-www-form-urlencoded")],
        Some("Action=CreateLaunchTemplate&LaunchTemplateName=web&LaunchTemplateData.IamInstanceProfile.Arn=arn%3Aaws%3Aiam%3A%3A111111111111%3Ainstance-profile%2Fweb&LaunchTemplateData.IamInstanceProfile.Name=web"),
    );

    const CLOUD_RUN_AS_APP: Built = (
        Cloud::Gcp,
        "cloudrun",
        Method::PATCH,
        "https://run.googleapis.com/v2/projects/p/locations/l/services/web",
        &[("content-type", "application/json")],
        Some(
            r#"{"labels":{"team":"a"},"template":{"serviceAccount":"app@p.iam.gserviceaccount.com"}}"#,
        ),
    );

    const INSTANCE_AS_APP: Built = (
        Cloud::Gcp,
        "compute",
        Method::POST,
        "https://compute.googleapis.com/compute/v1/projects/p/zones/z/instances",
        &[("content-type", "application/json")],
        Some(
            r#"{"name":"web","description":"","serviceAccounts":[{"email":"app@p.iam.gserviceaccount.com","scopes":[]}]}"#,
        ),
    );

    /// An identity is compared where it sits: a form value is read by its key, and a name moved
    /// to another field no longer names the identity the call acts as.
    #[test]
    fn an_identity_is_compared_where_the_call_names_it() {
        let cases: [(&str, Built, fn(&mut HookRequest)); 5] = [
            (
                "another instance profile ARN in a form",
                LAUNCH_TEMPLATE,
                |r| {
                    r.body = Some(
                        LAUNCH_TEMPLATE
                            .5
                            .unwrap()
                            .replace("profile%2Fweb", "profile%2Fadmin"),
                    )
                },
            ),
            (
                "another instance profile name in a form",
                LAUNCH_TEMPLATE,
                |r| {
                    r.body = Some(
                        LAUNCH_TEMPLATE
                            .5
                            .unwrap()
                            .replace("Profile.Name=web", "Profile.Name=admin"),
                    )
                },
            ),
            (
                "an instance profile ARN in the query",
                LAUNCH_TEMPLATE,
                |r| {
                    r.url
                        .set_query(Some("RoleArn=arn:aws:iam::111111111111:role/admin"))
                },
            ),
            (
                "a service account dropped and kept in a label",
                CLOUD_RUN_AS_APP,
                |r| {
                    r.body = Some(
                    r#"{"labels":{"team":"a","sa":"app@p.iam.gserviceaccount.com"},"template":{}}"#
                        .into(),
                )
                },
            ),
            (
                "a service account replaced and kept in the description",
                INSTANCE_AS_APP,
                |r| {
                    r.body = Some(
                    r#"{"name":"web","description":"app@p.iam.gserviceaccount.com","serviceAccounts":[{"email":"default","scopes":[]}]}"#
                        .into(),
                )
                },
            ),
        ];
        for (what, built, change) in cases {
            assert_change_refused(what, built, change);
        }
    }

    /// Runs `change` on `built` and expects the chain to send the changed call.
    fn assert_change_sent(what: &str, built: Built, change: fn(&mut HookRequest)) {
        let (cloud, service) = (built.0, built.1);
        let interceptors = chain(vec![TestInterceptor::new("acme", cloud, service)
            .on_request(move |r| {
                change(r);
                Ok(())
            })]);
        let mut request = build(&built);
        interceptors
            .intercept(cloud, service, &mut request)
            .unwrap_or_else(|error| panic!("{what}: {error}"))
            .expect("the interceptor matched");
        assert_ne!(Some(body_text(&request).as_str()), built.5, "{what}");
    }

    #[test]
    fn a_change_that_keeps_every_identity_where_it_was_is_sent() {
        const APP_AS_APP: Built = (
            Cloud::Azure,
            "management",
            Method::PUT,
            "https://management.azure.com/subscriptions/s/resourceGroups/rg/providers/Microsoft.App/containerApps/web",
            &[("content-type", "application/json")],
            Some(r#"{"location":"eastus","identity":{"type":"UserAssigned","userAssignedIdentities":{"/subscriptions/s/resourceGroups/rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/app":{}}}}"#),
        );
        let cases: [(&str, Built, fn(&mut HookRequest)); 4] = [
            (
                "a tag on a container app, its identity's keys written in another order",
                APP_AS_APP,
                |r| {
                    r.body = Some(
                    r#"{"tags":{"team":"a"},"identity":{"userAssignedIdentities":{"/subscriptions/s/resourceGroups/rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/app":{}},"type":"UserAssigned"},"location":"eastus"}"#
                        .into(),
                )
                },
            ),
            ("a tag on a launch template", LAUNCH_TEMPLATE, |r| {
                r.body.as_mut().unwrap().push_str(
                    "&TagSpecification.1.ResourceType=launch-template&TagSpecification.1.Tag.1.Key=team&TagSpecification.1.Tag.1.Value=a",
                )
            }),
            ("a label on a Cloud Run service", CLOUD_RUN_AS_APP, |r| {
                r.body = Some(
                    r#"{"labels":{"team":"b"},"template":{"serviceAccount":"app@p.iam.gserviceaccount.com"}}"#
                        .into(),
                )
            }),
            ("a description on an instance", INSTANCE_AS_APP, |r| {
                r.body = Some(
                    r#"{"name":"web","description":"web tier","serviceAccounts":[{"email":"app@p.iam.gserviceaccount.com","scopes":[]}]}"#
                        .into(),
                )
            }),
        ];
        for (what, built, change) in cases {
            assert_change_sent(what, built, change);
        }
    }

    const DEPLOYMENT: Built = (
        Cloud::Kubernetes,
        "kubernetes",
        Method::POST,
        "https://cluster.example.com/apis/apps/v1/namespaces/app/deployments",
        &[("content-type", "application/json")],
        Some(
            r#"{"metadata":{"name":"web","labels":{"alien.dev/deployment":"d1"}},"spec":{"template":{"spec":{"containers":[{"name":"main","image":"web","securityContext":{"runAsNonRoot":true}}],"volumes":[{"name":"data","emptyDir":{}}]}}}}"#,
        ),
    );

    /// On Kubernetes an interceptor adds or changes the object's labels, and nothing else: the
    /// cluster's shared parts (ingress hosts, runtime classes, nodes) read the rest.
    #[test]
    fn a_kubernetes_interceptor_changes_only_the_object_s_labels() {
        assert_change_sent("a label", DEPLOYMENT, |r| {
            let mut value: serde_json::Value =
                serde_json::from_str(r.body.as_deref().unwrap()).unwrap();
            value["metadata"]["labels"]["team"] = "a".into();
            value["metadata"]["labels"]["role"] = "frontend".into();
            r.body = Some(value.to_string());
        });
        fn edit(r: &mut HookRequest, change: fn(&mut serde_json::Value)) {
            let mut value: serde_json::Value =
                serde_json::from_str(r.body.as_deref().unwrap()).unwrap();
            change(&mut value);
            r.body = Some(value.to_string());
        }
        let cases: [(&str, fn(&mut HookRequest)); 12] = [
            ("a sidecar", |r| {
                edit(r, |object| {
                    object["spec"]["template"]["spec"]["containers"]
                        .as_array_mut()
                        .unwrap()
                        .insert(0, serde_json::json!({"name":"agent","image":"agent"}))
                })
            }),
            ("an annotation", |r| {
                edit(
                    r,
                    |object| {
                        object["metadata"]["annotations"] =
                            serde_json::json!({"nginx.ingress.kubernetes.io/server-snippet": "x"})
                    },
                )
            }),
            ("a runtime class", |r| {
                edit(r, |object| {
                    object["spec"]["template"]["spec"]["runtimeClassName"] = "runc".into()
                })
            }),
            ("a label with a prefix", |r| {
                edit(r, |object| {
                    object["metadata"]["labels"]["alien.dev/resource"] = "other".into()
                })
            }),
            ("a label Alien set, changed", |r| {
                edit(r, |object| {
                    object["metadata"]["labels"]["alien.dev/deployment"] = "d2".into()
                })
            }),
            ("a label Alien set, removed", |r| {
                edit(r, |object| {
                    object["metadata"]["labels"] = serde_json::json!({})
                })
            }),
            ("a managed-by label", |r| {
                edit(r, |object| {
                    object["metadata"]["labels"]["managed-by"] = "acme".into()
                })
            }),
            ("a header", |r| {
                r.headers
                    .insert("x-extension", HeaderValue::from_static("acme"));
            }),
            ("another method", |r| r.method = Method::PUT),
            ("a label that is not text", |r| {
                edit(r, |object| {
                    object["metadata"]["labels"] = serde_json::json!({"a": 1})
                })
            }),
            ("a label on the pod template", |r| {
                edit(r, |object| {
                    object["spec"]["template"]["metadata"] =
                        serde_json::json!({"labels": {"a": "b"}})
                })
            }),
            ("a query parameter", |r| r.url.set_query(Some("dryRun=All"))),
        ];
        for (what, change) in cases {
            assert_change_refused(what, DEPLOYMENT, change);
        }
    }

    /// Each check reads the body as its protocol reads it, so a body may not change format: a
    /// form body turned into a JSON string would be checked as a form and sent decoded.
    #[test]
    fn a_body_that_changes_format_is_refused_saying_so() {
        const SQS: Built = (
            Cloud::Aws,
            "sqs",
            Method::POST,
            "https://sqs.us-east-1.amazonaws.com/",
            &[("content-type", "application/x-www-form-urlencoded")],
            Some("Action=CreateQueue&Version=2012-11-05&QueueName=q"),
        );
        const APP: Built = (
            Cloud::Azure,
            "management",
            Method::PUT,
            "https://management.azure.com/subscriptions/s/resourceGroups/rg/providers/Microsoft.App/containerApps/web",
            &[("content-type", "application/json")],
            Some(r#"{"location":"eastus"}"#),
        );
        const BODYLESS_SQS: Built = (
            Cloud::Aws,
            "sqs",
            Method::POST,
            "https://sqs.us-east-1.amazonaws.com/",
            &[("content-type", "application/x-www-form-urlencoded")],
            None,
        );
        let cases: [(&str, Built, fn(&mut HookRequest), &str); 5] = [
            (
                "an escaped form added to a call with no body",
                BODYLESS_SQS,
                |r| r.body = Some(r#""x\u0026Action\u003dAddPermission\u0026Label\u003dl""#.into()),
                "changes who has access",
            ),
            (
                "a policy as a form inside a JSON object added to a call with no body",
                BODYLESS_SQS,
                |r| {
                    r.body = Some(
                        r#"{"a":"x&Attribute.1.Name=Policy&Attribute.1.Value=%7B%7D&y="}"#.into(),
                    )
                },
                "added a body",
            ),
            (
                "an identity as a form inside a JSON string added to a call with no body",
                BODYLESS_SQS,
                |r| r.body = Some(r#""x&IamInstanceProfile.Name=admin&y=""#.into()),
                "added a body",
            ),
            (
                "a form body turned into a JSON string",
                SQS,
                |r| {
                    r.body = Some(
                        r#""Action=CreateQueue&Version=2012-11-05&QueueName=q&Attribute.1.Name\u003dPolicy""#
                            .into(),
                    )
                },
                "not JSON into JSON",
            ),
            (
                "a JSON body turned into text",
                APP,
                |r| r.body = Some("location: eastus\n".into()),
                "into one that is not JSON",
            ),
        ];
        for (what, built, change, reason) in cases {
            let (cloud, service) = (built.0, built.1);
            let interceptors = chain(vec![TestInterceptor::new("acme", cloud, service)
                .on_request(move |r| {
                    change(r);
                    Ok(())
                })]);
            let mut request = build(&built);
            let error = interceptors
                .intercept(cloud, service, &mut request)
                .expect_err(what);
            assert!(!error.retryable, "{what}");
            assert!(format!("{error:?}").contains(reason), "{what}: {error:?}");
        }
    }

    /// The body sent is the one the checks read, so a key written twice can't carry a change
    /// a cloud's parser would merge in.
    #[test]
    fn a_json_body_is_sent_as_it_was_checked() {
        let interceptors = chain(vec![TestInterceptor::new("acme", Cloud::Gcp, "cloudrun")
            .on_request(|r| {
                r.body = Some(r#"{"labels":{"team":"a"},"labels":{"team":"b"}}"#.to_string());
                Ok(())
            })]);
        let mut request = build(&CLOUD_RUN_SERVICE);
        interceptors
            .intercept(Cloud::Gcp, "cloudrun", &mut request)
            .unwrap()
            .expect("the interceptor matched");
        assert_eq!(body_text(&request), r#"{"labels":{"team":"b"}}"#);
    }

    /// On AWS's query and JSON protocols the operation is a body or query parameter, or a
    /// header, not the path.
    #[test]
    fn a_change_of_the_aws_operation_is_refused() {
        const RDS: Built = (
            Cloud::Aws,
            "rds",
            Method::POST,
            "https://rds.us-east-1.amazonaws.com/",
            &[("content-type", "application/x-www-form-urlencoded")],
            Some("Action=ModifyDBCluster&Version=2014-10-31&DBClusterIdentifier=db"),
        );
        const KMS: Built = (
            Cloud::Aws,
            "kms",
            Method::POST,
            "https://kms.us-east-1.amazonaws.com/",
            &[
                ("content-type", "application/x-amz-json-1.1"),
                ("x-amz-target", "TrentService.DescribeKey"),
            ],
            Some(r#"{"KeyId":"k"}"#),
        );
        const TAGGING: Built = (
            Cloud::Aws,
            "s3",
            Method::PUT,
            "https://bucket.s3.amazonaws.com/?tagging",
            &[("content-type", "application/xml")],
            Some("<Tagging><TagSet/></Tagging>"),
        );
        const CREATE_API: Built = (
            Cloud::Aws,
            "apigateway",
            Method::POST,
            "https://apigateway.us-east-1.amazonaws.com/restapis",
            &[("content-type", "application/json")],
            Some(r#"{"name":"api"}"#),
        );
        let cases: [(&str, Built, fn(&mut HookRequest)); 10] = [
            ("a query parameter added to a query API's call", RDS, |r| {
                r.url.set_query(Some("Tags.Tag.1.Key=a"))
            }),
            ("an API created by importing a document", CREATE_API, |r| {
                r.url.set_query(Some("mode=import"))
            }),
            ("another Action in the body", RDS, |r| {
                r.body = Some("Action=DeleteDBCluster&Version=2014-10-31".into())
            }),
            ("a second Action in the body", RDS, |r| {
                r.body.as_mut().unwrap().push_str("&Action=DeleteDBCluster")
            }),
            ("an Action in the query", RDS, |r| {
                r.url.set_query(Some("Action=DeleteDBCluster"))
            }),
            (
                "an S3 subresource that sends the bucket's logs elsewhere",
                TAGGING,
                |r| r.url.set_query(Some("logging")),
            ),
            (
                "an S3 subresource added beside the call's own",
                TAGGING,
                |r| r.url.set_query(Some("tagging&replication")),
            ),
            ("an S3 subresource removed", TAGGING, |r| {
                r.url.set_query(None)
            }),
            ("another target", KMS, |r| {
                r.headers.insert(
                    "x-amz-target",
                    HeaderValue::from_static("TrentService.ScheduleKeyDeletion"),
                );
            }),
            ("a form body on a JSON service", KMS, |r| {
                r.headers.remove("x-amz-target");
                r.body = Some("Action=ScheduleKeyDeletion&KeyId=k".into());
            }),
        ];
        for (what, built, change) in cases {
            assert_change_refused(what, built, change);
        }
    }

    /// A query parameter, a header or a body field can turn a call on the deployment's own
    /// resource into one that changes who has access to it.
    #[test]
    fn a_call_turned_into_one_that_changes_access_is_refused() {
        const CREATE_BUCKET: Built = (
            Cloud::Aws,
            "s3",
            Method::PUT,
            "https://bucket.s3.amazonaws.com/",
            &[],
            None,
        );
        const SQS_JSON: Built = (
            Cloud::Aws,
            "sqs",
            Method::POST,
            "https://sqs.us-east-1.amazonaws.com/",
            &[
                ("content-type", "application/x-amz-json-1.0"),
                ("x-amz-target", "AmazonSQS.SetQueueAttributes"),
            ],
            Some(r#"{"QueueUrl":"q","Attributes":{"VisibilityTimeout":"30"}}"#),
        );
        const SQS_QUERY: Built = (
            Cloud::Aws,
            "sqs",
            Method::POST,
            "https://sqs.us-east-1.amazonaws.com/",
            &[("content-type", "application/x-www-form-urlencoded")],
            Some(
                "Action=SetQueueAttributes&Attribute.1.Name=VisibilityTimeout&Attribute.1.Value=30",
            ),
        );
        const CREATE_KEY: Built = (
            Cloud::Aws,
            "kms",
            Method::POST,
            "https://kms.us-east-1.amazonaws.com/",
            &[
                ("content-type", "application/x-amz-json-1.1"),
                ("x-amz-target", "TrentService.CreateKey"),
            ],
            Some(r#"{"Description":"k"}"#),
        );
        const BUCKET_PATCH: Built = (
            Cloud::Gcp,
            "storage",
            Method::PATCH,
            "https://storage.googleapis.com/storage/v1/b/bucket",
            &[("content-type", "application/json")],
            Some(r#"{"labels":{"team":"a"}}"#),
        );
        const RESOURCE_GROUP: Built = (
            Cloud::Azure,
            "management",
            Method::PUT,
            "https://management.azure.com/subscriptions/s/resourcegroups/g",
            &[("content-type", "application/json")],
            Some(r#"{"location":"eastus"}"#),
        );
        const REST_API: Built = (
            Cloud::Aws,
            "apigateway",
            Method::POST,
            "https://apigateway.us-east-1.amazonaws.com/restapis",
            &[("content-type", "application/json")],
            Some(r#"{"name":"api","endpointConfiguration":{"types":["REGIONAL"]}}"#),
        );
        const REST_API_PATCH: Built = (
            Cloud::Aws,
            "apigateway",
            Method::PATCH,
            "https://apigateway.us-east-1.amazonaws.com/restapis/a1",
            &[("content-type", "application/json")],
            Some(r#"{"patchOperations":[{"op":"replace","path":"/description","value":"d"}]}"#),
        );
        const CREATE_TABLE: Built = (
            Cloud::Aws,
            "dynamodb",
            Method::POST,
            "https://dynamodb.us-east-1.amazonaws.com/",
            &[
                ("content-type", "application/x-amz-json-1.0"),
                ("x-amz-target", "DynamoDB_20120810.CreateTable"),
            ],
            Some(r#"{"TableName":"t"}"#),
        );
        let cases: [(&str, Built, fn(&mut HookRequest)); 20] = [
            (
                "a DynamoDB resource policy in another case",
                CREATE_TABLE,
                |r| r.body = Some(r#"{"TableName":"t","resourcePolicy":"{}"}"#.into()),
            ),
            (
                "an API Gateway policy moved away in a patch",
                REST_API_PATCH,
                |r| {
                    r.body = Some(
                    r#"{"patchOperations":[{"op":"move","from":"/policy","path":"/description"}]}"#
                        .into(),
                )
                },
            ),
            ("a DynamoDB resource policy", CREATE_TABLE, |r| {
                r.body = Some(r#"{"TableName":"t","ResourcePolicy":"{}"}"#.into())
            }),
            ("an API Gateway resource policy", REST_API, |r| {
                r.body = Some(r#"{"name":"api","policy":"{}"}"#.into())
            }),
            ("an API Gateway policy in a patch", REST_API_PATCH, |r| {
                r.body = Some(
                    r#"{"patchOperations":[{"op":"replace","path":"/policy","value":"{}"}]}"#
                        .into(),
                )
            }),
            (
                "an API Gateway policy in an OpenAPI document",
                REST_API,
                |r| {
                    r.body = Some(
                        r#"{"name":"api","body":{"x-amazon-apigateway-policy":{"Statement":[]}}}"#
                            .into(),
                    )
                },
            ),
            ("an S3 bucket policy", CREATE_BUCKET, |r| {
                r.url.set_query(Some("policy"));
                r.body = Some(r#"{"Statement":[]}"#.into());
            }),
            ("an S3 ACL", CREATE_BUCKET, |r| r.url.set_query(Some("acl"))),
            ("an S3 canned ACL header", CREATE_BUCKET, |r| {
                r.headers
                    .insert("x-amz-acl", HeaderValue::from_static("public-read"));
            }),
            ("an SQS queue policy in JSON", SQS_JSON, |r| {
                r.body = Some(
                    r#"{"QueueUrl":"q","Attributes":{"VisibilityTimeout":"30","Policy":"{}"}}"#
                        .into(),
                )
            }),
            (
                "an SQS queue policy in JSON, in another case",
                SQS_JSON,
                |r| r.body = Some(r#"{"QueueUrl":"q","attributes":{"policy":"{}"}}"#.into()),
            ),
            (
                "an SQS queue policy in a form, in another case",
                SQS_QUERY,
                |r| {
                    r.body
                        .as_mut()
                        .unwrap()
                        .push_str("&Attribute.2.name=policy&Attribute.2.Value=%7B%7D")
                },
            ),
            (
                "a policy field in a form, in another case",
                SQS_QUERY,
                |r| r.body.as_mut().unwrap().push_str("&policy=%7B%7D"),
            ),
            ("an SQS queue policy in a form", SQS_QUERY, |r| {
                r.body
                    .as_mut()
                    .unwrap()
                    .push_str("&Attribute.2.Name=Policy&Attribute.2.Value=%7B%7D")
            }),
            ("an SQS queue policy in the query", SQS_QUERY, |r| {
                r.url
                    .set_query(Some("Attribute.2.Name=Policy&Attribute.2.Value=%7B%7D"))
            }),
            ("an SQS form moved into the query", SQS_QUERY, |r| {
                let form = r.body.take().unwrap();
                r.url.set_query(Some(&format!(
                    "{form}&Attribute.2.Name=Policy&Attribute.2.Value=%7B%7D"
                )))
            }),
            ("a KMS key policy", CREATE_KEY, |r| {
                r.body = Some(r#"{"Description":"k","Policy":"{}"}"#.into())
            }),
            ("a Cloud Storage predefined ACL", BUCKET_PATCH, |r| {
                r.url.set_query(Some("predefinedAcl=publicRead"))
            }),
            ("a Cloud Run IAM policy", CLOUD_RUN_SERVICE, |r| {
                r.url
                    .set_path("/v2/projects/p/locations/l/services/web:setIamPolicy");
                r.method = Method::POST;
            }),
            ("an Azure role assignment", RESOURCE_GROUP, |r| {
                r.url.set_path(
                    "/subscriptions/s/resourcegroups/g/providers/Microsoft.Authorization/roleAssignments/a",
                )
            }),
        ];
        for (what, built, change) in cases {
            assert_change_refused(what, built, change);
        }
    }

    /// A call Alien makes to change who has access, to act as another identity, or to read a
    /// policy it then writes back is sent as Alien built it: no interceptor is asked whether it
    /// matches it, so one that matches everything never fails it.
    #[test]
    fn a_call_that_changes_access_or_identity_is_never_offered() {
        let cases: [Built; 19] = [
            (
                Cloud::Aws,
                "iam",
                Method::POST,
                "https://iam.amazonaws.com/",
                &[("content-type", "application/x-www-form-urlencoded")],
                Some("Action=PutRolePolicy&RoleName=r&PolicyName=p&PolicyDocument=%7B%7D"),
            ),
            (
                Cloud::Aws,
                "cloudcontrolapi",
                Method::POST,
                "https://cloudcontrolapi.us-east-1.amazonaws.com/",
                &[(
                    "x-amz-target",
                    "CloudApiService.CreateResource",
                )],
                Some(r#"{"TypeName":"AWS::EC2::VPC"}"#),
            ),
            (
                Cloud::Aws,
                "s3",
                Method::PUT,
                "https://bucket.s3.amazonaws.com/?policy",
                &[],
                Some(r#"{"Statement":[]}"#),
            ),
            (
                Cloud::Aws,
                "s3",
                Method::GET,
                "https://bucket.s3.amazonaws.com/?policy",
                &[],
                None,
            ),
            (
                Cloud::Aws,
                "kms",
                Method::POST,
                "https://kms.us-east-1.amazonaws.com/",
                &[("x-amz-target", "TrentService.PutKeyPolicy")],
                Some(r#"{"KeyId":"k","Policy":"{}"}"#),
            ),
            (
                Cloud::Aws,
                "ecr",
                Method::POST,
                "https://api.ecr.us-east-1.amazonaws.com/",
                &[(
                    "x-amz-target",
                    "AmazonEC2ContainerRegistry_V20150921.GetRepositoryPolicy",
                )],
                Some(r#"{"repositoryName":"r"}"#),
            ),
            (
                Cloud::Aws,
                "lambda",
                Method::POST,
                "https://lambda.us-east-1.amazonaws.com/2015-03-31/functions/f/policy",
                &[],
                Some(r#"{"StatementId":"s","Action":"lambda:InvokeFunction"}"#),
            ),
            (
                Cloud::Aws,
                "sqs",
                Method::POST,
                "https://sqs.us-east-1.amazonaws.com/",
                &[],
                Some("Action=SetQueueAttributes&Attribute.1.Name=Policy&Attribute.1.Value=%7B%7D"),
            ),
            (
                Cloud::Aws,
                "logs",
                Method::POST,
                "https://logs.us-east-1.amazonaws.com/",
                &[("x-amz-target", "Logs_20140328.PutResourcePolicy")],
                Some(r#"{"policyName":"p","policyDocument":"{}"}"#),
            ),
            (
                Cloud::Gcp,
                "cloudrun",
                Method::POST,
                "https://run.googleapis.com/v2/projects/p/locations/l/services/web:setIamPolicy",
                &[],
                Some(r#"{"policy":{}}"#),
            ),
            (
                Cloud::Gcp,
                "cloudrun",
                Method::GET,
                "https://run.googleapis.com/v2/projects/p/locations/l/services/web:getIamPolicy",
                &[],
                None,
            ),
            (
                Cloud::Gcp,
                "storage",
                Method::GET,
                "https://storage.googleapis.com/storage/v1/b/bucket/iam",
                &[],
                None,
            ),
            (
                Cloud::Azure,
                "management",
                Method::PUT,
                "https://management.azure.com/subscriptions/s/providers/Microsoft.Authorization/roleAssignments/a",
                &[],
                Some(r#"{"properties":{}}"#),
            ),
            (
                Cloud::Azure,
                "management",
                Method::PUT,
                "https://management.azure.com/subscriptions/s/resourceGroups/g/providers/Microsoft.ManagedIdentity/userAssignedIdentities/u/federatedIdentityCredentials/c",
                &[],
                Some(r#"{"properties":{}}"#),
            ),
            (
                Cloud::Kubernetes,
                "kubernetes",
                Method::POST,
                "https://cluster.example.com/api/v1/namespaces/app/secrets",
                &[("content-type", "application/json")],
                Some(r#"{"metadata":{"name":"registry"},"type":"kubernetes.io/dockerconfigjson"}"#),
            ),
            (
                Cloud::Kubernetes,
                "kubernetes",
                Method::GET,
                "https://cluster.example.com/api/v1/namespaces/app/secrets/registry",
                &[],
                None,
            ),
            (
                Cloud::Kubernetes,
                "kubernetes",
                Method::GET,
                "https://cluster.example.com/api/v1/secrets?labelSelector=app",
                &[],
                None,
            ),
            (
                Cloud::Kubernetes,
                "kubernetes",
                Method::POST,
                "https://cluster.example.com/api/v1/namespaces/app/serviceaccounts/app/token",
                &[("content-type", "application/json")],
                Some(r#"{"spec":{"audiences":["api"]}}"#),
            ),
            (
                Cloud::Kubernetes,
                "kubernetes",
                Method::POST,
                "https://cluster.example.com/apis/authentication.k8s.io/v1/tokenreviews",
                &[("content-type", "application/json")],
                Some(r#"{"spec":{"token":"t"}}"#),
            ),
        ];
        for built in cases {
            let (cloud, service) = (built.0, built.1);
            let interceptors = chain(vec![TestInterceptor::new("acme", cloud, service)
                .when(|request| panic!("{} {} was offered", request.method, request.url))]);
            let mut request = build(&built);
            let before = format!("{request:?}");
            assert!(interceptors
                .intercept(cloud, service, &mut request)
                .unwrap()
                .is_none());
            assert_eq!(format!("{request:?}"), before);
        }
    }

    #[test]
    fn an_interceptor_never_sees_credentials_and_the_request_keeps_them() {
        let seen = Arc::new(Mutex::new(None));
        let spy = seen.clone();
        let interceptors = chain(vec![TestInterceptor::new(
            "acme",
            Cloud::Azure,
            "management",
        )
        .on_request(move |r| {
            *spy.lock().unwrap() = Some(r.headers.clone());
            Ok(())
        })]);
        let mut request = reqwest::Client::new()
            .get("https://management.azure.com/subscriptions/sub")
            .header("authorization", "Bearer azure-token")
            .header("proxy-authorization", "Basic proxy")
            .header("x-ms-version", "2023-11-03")
            .build()
            .unwrap();
        interceptors
            .intercept(Cloud::Azure, "management", &mut request)
            .unwrap();
        let seen = seen.lock().unwrap().clone().unwrap();
        assert!(seen.get("authorization").is_none());
        assert!(seen.get("proxy-authorization").is_none());
        assert_eq!(seen["x-ms-version"], "2023-11-03");
        assert_eq!(request.headers()["authorization"], "Bearer azure-token");
        assert_eq!(request.headers()["proxy-authorization"], "Basic proxy");
    }

    #[test]
    fn a_changed_body_gets_new_digest_headers() {
        let interceptors = chain(vec![TestInterceptor::new("acme", Cloud::Aws, "rds")
            .on_request(|r| {
                r.body
                    .as_mut()
                    .unwrap()
                    .push_str("&EnableIAMDatabaseAuthentication=true");
                Ok(())
            })]);
        let mut request = rds_request("Action=CreateDBCluster");
        interceptors
            .intercept(Cloud::Aws, "rds", &mut request)
            .unwrap();
        let body = "Action=CreateDBCluster&EnableIAMDatabaseAuthentication=true";
        assert_eq!(body_text(&request), body);
        assert_eq!(
            request.headers()["content-length"],
            body.len().to_string().as_str()
        );
        assert_eq!(
            request.headers()["x-amz-content-sha256"],
            format!("{:x}", Sha256::digest(body)).as_str()
        );
        assert_eq!(
            request.headers()["content-md5"],
            base64::engine::general_purpose::STANDARD
                .encode(md5::compute(body).0)
                .as_str()
        );
    }

    #[test]
    fn interceptors_run_in_order_on_the_request_the_previous_one_left() {
        let interceptors = chain(vec![
            TestInterceptor::new("first", Cloud::Gcp, "cloudsql").on_request(|r| {
                r.body = Some(r#"{"tier":"small"}"#.to_string());
                Ok(())
            }),
            TestInterceptor::new("second", Cloud::Gcp, "cloudsql").on_request(|r| {
                assert_eq!(r.body.as_deref(), Some(r#"{"tier":"small"}"#));
                r.body = Some(r#"{"tier":"large"}"#.to_string());
                Ok(())
            }),
        ]);
        let mut request = reqwest::Client::new()
            .post("https://sqladmin.googleapis.com/v1/projects/p/instances")
            .json(&serde_json::json!({"tier": "tiny"}))
            .build()
            .unwrap();
        interceptors
            .intercept(Cloud::Gcp, "cloudsql", &mut request)
            .unwrap();
        assert_eq!(body_text(&request), r#"{"tier":"large"}"#);
    }

    #[test]
    fn an_interceptor_error_fails_the_call_naming_its_extension() {
        let interceptors = chain(vec![TestInterceptor::new(
            "acme",
            Cloud::Kubernetes,
            "kubernetes",
        )
        .on_request(|_| {
            Err(AlienError::new(ErrorData::GenericError {
                message: "label value missing".to_string(),
            }))
        })]);
        let mut request = reqwest::Client::new()
            .post("https://cluster.local/api/v1/namespaces/ns/pods")
            .json(&serde_json::json!({"kind": "Pod"}))
            .build()
            .unwrap();
        let error = interceptors
            .intercept(Cloud::Kubernetes, "kubernetes", &mut request)
            .unwrap_err();
        assert!(error
            .source
            .as_ref()
            .unwrap()
            .message
            .contains("label value missing"));
        assert_fails_naming(error, "acme");
    }

    #[test]
    fn a_binary_request_body_fails_a_call_an_interceptor_matches() {
        let interceptors = chain(vec![TestInterceptor::new("acme", Cloud::Aws, "s3")
            .on_request(|_| panic!("a binary body is never offered"))]);
        let mut request = reqwest::Client::new()
            .put("https://bucket.s3.amazonaws.com/key")
            .header("content-type", "application/json")
            .body(vec![0xff, 0xfe])
            .build()
            .unwrap();
        let error = interceptors
            .intercept(Cloud::Aws, "s3", &mut request)
            .unwrap_err();
        assert_fails_naming(error, "acme");
    }

    #[test]
    fn an_object_payload_is_binary_whatever_its_content_type() {
        let interceptors = chain(vec![TestInterceptor::new("acme", Cloud::Aws, "s3")
            .on_request(|_| panic!("an object's bytes are never offered"))]);
        for builder in [
            reqwest::Client::new()
                .put("https://bucket.s3.amazonaws.com/config.json")
                .header("content-type", "application/json")
                .body(r#"{"looks":"like json"}"#),
            reqwest::Client::new().get("https://bucket.s3.amazonaws.com/config.json"),
        ] {
            let mut request = builder.build().unwrap();
            let error = interceptors
                .for_object_payload()
                .intercept(Cloud::Aws, "s3", &mut request)
                .unwrap_err();
            assert_fails_naming(error, "acme");
        }
    }

    #[test]
    fn a_binary_call_no_interceptor_matches_is_sent_as_it_was() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let spy = seen.clone();
        let tagging = TestInterceptor::new("acme", Cloud::Aws, "s3").when(move |request| {
            spy.lock()
                .unwrap()
                .push((request.method.clone(), request.body.clone()));
            request.url.query() == Some("tagging")
        });
        let interceptors = chain(vec![tagging]);
        let mut request = reqwest::Client::new()
            .put("https://bucket.s3.amazonaws.com/image.png")
            .body(vec![0x89, b'P', b'N', b'G'])
            .build()
            .unwrap();
        assert!(interceptors
            .for_object_payload()
            .intercept(Cloud::Aws, "s3", &mut request)
            .unwrap()
            .is_none());
        assert_eq!(
            request.body().unwrap().as_bytes(),
            Some(&[0x89, b'P', b'N', b'G'][..])
        );
        assert_eq!(*seen.lock().unwrap(), vec![(Method::PUT, None)]);
    }

    #[test]
    fn a_text_body_is_offered_whatever_its_content_type() {
        let interceptors = chain(vec![TestInterceptor::new(
            "acme",
            Cloud::Kubernetes,
            "kubernetes",
        )
        .on_request(|request| {
            assert_eq!(request.body.as_deref(), Some("plain words"));
            Ok(())
        })]);
        let mut request = reqwest::Client::new()
            .post("https://cluster.local/api/v1/namespaces/ns/configmaps")
            .header("content-type", "text/plain")
            .body("plain words")
            .build()
            .unwrap();
        assert!(interceptors
            .intercept(Cloud::Kubernetes, "kubernetes", &mut request)
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn modify_response_sees_and_can_change_an_error_response() {
        let interceptors = chain(vec![TestInterceptor::new(
            "acme",
            Cloud::Azure,
            "management",
        )
        .on_response(|request, response| {
            assert_eq!(request.method, Method::PUT);
            assert_eq!(response.status, 409);
            response.status = 200;
            response.body = r#"{"name":"recovered"}"#.to_string();
            Ok(())
        })]);
        let mut request = reqwest::Client::new()
            .put("https://management.azure.com/subscriptions/sub/thing")
            .json(&serde_json::json!({}))
            .build()
            .unwrap();
        let hook = interceptors
            .intercept(Cloud::Azure, "management", &mut request)
            .unwrap()
            .unwrap();
        let response = reqwest::Response::from(
            http::Response::builder()
                .status(409)
                .header("content-type", "application/json")
                .url(Url::parse("https://management.azure.com/subscriptions/sub/thing").unwrap())
                .body(r#"{"error":{"code":"Conflict"}}"#)
                .unwrap(),
        );
        let response = hook.apply(response).await.unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.url().as_str(),
            "https://management.azure.com/subscriptions/sub/thing"
        );
        assert_eq!(response.text().await.unwrap(), r#"{"name":"recovered"}"#);
    }

    async fn hooked_get(interceptor: TestInterceptor) -> ResponseHook {
        let mut request = reqwest::Client::new()
            .get("https://cluster.local/api/v1/namespaces/ns/pods/web")
            .build()
            .unwrap();
        chain(vec![interceptor])
            .intercept(Cloud::Kubernetes, "kubernetes", &mut request)
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn a_binary_response_fails_a_call_an_interceptor_matches() {
        let hook = hooked_get(
            TestInterceptor::new("acme", Cloud::Kubernetes, "kubernetes")
                .on_response(|_, _| panic!("a binary body is never offered")),
        )
        .await;
        let response = reqwest::Response::from(
            http::Response::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(vec![0x89, b'P', b'N', b'G', 0xff])
                .unwrap(),
        );
        assert_fails_naming(hook.apply(response).await.unwrap_err(), "acme");
    }

    /// A plain-text error page from an API server or a proxy is text: it reaches the client's own
    /// status handling and retries instead of failing the call.
    #[tokio::test]
    async fn a_plain_text_error_response_is_offered() {
        let hook = hooked_get(
            TestInterceptor::new("acme", Cloud::Kubernetes, "kubernetes").on_response(
                |_, response| {
                    assert_eq!(response.body, "upstream connect error");
                    Ok(())
                },
            ),
        )
        .await;
        let response = reqwest::Response::from(
            http::Response::builder()
                .status(503)
                .header("content-type", "text/plain")
                .body("upstream connect error")
                .unwrap(),
        );
        let response = hook.apply(response).await.unwrap();
        assert_eq!(response.status(), 503);
        assert_eq!(response.text().await.unwrap(), "upstream connect error");
    }

    /// A cloud's error can quote the request it refused, as AWS quotes the signed headers when a
    /// signature does not match. The credential the call was signed with is cleared before an
    /// interceptor reads the response, whether the client signed before the interceptors ran or
    /// after.
    #[tokio::test]
    async fn an_echoed_credential_is_cleared_before_an_interceptor_reads_the_response() {
        let token = "IQoJb3JpZ2luX2VjEXAMPLESESSIONTOKEN";
        let quoting = |request: &mut reqwest::Request| {
            chain(vec![TestInterceptor::new("acme", Cloud::Aws, "s3")
                .on_response(move |_, response| {
                    assert!(!response.body.contains(token), "{}", response.body);
                    assert!(response.body.contains("[redacted]"), "{}", response.body);
                    let echoed = response.headers["x-amz-echo"].to_str().unwrap();
                    assert!(!echoed.contains(token), "{echoed}");
                    Ok(())
                })])
            .intercept(Cloud::Aws, "s3", request)
            .unwrap()
            .unwrap()
        };
        let response = || {
            reqwest::Response::from(
                http::Response::builder()
                    .status(403)
                    .header("content-type", "application/xml")
                    .header("x-amz-echo", format!("token={token}"))
                    .body(format!(
                        "<Error><Code>SignatureDoesNotMatch</Code><CanonicalRequest>x-amz-security-token:{token}</CanonicalRequest></Error>"
                    ))
                    .unwrap(),
            )
        };
        let unsigned = || {
            reqwest::Client::new()
                .get("https://bucket.s3.us-east-1.amazonaws.com/key")
                .build()
                .unwrap()
        };

        // Signed after the interceptors ran: the signer hands the hook the signed request.
        let mut request = unsigned();
        let mut hook = quoting(&mut request);
        request.headers_mut().insert(
            "x-amz-security-token",
            HeaderValue::from_str(token).unwrap(),
        );
        hook.remember_credentials(&request);
        let read = hook.apply(response()).await.unwrap().text().await.unwrap();
        assert!(!read.contains(token), "{read}");

        // Signed before: the hook reads the credential off the request it intercepts.
        let mut request = unsigned();
        request.headers_mut().insert(
            "x-amz-security-token",
            HeaderValue::from_str(token).unwrap(),
        );
        let hook = quoting(&mut request);
        hook.apply(response()).await.unwrap();
    }

    /// A bearer token is cleared on its own as well as with its scheme, and a short value that
    /// could be ordinary text is left alone.
    #[test]
    fn a_bearer_token_is_cleared_on_its_own() {
        let mut request = reqwest::Client::new()
            .get("https://management.azure.com/subscriptions/sub?api-version=1")
            .header("authorization", "Bearer eyJ0eXAiOiJKV1QiLCJhbGciOi")
            .header("proxy-authorization", "Basic abc")
            .build()
            .unwrap();
        let hook = chain(vec![TestInterceptor::new(
            "acme",
            Cloud::Azure,
            "management",
        )])
        .intercept(Cloud::Azure, "management", &mut request)
        .unwrap()
        .unwrap();
        assert_eq!(
            hook.cleared(br#"{"token":"eyJ0eXAiOiJKV1QiLCJhbGciOi","word":"abc"}"#),
            br#"{"token":"[redacted]","word":"abc"}"#.to_vec()
        );
    }

    /// A create the cloud answered with success already made the resource, so a hook that turns
    /// the answer into a throttle fails the call for good instead of having it sent again.
    #[tokio::test]
    async fn a_hook_that_fails_a_successful_call_fails_it_for_good() {
        let mut request = reqwest::Client::new()
            .post("https://ec2.us-east-1.amazonaws.com/")
            .header("content-type", "application/x-www-form-urlencoded")
            .body("Action=CreateVpc&CidrBlock=10.0.0.0%2F16")
            .build()
            .unwrap();
        let hook = chain(vec![TestInterceptor::new("acme", Cloud::Aws, "ec2")
            .on_response(|_, response| {
                response.status = 429;
                Ok(())
            })])
        .intercept(Cloud::Aws, "ec2", &mut request)
        .unwrap()
        .unwrap();
        let response = reqwest::Response::from(
            http::Response::builder()
                .status(200)
                .header("content-type", "text/xml")
                .body("<CreateVpcResponse><vpc><vpcId>vpc-1</vpcId></vpc></CreateVpcResponse>")
                .unwrap(),
        );
        let error = hook.apply(response).await.unwrap_err();
        assert!(!error.retryable);
        assert_fails_naming(error, "acme");
    }

    /// Alien reads a Kubernetes response to decide its next calls on the cluster, so an
    /// interceptor may not change one.
    #[tokio::test]
    async fn a_kubernetes_response_is_not_changed() {
        let hook = hooked_get(
            TestInterceptor::new("acme", Cloud::Kubernetes, "kubernetes").on_response(
                |_, response| {
                    response.body = r#"{"metadata":{"name":"other"}}"#.to_string();
                    Ok(())
                },
            ),
        )
        .await;
        let response = reqwest::Response::from(
            http::Response::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(r#"{"metadata":{"name":"web"}}"#)
                .unwrap(),
        );
        let error = hook.apply(response).await.unwrap_err();
        assert!(!error.retryable);
        assert_fails_naming(error, "acme");
    }

    /// The cloud already acted on the call when `modify_response` runs, so a failure there is
    /// final even when the hook marked it retryable: a retry would create the resource twice.
    #[tokio::test]
    async fn a_failed_modify_response_is_never_sent_again() {
        use crate::request_utils::InterceptedRequestBuilder;
        use crate::testing::capture;

        let server = capture::start(|_| capture::Reply::new(200, "application/json", "{}")).await;
        let client = reqwest::Client::new();
        let mut request = client
            .post(format!("{}/servers", server.base_url))
            .body("{}")
            .build()
            .unwrap();
        let hook = chain(vec![TestInterceptor::new(
            "acme",
            Cloud::Azure,
            "management",
        )
        .on_response(|_, _| {
            let mut deadline = failed(
                "acme",
                "its hook 'h' failed (HOOK_DEADLINE_EXCEEDED)".into(),
            );
            deadline.retryable = true;
            Err(deadline)
        })])
        .intercept(Cloud::Azure, "management", &mut request)
        .unwrap();

        let error = InterceptedRequestBuilder::new(
            reqwest::RequestBuilder::from_parts(client, request),
            hook,
        )
        .with_retry()
        .send_json::<serde_json::Value>()
        .await
        .unwrap_err();
        assert_eq!(error.code, "INTERCEPTOR_FAILED");
        assert!(!error.retryable);
        assert_eq!(server.requests().len(), 1);
    }

    /// A 2xx means the cloud acted on the call, so a body that breaks off after it is not
    /// retried through the interceptors either.
    #[tokio::test]
    async fn success_body_read_failure_does_not_replay_an_intercepted_mutation() {
        use crate::request_utils::InterceptedRequestBuilder;
        use backon::ExponentialBuilder;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::Duration;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listen");
        let address = listener.local_addr().expect("address");
        let requests = Arc::new(AtomicUsize::new(0));
        let seen = requests.clone();
        let server = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.expect("accept");
                {
                    let mut reader = tokio::io::BufReader::new(&mut stream);
                    loop {
                        let mut line = String::new();
                        assert!(reader.read_line(&mut line).await.expect("request header") > 0);
                        if line == "\r\n" {
                            break;
                        }
                    }
                }
                seen.fetch_add(1, Ordering::SeqCst);
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nshort",
                    )
                    .await
                    .expect("response");
                stream.shutdown().await.expect("close response");
            }
        });

        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let mut request = client
            .post(format!("http://{address}/servers"))
            .build()
            .unwrap();
        let hook = chain(vec![TestInterceptor::new(
            "acme",
            Cloud::Azure,
            "management",
        )])
        .intercept(Cloud::Azure, "management", &mut request)
        .unwrap();
        assert!(hook.is_some(), "the interceptor matched");

        let error = InterceptedRequestBuilder::new(
            reqwest::RequestBuilder::from_parts(client, request),
            hook,
        )
        .with_retry()
        .backoff(
            ExponentialBuilder::default()
                .with_min_delay(Duration::from_millis(1))
                .with_max_delay(Duration::from_millis(1))
                .with_max_times(2),
        )
        .send_no_response()
        .await
        .expect_err("a truncated body fails the call");
        assert_eq!(
            requests.load(Ordering::SeqCst),
            1,
            "the mutation was sent again"
        );
        assert_eq!(error.code, "HTTP_RESPONSE_BODY_READ_FAILED", "{error}");
        assert!(!error.retryable);
        assert!(matches!(
            error.error,
            Some(ErrorData::HttpResponseBodyReadFailed { http_status: 200 })
        ));
        server.abort();
    }

    #[test]
    fn a_wrapper_keeps_an_interceptor_failure_final() {
        let wrapped: Result<()> =
            Err(failed("acme", "refused".to_string())).context(ErrorData::HttpRequestFailed {
                message: "GetObject request failed".to_string(),
            });
        assert!(wrapped.as_ref().unwrap_err().retryable);
        assert!(
            !keep_interceptor_failure_final(wrapped)
                .unwrap_err()
                .retryable
        );

        let mut deadline = failed(
            "acme",
            "its hook 'h' failed (HOOK_DEADLINE_EXCEEDED)".into(),
        );
        deadline.retryable = true;
        let wrapped: Result<()> = Err(deadline).context(ErrorData::HttpRequestFailed {
            message: "GetObject request failed".to_string(),
        });
        assert!(
            keep_interceptor_failure_final(wrapped)
                .unwrap_err()
                .retryable,
            "an interceptor's own retryable failure stays retryable"
        );

        let network: Result<()> = Err(AlienError::new(ErrorData::HttpRequestFailed {
            message: "connection reset".to_string(),
        }));
        assert!(
            keep_interceptor_failure_final(network)
                .unwrap_err()
                .retryable
        );
    }

    #[test]
    fn a_client_with_interceptors_signs_only_for_its_endpoint() {
        let endpoint = "https://management.azure.com";
        let elsewhere = Url::parse("https://evil.example.com/operations/1").unwrap();
        assert!(InterceptorChain::default()
            .check_destination(&elsewhere, endpoint)
            .is_ok());

        let interceptors = chain(vec![TestInterceptor::new(
            "acme",
            Cloud::Azure,
            "management",
        )]);
        let error = interceptors
            .check_destination(&elsewhere, endpoint)
            .unwrap_err();
        assert_eq!(error.code, "DESTINATION_REFUSED");
        assert!(!error.retryable);
        interceptors
            .check_destination(
                &Url::parse("https://management.azure.com/subscriptions/s/operations/1").unwrap(),
                endpoint,
            )
            .unwrap();
    }
}
