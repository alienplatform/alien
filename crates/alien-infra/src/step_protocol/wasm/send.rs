//! What one step's module may call: the cloud its step's credentials are for, signed here, and
//! the origins its extension declared, with no credential of the deployment's.

use super::egress::{self, HttpRequest};
use super::endpoint::{self, CloudRequest, CloudResponse, Resolved};
use super::{Interceptors, WasmHost, CALLS_PER_STEP, MAX_RESPONSE_BYTES};
use crate::step_protocol::protocol::Credentials;
use alien_client_core::interceptor::{Cloud, ResponseHook};
use alien_core::Platform;
use alien_hooks_runtime::Capabilities;
use base64::Engine;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::{ResponseBuilderExt, StatusCode};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) struct StepCapabilities {
    /// The step runs on a blocking thread; its calls run on the runtime that started it.
    pub(super) runtime: tokio::runtime::Handle,
    pub(super) host: Arc<WasmHost>,
    pub(super) credentials: Option<Credentials>,
    pub(super) egress: Arc<[String]>,
    pub(super) interceptors: Interceptors,
    pub(super) ends_at: Instant,
    /// Set when a call ended at the step's deadline or came after it. The module can't tell
    /// that from a call that failed for good, so the host decides what the step's outcome is.
    pub(super) deadline_hit: Arc<AtomicBool>,
    /// Set when an interceptor refused the response to a call that was sent. The call
    /// happened, so a step the module fails after it is not sent again to repeat it.
    pub(super) response_refused: Arc<AtomicBool>,
    /// Set once the step's caller stops waiting for its reply. A call not sent by then is
    /// refused; one already sent runs to its end.
    pub(super) abandoned: Arc<AtomicBool>,
    pub(super) calls: usize,
}

/// A request ready to send, with what its response goes through before the module reads it.
struct Prepared {
    client: reqwest::Client,
    request: reqwest::Request,
    hook: Option<ResponseHook>,
    /// What the host attached to the request, which the response is cleared of.
    secrets: Vec<String>,
}

/// A response read whole, up to [`MAX_RESPONSE_BYTES`].
struct Read {
    status: StatusCode,
    url: reqwest::Url,
    headers: HeaderMap,
    body: Vec<u8>,
}

fn refused(reason: impl std::fmt::Display) -> String {
    format!("refused: {reason}")
}

/// An interceptor's failure before the call was sent. One the interceptor marked retryable, as
/// a hook stopped at its deadline is, is a call the module may try again.
fn interceptor_failed(error: alien_client_core::Error) -> String {
    if error.retryable {
        format!("failed: {}", error.message)
    } else {
        refused(error.message)
    }
}

impl StepCapabilities {
    /// Counts the call, and returns how long it may take. Nothing stops a module while the
    /// host is in a call for it, so no call outlasts the step's deadline.
    fn admit(&mut self) -> Result<Duration, String> {
        if self.abandoned.load(Ordering::Relaxed) {
            return Err(refused("nothing waits for the step's reply any more"));
        }
        if self.calls >= CALLS_PER_STEP {
            return Err(refused(format!(
                "a step may send {CALLS_PER_STEP} requests"
            )));
        }
        self.calls += 1;
        let time_left = self.ends_at.saturating_duration_since(Instant::now());
        if time_left.is_zero() {
            self.deadline_hit.store(true, Ordering::Relaxed);
            return Err(refused("the step's deadline has passed"));
        }
        Ok(time_left)
    }

    fn cloud_call(&self, request: &CloudRequest) -> Result<Prepared, String> {
        let credentials = self
            .credentials
            .as_ref()
            .ok_or_else(|| {
                refused(
                    "the step has no cloud credentials: a module makes no cloud calls in direct setup or for an extension without an identity",
                )
            })?;
        let (cloud, region, account) = match credentials {
            Credentials::Aws { region, .. } => (Platform::Aws, region.as_str(), ""),
            Credentials::Gcp {
                region, project_id, ..
            } => (Platform::Gcp, region.as_str(), project_id.as_str()),
            Credentials::Azure {
                subscription_id, ..
            } => (Platform::Azure, "", subscription_id.as_str()),
        };
        let resolved = endpoint::resolve(request, cloud, region, account).map_err(refused)?;
        let client = self.host.client.clone();
        let body = request.body_bytes().map_err(refused)?;
        let builder = builder(&client, request, &resolved, body.clone())?;
        let (mut prepared, secrets) = match credentials {
            Credentials::Aws {
                access_key_id,
                secret_access_key,
                session_token,
                ..
            } => (
                self.signed_for_aws(
                    builder,
                    &body.unwrap_or_default(),
                    &resolved,
                    access_key_id,
                    secret_access_key,
                    session_token.as_deref(),
                )?,
                vec![
                    secret_access_key.clone(),
                    session_token.clone().unwrap_or_default(),
                ],
            ),
            Credentials::Gcp { access_token, .. } => (
                with_bearer(
                    builder,
                    (self.interceptors)(Cloud::Gcp, None),
                    Cloud::Gcp,
                    &resolved,
                    access_token,
                )?,
                vec![access_token.clone()],
            ),
            Credentials::Azure {
                access_token,
                tenant_id,
                ..
            } => (
                with_bearer(
                    builder,
                    (self.interceptors)(Cloud::Azure, Some(tenant_id)),
                    Cloud::Azure,
                    &resolved,
                    access_token,
                )?,
                vec![access_token.clone()],
            ),
        };
        // On AWS the interceptors run inside the signer, so the request is checked signed; it
        // is sent only if it passes.
        endpoint::check_sent(request, &resolved, account, &prepared.0).map_err(refused)?;
        self.reroute_for_tests(&resolved, &mut prepared.0)?;
        Ok(Prepared {
            client,
            request: prepared.0,
            hook: prepared.1,
            secrets,
        })
    }

    /// Runs the deployment's interceptors on the request and signs it, through the signer
    /// Alien's own AWS clients use.
    #[cfg(feature = "aws")]
    fn signed_for_aws(
        &self,
        builder: reqwest::RequestBuilder,
        body: &[u8],
        resolved: &Resolved,
        access_key_id: &str,
        secret_access_key: &str,
        session_token: Option<&str>,
    ) -> Result<(reqwest::Request, Option<ResponseHook>), String> {
        use alien_aws_clients::aws_request_utils::{
            AwsRequestBuilderExt, AwsRequestSigner, AwsSignConfig,
        };
        let Some((signing_region, signing_name)) = resolved.aws_signing.clone() else {
            return Err(refused("the endpoint has no signing name"));
        };
        let config = AwsSignConfig {
            service_name: signing_name,
            region: signing_region,
            credentials: aws_credential_types::Credentials::new(
                access_key_id,
                secret_access_key,
                session_token.map(str::to_string),
                None,
                "extension-step",
            ),
            signing_region: None,
            interceptors: (self.interceptors)(Cloud::Aws, None),
        };
        // S3 takes no request without the digest of its body, and a module may not set the
        // header: it is signed.
        let (builder, hook) = builder
            .content_sha256_bytes(body)
            .sign_aws_request(&config)
            .map_err(interceptor_failed)?
            .into_parts();
        let request = builder
            .build()
            .map_err(|error| refused(error.without_url()))?;
        Ok((request, hook))
    }

    #[cfg(not(feature = "aws"))]
    fn signed_for_aws(
        &self,
        _: reqwest::RequestBuilder,
        _: &[u8],
        _: &Resolved,
        _: &str,
        _: &str,
        _: Option<&str>,
    ) -> Result<(reqwest::Request, Option<ResponseHook>), String> {
        Err(refused("this build of Alien signs no AWS requests"))
    }

    fn http_call(&self, request: &HttpRequest) -> Result<(reqwest::Url, Prepared), String> {
        let (url, body) =
            egress::check(request, &self.egress, self.host.local_egress()).map_err(refused)?;
        let method = reqwest::Method::from_bytes(request.method.as_bytes()).map_err(refused)?;
        // Any client builds the request; `send_to_origin` sends it on one that reaches only
        // the addresses the origin resolves to.
        let mut builder = self.host.client.request(method, url.clone());
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        if let Some(body) = body {
            builder = builder.body(body);
        }
        let request = builder
            .build()
            .map_err(|error| refused(error.without_url()))?;
        Ok((
            url,
            Prepared {
                client: self.host.client.clone(),
                request,
                hook: None,
                secrets: Vec::new(),
            },
        ))
    }

    #[cfg_attr(not(any(test, feature = "test-utils")), allow(unused_variables))]
    fn reroute_for_tests(
        &self,
        resolved: &Resolved,
        request: &mut reqwest::Request,
    ) -> Result<(), String> {
        #[cfg(any(test, feature = "test-utils"))]
        if let Some(stub) = self
            .host
            .testing
            .routes
            .get(&resolved.url.origin().ascii_serialization())
        {
            let mut url = reqwest::Url::parse(stub).map_err(|e| e.to_string())?;
            url.set_path(request.url().path());
            url.set_query(request.url().query());
            *request.url_mut() = url;
        }
        Ok(())
    }

    /// Sends `prepared` and returns what the module reads, within what is left of the step.
    fn send(&self, prepared: Prepared) -> Result<CloudResponse, String> {
        if self.abandoned.load(Ordering::Relaxed) {
            return Err(refused("nothing waits for the step's reply any more"));
        }
        // Counted after the interceptors, or the origin's lookup, ran.
        let time_left = self.ends_at.saturating_duration_since(Instant::now());
        if time_left.is_zero() {
            self.deadline_hit.store(true, Ordering::Relaxed);
            return Err(refused("the step's deadline has passed"));
        }
        let Prepared {
            client,
            request,
            hook,
            secrets,
        } = prepared;
        self.runtime.block_on(async move {
            let exchange = async {
                // Cleared before any hook runs: a hook could encode an echoed credential into
                // its output, where the later clearing would no longer find it.
                let read = scrubbed(read(client.execute(request).await).await?, &secrets);
                // The interceptors that matched the request see its response before the module
                // does, as on Alien's own calls.
                let read = match &hook {
                    Some(hook) => intercepted(hook, read, &self.response_refused).await?,
                    None => read,
                };
                Ok(for_module(read, &secrets))
            };
            tokio::time::timeout(time_left, exchange)
                .await
                .unwrap_or_else(|_| {
                    self.deadline_hit.store(true, Ordering::Relaxed);
                    Err("failed: no answer before the step's deadline".to_string())
                })
        })
    }

    /// Resolves the declared origin now and sends `prepared` only to public addresses it
    /// resolves to.
    fn send_to_origin(
        &self,
        url: reqwest::Url,
        prepared: Prepared,
        time_left: Duration,
    ) -> Result<CloudResponse, String> {
        let allow_local = self.host.local_egress();
        let pinned = self.runtime.block_on(async {
            tokio::time::timeout(
                time_left,
                crate::public_host::pinned_client(
                    &url,
                    reqwest::Client::builder().connect_timeout(super::CONNECT_TIMEOUT),
                    allow_local,
                ),
            )
            .await
        });
        let client = match pinned {
            Ok(Ok(client)) => client,
            Ok(Err(crate::public_host::Refusal::Policy(reason))) => return Err(refused(reason)),
            Ok(Err(crate::public_host::Refusal::Unreachable(reason))) => {
                return Err(format!("failed: {reason}"))
            }
            Err(_) => {
                self.deadline_hit.store(true, Ordering::Relaxed);
                return Err("failed: no answer before the step's deadline".to_string());
            }
        };
        self.send(Prepared { client, ..prepared })
    }
}

/// The request as the module wrote it, on the URL the host resolved.
fn builder(
    client: &reqwest::Client,
    request: &CloudRequest,
    resolved: &Resolved,
    body: Option<Vec<u8>>,
) -> Result<reqwest::RequestBuilder, String> {
    let method = reqwest::Method::from_bytes(request.method.as_bytes()).map_err(refused)?;
    let mut builder = client.request(method, resolved.url.clone());
    for (name, value) in &request.headers {
        builder = builder.header(name, value);
    }
    if let Some(body) = body {
        builder = builder.body(body);
    }
    Ok(builder)
}

/// Runs `chain` on the request, then attaches the token, so no interceptor sees it: the order
/// Alien's Google Cloud and Kubernetes clients keep.
fn with_bearer(
    builder: reqwest::RequestBuilder,
    chain: alien_client_core::interceptor::InterceptorChain,
    cloud: Cloud,
    resolved: &Resolved,
    token: &str,
) -> Result<(reqwest::Request, Option<ResponseHook>), String> {
    let mut request = builder
        .build()
        .map_err(|error| refused(error.without_url()))?;
    let hook = chain
        .intercept(cloud, &resolved.intercepted_service, &mut request)
        .map_err(interceptor_failed)?;
    let mut authorization = HeaderValue::from_str(&format!("Bearer {token}"))
        .map_err(|_| refused("the step's token is not a header value"))?;
    authorization.set_sensitive(true);
    request.headers_mut().insert(AUTHORIZATION, authorization);
    Ok((request, hook))
}

/// Reads the response's body up to [`MAX_RESPONSE_BYTES`], before anything buffers it whole.
async fn read(sent: reqwest::Result<reqwest::Response>) -> Result<Read, String> {
    let mut response = sent.map_err(|error| format!("failed: {}", error.without_url()))?;
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("failed: {}", error.without_url()))?
    {
        if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(refused(format!(
                "the response is larger than {MAX_RESPONSE_BYTES} bytes"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Read {
        status: response.status(),
        url: response.url().clone(),
        headers: response.headers().clone(),
        body,
    })
}

/// Passes a response through the interceptors that matched its request.
async fn intercepted(
    hook: &ResponseHook,
    read: Read,
    response_refused: &AtomicBool,
) -> Result<Read, String> {
    let mut response = http::Response::builder()
        .status(read.status)
        .url(read.url)
        .body(read.body)
        .map_err(|error| format!("failed: {error}"))?;
    *response.headers_mut() = read.headers;
    let response = hook
        .apply(reqwest::Response::from(response))
        .await
        .map_err(|error| {
            response_refused.store(true, Ordering::Relaxed);
            format!(
                "failed: the call was sent, and its response was refused: {}",
                error.message
            )
        })?;
    self::read(Ok(response)).await
}

/// The response as the module reads it, with every secret the host put on the request blanked:
/// a cloud's error can quote the request it refused, as AWS quotes the signed headers, session
/// token included, when a signature does not match.
/// `read` with each of `secrets` cleared from its headers and body.
fn scrubbed(read: Read, secrets: &[String]) -> Read {
    let clear = |bytes: &[u8]| {
        secrets
            .iter()
            .filter(|secret| !secret.is_empty())
            .fold(bytes.to_vec(), |bytes, secret| {
                blank(bytes, secret.as_bytes())
            })
    };
    let mut headers = HeaderMap::with_capacity(read.headers.len());
    for (name, value) in &read.headers {
        // A cleared value is the original with `[redacted]` in place, so it stays a valid header.
        if let Ok(value) = HeaderValue::from_bytes(&clear(value.as_bytes())) {
            headers.append(name.clone(), value);
        }
    }
    Read {
        status: read.status,
        url: read.url,
        headers,
        body: clear(&read.body),
    }
}

fn for_module(read: Read, secrets: &[String]) -> CloudResponse {
    let clear = |bytes: &[u8]| {
        secrets
            .iter()
            .filter(|secret| !secret.is_empty())
            .fold(bytes.to_vec(), |bytes, secret| {
                blank(bytes, secret.as_bytes())
            })
    };
    let headers = read
        .headers
        .iter()
        .map(|(name, value)| {
            (
                name.to_string(),
                String::from_utf8_lossy(&clear(value.as_bytes())).into_owned(),
            )
        })
        .collect();
    let (body, body_base64) = match String::from_utf8(clear(&read.body)) {
        Ok(text) => (text, None),
        Err(binary) => (
            String::new(),
            Some(base64::engine::general_purpose::STANDARD.encode(binary.into_bytes())),
        ),
    };
    CloudResponse {
        status: read.status.as_u16(),
        headers,
        body,
        body_base64,
    }
}

/// `bytes` with every occurrence of `secret` replaced.
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

impl Capabilities for StepCapabilities {
    fn cloud(&mut self, request: &[u8]) -> Result<Vec<u8>, String> {
        let request: CloudRequest = serde_json::from_slice(request)
            .map_err(|error| refused(format!("not a cloud request: {error}")))?;
        self.admit()?;
        let prepared = self.cloud_call(&request)?;
        let response = self.send(prepared)?;
        serde_json::to_vec(&response).map_err(|error| format!("failed: {error}"))
    }

    fn http(&mut self, request: &[u8]) -> Result<Vec<u8>, String> {
        let request: HttpRequest = serde_json::from_slice(request)
            .map_err(|error| refused(format!("not an http request: {error}")))?;
        let time_left = self.admit()?;
        let (url, prepared) = self.http_call(&request)?;
        let response = self.send_to_origin(url, prepared, time_left)?;
        serde_json::to_vec(&response).map_err(|error| format!("failed: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_of(body: Vec<u8>) -> Read {
        let mut headers = HeaderMap::new();
        headers.insert("x-echo", HeaderValue::from_static("token=the-token;"));
        Read {
            status: StatusCode::OK,
            url: reqwest::Url::parse("https://sts.amazonaws.com/").unwrap(),
            headers,
            body,
        }
    }

    #[test]
    fn a_text_body_stays_text_and_any_other_bytes_arrive_in_base64() {
        let text = for_module(read_of(b"{\"ok\":true}".to_vec()), &[]);
        assert_eq!(text.body, "{\"ok\":true}");
        assert_eq!(text.body_base64, None);

        let binary = for_module(read_of(vec![0, 159, 255, 10]), &[]);
        assert_eq!(binary.body, "");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(binary.body_base64.unwrap())
            .unwrap();
        assert_eq!(decoded, vec![0, 159, 255, 10]);
    }

    /// A response that quotes what the host attached reaches the module without it, in a
    /// header, in a text body and in a binary one, however often it is quoted.
    #[test]
    fn a_response_that_quotes_the_step_s_secret_reaches_the_module_without_it() {
        let secrets = ["the-token".to_string(), String::new()];
        let text = for_module(
            read_of(b"x-amz-security-token:the-token\nagain the-token.".to_vec()),
            &secrets,
        );
        assert_eq!(
            text.body,
            "x-amz-security-token:[redacted]\nagain [redacted]."
        );
        assert_eq!(text.headers[0].1, "token=[redacted];");

        let mut binary = vec![0, 159];
        binary.extend_from_slice(b"the-token");
        binary.push(255);
        let cleared = for_module(read_of(binary), &secrets);
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(cleared.body_base64.unwrap())
            .unwrap();
        let mut expected = vec![0, 159];
        expected.extend_from_slice(b"[redacted]");
        expected.push(255);
        assert_eq!(decoded, expected);
    }
}
