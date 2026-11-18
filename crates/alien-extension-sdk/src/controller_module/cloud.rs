//! The two capabilities a controller module calls: its step's cloud, which the host signs, and
//! the origins its extension declares, sent with no credential.

use super::{ErrorData, Result};
use alien_core::Platform;
use alien_error::{AlienError, Context, IntoAlienError};
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// The most of a response's body an error's message carries. The message is saved with the
/// deployment.
const MAX_ERROR_BODY_BYTES: usize = 2048;

/// Where a step's cloud calls go: the account, project or subscription the host holds the
/// step's credentials for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "platform", rename_all = "camelCase")]
pub enum CloudTarget {
    #[serde(rename_all = "camelCase")]
    Aws { account_id: String, region: String },
    #[serde(rename_all = "camelCase")]
    Gcp { project_id: String, region: String },
    #[serde(rename_all = "camelCase")]
    Azure {
        subscription_id: String,
        tenant_id: String,
    },
}

/// A request's body: text, or any bytes in base64.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Body {
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    body_base64: Option<String>,
}

impl Body {
    fn text(text: String) -> Self {
        Self {
            body: Some(text),
            body_base64: None,
        }
    }

    fn bytes(bytes: &[u8]) -> Self {
        match std::str::from_utf8(bytes) {
            Ok(text) => Self::text(text.to_string()),
            Err(_) => Self {
                body: None,
                body_base64: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
            },
        }
    }
}

fn json_body(value: &impl Serialize) -> Result<String> {
    serde_json::to_string(value)
        .into_alien_error()
        .context(ErrorData::RequestInvalid {
            message: "the request's body did not serialize".to_string(),
        })
}

/// A request to the step's cloud. The module names a service and a path; the host picks the
/// scheme, the host name and the port from its own table of the cloud's API endpoints, and
/// signs the request with the step's credentials.
///
/// A module may set these headers and no others: `accept`, `user-agent`, `content-type`,
/// `content-md5` and the `if-*` conditions. The host refuses a request that sets another.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudRequest {
    cloud: Platform,
    service: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    region: Option<String>,
    method: String,
    path: String,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    #[serde(flatten)]
    body: Body,
}

impl CloudRequest {
    fn new(cloud: Platform, service: &str, method: &str, path: &str) -> Self {
        Self {
            cloud,
            service: service.to_string(),
            region: None,
            method: method.to_string(),
            path: path.to_string(),
            query: Vec::new(),
            headers: Vec::new(),
            body: Body::default(),
        }
    }

    /// A request to the AWS service with endpoint prefix `service` (`route53`, `sts`,
    /// `api.ecr`), in the step's region unless [`Self::region`] names another.
    pub fn aws(service: &str, method: &str, path: &str) -> Self {
        Self::new(Platform::Aws, service, method, path)
    }

    /// A request to the Google Cloud API `service`, the first label of its host: `dns` for
    /// `dns.googleapis.com`.
    pub fn gcp(service: &str, method: &str, path: &str) -> Self {
        Self::new(Platform::Gcp, service, method, path)
    }

    /// A request to Azure Resource Manager.
    pub fn azure(method: &str, path: &str) -> Self {
        Self::new(Platform::Azure, "management", method, path)
    }

    /// Sends an AWS request to `region` instead of the step's.
    pub fn region(mut self, region: &str) -> Self {
        self.region = Some(region.to_string());
        self
    }

    /// Adds a query parameter. The host encodes it.
    pub fn query(mut self, key: &str, value: &str) -> Self {
        self.query.push((key.to_string(), value.to_string()));
        self
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    /// A body of text.
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = Body::text(body.into());
        self
    }

    /// A body of any bytes.
    pub fn body_bytes(mut self, body: &[u8]) -> Self {
        self.body = Body::bytes(body);
        self
    }

    /// `body` as JSON, with its content type.
    pub fn json(self, body: &impl Serialize) -> Result<Self> {
        Ok(self
            .header("content-type", "application/json")
            .body(json_body(body)?))
    }
}

/// A request to an origin the extension declares in `egress`, such as the vendor's own API.
/// The host attaches none of the deployment's credentials; a key the module sends in
/// `authorization` is its vendor's.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpRequest {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    #[serde(flatten)]
    body: Body,
}

impl HttpRequest {
    /// A request to `url`, an absolute `https` URL on a declared origin.
    pub fn new(method: &str, url: &str) -> Self {
        Self {
            method: method.to_string(),
            url: url.to_string(),
            headers: Vec::new(),
            body: Body::default(),
        }
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    /// A body of text.
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = Body::text(body.into());
        self
    }

    /// A body of any bytes.
    pub fn body_bytes(mut self, body: &[u8]) -> Self {
        self.body = Body::bytes(body);
        self
    }

    /// `body` as JSON, with its content type.
    pub fn json(self, body: &impl Serialize) -> Result<Self> {
        Ok(self
            .header("content-type", "application/json")
            .body(json_body(body)?))
    }
}

/// What the cloud or the origin answered. Any status is a response: a handler checks it, with
/// [`Self::ok`] or by reading `status`. The host follows no redirect.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    /// The body when it is text; empty when it is not, and [`Self::bytes`] has it.
    pub body: String,
    #[serde(default)]
    body_base64: Option<String>,
}

impl CloudResponse {
    /// The body's bytes, whether it is text or not.
    pub fn bytes(&self) -> Result<Vec<u8>> {
        match &self.body_base64 {
            Some(encoded) => base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .into_alien_error()
                .context(ErrorData::RequestInvalid {
                    message: "the host's reply holds a body that is not base64".to_string(),
                }),
            None => Ok(self.body.as_bytes().to_vec()),
        }
    }

    /// The body read as JSON.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_str(&self.body)
            .into_alien_error()
            .context(ErrorData::UnexpectedResponse {
                message: format!(
                    "the response with status {} did not read as the type asked for",
                    self.status
                ),
            })
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The error Alien's own cloud clients return for this status, naming `what` was called.
    pub fn error(&self, what: &str) -> AlienError<ErrorData> {
        let mut end = self.body.len().min(MAX_ERROR_BODY_BYTES);
        while !self.body.is_char_boundary(end) {
            end -= 1;
        }
        let message = format!("{what} answered {}: {}", self.status, &self.body[..end]);
        AlienError::new(match self.status {
            404 => ErrorData::RemoteResourceNotFound { message },
            401 | 403 => ErrorData::RemoteAccessDenied { message },
            409 => ErrorData::RemoteResourceConflict { message },
            429 => ErrorData::RateLimitExceeded { message },
            500..=599 => ErrorData::RemoteServiceUnavailable { message },
            status => ErrorData::HttpResponseError {
                message,
                http_status: status,
            },
        })
    }

    /// `Ok(self)` on a 2xx, else [`Self::error`].
    pub fn ok(self, what: &str) -> Result<Self> {
        if self.is_success() {
            Ok(self)
        } else {
            Err(self.error(what))
        }
    }
}

/// The step's cloud, through the host.
#[derive(Debug, Clone)]
pub struct Cloud {
    target: Option<CloudTarget>,
    deadline: DateTime<Utc>,
}

impl Cloud {
    pub(super) fn new(target: Option<CloudTarget>, deadline: DateTime<Utc>) -> Self {
        Self { target, deadline }
    }

    /// Where the step's cloud calls go, or `None` for a step the host holds no cloud
    /// credentials for.
    pub fn target(&self) -> Option<&CloudTarget> {
        self.target.as_ref()
    }

    pub(super) fn time_left(&self) -> Duration {
        (self.deadline - Utc::now()).to_std().unwrap_or_default()
    }

    /// Sends `request` to the step's cloud, once, and returns what it answered, whatever the
    /// status. Nothing here sends a call again: a handler returns the error, and Alien runs
    /// the step again when the error is retryable, as it does for its own controllers. A call
    /// that got no answer may still have reached the cloud, so that next run must be safe to
    /// make it again.
    pub fn send(&self, request: &CloudRequest) -> Result<CloudResponse> {
        self.call(host::cloud, request, &request.service)
    }

    /// Sends `request` to an origin the extension declares, with none of the deployment's
    /// credentials.
    pub fn http(&self, request: &HttpRequest) -> Result<CloudResponse> {
        self.call(host::http, request, &request.url)
    }

    fn call(
        &self,
        import: fn(&[u8]) -> std::result::Result<Vec<u8>, String>,
        request: &impl Serialize,
        what: &str,
    ) -> Result<CloudResponse> {
        if self.time_left().is_zero() {
            return Err(AlienError::new(ErrorData::StepDeadline {
                call: format!("the call to {what} started"),
            }));
        }
        let bytes =
            serde_json::to_vec(request)
                .into_alien_error()
                .context(ErrorData::RequestInvalid {
                    message: "the call did not serialize".to_string(),
                })?;
        let reply = import(&bytes).map_err(|message| {
            // The host says `refused:` for a call it will not send, and `failed:` for one it
            // sent or tried to send, and for one a hook stopped before it was sent that may
            // pass on another try.
            if message.starts_with("refused:") {
                refused(message)
            } else {
                let message = message
                    .strip_prefix("failed:")
                    .map(str::trim_start)
                    .unwrap_or(&message)
                    .to_string();
                AlienError::new(ErrorData::HttpRequestFailed { message })
            }
        })?;
        serde_json::from_slice(&reply)
            .into_alien_error()
            .context(ErrorData::RequestInvalid {
                message: "the host's reply to the call did not read".to_string(),
            })
    }
}

fn refused(message: String) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::CallRefused { message })
}

#[cfg(target_arch = "wasm32")]
mod host {
    const ERROR_BIT: u64 = 1 << 63;

    #[link(wasm_import_module = "alien_cloud_v1")]
    unsafe extern "C" {
        #[link_name = "send"]
        fn cloud_send(ptr: i32, len: i32) -> i64;
    }

    #[link(wasm_import_module = "alien_http_v1")]
    unsafe extern "C" {
        #[link_name = "send"]
        fn http_send(ptr: i32, len: i32) -> i64;
    }

    fn unpack(packed: i64) -> std::result::Result<Vec<u8>, String> {
        let packed = packed as u64;
        let ptr = (packed & 0xffff_ffff) as usize;
        let len = ((packed >> 32) & 0x7fff_ffff) as usize;
        // SAFETY: the host wrote `len` bytes at `ptr`, memory it got from `alien_alloc`.
        let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len) }.to_vec();
        if packed & ERROR_BIT != 0 {
            Err(String::from_utf8_lossy(&bytes).into_owned())
        } else {
            Ok(bytes)
        }
    }

    pub(super) fn cloud(request: &[u8]) -> std::result::Result<Vec<u8>, String> {
        // SAFETY: the host reads `len` bytes at `ptr`, which `request` holds for the call.
        unpack(unsafe { cloud_send(request.as_ptr() as i32, request.len() as i32) })
    }

    pub(super) fn http(request: &[u8]) -> std::result::Result<Vec<u8>, String> {
        // SAFETY: as in `cloud`.
        unpack(unsafe { http_send(request.as_ptr() as i32, request.len() as i32) })
    }
}

/// Built natively, the library has no host: a test answers its calls
/// (`controller_module::testing::answer`).
#[cfg(not(target_arch = "wasm32"))]
pub(super) mod host {
    use serde_json::Value;
    use std::cell::RefCell;

    pub(in super::super) type Answer = Box<dyn FnMut(&Value) -> std::result::Result<Value, String>>;

    thread_local! {
        pub(in super::super) static ANSWER: RefCell<Option<Answer>> = const { RefCell::new(None) };
    }

    fn answer(request: &[u8]) -> std::result::Result<Vec<u8>, String> {
        let request: Value = serde_json::from_slice(request).map_err(|e| e.to_string())?;
        ANSWER.with(|answer| match answer.borrow_mut().as_mut() {
            Some(answer) => answer(&request).map(|reply| reply.to_string().into_bytes()),
            None => Err(
                "refused: a controller's calls go through Alien, and a native build has none; step it with `controller_module::testing::answer`"
                    .to_string(),
            ),
        })
    }

    pub(super) fn cloud(request: &[u8]) -> std::result::Result<Vec<u8>, String> {
        answer(request)
    }

    pub(super) fn http(request: &[u8]) -> std::result::Result<Vec<u8>, String> {
        answer(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(body: &str, body_base64: Option<&str>) -> CloudResponse {
        CloudResponse {
            status: 200,
            headers: Vec::new(),
            body: body.to_string(),
            body_base64: body_base64.map(str::to_string),
        }
    }

    /// A body that doesn't read keeps the parser's error as its cause, so the step's message
    /// says where it stopped reading.
    #[test]
    fn a_body_that_does_not_read_keeps_the_parser_s_error() {
        let error = response("{\"zone\":", None)
            .json::<serde_json::Value>()
            .unwrap_err();
        assert_eq!(error.code, "UNEXPECTED_RESPONSE");
        assert!(error.message.contains("did not read as the type asked for"));
        assert!(error.source.is_some(), "{error}");

        let error = response("", Some("not base64!")).bytes().unwrap_err();
        assert_eq!(error.code, "EXTENSION_REQUEST_INVALID");
        assert!(error.source.is_some(), "{error}");
    }
}
