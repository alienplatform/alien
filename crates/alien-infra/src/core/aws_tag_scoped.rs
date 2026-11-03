//! AWS objects a runtime role may delete and read only while they carry the stack's tags, and
//! ACM certificates found again by the token their import was tagged with.

use std::future::Future;

use alien_aws_clients::acm::{find_imported_certificates_by_tag, AcmApi, Tag};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_error::AlienError;
use tracing::warn;

/// Tag carrying the token of the create call that made an object, recorded in controller state
/// before the call so a retry after a lost response finds that object and no other.
pub(crate) const CREATE_ATTEMPT_TAG: &str = "CreateAttempt";

pub(crate) fn is_remote_access_denied(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(CloudClientErrorData::RemoteAccessDenied { .. })
    )
}

/// What AWS said when it refused a call: the service's own message when the client kept it
/// (it names the denied action and the caller), otherwise the error's message.
pub(crate) fn remote_rejection_message(error: &AlienError<CloudClientErrorData>) -> String {
    let mut source = error.source.as_deref();
    while let Some(layer) = source {
        if layer.code == "REMOTE_REQUEST_REJECTED" {
            return layer.message.clone();
        }
        source = layer.source.as_deref();
    }
    error.message.clone()
}

pub(crate) fn is_remote_not_found(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
    )
}

/// How a delete of an object behind a tag-conditioned grant ended.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TagScopedDelete {
    Deleted,
    /// Confirmed gone: the delete or the read answered NotFound.
    Gone,
}

/// Interprets a delete of an object behind a tag-conditioned grant.
///
/// The role may delete such an object only while it carries the stack's tags. A gone object
/// has no tags, so AWS answers its delete with AccessDenied, not NotFound. A denied delete is
/// checked with `probe`, a read the permission sets also grant on objects without the stack's
/// tag, so a gone object answers it NotFound:
///
/// - NotFound: the object is gone.
/// - Success: the object is there and the denial is real. It is returned as not retryable, since
///   the same call with the same grants cannot succeed.
/// - AccessDenied: a role set up before that read was granted cannot tell. The delete's denial
///   is returned and the object is never treated as gone.
pub(crate) async fn delete_tag_scoped<D, P, F>(
    result: std::result::Result<D, AlienError<CloudClientErrorData>>,
    probe: F,
) -> std::result::Result<TagScopedDelete, AlienError<CloudClientErrorData>>
where
    F: FnOnce() -> P,
    P: Future<Output = std::result::Result<(), AlienError<CloudClientErrorData>>>,
{
    let mut denial = match result {
        Ok(_) => return Ok(TagScopedDelete::Deleted),
        Err(error) if is_remote_not_found(&error) => return Ok(TagScopedDelete::Gone),
        Err(error) if is_remote_access_denied(&error) => error,
        Err(error) => return Err(error),
    };
    match probe().await {
        Ok(()) => {
            denial.retryable = false;
            Err(denial)
        }
        Err(probe_error) if is_remote_not_found(&probe_error) => Ok(TagScopedDelete::Gone),
        Err(probe_error) if is_remote_access_denied(&probe_error) => {
            warn!(
                error = %denial,
                "A denied delete could not be checked: the read that tells a gone object from a real denial is denied too. Rerunning setup grants it"
            );
            Err(denial)
        }
        Err(probe_error) => Err(probe_error),
    }
}

/// Deletes an ACM certificate this resource imported. DescribeCertificate tells a certificate
/// that is gone from a real denial.
pub(crate) async fn delete_imported_certificate(
    acm: &dyn AcmApi,
    certificate_arn: &str,
) -> std::result::Result<TagScopedDelete, AlienError<CloudClientErrorData>> {
    delete_tag_scoped(acm.delete_certificate(certificate_arn).await, || async {
        acm.describe_certificate(certificate_arn).await.map(|_| ())
    })
    .await
}

/// The tags of a certificate import: `tags` plus the import token.
pub(crate) fn with_import_token(mut tags: Vec<Tag>, token: &str) -> Vec<Tag> {
    tags.retain(|tag| tag.key != CREATE_ATTEMPT_TAG);
    tags.push(Tag {
        key: CREATE_ATTEMPT_TAG.to_string(),
        value: token.to_string(),
    });
    tags
}

/// The certificates imported under `token`, sorted.
///
/// `None` when the role may not list certificates: a role installed before ListCertificates was
/// part of its permission set. The caller then has only the ARNs it recorded.
pub(crate) async fn certificates_imported_with_token(
    acm: &dyn AcmApi,
    token: &str,
) -> std::result::Result<Option<Vec<String>>, AlienError<CloudClientErrorData>> {
    match find_imported_certificates_by_tag(acm, CREATE_ATTEMPT_TAG, token).await {
        Ok(mut found) => {
            found.sort();
            Ok(Some(found))
        }
        Err(error) if is_remote_access_denied(&error) => {
            warn!(
                token = %token,
                "ACM ListCertificates is denied; rerun setup to grant it. A certificate whose import response was lost cannot be found until then"
            );
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn denied() -> AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::RemoteAccessDenied {
            resource_type: "Lambda function".to_string(),
            resource_name: "stack-api".to_string(),
        })
    }

    fn not_found() -> AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
            resource_type: "Lambda function".to_string(),
            resource_name: "stack-api".to_string(),
        })
    }

    /// A role set up before the tag-absent reads were granted: the read of a gone object is
    /// denied like the delete, so nothing tells it from a real denial. The denial stands and
    /// stays retryable, never gone.
    #[tokio::test]
    async fn denied_delete_whose_read_is_denied_too_keeps_the_denial() {
        let error = delete_tag_scoped(Err::<(), _>(denied()), || async { Err(denied()) })
            .await
            .expect_err("the denial stands");
        assert!(is_remote_access_denied(&error), "{error:?}");
        assert!(error.retryable);
    }

    /// A readable object is there: its denied delete is real.
    #[tokio::test]
    async fn denied_delete_of_a_readable_object_is_a_permanent_error() {
        let error = delete_tag_scoped(Err::<(), _>(denied()), || async { Ok(()) })
            .await
            .expect_err("the denial is real");
        assert!(is_remote_access_denied(&error));
        assert!(!error.retryable);
    }

    #[tokio::test]
    async fn not_found_from_the_delete_or_the_read_is_gone() {
        let from_delete = delete_tag_scoped(Err::<(), _>(not_found()), || async {
            panic!("no read after NotFound")
        })
        .await;
        assert_eq!(from_delete.unwrap(), TagScopedDelete::Gone);
        // A gone object answers the tag-absent read NotFound.
        let from_read =
            delete_tag_scoped(Err::<(), _>(denied()), || async { Err(not_found()) }).await;
        assert_eq!(from_read.unwrap(), TagScopedDelete::Gone);
    }
}

/// CloudWatch Logs errors as the real client returns them.
#[cfg(test)]
pub(crate) mod logs_access_denied {
    use std::collections::HashMap;

    use alien_aws_clients::{
        cloudwatch_logs::{CreateLogGroupRequest, PutRetentionPolicyRequest},
        AwsCredentialProvider, CloudWatchLogsApi, CloudWatchLogsClient,
    };
    use alien_client_core::ErrorData as CloudClientErrorData;
    use alien_core::{AwsClientConfig, AwsCredentials, AwsServiceOverrides};
    use alien_error::{AlienError, ContextError};
    use httpmock::{Method::POST, MockServer};

    fn aws_message(action: &str, log_group_name: &str) -> String {
        format!(
            "User: arn:aws:sts::123456789012:assumed-role/test-management/session is not \
             authorized to perform: logs:{action} on resource: \
             arn:aws:logs:us-east-1:123456789012:log-group:{log_group_name} because no \
             identity-based policy allows the logs:{action} action"
        )
    }

    /// The body AWS returns to a role without the grant, as recorded (account id replaced).
    fn recorded_body(action: &str, log_group_name: &str) -> String {
        serde_json::json!({
            "__type": "AccessDeniedException",
            "Message": aws_message(action, log_group_name),
        })
        .to_string()
    }

    /// The error the client returns when AWS denies `action` (`CreateLogGroup` or
    /// `PutRetentionPolicy`) on the group; `matches_the_real_client` checks it.
    pub(crate) fn error(action: &str, log_group_name: &str) -> AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::HttpResponseError {
            message: "Request failed with HTTP 400: Bad Request".to_string(),
            url: "https://logs.us-east-1.amazonaws.com/".to_string(),
            http_status: 400,
            http_request_text: None,
            http_response_text: Some(recorded_body(action, log_group_name)),
        })
        .context(CloudClientErrorData::RemoteRequestRejected {
            service: "CloudWatch Logs".to_string(),
            http_status: 400,
            provider_code: "AccessDeniedException".to_string(),
            message: aws_message(action, log_group_name),
        })
        .context(CloudClientErrorData::RemoteAccessDenied {
            resource_type: "CloudWatch Logs log group".to_string(),
            resource_name: log_group_name.to_string(),
        })
    }

    /// AWS's answer to a throttled call, as recorded.
    const THROTTLED_BODY: &str = r#"{"__type":"ThrottlingException","message":"Rate exceeded"}"#;

    /// The error the client returns when AWS throttles a call; `matches_the_real_client`
    /// checks it.
    pub(crate) fn throttled() -> AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::HttpResponseError {
            message: "Request failed with HTTP 400: Bad Request".to_string(),
            url: "https://logs.us-east-1.amazonaws.com/".to_string(),
            http_status: 400,
            http_request_text: None,
            http_response_text: Some(THROTTLED_BODY.to_string()),
        })
        .context(CloudClientErrorData::RateLimitExceeded {
            message: "Rate exceeded".to_string(),
        })
    }

    /// The text the controllers record for that denial.
    pub(crate) fn denial(action: &str, log_group_name: &str) -> String {
        format!(
            "logs:{action} was denied: CloudWatch Logs rejected the request \
             (AccessDeniedException): {}",
            aws_message(action, log_group_name)
        )
    }

    async fn real_client_error(
        action: &str,
        log_group_name: &str,
        body: String,
    ) -> AlienError<CloudClientErrorData> {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("x-amz-target", format!("Logs_20140328.{action}"));
                then.status(400).body(body);
            })
            .await;
        let client = CloudWatchLogsClient::new(
            reqwest::Client::new(),
            AwsCredentialProvider::from_config_sync(AwsClientConfig {
                account_id: "123456789012".into(),
                region: "us-east-1".into(),
                credentials: AwsCredentials::AccessKeys {
                    access_key_id: "test-access".into(),
                    secret_access_key: "test-secret".into(),
                    session_token: None,
                },
                service_overrides: Some(AwsServiceOverrides {
                    endpoints: HashMap::from([("logs".into(), server.base_url())]),
                }),
            }),
        );
        let result = if action == "CreateLogGroup" {
            client
                .create_log_group(
                    CreateLogGroupRequest::builder()
                        .log_group_name(log_group_name.to_string())
                        .build(),
                )
                .await
        } else {
            client
                .put_retention_policy(
                    PutRetentionPolicyRequest::builder()
                        .log_group_name(log_group_name.to_string())
                        .retention_in_days(365)
                        .build(),
                )
                .await
        };
        result.expect_err("AWS refused the call")
    }

    /// The real client, given the recorded response, returns the error the controller tests use
    /// (the outer two layers; the HTTP layer only differs in the mock server's address), and
    /// the controllers read the same denial text from both.
    #[tokio::test]
    async fn matches_the_real_client() {
        for action in ["CreateLogGroup", "PutRetentionPolicy"] {
            let real = real_client_error(
                action,
                "/aws/lambda/test-func",
                recorded_body(action, "/aws/lambda/test-func"),
            )
            .await;
            let used = error(action, "/aws/lambda/test-func");
            assert_eq!(
                serde_json::to_value(&real.error).unwrap(),
                serde_json::to_value(&used.error).unwrap()
            );
            assert_eq!(real.message, used.message);
            let (real_source, used_source) = (
                real.source.as_deref().unwrap(),
                used.source.as_deref().unwrap(),
            );
            assert_eq!(real_source.code, used_source.code);
            assert_eq!(real_source.message, used_source.message);
            assert!(super::is_remote_access_denied(&real));
            assert_eq!(
                format!(
                    "logs:{action} was denied: {}",
                    super::remote_rejection_message(&real)
                ),
                denial(action, "/aws/lambda/test-func")
            );
        }

        let real = real_client_error(
            "PutRetentionPolicy",
            "/aws/lambda/test-func",
            THROTTLED_BODY.to_string(),
        )
        .await;
        let used = throttled();
        assert_eq!(
            serde_json::to_value(&real.error).unwrap(),
            serde_json::to_value(&used.error).unwrap()
        );
        assert_eq!(real.message, used.message);
        assert!(!super::is_remote_access_denied(&real));
    }
}
