//! What the cloud itself said, recovered from a client error's chain.
//!
//! A wrapper that quotes only its own message names the operation and not the quota, role or ARN
//! that refused it. The transport records a refused call's response body on the chain, so the
//! sentence the service wrote is there to be lifted into the wrapper's `reason`.

use alien_error::{AlienError, AlienErrorData, ContextError};
use serde::Serialize;

use crate::error::{ErrorData, Result};

/// Key the transport records a refused call's response body under. The error derive builds
/// `context` from raw field names, so this tracks `HttpResponseError`'s own field name.
const HTTP_RESPONSE_TEXT: &str = "http_response_text";

/// The longest run of cloud text a `reason` carries. A response body has no limit of its own,
/// and the operation this is appended to has already been named.
const DETAIL_LIMIT: usize = 300;

/// What the cloud refused with, as one line: the client's own classification followed by the
/// service message out of the response body it captured, or by the innermost cause when nothing
/// captured one.
///
/// Private, because it is sound only under `unreachable`'s inherited visibility: text from a
/// layer the client marked internal is quoted only when `error` is internal too, so the wrapper
/// a caller sees is never more public than what it now carries. A variant fixing
/// `internal = "false"` would publish it.
fn cloud_reason<E>(error: &AlienError<E>) -> String
where
    E: AlienErrorData + Clone + std::fmt::Debug + Serialize,
{
    match cloud_detail(error) {
        Some(detail) if detail != error.message => {
            format!("{}: {}", error.message, clipped(&detail))
        }
        _ => error.message.clone(),
    }
}

/// Wraps a client failure as `SandboxUnreachable`, carrying what the cloud refused with.
///
/// `what` names the call in the binding's own terms; the cloud's own sentence follows it.
pub(crate) fn unreachable<E>(
    error: AlienError<E>,
    operation: &str,
    what: &str,
) -> AlienError<ErrorData>
where
    E: AlienErrorData + Clone + std::fmt::Debug + Serialize + Send + Sync + 'static,
{
    let reason = format!("{what}: {}", cloud_reason(&error));
    error.context(ErrorData::SandboxUnreachable {
        operation: operation.to_string(),
        reason,
    })
}

/// `.unreachable(…)` at a call site, where `.context(ErrorData::SandboxUnreachable { … })` stood.
pub(crate) trait Unreachable<T> {
    fn unreachable(self, operation: &str, what: &str) -> Result<T>;
}

impl<T, E> Unreachable<T> for std::result::Result<T, AlienError<E>>
where
    E: AlienErrorData + Clone + std::fmt::Debug + Serialize + Send + Sync + 'static,
{
    fn unreachable(self, operation: &str, what: &str) -> Result<T> {
        self.map_err(|error| unreachable(error, operation, what))
    }
}

/// The most specific thing any layer of the chain says, subject to the visibility rule above.
fn cloud_detail<E>(error: &AlienError<E>) -> Option<String>
where
    E: AlienErrorData + Clone + std::fmt::Debug + Serialize,
{
    // A layer is quotable only when the wrapper is internal too, the head included: nothing
    // says a client's public variants never carry a captured response body.
    let quotable = error.internal;
    let mut service = quotable
        .then(|| service_message(error.context.as_ref()))
        .flatten();
    let mut innermost = None;

    let mut layer = error.source.as_deref();
    while let Some(current) = layer {
        if quotable || !current.internal {
            if let Some(message) = service_message(current.context.as_ref()) {
                service = Some(message);
            }
            innermost = Some(current.message.clone());
        }
        layer = current.source.as_deref();
    }

    service.or(innermost)
}

/// The service's own sentence out of a captured JSON error body — AWS answers `{"message": …}`
/// and GCP and Azure nest the same field under `error`. A body that is not JSON is left to the
/// chain's innermost message instead: the first line of an HTML error page says less.
fn service_message(context: Option<&serde_json::Value>) -> Option<String> {
    let body = context?.get(HTTP_RESPONSE_TEXT)?.as_str()?;
    let body: serde_json::Value = serde_json::from_str(body).ok()?;
    message_field(&body).or_else(|| message_field(body.get("error")?))
}

fn message_field(body: &serde_json::Value) -> Option<String> {
    ["message", "Message"]
        .into_iter()
        .find_map(|key| Some(body.get(key)?.as_str()?.trim().to_string()))
        .filter(|message| !message.is_empty())
}

fn clipped(text: &str) -> String {
    let text = text.trim();
    if text.len() <= DETAIL_LIMIT {
        return text.to_string();
    }
    let end = (0..=DETAIL_LIMIT)
        .rev()
        .find(|at| text.is_char_boundary(*at))
        .unwrap_or(0);
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_client_core::ErrorData as ClientErrorData;

    /// The body AWS answered the failure this exists for with, verbatim but for the account id.
    const REFUSED_BODY: &str = r#"{"Message":"User: arn:aws:sts::123456789012:assumed-role/stack-access/session is not authorized to perform: lambda:PassNetworkConnector on resource: arn:aws:lambda:us-east-2:aws:network-connector:aws-network-connector:INTERNET_EGRESS"}"#;

    /// A refused call as the clients build one: the transport records the response, and the
    /// client's own classification wraps it.
    fn refused(status: u16, body: &str, classification: &str) -> AlienError<ClientErrorData> {
        AlienError::new(ClientErrorData::HttpResponseError {
            message: format!("Request failed with HTTP {status}"),
            url: "https://lambda.us-east-2.amazonaws.com/2025-09-09/microvms".to_string(),
            http_status: status,
            http_request_text: None,
            http_response_text: Some(body.to_string()),
        })
        .context(ClientErrorData::GenericError {
            message: classification.to_string(),
        })
    }

    /// The whole point: the sentence AWS wrote has to survive into the line a caller reads.
    #[test]
    fn the_service_message_is_lifted_out_of_the_refused_calls_body() {
        let reason = cloud_reason(&refused(
            403,
            REFUSED_BODY,
            "Lambda MicroVMs RunMicrovm failed",
        ));

        assert!(
            reason.contains("lambda:PassNetworkConnector"),
            "the refused action is what sends a reader to the role rather than the code: {reason}"
        );
        assert!(
            reason.starts_with("Lambda MicroVMs RunMicrovm failed: "),
            "the client's own classification stays in front of it: {reason}"
        );
    }

    /// The rule the interpolation rests on: a wrapper carrying an internal cause is itself
    /// internal, so `into_external` replaces the whole thing rather than publishing the ARNs.
    #[test]
    fn a_wrapper_inherits_the_visibility_of_what_it_carries() {
        let internal = unreachable(
            refused(403, REFUSED_BODY, "Lambda MicroVMs RunMicrovm failed"),
            "sandbox.create",
            "could not start a MicroVM",
        );
        assert!(
            internal.internal,
            "an internal cause makes the wrapper internal: {internal}"
        );
        assert_eq!(
            internal.into_external().message,
            "Internal server error",
            "and an internal wrapper publishes none of it"
        );

        let external = unreachable(
            AlienError::new(ClientErrorData::RemoteResourceNotFound {
                resource_type: "Microvm".to_string(),
                resource_name: "GetMicrovm".to_string(),
            }),
            "sandbox.session",
            "could not read session 'mvm-1'",
        );
        assert!(
            !external.internal,
            "a cause the client publishes leaves the wrapper public: {external}"
        );
    }

    /// A client that classifies a refusal as public must not have a deeper internal layer's
    /// response body lifted into its now-public message.
    #[test]
    fn an_internal_layer_is_not_quoted_into_a_public_wrapper() {
        let error = AlienError::new(ClientErrorData::HttpResponseError {
            message: "Request failed with HTTP 403".to_string(),
            url: "https://lambda.us-east-2.amazonaws.com/2025-09-09/microvms".to_string(),
            http_status: 403,
            http_request_text: None,
            http_response_text: Some(REFUSED_BODY.to_string()),
        })
        .context(ClientErrorData::RemoteAccessDenied {
            resource_type: "Microvm".to_string(),
            resource_name: "RunMicrovm".to_string(),
        });
        assert!(!error.internal, "the fixture's head has to be a public one");

        let reason = cloud_reason(&error);

        assert_eq!(
            reason, error.message,
            "a public wrapper carries its own message and nothing the client kept private"
        );
        assert!(
            !reason.contains("assumed-role"),
            "no identity out of the private layer reaches it: {reason}"
        );
    }

    /// Nothing captures a body when the call never reached the service, and the innermost cause
    /// is what says why — the head only says which call it was.
    #[test]
    fn the_innermost_cause_stands_in_when_nothing_captured_a_body() {
        let error = AlienError::new(ClientErrorData::HttpRequestFailed {
            message: "dns error: failed to lookup address information".to_string(),
        })
        .context(ClientErrorData::GenericError {
            message: "Lambda MicroVMs RunMicrovm failed".to_string(),
        });

        assert_eq!(
            cloud_reason(&error),
            "Lambda MicroVMs RunMicrovm failed: dns error: failed to lookup address information"
        );
    }

    /// A body with no length limit of its own must not become the message.
    #[test]
    fn cloud_text_is_bounded() {
        let body = format!(r#"{{"message":"{}"}}"#, "é".repeat(400));
        let reason = cloud_reason(&refused(500, &body, "Lambda MicroVMs RunMicrovm failed"));

        assert!(reason.ends_with('…'), "{reason}");
        assert!(
            reason.len() <= "Lambda MicroVMs RunMicrovm failed: ".len() + DETAIL_LIMIT + 4,
            "a multi-byte body is clipped at a character boundary near the limit, not past it: {}",
            reason.len()
        );
    }
}
