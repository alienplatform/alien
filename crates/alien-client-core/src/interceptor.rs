//! Interceptors: an extension's code that changes a cloud call after Alien builds it and before
//! Alien signs it, and changes the response before Alien reads it.
//!
//! A deployment's chain travels on the clients built for it, never on a client config or in a
//! global, so two deployments in one process never see each other's interceptors. An
//! interceptor never sees a credential, and Alien refuses any change to where a call goes or to
//! a header that carries or protects a credential.

use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use base64::Engine;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_LENGTH};
use reqwest::{Method, Url};
#[cfg(not(target_arch = "wasm32"))]
use reqwest::{ResponseBuilderExt, StatusCode};
use sha2::{Digest, Sha256};
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

/// Headers an interceptor may not set, change or remove: they pick the destination, carry a
/// credential, or are signed with one.
const PROTECTED_HEADERS: [&str; 5] = [
    "host",
    "authorization",
    "x-amz-date",
    "x-amz-security-token",
    "proxy-authorization",
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

        Ok(Some(ResponseHook {
            matched: matched.into(),
            request: Arc::new(view),
        }))
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
    /// Reads the whole response, lets each interceptor that matched the call change it, and
    /// returns a response Alien reads as if the cloud had sent it.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn apply(&self, response: reqwest::Response) -> Result<reqwest::Response> {
        let status = response.status();
        let url = response.url().clone();
        let headers = response.headers().clone();
        let bytes =
            response
                .bytes()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Failed to read response body".to_string(),
                })?;
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

        let mut view = HookResponse {
            status: status.as_u16(),
            headers,
            body,
        };
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
            if StatusCode::from_u16(view.status).is_err() {
                return Err(failed(
                    interceptor.extension(),
                    format!("{} is not an HTTP status", view.status),
                ));
            }
        }

        let mut headers = view.headers;
        // The body's own length decides it; a stale one would cut or stall the read.
        headers.remove(CONTENT_LENGTH);
        let mut rebuilt = http::Response::builder()
            .status(view.status)
            .url(url)
            .body(view.body)
            .into_alien_error()
            .context(ErrorData::GenericError {
                message: "Failed to rebuild an intercepted response".to_string(),
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
    if changed.url.origin() != original.url.origin() {
        return Some(format!(
            "it moved the call from {} to {}; it may change the path and query, not the scheme, host or port",
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
    PROTECTED_HEADERS
        .iter()
        .find(|name| {
            !original
                .headers
                .get_all(**name)
                .iter()
                .eq(changed.headers.get_all(**name).iter())
        })
        .map(|name| format!("it set, changed or removed the '{name}' header"))
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

    #[test]
    fn the_path_query_method_and_other_headers_can_change() {
        let interceptors = chain(vec![TestInterceptor::new("acme", Cloud::Aws, "rds")
            .on_request(|r| {
                r.url.set_path("/other");
                r.url.set_query(Some("extension=1"));
                r.method = Method::PUT;
                r.headers
                    .insert("x-extension", HeaderValue::from_static("acme"));
                Ok(())
            })]);
        let mut request = rds_request("Action=CreateDBCluster");
        interceptors
            .intercept(Cloud::Aws, "rds", &mut request)
            .unwrap()
            .expect("the interceptor matched");
        assert_eq!(
            request.url().as_str(),
            "https://rds.us-east-1.amazonaws.com/other?extension=1"
        );
        assert_eq!(request.method(), Method::PUT);
        assert_eq!(request.headers()["x-extension"], "acme");
        assert_eq!(body_text(&request), "Action=CreateDBCluster");
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
