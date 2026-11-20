//! What a handler adds to the context in a controller module: an AWS SDK config whose HTTP
//! client hands each request to the host, and a dependency's outputs.
//!
//! The SDK signs with placeholder credentials. The client drops that signature and sends the
//! service, region, path, query, headers and body the SDK signed for, and the host signs again
//! with the step's real credentials.

use super::cloud::{Cloud, CloudRequest, CloudTarget};
use super::{ErrorData, ResourceControllerContext, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use aws_credential_types::provider::SharedCredentialsProvider;
use aws_credential_types::Credentials;
use aws_smithy_async::rt::sleep::{AsyncSleep, SharedAsyncSleep, Sleep};
use aws_smithy_runtime_api::client::behavior_version::BehaviorVersion;
use aws_smithy_runtime_api::client::http::{
    HttpClient, HttpConnector, HttpConnectorFuture, HttpConnectorSettings, SharedHttpConnector,
};
use aws_smithy_runtime_api::client::orchestrator::{HttpRequest, HttpResponse};
use aws_smithy_runtime_api::client::result::{ConnectorError, SdkError};
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_runtime_api::http::StatusCode;
use aws_smithy_types::body::SdkBody;
use aws_smithy_types::error::metadata::ProvideErrorMetadata;
use aws_types::region::Region;
use aws_types::sdk_config::{RetryConfig, StalledStreamProtectionConfig};
use serde::de::DeserializeOwned;
use std::future::Future;

/// What a controller's handler reads besides the context's own fields.
pub trait ControllerContextExt {
    /// An AWS SDK config for any `aws-sdk-*` client, for a crate that depends on the client
    /// with `default-features = false`: the host sends each request, so the client needs no
    /// HTTP stack and no runtime.
    ///
    /// The client sends each call once and never waits: Alien runs a failed step again when
    /// its error is retryable, as it does for its own controllers. Turn the SDK's error into
    /// one that says so with [`aws_sdk_error`]. `into_alien_error()` gives an error that is
    /// never retried, also for a throttled call or one that got no answer.
    ///
    /// The host finds a service by the first labels of its endpoint's host name, so an S3
    /// client needs `force_path_style(true)`.
    fn aws_sdk_config(&self) -> impl Future<Output = Result<aws_types::SdkConfig>> + Send;

    /// The outputs of `id`, a resource this one depends on, as `T`. A controller reads a
    /// dependency's outputs, never its saved state.
    fn dependency_outputs<T: DeserializeOwned>(&self, id: &str) -> Result<T>;
}

impl ControllerContextExt for ResourceControllerContext<'_> {
    fn aws_sdk_config(&self) -> impl Future<Output = Result<aws_types::SdkConfig>> + Send {
        let cloud = self.cloud().clone();
        async move {
            let Some(CloudTarget::Aws { region, .. }) = cloud.target() else {
                return Err(AlienError::new(ErrorData::CallRefused {
                    message: "the step is not for an AWS account".to_string(),
                }));
            };
            let region = Region::new(region.clone());
            Ok(aws_types::SdkConfig::builder()
                .behavior_version(BehaviorVersion::latest())
                .region(region)
                .credentials_provider(SharedCredentialsProvider::new(Credentials::new(
                    "PLACEHOLDER",
                    "placeholder",
                    None,
                    None,
                    "alien-host-signs",
                )))
                .http_client(HostClient(cloud))
                .sleep_impl(SharedAsyncSleep::new(NoWait))
                .retry_config(RetryConfig::disabled())
                .stalled_stream_protection(StalledStreamProtectionConfig::disabled())
                .build())
        }
    }

    fn dependency_outputs<T: DeserializeOwned>(&self, id: &str) -> Result<T> {
        let own = self.desired_config.id();
        let dependency = self
            .dependencies
            .get(id)
            .filter(|_| id != own)
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotFound {
                    resource_id: own.to_string(),
                    dependency_id: id.to_string(),
                })
            })?;
        let mut outputs = dependency
            .outputs
            .clone()
            .filter(|outputs| !outputs.is_null())
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: own.to_string(),
                    dependency_id: id.to_string(),
                })
            })?;
        if let Some(fields) = outputs.as_object_mut() {
            fields.remove("type");
        }
        serde_json::from_value(outputs).into_alien_error().context(
            ErrorData::ResourceConfigInvalid {
                message: format!("the outputs of '{id}' did not read as the type asked for"),
                resource_id: Some(own.to_string()),
            },
        )
    }
}

/// The module has no timer, so nothing in it waits: a step that must wait returns, and Alien
/// steps again after the delay it suggests.
#[derive(Debug)]
struct NoWait;

impl AsyncSleep for NoWait {
    fn sleep(&self, _: std::time::Duration) -> Sleep {
        Sleep::new(std::future::ready(()))
    }
}

#[derive(Debug)]
struct HostClient(Cloud);

impl HttpClient for HostClient {
    fn http_connector(
        &self,
        _: &HttpConnectorSettings,
        _: &RuntimeComponents,
    ) -> SharedHttpConnector {
        SharedHttpConnector::new(HostConnector(self.0.clone()))
    }
}

#[derive(Debug)]
struct HostConnector(Cloud);

impl HttpConnector for HostConnector {
    fn call(&self, request: HttpRequest) -> HttpConnectorFuture {
        HttpConnectorFuture::ready(through_host(&self.0, request))
    }
}

/// A request the connector could not make. It goes to the SDK as an Alien error, so
/// `aws_sdk_error` gives it back final: another try makes the same request.
fn other(message: String) -> ConnectorError {
    ConnectorError::other(
        Box::new(AlienError::new(ErrorData::RequestInvalid { message })),
        None,
    )
}

/// AWS error codes that mean the call was throttled, as the AWS SDKs list them.
const THROTTLING_CODES: [&str; 14] = [
    "Throttling",
    "ThrottlingException",
    "ThrottledException",
    "RequestThrottledException",
    "TooManyRequestsException",
    "ProvisionedThroughputExceededException",
    "TransactionInProgressException",
    "RequestLimitExceeded",
    "BandwidthLimitExceeded",
    "LimitExceededException",
    "RequestThrottled",
    "SlowDown",
    "PriorRequestNotComplete",
    "EC2ThrottledException",
];

/// The error of an AWS SDK call, with the code and the retry flag Alien's own AWS clients give
/// the same answer: `RATE_LIMIT_EXCEEDED` for a throttled call, `REMOTE_RESOURCE_NOT_FOUND`
/// for a 404, and so on, each retryable; `HTTP_REQUEST_FAILED`, retryable, for a call that got
/// no answer; and the host's own refusal, which is not.
///
/// ```ignore
/// let created = request.send().await.map_err(aws_sdk_error).context(failed())?;
/// ```
pub fn aws_sdk_error<E>(error: SdkError<E, HttpResponse>) -> AlienError<ErrorData>
where
    E: ProvideErrorMetadata + std::error::Error + 'static,
{
    use std::error::Error;
    // The error `Cloud::send` gave the SDK's connector, as it was.
    let from_host = |source: Option<&(dyn Error + 'static)>| {
        source
            .and_then(|source| source.downcast_ref::<AlienError<ErrorData>>())
            .cloned()
    };
    match &error {
        SdkError::ServiceError(service) => {
            let status = service.raw().status().as_u16();
            let code = service.err().code().unwrap_or("no error code");
            let message = format!(
                "AWS answered {status} ({code}): {}",
                service.err().message().unwrap_or_default()
            );
            AlienError::new(match status {
                _ if THROTTLING_CODES.contains(&code) => ErrorData::RateLimitExceeded { message },
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
        SdkError::DispatchFailure(failure) => from_host(
            failure
                .as_connector_error()
                .and_then(|connector| connector.source()),
        )
        .unwrap_or_else(|| {
            AlienError::new(ErrorData::HttpRequestFailed {
                message: format!("{failure:?}"),
            })
        }),
        // The answer did not read as the service's, or the SDK gave up on it: the call may
        // have happened.
        SdkError::ResponseError(_) | SdkError::TimeoutError(_) => {
            AlienError::new(ErrorData::HttpRequestFailed {
                message: error.to_string(),
            })
        }
        // The SDK refused the request before it sent anything.
        _ => AlienError::new(ErrorData::RequestInvalid {
            message: format!("the AWS SDK did not send the request: {error:?}"),
        }),
    }
}

/// The headers the host signs or sets itself.
const DROPPED: [&str; 6] = [
    "authorization",
    "x-amz-date",
    "x-amz-security-token",
    "x-amz-content-sha256",
    "host",
    "content-length",
];

fn through_host(
    cloud: &Cloud,
    request: HttpRequest,
) -> std::result::Result<HttpResponse, ConnectorError> {
    // `Credential=<key>/<date>/<region>/<service>/aws4_request` names the region the SDK
    // signed for, which for a global service is not the client's.
    let authorization = request
        .headers()
        .get("authorization")
        .ok_or_else(|| other("the SDK sent an unsigned request".to_string()))?;
    let scope = authorization
        .split("Credential=")
        .nth(1)
        .and_then(|rest| rest.split(',').next())
        .ok_or_else(|| other("the request has no credential scope".to_string()))?;
    let parts: Vec<&str> = scope.split('/').collect();
    let [_, _, region, _, ..] = parts.as_slice() else {
        return Err(other(format!("unexpected credential scope {scope}")));
    };
    let uri = url::Url::parse(request.uri()).map_err(|e| other(e.to_string()))?;
    let prefix = endpoint_prefix(uri.host_str().unwrap_or_default(), region);
    let path = raw_path(request.uri());
    let mut sent = CloudRequest::aws(
        prefix,
        request.method(),
        if path.is_empty() { "/" } else { path },
    )
    .region(region);
    for (key, value) in uri.query_pairs() {
        sent = sent.query(&key, &value);
    }
    for (name, value) in request.headers() {
        if !DROPPED.contains(&name.to_ascii_lowercase().as_str()) {
            sent = sent.header(name, value);
        }
    }
    if let Some(body) = request_body(request.body()).map_err(other)? {
        sent = sent.body_bytes(body);
    }
    // The error goes to the SDK as it is, so `aws_sdk_error` can give it back.
    let from_host = |error: AlienError<ErrorData>| ConnectorError::other(Box::new(error), None);
    let response = cloud.send(&sent).map_err(from_host)?;
    let status = StatusCode::try_from(response.status).map_err(|e| other(e.to_string()))?;
    let body = response.bytes().map_err(from_host)?;
    let mut out = HttpResponse::new(status, SdkBody::from(body));
    for (name, value) in response.headers {
        out.headers_mut().insert(name, value);
    }
    Ok(out)
}

/// The bytes `body` holds, or `None` when it is empty. A streaming body has no bytes to read
/// here, and sending the call without them would upload nothing, so it is refused.
fn request_body(body: &SdkBody) -> std::result::Result<Option<&[u8]>, String> {
    match body.bytes() {
        Some(bytes) if bytes.is_empty() => Ok(None),
        Some(bytes) => Ok(Some(bytes)),
        None => Err(
            "a streaming request body can't be sent through Alien; build the body from bytes"
                .to_string(),
        ),
    }
}

/// The endpoint prefix the host's table knows a service by: its host name without the region
/// and AWS's domain, `amazonaws.com` or `api.aws`, so `api.ecr` for
/// `api.ecr.eu-west-1.amazonaws.com`, `eks-auth` for `eks-auth.eu-west-1.api.aws` and `route53`
/// for `route53.amazonaws.com`. A host name of another shape gives a prefix the host refuses.
fn endpoint_prefix<'a>(host: &'a str, region: &str) -> &'a str {
    let service = host
        .strip_suffix(".amazonaws.com")
        .or_else(|| host.strip_suffix(".api.aws"))
        .unwrap_or(host);
    service
        .strip_suffix(region)
        .and_then(|service| service.strip_suffix('.'))
        .unwrap_or(service)
}

/// The path of `uri` as the SDK wrote it. `url::Url` resolves `.` and `..` segments, which would
/// turn a request for S3 key `a/../b` into one for `b` before the host's check could refuse it.
fn raw_path(uri: &str) -> &str {
    let after_scheme = uri.split_once("://").map_or(uri, |(_, rest)| rest);
    let rest = &after_scheme[after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len())..];
    if !rest.starts_with('/') {
        return "";
    }
    rest.split(['?', '#']).next().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{endpoint_prefix, raw_path, request_body};

    /// Dot segments reach the host as the SDK wrote them, so it refuses them instead of acting
    /// on another key.
    #[test]
    fn the_path_keeps_its_dot_segments() {
        assert_eq!(
            raw_path("https://s3.eu-west-1.amazonaws.com/bucket/a/../b?x-id=DeleteObject"),
            "/bucket/a/../b"
        );
        assert_eq!(
            raw_path("https://s3.eu-west-1.amazonaws.com/bucket/./key"),
            "/bucket/./key"
        );
        assert_eq!(raw_path("https://sts.amazonaws.com"), "");
        assert_eq!(raw_path("https://sts.amazonaws.com?Action=Get&Next=/x"), "");
    }
    use aws_smithy_types::body::SdkBody;

    /// A body in memory is sent as its bytes, an empty one as none, and a streaming one, which
    /// has no bytes to read here, is refused rather than sent empty.
    #[test]
    fn a_streaming_body_is_refused_rather_than_sent_empty() {
        assert_eq!(
            request_body(&SdkBody::from("data")).unwrap(),
            Some(&b"data"[..])
        );
        assert_eq!(request_body(&SdkBody::empty()).unwrap(), None);
        // Like a stream, a taken body has no bytes in memory.
        assert!(request_body(&SdkBody::taken()).is_err());
    }

    /// The prefix is every label before the region and AWS's domain, so a service whose
    /// prefix has a dot is found, and a global service's host name has no region to drop.
    #[test]
    fn the_endpoint_prefix_is_the_host_name_without_the_region_and_the_domain() {
        for (host, region, prefix) in [
            ("sts.eu-west-1.amazonaws.com", "eu-west-1", "sts"),
            ("api.ecr.eu-west-1.amazonaws.com", "eu-west-1", "api.ecr"),
            ("route53.amazonaws.com", "us-east-1", "route53"),
            ("eks-auth.eu-west-1.api.aws", "eu-west-1", "eks-auth"),
            (
                "notifications-contacts.us-east-1.api.aws",
                "us-east-1",
                "notifications-contacts",
            ),
            ("s3.eu-west-1.amazonaws.com", "eu-west-1", "s3"),
            // A bucket in the host name is no prefix the host knows, so it refuses the call.
            (
                "bucket.s3.eu-west-1.amazonaws.com",
                "eu-west-1",
                "bucket.s3",
            ),
        ] {
            assert_eq!(endpoint_prefix(host, region), prefix, "{host}");
        }
    }
}
